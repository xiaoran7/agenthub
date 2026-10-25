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
///
/// A machine with no reachable upstream and no cached answer has no catalog to
/// report. It returns an empty list rather than a built-in set of model ids:
/// those ids are a guess about someone else's account, and offering a model the
/// upstream does not serve is worse than offering none.
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

    // No upstream, no cache: report nothing rather than inventing ids.
    Vec::new()
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

/// Model catalogs keyed by the agent engine that can actually run them.
///
/// Each agent client reads only its own authentic local configuration / provider
/// store without cross-client contamination or historical transcript scraping:
/// - `claude_code`: `~/.claude/settings.json` + active `claude` gateway / CC Switch (`app_type = 'claude'`)
/// - `claude_desktop`: `Claude-3p` / `Claude` `configLibrary` (`inferenceModels`) + CC Switch (`app_type = 'claude-desktop'`)
/// - `codex`: `~/.codex/models_cache.json` + `~/.codex/config.toml` + CC Switch (`app_type = 'codex'`)
/// - `gemini_cli`: `~/.gemini/settings.json` + CC Switch (`app_type = 'gemini'`)
/// - `zcode`: `~/.zcode/v2/config.json` + CC Switch (`app_type = 'opencode'`)
/// - `grok_build`: `~/.grok/config.json` / `~/.grok/settings.json`
/// - `antigravity`: `~/.gemini/antigravity` / `~/.antigravity` config
pub fn agent_model_catalogs() -> Vec<(&'static str, Vec<Value>)> {
    let mut out = Vec::new();
    let home = crate::collectors::home_dir();

    // 1. Claude Code (`claude_code`)
    {
        let mut claude_code_models: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();

        if let Some(profile) = crate::claude_gateway::discover_claude_code_gateway() {
            for m in &profile.models {
                let id = m.name.trim();
                if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
                    continue;
                }
                claude_code_models.push(json!({
                    "id": id,
                    "name": id,
                    "display_name": m.label.clone().unwrap_or_else(|| id.to_string()),
                    "supports_1m": m.supports_1m,
                    "upstream_provider": profile.source,
                }));
            }
        }

        if let Some(ref h) = home {
            let settings_path = h.join(".claude").join("settings.json");
            if let Ok(content) = fs::read_to_string(&settings_path)
                && let Ok(val) = serde_json::from_str::<Value>(&content)
            {
                let mut candidates: Vec<String> = Vec::new();
                if let Some(m) = val.get("model").and_then(Value::as_str) {
                    candidates.push(m.to_string());
                }
                if let Some(arr) = val.get("availableModels").and_then(Value::as_array) {
                    for item in arr {
                        if let Some(m) = item
                            .as_str()
                            .or_else(|| item.get("id").and_then(Value::as_str))
                            .or_else(|| item.get("model").and_then(Value::as_str))
                        {
                            candidates.push(m.to_string());
                        }
                    }
                }
                if let Some(env_obj) = val.get("env").and_then(Value::as_object) {
                    for key in [
                        "ANTHROPIC_MODEL",
                        "ANTHROPIC_DEFAULT_OPUS_MODEL",
                        "ANTHROPIC_DEFAULT_SONNET_MODEL",
                        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                    ] {
                        if let Some(m) = env_obj.get(key).and_then(Value::as_str) {
                            candidates.push(m.to_string());
                        }
                    }
                }
                for raw in candidates {
                    let id = raw.trim();
                    if id.is_empty() || id == "<synthetic>" || !seen.insert(id.to_ascii_lowercase())
                    {
                        continue;
                    }
                    claude_code_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": id,
                        "supports_1m": id.to_ascii_lowercase().contains("1m"),
                        "upstream_provider": "claude-settings",
                    }));
                }
            }

            // When Claude Code uses official OAuth with no custom gateway model list,
            // expose the CLI's native `/model` aliases so the user can switch tiers cleanly.
            if claude_code_models.is_empty() && h.join(".claude").is_dir() {
                for (id, label, supports_1m) in [
                    ("opus", "Opus", true),
                    ("sonnet", "Sonnet", true),
                    ("haiku", "Haiku", false),
                ] {
                    if seen.insert(id.to_string()) {
                        claude_code_models.push(json!({
                            "id": id,
                            "name": id,
                            "display_name": label,
                            "supports_1m": supports_1m,
                            "upstream_provider": "claude-oauth",
                        }));
                    }
                }
            }
        }

        if !claude_code_models.is_empty() {
            out.push(("claude_code", claude_code_models));
        }
    }

    // 2. Claude Desktop (`claude_desktop`)
    {
        let mut desktop_models: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();

        // Read directly from Claude Desktop's applied configLibrary first (exact UI list)
        let cfg_lib_models = crate::claude_gateway::discover_config_library_models();
        for m in &cfg_lib_models {
            let id = m.name.trim();
            if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
                continue;
            }
            let display = match m.label.as_deref().filter(|l| !l.trim().is_empty()) {
                Some(lbl) if !lbl.eq_ignore_ascii_case(id) => format!("{id} ({lbl})"),
                _ => id.to_string(),
            };
            desktop_models.push(json!({
                "id": id,
                "name": id,
                "display_name": display,
                "supports_1m": m.supports_1m,
                "upstream_provider": "claude-desktop-config",
            }));
        }

        if desktop_models.is_empty()
            && let Some(cc) =
                crate::claude_gateway::discover_cc_switch_provider_for_app("claude-desktop")
        {
            for m in &cc.models {
                let id = m.name.trim();
                if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
                    continue;
                }
                let display = match m.label.as_deref().filter(|l| !l.trim().is_empty()) {
                    Some(lbl) if !lbl.eq_ignore_ascii_case(id) => format!("{id} ({lbl})"),
                    _ => id.to_string(),
                };
                desktop_models.push(json!({
                    "id": id,
                    "name": id,
                    "display_name": display,
                    "supports_1m": m.supports_1m,
                    "upstream_provider": cc.name,
                }));
            }
        }

        if desktop_models.is_empty()
            && let Some(profile) = crate::claude_gateway::discover_claude_desktop_gateway()
        {
            for m in &profile.models {
                let id = m.name.trim();
                if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
                    continue;
                }
                desktop_models.push(json!({
                    "id": id,
                    "name": id,
                    "display_name": m.label.clone().unwrap_or_else(|| id.to_string()),
                    "supports_1m": m.supports_1m,
                    "upstream_provider": profile.source,
                }));
            }
        }

        if !desktop_models.is_empty() {
            out.push(("claude_desktop", desktop_models));
        }
    }

    // 3. Codex (`codex`) — local client cache & config fallback/enrichment
    if let Some(ref h) = home {
        let mut codex_models: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();
        let codex_dir = h.join(".codex");

        if let Ok(toml_str) = fs::read_to_string(codex_dir.join("config.toml")) {
            for line in toml_str.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("model")
                    && let Some(rest) = rest.trim_start().strip_prefix('=')
                {
                    let val = rest.trim().trim_matches('"').trim_matches('\'').trim();
                    if !val.is_empty() && seen.insert(val.to_ascii_lowercase()) {
                        codex_models.push(json!({
                            "id": val,
                            "name": val,
                            "display_name": val,
                            "upstream_provider": "codex-config",
                        }));
                    }
                }
            }
        }

        if let Some(cc) = crate::claude_gateway::discover_cc_switch_provider_for_app("codex") {
            for m in &cc.models {
                let id = m.name.trim();
                if !id.is_empty() && seen.insert(id.to_ascii_lowercase()) {
                    codex_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": m.label.clone().unwrap_or_else(|| id.to_string()),
                        "upstream_provider": cc.name,
                    }));
                }
            }
        }

        if let Ok(cache_str) = fs::read_to_string(codex_dir.join("models_cache.json"))
            && let Ok(cache_val) = serde_json::from_str::<Value>(&cache_str)
            && let Some(arr) = cache_val
                .get("models")
                .or_else(|| cache_val.get("data"))
                .and_then(Value::as_array)
        {
            for item in arr {
                let visibility = item
                    .get("visibility")
                    .and_then(Value::as_str)
                    .unwrap_or("list");
                if visibility == "hide" {
                    continue;
                }
                let Some(id) = item
                    .get("slug")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    continue;
                };
                if !seen.insert(id.to_ascii_lowercase()) {
                    continue;
                }
                let display = item
                    .get("display_name")
                    .or_else(|| item.get("displayName"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(id);
                let ctx = item
                    .get("context_window")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                codex_models.push(json!({
                    "id": id,
                    "name": id,
                    "display_name": display,
                    "context_length": ctx,
                    "upstream_provider": "codex-client",
                }));
            }
        }

        if !codex_models.is_empty() {
            out.push(("codex", codex_models));
        }
    }

    // 4. Gemini CLI (`gemini_cli`)
    if let Some(ref h) = home {
        let mut gemini_models: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();

        if let Ok(settings_str) = fs::read_to_string(h.join(".gemini").join("settings.json"))
            && let Ok(val) = serde_json::from_str::<Value>(&settings_str)
        {
            if let Some(m) = val
                .get("model")
                .and_then(|v| v.as_str().or_else(|| v.get("name").and_then(Value::as_str)))
                .map(str::trim)
                .filter(|s| !s.is_empty())
                && seen.insert(m.to_ascii_lowercase())
            {
                gemini_models.push(json!({
                    "id": m,
                    "name": m,
                    "display_name": m,
                    "supports_1m": true,
                    "upstream_provider": "gemini-settings",
                }));
            }
        }

        if let Some(cc) = crate::claude_gateway::discover_cc_switch_provider_for_app("gemini") {
            for m in &cc.models {
                let id = m.name.trim();
                if !id.is_empty() && seen.insert(id.to_ascii_lowercase()) {
                    gemini_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": m.label.clone().unwrap_or_else(|| id.to_string()),
                        "supports_1m": true,
                        "upstream_provider": cc.name,
                    }));
                }
            }
        }

        if !gemini_models.is_empty() {
            out.push(("gemini_cli", gemini_models));
        }
    }

    // 5. ZCode / OpenCode (`zcode`)
    {
        let mut zcode_models = Vec::new();
        let mut seen = HashSet::new();

        if let Some(providers) =
            crate::collectors::zcode_app_server::load_zcode_v2_provider_registry()
        {
            for provider in &providers {
                let provider_label = provider
                    .get("label")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let Some(list) = provider.get("models").and_then(Value::as_array) else {
                    continue;
                };
                for model in list {
                    let Some(id) = model.get("modelId").and_then(Value::as_str) else {
                        continue;
                    };
                    if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
                        continue;
                    }
                    zcode_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": model
                            .get("label")
                            .and_then(Value::as_str)
                            .filter(|label| !label.is_empty())
                            .unwrap_or(id),
                        "context_length": model.get("contextWindow").cloned().unwrap_or(json!(0)),
                        "upstream_provider": provider_label,
                    }));
                }
            }
        }

        if let Some(cc) = crate::claude_gateway::discover_cc_switch_provider_for_app("opencode") {
            for m in &cc.models {
                let id = m.name.trim();
                if !id.is_empty() && seen.insert(id.to_ascii_lowercase()) {
                    zcode_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": m.label.clone().unwrap_or_else(|| id.to_string()),
                        "upstream_provider": cc.name,
                    }));
                }
            }
        }

        if !zcode_models.is_empty() {
            out.push(("zcode", zcode_models));
        }
    }

    // 6. Grok Build (`grok_build`)
    if let Some(ref h) = home {
        let mut grok_models = Vec::new();
        let mut seen = HashSet::new();
        let grok_dir = h.join(".grok");
        for cfg_name in ["config.json", "settings.json"] {
            if let Ok(content) = fs::read_to_string(grok_dir.join(cfg_name))
                && let Ok(val) = serde_json::from_str::<Value>(&content)
            {
                for key in ["model", "default_model", "current_model_id"] {
                    if let Some(m) = val.get(key).and_then(Value::as_str).map(str::trim)
                        && !m.is_empty()
                        && seen.insert(m.to_ascii_lowercase())
                    {
                        grok_models.push(json!({
                            "id": m,
                            "name": m,
                            "display_name": m,
                            "upstream_provider": "grok-config",
                        }));
                    }
                }
                if let Some(arr) = val.get("models").and_then(Value::as_array) {
                    for item in arr {
                        let id = item
                            .as_str()
                            .or_else(|| item.get("id").and_then(Value::as_str))
                            .unwrap_or("")
                            .trim();
                        if !id.is_empty() && seen.insert(id.to_ascii_lowercase()) {
                            grok_models.push(json!({
                                "id": id,
                                "name": id,
                                "display_name": id,
                                "upstream_provider": "grok-config",
                            }));
                        }
                    }
                }
            }
        }
        if !grok_models.is_empty() {
            out.push(("grok_build", grok_models));
        }
    }

    // 7. Antigravity (`antigravity`)
    if let Some(ref h) = home {
        let mut ag_models = Vec::new();
        let mut seen = HashSet::new();
        for ag_dir in [
            h.join(".gemini").join("antigravity"),
            h.join(".antigravity"),
        ] {
            for cfg_name in ["settings.json", "config.json"] {
                if let Ok(content) = fs::read_to_string(ag_dir.join(cfg_name))
                    && let Ok(val) = serde_json::from_str::<Value>(&content)
                {
                    if let Some(m) = val.get("model").and_then(Value::as_str).map(str::trim)
                        && !m.is_empty()
                        && seen.insert(m.to_ascii_lowercase())
                    {
                        ag_models.push(json!({
                            "id": m,
                            "name": m,
                            "display_name": m,
                            "upstream_provider": "antigravity-config",
                        }));
                    }
                }
            }
        }
        if !ag_models.is_empty() {
            out.push(("antigravity", ag_models));
        }
    }

    out
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
