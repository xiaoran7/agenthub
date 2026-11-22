use super::*;

/// Discovers active Claude gateway configuration for a specific client (`desktop == true` for Claude Desktop,
/// `desktop == false` for Claude Code CLI).
/// For Claude Desktop sessions, if desktop-specific profile discovery yields no viable endpoint,
/// it smoothly falls back to the universal Claude Code gateway before returning None.
pub fn discover_claude_gateway_for_client(desktop: bool) -> Option<ClaudeGatewayProfile> {
    if desktop {
        discover_claude_desktop_gateway().or_else(discover_claude_code_gateway)
    } else {
        discover_claude_code_gateway()
    }
}

/// Discovers active gateway configuration for OpenAI Codex CLI (`codex`).
pub fn discover_codex_gateway() -> Option<CodexGatewayProfile> {
    if let (Ok(base_url), Ok(api_key)) = (env::var("OPENAI_BASE_URL"), env::var("OPENAI_API_KEY")) {
        let base_url = base_url.trim().to_string();
        let api_key = api_key.trim().to_string();
        if !base_url.is_empty()
            && is_token_compatible_with_endpoint(&base_url, &api_key)
            && is_gateway_url_reachable(&base_url)
        {
            let cc_codex = discover_cc_switch_provider_for_app("codex");
            let models = cc_codex.map(|c| c.models).unwrap_or_default();
            return Some(CodexGatewayProfile::new(
                base_url,
                api_key,
                "environment",
                models,
            ));
        }
    }

    if let Some(cc) = discover_cc_switch_provider_for_app("codex")
        && let (Some(upstream_url), Some(upstream_token)) =
            (cc.upstream_base_url, cc.upstream_auth_token)
    {
        let upstream_url = upstream_url.trim().to_string();
        let upstream_token = upstream_token.trim().to_string();
        if !upstream_url.is_empty()
            && is_token_compatible_with_endpoint(&upstream_url, &upstream_token)
            && is_gateway_url_reachable(&upstream_url)
        {
            let models = if !cc.upstream_models.is_empty() {
                cc.upstream_models
            } else {
                cc.models
            };
            return Some(CodexGatewayProfile::new(
                upstream_url,
                upstream_token,
                format!("cc-switch:{}", cc.name),
                models,
            ));
        }
    }

    None
}

/// Discovers active gateway configuration for Google Antigravity CLI (`antigravity`).
pub fn discover_antigravity_gateway() -> Option<AntigravityGatewayProfile> {
    let env_url = env::var("GOOGLE_GEMINI_BASE_URL")
        .or_else(|_| env::var("GEMINI_BASE_URL"))
        .or_else(|_| env::var("GOOGLE_BASE_URL"))
        .ok();
    let env_key = env::var("GEMINI_API_KEY")
        .or_else(|_| env::var("GOOGLE_API_KEY"))
        .ok();
    if let (Some(base_url), Some(api_key)) = (env_url, env_key) {
        let base_url = base_url.trim().to_string();
        let api_key = api_key.trim().to_string();
        if !base_url.is_empty()
            && is_token_compatible_with_endpoint(&base_url, &api_key)
            && is_gateway_url_reachable(&base_url)
        {
            let cc = discover_cc_switch_provider_for_app("antigravity")
                .or_else(|| discover_cc_switch_provider_for_app("gemini"));
            let models = cc.map(|c| c.models).unwrap_or_default();
            return Some(AntigravityGatewayProfile::new(
                base_url,
                api_key,
                "environment",
                models,
            ));
        }
    }

    let cc_opt = discover_cc_switch_provider_for_app("antigravity")
        .or_else(|| discover_cc_switch_provider_for_app("gemini"));
    if let Some(cc) = cc_opt
        && let (Some(upstream_url), Some(upstream_token)) =
            (cc.upstream_base_url, cc.upstream_auth_token)
    {
        let upstream_url = upstream_url.trim().to_string();
        let upstream_token = upstream_token.trim().to_string();
        if !upstream_url.is_empty()
            && is_token_compatible_with_endpoint(&upstream_url, &upstream_token)
            && is_gateway_url_reachable(&upstream_url)
        {
            let models = if !cc.upstream_models.is_empty() {
                cc.upstream_models
            } else {
                cc.models
            };
            return Some(AntigravityGatewayProfile::new(
                upstream_url,
                upstream_token,
                format!("cc-switch:{}", cc.name),
                models,
            ));
        }
    }

    None
}

/// Discovers active gateway configuration specifically for Claude Code CLI (`claude`).
///
/// By default, Claude Code uses official OAuth unless explicitly configured via process env,
/// `~/.claude/settings.json`, or an active `cc-switch.db` provider for `app_type = 'claude'`.
/// It never inherits Claude Desktop's `Claude-3p` or `host-creds` profiles.
pub fn discover_claude_code_gateway() -> Option<ClaudeGatewayProfile> {
    #[cfg(not(test))]
    if let Ok(guard) = CLAUDE_CODE_GATEWAY_CACHE.read()
        && let Some((cached_at, ref cached)) = *guard
        && cached_at.elapsed() < Duration::from_secs(10)
    {
        return cached.clone();
    }

    let profile = discover_claude_code_gateway_uncached();

    #[cfg(not(test))]
    if let Ok(mut guard) = CLAUDE_CODE_GATEWAY_CACHE.write() {
        *guard = Some((Instant::now(), profile.clone()));
    }

    profile
}

pub(crate) fn discover_claude_code_gateway_uncached() -> Option<ClaudeGatewayProfile> {
    let env_gateway = discover_environment_gateway();
    let cc_claude = discover_cc_switch_provider_for_app("claude");

    if let Some((base_url, auth_token)) = env_gateway
        && is_gateway_url_reachable(&base_url)
    {
        let mut models = cc_claude
            .as_ref()
            .map(|c| c.models.clone())
            .unwrap_or_default();
        if models.is_empty() {
            models = fetch_live_upstream_models(&base_url, &auth_token);
        }
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            "environment",
            models,
        ));
    }

    if let Some((base_url, auth_token, mut models, source_name)) = discover_claude_settings_env() {
        if is_gateway_url_reachable(&base_url) {
            if let Some(ref cc) = cc_claude
                && cc_switch_manages_live_url(cc, Some(&base_url))
                && !cc.models.is_empty()
            {
                models = cc.models.clone();
            }
            return Some(ClaudeGatewayProfile::new(
                base_url,
                auth_token,
                source_name,
                models,
            ));
        }
    }

    if let Some(cc) = cc_claude
        && let Some(upstream_url) = cc.upstream_base_url
        && let Some(upstream_token) = cc.upstream_auth_token
        && is_token_compatible_with_endpoint(&upstream_url, &upstream_token)
        && is_gateway_url_reachable(&upstream_url)
    {
        let mut models = if !cc.upstream_models.is_empty() {
            cc.upstream_models
        } else {
            cc.models
        };
        if models.is_empty() {
            models = fetch_live_upstream_models(&upstream_url, &upstream_token);
        }
        return Some(ClaudeGatewayProfile::new(
            upstream_url,
            upstream_token,
            format!("cc-switch:{}", cc.name),
            models,
        ));
    }

    None
}

/// A process-level gateway is authoritative only when both the endpoint and its
/// credential are present. A lone inherited `ANTHROPIC_BASE_URL` can belong to
/// another client's proxy while `settings.json` carries the CLI's real token;
/// combining those halves produces an authentication failure.
pub(crate) fn discover_environment_gateway() -> Option<(String, String)> {
    gateway_pair_from_env()
}

/// Reads the current process's gateway variables as a complete pair.
pub fn gateway_pair_from_env() -> Option<(String, String)> {
    let base_url = env::var("ANTHROPIC_BASE_URL").ok();
    let auth_token = env::var("ANTHROPIC_AUTH_TOKEN").ok();
    let api_key = env::var("ANTHROPIC_API_KEY").ok();
    gateway_pair_from_values(
        base_url.as_deref(),
        auth_token.as_deref(),
        api_key.as_deref(),
    )
}

pub(crate) fn gateway_pair_from_values(
    base_url: Option<&str>,
    auth_token: Option<&str>,
    api_key: Option<&str>,
) -> Option<(String, String)> {
    let base_url = base_url.map(str::trim).filter(|v| !v.is_empty())?;
    let auth_token = auth_token
        .map(str::trim)
        .filter(|v| is_token_compatible_with_endpoint(base_url, v))
        .or_else(|| {
            api_key
                .map(str::trim)
                .filter(|v| is_token_compatible_with_endpoint(base_url, v))
        })?;
    Some((base_url.to_string(), auth_token.to_string()))
}

/// Discovers active gateway configuration specifically for Claude Desktop (`claude-desktop`).
pub fn discover_claude_desktop_gateway() -> Option<ClaudeGatewayProfile> {
    #[cfg(not(test))]
    if let Ok(guard) = CLAUDE_DESKTOP_GATEWAY_CACHE.read()
        && let Some((cached_at, ref cached)) = *guard
        && cached_at.elapsed() < Duration::from_secs(10)
    {
        return cached.clone();
    }

    let profile = discover_claude_desktop_gateway_uncached();

    #[cfg(not(test))]
    if let Ok(mut guard) = CLAUDE_DESKTOP_GATEWAY_CACHE.write() {
        *guard = Some((Instant::now(), profile.clone()));
    }

    profile
}

pub(crate) fn discover_claude_desktop_gateway_uncached() -> Option<ClaudeGatewayProfile> {
    let cc_desktop = discover_cc_switch_provider_for_app("claude-desktop");
    let roots = gateway_search_roots();

    // 1. Explicit process environment variables take precedence only as a pair.

    if let Some((base_url, auth_token)) = discover_environment_gateway()
        && is_gateway_url_reachable(&base_url)
    {
        let namespace = ClaudeGatewayProfile::namespace_for_endpoint(&base_url, &auth_token);
        let mut models = if namespace == GatewayModelNamespace::RouteAlias
            && let Some(ref cc) = cc_desktop
            && !cc.models.is_empty()
        {
            cc.models.clone()
        } else {
            Vec::new()
        };
        if models.is_empty() && namespace == GatewayModelNamespace::RouteAlias {
            models = discover_config_library_models_in_roots(&roots);
        }
        if models.is_empty() {
            models = fetch_live_upstream_models(&base_url, &auth_token);
        }
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            "environment",
            models,
        ));
    }

    // 2. Discover from Claude Desktop's applied configLibrary (`Claude-3p/configLibrary/<appliedId>.json`)
    if let Some((base_url, auth_token, mut models, source_name)) = discover_config_library(&roots) {
        let is_ccs_token = auth_token.starts_with("ccs-");
        let is_ccs_target = is_cc_switch_endpoint(&base_url);
        let reachable = is_gateway_url_reachable(&base_url);

        // Internal ccs-* tokens are only valid when communicating directly with a running
        // CC Switch proxy (loopback, Tailnet /claude-desktop, or port 15721). If the endpoint is non-CCS
        // or the proxy is offline, forwarding a ccs-* token causes 401 "Claude Desktop
        // gateway authentication failed". In that case, bypass directly to the live upstream provider.
        let orphan_ccs = is_ccs_token && (!is_ccs_target || !reachable);
        let should_bypass = !reachable || orphan_ccs;

        if should_bypass
            && let Some(ref cc) = cc_desktop
            && let Some(ref upstream_url) = cc.upstream_base_url
            && let Some(ref upstream_token) = cc.upstream_auth_token
            && is_token_compatible_with_endpoint(upstream_url, upstream_token)
            && is_gateway_url_reachable(upstream_url)
        {
            let mut upstream_models = cc.upstream_models.clone();
            if upstream_models.is_empty() {
                upstream_models = fetch_live_upstream_models(upstream_url, upstream_token);
            }
            let profile = ClaudeGatewayProfile::new(
                upstream_url.clone(),
                upstream_token.clone(),
                format!("cc-switch:upstream:{}", cc.name),
                upstream_models,
            );
            // This branch explicitly bypasses the local route translator. Route
            // aliases are only meaningful when requests actually traverse CCS.
            return Some(profile.with_model_namespace(GatewayModelNamespace::Upstream));
        }

        // Only adopt configLibrary directly if the endpoint is reachable and not an orphaned ccs-* token
        if reachable && !orphan_ccs {
            let namespace = ClaudeGatewayProfile::namespace_for_endpoint(&base_url, &auth_token);
            if models.is_empty() {
                if namespace == GatewayModelNamespace::RouteAlias {
                    if let Some(ref cc) = cc_desktop {
                        models = cc.models.clone();
                    }
                } else if let Some(ref cc) = cc_desktop
                    && cc_switch_manages_live_url(cc, Some(&base_url))
                    && !cc.upstream_models.is_empty()
                {
                    models = cc.upstream_models.clone();
                } else {
                    models = fetch_live_upstream_models(&base_url, &auth_token);
                }
            }
            return Some(ClaudeGatewayProfile::new(
                base_url,
                auth_token,
                source_name,
                models,
            ));
        }
    }

    // 3. Discover from unexpired, reachable host-creds-*.json (active local proxy loopback)
    if let Some((proxy_url, proxy_token, source_name)) = discover_host_creds(&roots) {
        let namespace = ClaudeGatewayProfile::namespace_for_endpoint(&proxy_url, &proxy_token);
        let mut models = if namespace == GatewayModelNamespace::RouteAlias {
            if let Some(ref cc) = cc_desktop
                && !cc.models.is_empty()
            {
                cc.models.clone()
            } else {
                discover_config_library_models_in_roots(&roots)
            }
        } else if let Some(ref cc) = cc_desktop
            && !cc.upstream_models.is_empty()
        {
            cc.upstream_models.clone()
        } else {
            fetch_live_upstream_models(&proxy_url, &proxy_token)
        };
        if models.is_empty()
            && let Some(ref cc) = cc_desktop
            && let Some(ref upstream_url) = cc.upstream_base_url
            && let Some(ref upstream_token) = cc.upstream_auth_token
            && is_token_compatible_with_endpoint(upstream_url, upstream_token)
            && is_gateway_url_reachable(upstream_url)
        {
            models = fetch_live_upstream_models(upstream_url, upstream_token);
        }
        return Some(ClaudeGatewayProfile::new(
            proxy_url,
            proxy_token,
            source_name,
            models,
        ));
    }

    // 4. Fallback directly to cc-switch.db upstream if local proxy is unavailable
    if let Some(cc) = cc_desktop
        && let Some(upstream_url) = cc.upstream_base_url
        && let Some(upstream_token) = cc.upstream_auth_token
        && is_token_compatible_with_endpoint(&upstream_url, &upstream_token)
        && is_gateway_url_reachable(&upstream_url)
    {
        // Direct upstream connection bypasses CC Switch's local route translator, so prefer
        // upstream target model IDs rather than local route aliases.
        let mut models = cc.upstream_models;
        if models.is_empty() {
            models = fetch_live_upstream_models(&upstream_url, &upstream_token);
        }
        return Some(
            ClaudeGatewayProfile::new(
                upstream_url,
                upstream_token,
                format!("cc-switch:upstream:{}", cc.name),
                models,
            )
            .with_model_namespace(GatewayModelNamespace::Upstream),
        );
    }

    // 5. Magpie or Claude settings fallback
    if let Some((base_url, auth_token, models, source_name)) = discover_magpie_provider()
        && is_gateway_url_reachable(&base_url)
    {
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            source_name,
            models,
        ));
    }

    if let Some((base_url, auth_token, models, source_name)) = discover_claude_settings_env()
        && is_gateway_url_reachable(&base_url)
    {
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            source_name,
            models,
        ));
    }

    None
}

/// Discovers active Claude gateway configuration across desktop or CLI profiles.
pub fn discover_claude_gateway() -> Option<ClaudeGatewayProfile> {
    discover_claude_desktop_gateway().or_else(discover_claude_code_gateway)
}

/// Discovers gateway env vars directly from `~/.claude/settings.json`.
pub fn discover_claude_settings_env() -> Option<(String, String, Vec<ClaudeGatewayModel>, String)> {
    let home = crate::collectors::home_dir()?;
    let path = home.join(".claude").join("settings.json");
    if !path.is_file() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let val: Value = serde_json::from_str(&content).ok()?;
    let env_obj = val.get("env")?.as_object()?;
    let base_url = env_obj
        .get("ANTHROPIC_BASE_URL")?
        .as_str()?
        .trim()
        .to_string();
    if base_url.is_empty() {
        return None;
    }
    let auth_token = env_obj
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| env_obj.get("ANTHROPIC_API_KEY"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if !is_token_compatible_with_endpoint(&base_url, &auth_token)
        || !is_gateway_url_reachable(&base_url)
    {
        return None;
    }
    let live_models = fetch_live_upstream_models(&base_url, &auth_token);
    Some((
        base_url,
        auth_token,
        live_models,
        "claude-settings".to_string(),
    ))
}

/// Discovers active gateway from magpie configuration (`~/.config/magpie/providers.json` or `%APPDATA%/magpie/providers.json`).
pub fn discover_magpie_provider() -> Option<(String, String, Vec<ClaudeGatewayModel>, String)> {
    let mut candidate_paths = Vec::new();
    #[cfg(windows)]
    {
        if let Some(appdata) = env::var_os("APPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
        {
            candidate_paths.push(appdata.join("magpie").join("providers.json"));
        }
    }
    if let Some(home) = crate::collectors::home_dir() {
        candidate_paths.push(home.join(".config").join("magpie").join("providers.json"));
    }

    for path in candidate_paths {
        if !path.is_file() {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        let Some(providers) = val.as_array() else {
            continue;
        };
        for p in providers {
            let key = p.get("key").and_then(Value::as_str).unwrap_or("").trim();
            let name = p.get("name").and_then(Value::as_str).unwrap_or("").trim();
            let anthropic_base = p
                .get("anthropic")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();

            let (base_url, auth_token) = if !anthropic_base.is_empty() {
                if !is_token_compatible_with_endpoint(anthropic_base, key) {
                    continue;
                }
                (anthropic_base.to_string(), key.to_string())
            } else {
                let local_url = "http://127.0.0.1:3425/v1";
                let token = if key.is_empty() {
                    "magpie"
                } else if is_token_compatible_with_endpoint(local_url, key) {
                    key
                } else {
                    continue;
                };
                (local_url.to_string(), token.to_string())
            };

            if !is_gateway_url_reachable(&base_url) {
                continue;
            }
            let live_models = fetch_live_upstream_models(&base_url, &auth_token);
            let display_name = if name.is_empty() {
                "magpie".to_string()
            } else {
                format!("magpie:{name}")
            };
            return Some((base_url, auth_token, live_models, display_name));
        }
    }
    None
}
