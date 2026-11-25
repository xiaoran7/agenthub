use super::*;

/// Model catalogs keyed by the agent engine that can actually run them.
///
/// Each agent client reads only its own authentic local configuration / provider
/// store without cross-client contamination or historical transcript scraping:
/// - `claude_code`: the active gateway's live `/v1/models` catalog, `~/.claude/settings.json`, and CC Switch (`app_type = 'claude'`)
/// - `claude_desktop`: `Claude-3p` / `Claude` `configLibrary` (`inferenceModels`) + CC Switch (`app_type = 'claude-desktop'`)
/// - `codex`: `~/.codex/models_cache.json` + `~/.codex/config.toml` + CC Switch (`app_type = 'codex'`)
/// - `gemini_cli`: `~/.gemini/settings.json` + CC Switch (`app_type = 'gemini'`)
/// - `zcode`: `~/.zcode/v2/config.json` + CC Switch (`app_type = 'opencode'`)
/// - `grok_build`: `~/.grok/config.json` / `~/.grok/settings.json`
/// - `antigravity`: `~/.gemini/antigravity` / `~/.antigravity` config
pub use crate::claude_gateway::default_upstream_model_for_claude_code;

pub(crate) fn desktop_models_for_gateway(
    profile: &crate::claude_gateway::ClaudeGatewayProfile,
    catalogs: &[(String, Vec<Value>)],
    route_models: &[crate::claude_gateway::ClaudeGatewayModel],
) -> Vec<Value> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for item in route_models {
        let id = item.name.trim();
        if id.is_empty() || !seen.insert(id.to_ascii_lowercase()) {
            continue;
        }
        let display = match item.label.as_deref().filter(|l| !l.trim().is_empty()) {
            Some(label) if !label.eq_ignore_ascii_case(id) => format!("{id} ({label})"),
            _ => id.to_string(),
        };
        models.push(json!({
            "id": id,
            "name": id,
            "display_name": display,
            "supports_1m": item.supports_1m,
            "upstream_provider": "claude-desktop-config",
        }));
    }
    if profile.model_namespace == crate::claude_gateway::GatewayModelNamespace::Upstream {
        let mut push_real_model = |item: &Value| {
            let Some(id) = item.get("id").and_then(Value::as_str).map(str::trim) else {
                return;
            };
            if id.is_empty()
                || id.to_ascii_lowercase().starts_with("claude-")
                || !seen.insert(id.to_ascii_lowercase())
            {
                return;
            }
            models.push(item.clone());
        };
        if let Some((_, upstream)) = catalogs.iter().find(|(agent, _)| agent == "claude_code") {
            for item in upstream {
                push_real_model(item);
            }
        }
        for item in &profile.models {
            let id = item.name.trim();
            if id.is_empty()
                || id.to_ascii_lowercase().starts_with("claude-")
                || !seen.insert(id.to_ascii_lowercase())
            {
                continue;
            }
            models.push(json!({
                "id": id,
                "name": id,
                "display_name": item.label.clone().unwrap_or_else(|| id.to_string()),
                "supports_1m": item.supports_1m,
                "upstream_provider": profile.source,
            }));
        }
        models.retain(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .is_none_or(|id| !id.to_ascii_lowercase().starts_with("claude-"))
        });
    }
    models
}

pub fn agent_model_catalogs() -> Vec<(String, Vec<Value>)> {
    #[cfg(not(test))]
    if let Ok(guard) = agent_catalogs_cache().read()
        && let Some((cached_at, ref cached)) = *guard
        && cached_at.elapsed() < Duration::from_secs(10)
    {
        return cached.clone();
    }

    let mut out: Vec<(String, Vec<Value>)> = Vec::new();
    let home = crate::collectors::home_dir();

    // 1. Claude Code (`claude_code`)
    {
        let mut claude_code_models: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();

        let claude_gateway_profile = crate::claude_gateway::discover_claude_code_gateway();
        if let Some(ref profile) = claude_gateway_profile {
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

        merge_live_upstream_models(
            &mut claude_code_models,
            &mut seen,
            claude_gateway_profile.as_ref(),
        );

        if !claude_code_models.is_empty() {
            out.push(("claude_code".to_string(), claude_code_models));
        }
    }

    // 2. Claude Desktop (`claude_desktop`)
    {
        let cfg_lib_models = crate::claude_gateway::discover_config_library_models();
        let profile = crate::claude_gateway::discover_claude_desktop_gateway();
        let route_models = if cfg_lib_models.is_empty() {
            profile
                .as_ref()
                .filter(|profile| {
                    profile.model_namespace
                        == crate::claude_gateway::GatewayModelNamespace::RouteAlias
                })
                .map(|profile| profile.models.clone())
                .or_else(|| {
                    crate::claude_gateway::discover_cc_switch_provider_for_app("claude-desktop")
                        .map(|cc| cc.models)
                })
                .unwrap_or_default()
        } else {
            cfg_lib_models
        };
        let desktop_profile = profile.unwrap_or_else(|| {
            crate::claude_gateway::ClaudeGatewayProfile::new(
                "",
                "",
                "claude-desktop-config",
                route_models.clone(),
            )
        });
        let desktop_models = desktop_models_for_gateway(&desktop_profile, &out, &route_models);
        if !desktop_models.is_empty() {
            out.push(("claude_desktop".to_string(), desktop_models));
        }
    }

    // 3. Codex (`codex`) — local client cache & config fallback/enrichment
    if let Some(ref h) = home {
        let mut codex_models: Vec<Value> = Vec::new();
        let mut seen = HashSet::new();
        let codex_dir = h.join(".codex");
        let mut config_default_model: Option<String> = None;

        if let Ok(toml_str) = fs::read_to_string(codex_dir.join("config.toml")) {
            for line in toml_str.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("model")
                    && let Some(rest) = rest.trim_start().strip_prefix('=')
                {
                    let val = rest.trim().trim_matches('"').trim_matches('\'').trim();
                    if !val.is_empty() {
                        config_default_model = Some(val.to_owned());
                        break;
                    }
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
                    .unwrap_or(0) as usize;
                let has_reasoning = item
                    .get("supported_reasoning_levels")
                    .or_else(|| item.get("supportedReasoningEfforts"))
                    .and_then(Value::as_array)
                    .is_some_and(|a| !a.is_empty());
                let input_mods: Vec<String> = item
                    .get("input_modalities")
                    .or_else(|| item.get("inputModalities"))
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_else(|| vec!["text".to_owned(), "image".to_owned()]);
                let supports_vision = input_mods.iter().any(|m| m == "image");
                let is_default = config_default_model
                    .as_deref()
                    .is_some_and(|d| d.eq_ignore_ascii_case(id));
                codex_models.push(json!({
                    "id": id,
                    "name": id,
                    "display_name": display,
                    "is_default": is_default,
                    "context_length": ctx,
                    "context_window_str": if ctx > 0 { format_context_tokens(ctx) } else { String::new() },
                    "supports_reasoning": has_reasoning || id.contains("gpt-") || id.contains("codex"),
                    "supports_vision": supports_vision,
                    "supports_tools": true,
                    "input_modalities": input_mods,
                    "upstream_provider": "codex-client",
                }));
            }
        }

        if let Some(ref cfg_m) = config_default_model
            && seen.insert(cfg_m.to_ascii_lowercase())
        {
            codex_models.insert(
                0,
                json!({
                    "id": cfg_m,
                    "name": cfg_m,
                    "display_name": cfg_m,
                    "is_default": true,
                    "supports_reasoning": true,
                    "supports_vision": true,
                    "supports_tools": true,
                    "upstream_provider": "codex-config",
                }),
            );
        }

        if let Some(cc) = crate::claude_gateway::discover_cc_switch_provider_for_app("codex") {
            for m in cc.models.iter().chain(cc.upstream_models.iter()) {
                let id = m.name.trim();
                if !id.is_empty() && seen.insert(id.to_ascii_lowercase()) {
                    codex_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": m.label.clone().unwrap_or_else(|| id.to_string()),
                        "upstream_provider": cc.name.as_str(),
                    }));
                }
            }
        }

        if !codex_models.is_empty() {
            out.push(("codex".to_string(), codex_models));
        }
    }

    // 4. Antigravity (`antigravity`)
    if let Some(ref h) = home {
        let mut ag_models = Vec::new();
        let mut seen = HashSet::new();

        #[cfg(not(test))]
        if let Some(live) = crate::collectors::antigravity::discover_live_antigravity_models() {
            for m in live.models {
                if seen.insert(m.model_id.to_ascii_lowercase()) {
                    let is_default = live
                        .default_model_id
                        .as_deref()
                        .is_some_and(|d| d.eq_ignore_ascii_case(&m.model_id));
                    let ctx_len = crate::collectors::antigravity::resolve_antigravity_context_window(
                        &m.model_id,
                    ) as usize;
                    let mut input_mods = vec!["text"];
                    if m.supports_images {
                        input_mods.push("image");
                    }
                    ag_models.push(json!({
                        "id": m.model_id,
                        "name": m.model_id,
                        "display_name": m.label,
                        "is_default": is_default,
                        "context_length": ctx_len,
                        "context_window_str": format_context_tokens(ctx_len),
                        "supports_vision": m.supports_images,
                        "supports_reasoning": m.supports_thinking,
                        "supports_tools": true,
                        "input_modalities": input_mods,
                        "upstream_provider": "antigravity-daemon",
                    }));
                }
            }
        }

        let ag_cfg = crate::collectors::antigravity::load_antigravity_local_config_from(h, None);
        for m in ag_cfg.configured_models {
            if seen.insert(m.to_ascii_lowercase()) {
                ag_models.push(json!({
                    "id": m,
                    "name": m,
                    "display_name": m,
                    "upstream_provider": "antigravity-config",
                }));
            }
        }

        let cc_opt = crate::claude_gateway::discover_cc_switch_provider_for_app("antigravity")
            .or_else(|| crate::claude_gateway::discover_cc_switch_provider_for_app("gemini"));
        if let Some(cc) = cc_opt {
            let prov_label = if cc.name.trim().is_empty() {
                "gemini-gateway".to_string()
            } else {
                cc.name.clone()
            };
            for m in cc.models.iter().chain(cc.upstream_models.iter()) {
                let id = m.name.trim();
                if !id.is_empty() && seen.insert(id.to_ascii_lowercase()) {
                    ag_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": m.label.clone().unwrap_or_else(|| id.to_string()),
                        "upstream_provider": prov_label.as_str(),
                    }));
                }
            }
        }

        if ag_models.is_empty() {
            for (id, display) in [
                ("gemini-3.8-flash-high[1M]", "Gemini 3.8 Flash High (1M)"),
                ("gemini-3-pro", "Gemini 3 Pro"),
                ("gemini-4-pro", "Gemini 4 Pro"),
            ] {
                if seen.insert(id.to_ascii_lowercase()) {
                    ag_models.push(json!({
                        "id": id,
                        "name": id,
                        "display_name": display,
                        "upstream_provider": "google-gemini",
                    }));
                }
            }
        }

        if !ag_models.is_empty() {
            out.push(("antigravity".to_string(), ag_models));
        }
    }

    // 5. Installed and enabled ACP plugins
    for (plugin_id, models) in crate::plugins::collect_plugin_models() {
        if !models.is_empty() && !out.iter().any(|(k, _)| k == &plugin_id) {
            out.push((plugin_id, models));
        }
    }

    #[cfg(not(test))]
    if let Ok(mut guard) = agent_catalogs_cache().write() {
        *guard = Some((Instant::now(), out.clone()));
    }

    out
}
/// Enriches raw `codex app-server` `model/list` entries with `display_name`, `supports_reasoning`,
/// `supports_vision`, and measured `context_length` from `~/.codex/models_cache.json`.
pub fn enrich_codex_live_models(resp: &mut Value) {
    let mut ctx_by_slug: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    if let Some(home) = crate::collectors::home_dir()
        && let Ok(raw) = fs::read_to_string(home.join(".codex").join("models_cache.json"))
        && let Ok(val) = serde_json::from_str::<Value>(&raw)
        && let Some(arr) = val
            .get("models")
            .or_else(|| val.get("data"))
            .and_then(Value::as_array)
    {
        for entry in arr {
            if let Some(slug) = entry
                .get("slug")
                .or_else(|| entry.get("id"))
                .and_then(Value::as_str)
                && let Some(ctx) = entry.get("context_window").and_then(Value::as_u64)
                && ctx > 0
            {
                ctx_by_slug.insert(slug.to_ascii_lowercase(), ctx as usize);
            }
        }
    }

    let Some(items) = resp.get_mut("data").and_then(Value::as_array_mut) else {
        return;
    };
    for item in items {
        let Some(id) = item
            .get("id")
            .or_else(|| item.get("model"))
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        if item.get("name").is_none() {
            item["name"] = json!(id);
        }
        if item.get("display_name").is_none() {
            let disp = item
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_owned();
            item["display_name"] = json!(disp);
        }
        if let Some(is_def) = item.get("isDefault").and_then(Value::as_bool)
            && item.get("is_default").is_none()
        {
            item["is_default"] = json!(is_def);
        }
        let has_efforts = item
            .get("supportedReasoningEfforts")
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty());
        let has_image = item
            .get("inputModalities")
            .and_then(Value::as_array)
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some("image")));
        item["supports_reasoning"] = json!(has_efforts);
        item["supports_vision"] = json!(has_image);
        item["supports_tools"] = json!(true);
        if let Some(&ctx) = ctx_by_slug.get(&id.to_ascii_lowercase()) {
            item["context_length"] = json!(ctx);
            item["context_window_str"] = json!(format_context_tokens(ctx));
        }
    }
}
