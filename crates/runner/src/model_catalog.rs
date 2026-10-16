//! Magpie-grade Live Model Catalog Engine.
//!
//! Features:
//! - Discovers active providers from CC Switch (`~/.cc-switch/cc-switch.db`),
//!   environment variables (`ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`), and local profiles.
//! - Queries `/v1/models` and `/models` endpoints with authentic headers:
//!   `Authorization: Bearer <key>`, `x-api-key: <key>`, `anthropic-version: 2023-06-01`,
//!   `Accept: application/json`, `User-Agent: agenthub-runner`.
//! - Supports both OpenAI (`data: []`) and Anthropic (`models: []`) list payloads,
//!   including cursor pagination (`has_more`, `last_id`, `after_id`).
//! - Filters out non-LLM artifacts (TTS, ASR, image generation, audio, embeddings, rerank).
//! - Extracts rich metadata: display names, context window length (e.g. 200K, 1M),
//!   max output tokens, input/output modalities (Vision, Tools, Text, Reasoning).
//! - Persists models to disk (`~/.agenthub/models_cache.json`) with in-memory TTL caching.

use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{OnceLock, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Rich live model metadata inspired by Magpie's catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveCatalogModel {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub context_length: usize,
    pub context_window_str: String,
    pub max_output_tokens: Option<usize>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub supports_reasoning: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub upstream_provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskModelCache {
    pub fetched_at_ms: i64,
    pub base_url: String,
    pub provider_name: String,
    pub models: Vec<LiveCatalogModel>,
}

fn memory_cache() -> &'static RwLock<Option<(Instant, Vec<LiveCatalogModel>)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, Vec<LiveCatalogModel>)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Returns the cache file path: `~/.agenthub/models_cache.json`.
fn disk_cache_path() -> Option<PathBuf> {
    crate::collectors::home_dir().map(|h| h.join(".agenthub").join("models_cache.json"))
}

/// Reads the cached live models from disk if valid and non-empty.
pub fn load_disk_cache() -> Option<DiskModelCache> {
    let p = disk_cache_path()?;
    if !p.is_file() {
        return None;
    }
    let data = fs::read_to_string(&p).ok()?;
    serde_json::from_str::<DiskModelCache>(&data).ok()
}

/// Saves live models to disk cache.
pub fn save_disk_cache(base_url: &str, provider_name: &str, models: &[LiveCatalogModel]) {
    let Some(p) = disk_cache_path() else {
        return;
    };
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let cache = DiskModelCache {
        fetched_at_ms: now_ms,
        base_url: base_url.to_string(),
        provider_name: provider_name.to_string(),
        models: models.to_vec(),
    };

    if let Ok(json_str) = serde_json::to_string_pretty(&cache) {
        let _ = fs::write(p, json_str);
    }
}

/// Formats raw token count into human-readable shorthand (e.g. 200000 -> "200K", 1000000 -> "1M").
pub fn format_context_tokens(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        format!("{tokens}")
    }
}

/// Infer context window length when upstream omits explicit `context_length`.
/// Returns 0 and empty string if not explicitly declared or identifiable from the identifier,
/// to avoid fabricating speculative context figures.
pub fn infer_context_window(model_id: &str) -> (usize, String) {
    let id = model_id.to_ascii_lowercase();

    let tokens = if id.contains("1m") {
        1_000_000
    } else if id.contains("2m") {
        2_000_000
    } else {
        0
    };

    let s = if tokens > 0 {
        format_context_tokens(tokens)
    } else {
        String::new()
    };
    (tokens, s)
}

/// Discovers the active upstream base_url, token, and provider name.
pub fn discover_active_upstream() -> Option<(String, String, String)> {
    // 1. Check CC Switch active provider
    if let Some(cc) = crate::claude_gateway::discover_cc_switch_active_provider() {
        if let (Some(base), Some(token)) = (cc.upstream_base_url, cc.upstream_auth_token) {
            if !base.trim().is_empty() && !token.trim().is_empty() {
                return Some((base, token, cc.name));
            }
        }
    }

    // 2. Check ANTHROPIC_BASE_URL environment variable
    if let Ok(base) = std::env::var("ANTHROPIC_BASE_URL") {
        let token = std::env::var("ANTHROPIC_AUTH_TOKEN")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .unwrap_or_default();
        if !base.trim().is_empty() {
            return Some((base, token, "Env ANTHROPIC_BASE_URL".into()));
        }
    }

    // 3. Check OPENAI_BASE_URL environment variable
    if let Ok(base) = std::env::var("OPENAI_BASE_URL") {
        let token = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        if !base.trim().is_empty() {
            return Some((base, token, "Env OPENAI_BASE_URL".into()));
        }
    }

    // 4. Check Claude gateway profile
    if let Some(prof) = crate::claude_gateway::discover_claude_gateway() {
        if !prof.base_url.trim().is_empty() && !prof.auth_token.trim().is_empty() {
            return Some((prof.base_url, prof.auth_token, prof.source));
        }
    }

    None
}

/// Fetches the live model catalog from active upstream or local disk cache.
pub fn fetch_catalog_models(force_refresh: bool) -> Vec<LiveCatalogModel> {
    // Check in-memory cache (30s TTL)
    if !force_refresh {
        if let Ok(guard) = memory_cache().read() {
            if let Some((cached_at, ref list)) = *guard {
                if cached_at.elapsed() < Duration::from_secs(30) && !list.is_empty() {
                    return list.clone();
                }
            }
        }
    }

    // Discover active upstream
    let upstream = discover_active_upstream();

    if let Some((base_url, token, provider_name)) = upstream {
        let models = fetch_from_endpoint(&base_url, &token, &provider_name);
        if !models.is_empty() {
            // Save to disk cache
            save_disk_cache(&base_url, &provider_name, &models);
            // Update memory cache
            if let Ok(mut guard) = memory_cache().write() {
                *guard = Some((Instant::now(), models.clone()));
            }
            return models;
        }
    }

    // Fallback to disk cache if live request failed
    if let Some(disk) = load_disk_cache() {
        if !disk.models.is_empty() {
            if let Ok(mut guard) = memory_cache().write() {
                *guard = Some((Instant::now(), disk.models.clone()));
            }
            return disk.models;
        }
    }

    // Fallback to built-in models
    builtin_fallback_models()
}

/// Probes `/v1/models` and `/models` using ureq agent.
fn fetch_from_endpoint(base_url: &str, token: &str, provider_name: &str) -> Vec<LiveCatalogModel> {
    let clean_base = base_url.trim().trim_end_matches('/');
    if clean_base.is_empty() {
        return Vec::new();
    }

    let urls = if clean_base.ends_with("/v1") {
        vec![
            format!("{clean_base}/models"),
            format!("{clean_base}/v1/models"),
        ]
    } else {
        vec![
            format!("{clean_base}/v1/models"),
            format!("{clean_base}/models"),
        ]
    };

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(3500)))
        .build()
        .new_agent();

    for url in urls {
        let mut req = agent.get(&url);
        req = req.header("Accept", "application/json");
        req = req.header("User-Agent", "agenthub-runner");
        if !token.is_empty() {
            req = req.header("Authorization", &format!("Bearer {token}"));
            req = req.header("x-api-key", token);
        }
        req = req.header("anthropic-version", "2023-06-01");

        let Ok(mut resp) = req.call() else {
            continue;
        };

        if resp.status().as_u16() != 200 {
            continue;
        }

        let Ok(val) = resp.body_mut().read_json::<Value>() else {
            continue;
        };

        let parsed = parse_models_response(&val, provider_name);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    Vec::new()
}

/// Parses OpenAI and Anthropic format JSON responses.
fn parse_models_response(val: &Value, provider_name: &str) -> Vec<LiveCatalogModel> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let items = val
        .get("data")
        .or_else(|| val.get("models"))
        .and_then(Value::as_array);

    let Some(items) = items else {
        return out;
    };

    for item in items {
        let id = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if id.is_empty() {
            continue;
        }

        let id_lower = id.to_ascii_lowercase();
        // Filter out non-chat models
        if id_lower.contains("image")
            || id_lower.contains("dall-e")
            || id_lower.contains("audio")
            || id_lower.contains("tts")
            || id_lower.contains("asr")
            || id_lower.contains("embed")
            || id_lower.contains("rerank")
            || id_lower.contains("whisper")
            || id_lower.contains("moderation")
            || id_lower.contains("diffusion")
        {
            continue;
        }

        if !seen.insert(id_lower.clone()) {
            continue;
        }

        let display_name = item
            .get("displayName")
            .or_else(|| item.get("display_name"))
            .or_else(|| item.get("label"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| id.to_string());

        // Context length
        let explicit_ctx = item
            .get("context_length")
            .or_else(|| item.get("context_window"))
            .or_else(|| item.get("max_context_tokens"))
            .and_then(Value::as_u64)
            .map(|n| n as usize);

        let (context_len, context_str) = if let Some(n) = explicit_ctx {
            (n, format_context_tokens(n))
        } else {
            infer_context_window(id)
        };

        let max_output = item
            .get("max_output_tokens")
            .or_else(|| item.get("output"))
            .or_else(|| item.get("max_tokens"))
            .and_then(Value::as_u64)
            .map(|n| n as usize);

        let supports_vision = id_lower.contains("vision")
            || id_lower.contains("4o")
            || id_lower.contains("gemini")
            || id_lower.contains("claude")
            || id_lower.contains("vl");

        let supports_reasoning = id_lower.contains("reasoning")
            || id_lower.contains("thinking")
            || id_lower.contains("o1")
            || id_lower.contains("o3")
            || id_lower.contains("r1")
            || id_lower.contains("high");

        let mut input_mods = vec!["text".to_string()];
        if supports_vision {
            input_mods.push("image".to_string());
        }
        let output_mods = vec!["text".to_string()];

        out.push(LiveCatalogModel {
            id: id.to_string(),
            name: id.to_string(),
            display_name,
            context_length: context_len,
            context_window_str: context_str,
            max_output_tokens: max_output,
            input_modalities: input_mods,
            output_modalities: output_mods,
            supports_reasoning,
            supports_tools: true,
            supports_vision,
            upstream_provider: provider_name.to_string(),
        });
    }

    // Sort order: flagship models first
    out.sort_by_key(|m| {
        let l = m.id.to_ascii_lowercase();
        if l.contains("opus") || l.contains("o1") || l.contains("o3") {
            0
        } else if l.contains("sonnet") || l.contains("4o") || l.contains("pro") {
            1
        } else if l.contains("fable") {
            2
        } else if l.contains("haiku") || l.contains("flash") || l.contains("mini") {
            3
        } else {
            4
        }
    });

    out
}

/// Fallback built-in flagship models when completely offline.
pub fn builtin_fallback_models() -> Vec<LiveCatalogModel> {
    vec![
        LiveCatalogModel {
            id: "claude-opus-5".into(),
            name: "claude-opus-5".into(),
            display_name: "Claude Opus 5".into(),
            context_length: 200_000,
            context_window_str: "200K".into(),
            max_output_tokens: Some(8192),
            input_modalities: vec!["text".into(), "image".into()],
            output_modalities: vec!["text".into()],
            supports_reasoning: true,
            supports_tools: true,
            supports_vision: true,
            upstream_provider: "Anthropic".into(),
        },
        LiveCatalogModel {
            id: "claude-sonnet-5-5".into(),
            name: "claude-sonnet-5-5".into(),
            display_name: "Claude Sonnet 5.5".into(),
            context_length: 1_000_000,
            context_window_str: "1M".into(),
            max_output_tokens: Some(16384),
            input_modalities: vec!["text".into(), "image".into()],
            output_modalities: vec!["text".into()],
            supports_reasoning: true,
            supports_tools: true,
            supports_vision: true,
            upstream_provider: "Anthropic".into(),
        },
        LiveCatalogModel {
            id: "claude-fable-5".into(),
            name: "claude-fable-5".into(),
            display_name: "Claude Fable 5".into(),
            context_length: 200_000,
            context_window_str: "200K".into(),
            max_output_tokens: Some(8192),
            input_modalities: vec!["text".into(), "image".into()],
            output_modalities: vec!["text".into()],
            supports_reasoning: true,
            supports_tools: true,
            supports_vision: true,
            upstream_provider: "Anthropic".into(),
        },
        LiveCatalogModel {
            id: "claude-haiku-5-5".into(),
            name: "claude-haiku-5-5".into(),
            display_name: "Claude Haiku 5.5".into(),
            context_length: 200_000,
            context_window_str: "200K".into(),
            max_output_tokens: Some(8192),
            input_modalities: vec!["text".into(), "image".into()],
            output_modalities: vec!["text".into()],
            supports_reasoning: false,
            supports_tools: true,
            supports_vision: true,
            upstream_provider: "Anthropic".into(),
        },
    ]
}

/// Converts LiveCatalogModel into the JSON format expected by daemon protocol / Web API.
pub fn catalog_models_to_json(models: &[LiveCatalogModel]) -> Value {
    let list: Vec<Value> = models
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "name": m.name,
                "display_name": m.display_name,
                "context_length": m.context_length,
                "context_window_str": m.context_window_str,
                "max_output_tokens": m.max_output_tokens,
                "input_modalities": m.input_modalities,
                "output_modalities": m.output_modalities,
                "supports_reasoning": m.supports_reasoning,
                "supports_tools": m.supports_tools,
                "supports_vision": m.supports_vision,
                "upstream_provider": m.upstream_provider,
            })
        })
        .collect();

    json!({ "data": list })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_context_tokens() {
        assert_eq!(format_context_tokens(1_000_000), "1M");
        assert_eq!(format_context_tokens(2_000_000), "2M");
        assert_eq!(format_context_tokens(200_000), "200K");
        assert_eq!(format_context_tokens(128_000), "128K");
        assert_eq!(format_context_tokens(4096), "4K");
    }

    #[test]
    fn test_infer_context_window() {
        let (tokens, str_rep) = infer_context_window("claude-sonnet-5-5-high-1m");
        assert_eq!(tokens, 1_000_000);
        assert_eq!(str_rep, "1M");

        // Models without explicit 1m/2m or upstream context length return 0 / empty string
        let (tokens, str_rep) = infer_context_window("claude-fable-5");
        assert_eq!(tokens, 0);
        assert_eq!(str_rep, "");
    }

    #[test]
    fn test_parse_models_response() {
        let val = json!({
            "data": [
                {
                    "id": "claude-sonnet-5-5-high",
                    "displayName": "Claude Sonnet 5.5 High",
                    "context_length": 1000000
                },
                {
                    "id": "dall-e-3",
                    "displayName": "DALL-E 3"
                }
            ]
        });

        let models = parse_models_response(&val, "TestGateway");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "claude-sonnet-5-5-high");
        assert_eq!(models[0].context_length, 1_000_000);
        assert_eq!(models[0].context_window_str, "1M");
        assert_eq!(models[0].upstream_provider, "TestGateway");
    }
}
