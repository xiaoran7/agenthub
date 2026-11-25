use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

static CATALOG_REFRESH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct RefreshFlightGuard;

impl Drop for RefreshFlightGuard {
    fn drop(&mut self) {
        CATALOG_REFRESH_IN_FLIGHT.store(false, Ordering::Release);
    }
}

pub(crate) fn validated_upstream_candidate(
    base_url: &str,
    token: &str,
    source: &str,
) -> Option<(String, String, String)> {
    let base = base_url.trim();
    let tok = token.trim();
    if base.is_empty()
        || !crate::claude_gateway::is_token_compatible_with_endpoint(base, tok)
        || !crate::claude_gateway::is_gateway_url_reachable(base)
    {
        return None;
    }
    Some((base.to_string(), tok.to_string(), source.to_string()))
}

/// Discovers the active upstream base_url, token, and provider name.
/// Prioritizes Claude Code CLI gateways so `merge_live_upstream_models` shares the
/// exact upstream endpoint used by `claude_code`, rather than being shadowed by
/// a stale Claude Desktop `configLibrary` route proxy.
pub fn discover_active_upstream() -> Option<(String, String, String)> {
    if let Some(prof) = crate::claude_gateway::discover_claude_code_gateway()
        && let Some(hit) =
            validated_upstream_candidate(&prof.base_url, &prof.auth_token, &prof.source)
    {
        return Some(hit);
    }

    if let Some(prof) = crate::claude_gateway::discover_codex_gateway()
        && let Some(hit) = validated_upstream_candidate(&prof.base_url, &prof.api_key, &prof.source)
    {
        return Some(hit);
    }

    if let Some(prof) = crate::claude_gateway::discover_antigravity_gateway()
        && let Some(hit) = validated_upstream_candidate(&prof.base_url, &prof.api_key, &prof.source)
    {
        return Some(hit);
    }

    if let Some(cc) = crate::claude_gateway::discover_cc_switch_active_provider()
        && let (Some(base), Some(token)) = (cc.upstream_base_url, cc.upstream_auth_token)
        && let Some(hit) = validated_upstream_candidate(&base, &token, &cc.name)
    {
        return Some(hit);
    }

    if let Some(prof) = crate::claude_gateway::discover_claude_desktop_gateway()
        && let Some(hit) =
            validated_upstream_candidate(&prof.base_url, &prof.auth_token, &prof.source)
    {
        return Some(hit);
    }

    None
}

fn load_matching_disk_cache(active_base_url: Option<&str>) -> Vec<LiveCatalogModel> {
    let Some(expected) = active_base_url.map(|u| u.trim().trim_end_matches('/')) else {
        return Vec::new();
    };
    load_disk_cache()
        .filter(|d| d.base_url.trim().trim_end_matches('/') == expected)
        .map(|d| d.models)
        .unwrap_or_default()
}

fn is_loopback_endpoint(base_url: &str) -> bool {
    url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .is_some_and(|host| {
            let trimmed = host.trim_start_matches('[').trim_end_matches(']');
            trimmed == "localhost"
                || trimmed == "::1"
                || trimmed == "127.0.0.1"
                || trimmed.starts_with("127.")
        })
}

/// Fetches the live model catalog from active upstream or local disk cache.
///
/// A machine with no reachable upstream and no cached answer has no catalog to
/// report. It returns an empty list rather than a built-in set of model ids:
/// those ids are a guess about someone else's account, and offering a model the
/// upstream does not serve is worse than offering none.
pub fn fetch_catalog_models(force_refresh: bool) -> Vec<LiveCatalogModel> {
    // Check in-memory cache (120s TTL, including empty results so unreachable
    // endpoints never block repeated dashboard polls).
    if !force_refresh {
        if let Ok(guard) = memory_cache().read()
            && let Some((cached_at, _, ref list)) = *guard
            && cached_at.elapsed() < Duration::from_secs(120)
        {
            return list.clone();
        }
        // Serve disk cache immediately and run upstream discovery + HTTP probing
        // on at most one background thread so `/api/dashboard` never blocks on
        // network reachability checks or HTTP timeouts.
        let disk = load_disk_cache();
        let cached_base = disk.as_ref().map(|d| d.base_url.clone());
        let initial = disk.map(|d| d.models).unwrap_or_default();
        if let Ok(mut guard) = memory_cache().write() {
            *guard = Some((Instant::now(), cached_base, initial.clone()));
        }
        if CATALOG_REFRESH_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let flight = RefreshFlightGuard;
            let _ = std::thread::Builder::new()
                .name("model-catalog-refresh".into())
                .spawn(move || {
                    let _flight = flight;
                    if let Some((base_url, token, provider_name)) = discover_active_upstream() {
                        let models = fetch_from_endpoint(&base_url, &token, &provider_name);
                        if !models.is_empty() {
                            save_disk_cache(&base_url, &provider_name, &models);
                            if let Ok(mut guard) = memory_cache().write() {
                                *guard = Some((Instant::now(), Some(base_url), models));
                            }
                        } else {
                            let fallback = load_matching_disk_cache(Some(base_url.as_str()));
                            if let Ok(mut guard) = memory_cache().write() {
                                *guard = Some((Instant::now(), Some(base_url), fallback));
                            }
                        }
                    } else if let Ok(mut guard) = memory_cache().write() {
                        *guard = Some((Instant::now(), None, Vec::new()));
                    }
                });
        }
        return initial;
    }

    // Explicit `force_refresh == true`: synchronous probe
    let upstream = discover_active_upstream();

    if let Some((ref base_url, ref token, ref provider_name)) = upstream {
        let models = fetch_from_endpoint(base_url, token, provider_name);
        if !models.is_empty() {
            save_disk_cache(base_url, provider_name, &models);
            if let Ok(mut guard) = memory_cache().write() {
                *guard = Some((Instant::now(), Some(base_url.clone()), models.clone()));
            }
            return models;
        }
    }

    // Fallback to disk cache only if it matches the active upstream base_url
    let fallback = load_matching_disk_cache(upstream.as_ref().map(|(b, _, _)| b.as_str()));
    if let Ok(mut guard) = memory_cache().write() {
        *guard = Some((
            Instant::now(),
            upstream.map(|(b, _, _)| b),
            fallback.clone(),
        ));
    }
    fallback
}

/// Probes `/v1/models` and `/models` using ureq agent.
fn fetch_from_endpoint(base_url: &str, token: &str, provider_name: &str) -> Vec<LiveCatalogModel> {
    let clean_base = base_url.trim().trim_end_matches('/');
    let token = token.trim();
    if clean_base.is_empty()
        || !crate::claude_gateway::is_token_compatible_with_endpoint(clean_base, token)
        || !crate::claude_gateway::is_gateway_url_reachable(clean_base)
    {
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

    let mut builder = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_millis(3500)));
    if is_loopback_endpoint(clean_base) {
        builder = builder.proxy(None);
    }
    let agent = builder.build().new_agent();

    for url in urls {
        let mut req = agent.get(&url);
        req = req.header("Accept", "application/json");
        req = req.header("Accept-Encoding", "identity");
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
fn recover_relay_model_id(raw_id: &str, display_hint: Option<&str>) -> Option<String> {
    const PREFIX: &str = "claude-fable-5-dd-";
    let encoded = raw_id.strip_prefix(PREFIX)?;
    let decoded = encoded.chars().rev().collect::<String>();
    let normalize = |v: &str| {
        v.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let display = display_hint.map(normalize).unwrap_or_default();
    let candidate = normalize(&decoded);
    let exact = display == candidate;
    let related =
        !display.is_empty() && (display.starts_with(&candidate) || candidate.starts_with(&display));
    let useful = decoded
        .chars()
        .any(|c| c.is_ascii_alphanumeric() && !c.is_ascii_uppercase());
    (exact || related || useful).then_some(decoded)
}

pub(crate) fn parse_models_response(val: &Value, provider_name: &str) -> Vec<LiveCatalogModel> {
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
        let raw_id = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if raw_id.is_empty() {
            continue;
        }

        // Some legacy cn-relay catalogs prefix "claude-fable-5-dd-" to a
        // reversed upstream id. Recover the real id before it reaches the
        // web console's `--model` payload.
        let display_hint = item
            .get("displayName")
            .or_else(|| item.get("display_name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let recovered = recover_relay_model_id(raw_id, display_hint);
        let id = recovered.as_deref().unwrap_or(raw_id);

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
            || id_lower.contains("gpt-5")
            || id_lower.contains("gpt-6")
            || id_lower.contains("codex")
            || id_lower.contains("o3")
            || id_lower.contains("o4")
            || id_lower.contains("gemini")
            || id_lower.contains("claude")
            || id_lower.contains("vl");

        let supports_reasoning = id_lower.contains("reasoning")
            || id_lower.contains("thinking")
            || id_lower.contains("gpt-5")
            || id_lower.contains("gpt-6")
            || id_lower.contains("codex")
            || id_lower.contains("o1")
            || id_lower.contains("o3")
            || id_lower.contains("o4")
            || id_lower.contains("r1")
            || id_lower.contains("high")
            || id_lower.contains("opus")
            || id_lower.contains("sonnet")
            || id_lower.contains("gemini-3")
            || id_lower.contains("gemini-4");

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

// Full metadata for one live model as the web console expects it.
pub(crate) fn live_catalog_model_json(m: &LiveCatalogModel) -> Value {
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
}

/// Extends an agent's configured model list with ids from the active gateway's
/// own `/v1/models` catalog. The discovered gateway is the endpoint that will
/// actually receive the CLI request, so its ids are valid `--model` values even
/// when a local settings file only lists a few default aliases.
pub(crate) fn merge_live_upstream_models(
    agent_models: &mut Vec<Value>,
    seen: &mut HashSet<String>,
    profile: Option<&crate::claude_gateway::ClaudeGatewayProfile>,
) {
    let Some(profile) = profile else {
        return;
    };
    let models = fetch_catalog_models(false);
    let cached_base = memory_cache()
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().and_then(|(_, base, _)| base.clone()));
    let expected = profile.base_url.trim().trim_end_matches('/');
    let same_upstream = cached_base
        .as_deref()
        .map(|u| u.trim().trim_end_matches('/'))
        .is_some_and(|base| !expected.is_empty() && base == expected);
    if !same_upstream {
        return;
    }
    for model in models {
        let id = model.id.trim();
        if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
            continue;
        }
        agent_models.push(live_catalog_model_json(&model));
    }
}

/// Converts LiveCatalogModel into the JSON format expected by daemon protocol / Web API.
pub fn catalog_models_to_json(models: &[LiveCatalogModel]) -> Value {
    let list: Vec<Value> = models.iter().map(live_catalog_model_json).collect();

    json!({ "data": list })
}
