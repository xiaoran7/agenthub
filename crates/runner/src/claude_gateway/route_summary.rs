use std::collections::HashSet;

use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct RouteEnvOverrides {
    pub anthropic_base_url: Option<String>,
    pub openai_base_url: Option<String>,
    pub gemini_base_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct InstalledAgentFlags {
    pub claude_code: bool,
    pub codex: bool,
    pub antigravity: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RouteDiscoveryContext {
    pub home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub local_appdata: Option<PathBuf>,
    pub claude_roots: Vec<PathBuf>,
    pub cc_providers: Vec<CcSwitchActiveProvider>,
    pub installed: InstalledAgentFlags,
    pub env: RouteEnvOverrides,
}

impl RouteDiscoveryContext {
    fn from_local_env() -> Self {
        let home = crate::collectors::home_dir();
        let appdata = env::var_os("APPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let local_appdata = env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let env_non_empty = |key: &str| {
            env::var(key)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Self {
            claude_roots: gateway_search_roots(),
            cc_providers: discover_all_cc_switch_active_providers(),
            installed: InstalledAgentFlags {
                claude_code: crate::web_server::device_info::find_executable(
                    "claude",
                    &["AppData/Roaming/npm/claude.cmd", ".local/bin/claude"],
                )
                .0,
                codex: crate::web_server::device_info::find_executable(
                    "codex",
                    &["AppData/Roaming/npm/codex.cmd", ".local/bin/codex"],
                )
                .0,
                antigravity: crate::collectors::antigravity::check_antigravity_installed_at(
                    home.as_deref(),
                )
                .0,
            },
            env: RouteEnvOverrides {
                anthropic_base_url: env_non_empty("ANTHROPIC_BASE_URL"),
                openai_base_url: env_non_empty("OPENAI_BASE_URL"),
                gemini_base_url: env_non_empty("GOOGLE_GEMINI_BASE_URL")
                    .or_else(|| env_non_empty("GEMINI_BASE_URL"))
                    .or_else(|| env_non_empty("GOOGLE_BASE_URL")),
            },
            home,
            appdata,
            local_appdata,
        }
    }
}

/// Reports whether client route summaries have been discovered at least once.
pub fn is_route_summaries_warm() -> bool {
    #[cfg(not(test))]
    if let Ok(guard) = ROUTE_SUMMARIES_CACHE.read() {
        return guard.is_some();
    }
    true
}

/// Collects real provider + routing mode summaries across local Agent clients.
pub fn discover_all_client_route_summaries() -> Vec<ClientRouteSummary> {
    #[cfg(not(test))]
    if let Ok(guard) = ROUTE_SUMMARIES_CACHE.read()
        && let Some((cached_at, ref cached)) = *guard
        && cached_at.elapsed() < Duration::from_secs(2)
    {
        return cached.clone();
    }

    let ctx = RouteDiscoveryContext::from_local_env();
    let summaries = discover_client_route_summaries_from(&ctx);

    #[cfg(not(test))]
    if let Ok(mut guard) = ROUTE_SUMMARIES_CACHE.write() {
        *guard = Some((Instant::now(), summaries.clone()));
    }

    summaries
}

pub(crate) fn discover_client_route_summaries_from(
    ctx: &RouteDiscoveryContext,
) -> Vec<ClientRouteSummary> {
    let mut summaries = Vec::new();
    let mut covered_ids = HashSet::new();

    for summary in [
        discover_claude_code_summary(ctx),
        discover_claude_desktop_summary(ctx),
        discover_codex_summary(ctx),
        discover_antigravity_summary(ctx),
    ]
    .into_iter()
    .flatten()
    {
        covered_ids.insert(summary.agent_id.clone());
        summaries.push(summary);
    }

    // Stage B: dynamically admit any additional CC Switch app_type under active takeover.
    for cc in &ctx.cc_providers {
        if !cc.proxy_takeover {
            continue;
        }
        let agent_id = canonical_agent_id(&cc.app_type);
        if agent_id.is_empty() || !covered_ids.insert(agent_id.clone()) {
            continue;
        }
        let provider_name = clean_cc_name(cc, cc.upstream_base_url.as_deref(), ctx, "CC Switch");
        summaries.push(ClientRouteSummary {
            agent_id,
            title: format_dynamic_title(&cc.app_type),
            provider_name,
            is_routed: true,
        });
    }

    summaries
}

fn discover_claude_code_summary(ctx: &RouteDiscoveryContext) -> Option<ClientRouteSummary> {
    let settings_path = ctx
        .home
        .as_ref()
        .map(|h| h.join(".claude").join("settings.json"));
    let settings_exists = settings_path.as_ref().is_some_and(|p| p.is_file());
    let settings_url = settings_path
        .as_deref()
        .and_then(|p| read_json_url_from_paths(p, &["/env/ANTHROPIC_BASE_URL"]));
    let live_url = ctx
        .env
        .anthropic_base_url
        .as_deref()
        .or(settings_url.as_deref());
    let has_projects = ctx
        .home
        .as_ref()
        .is_some_and(|h| has_non_empty_dir(&h.join(".claude").join("projects")));
    let cc = find_cc(&ctx.cc_providers, &["claude", "claude_code"]);

    let present = live_url.is_some()
        || ctx.installed.claude_code
        || settings_exists
        || has_projects
        || cc.is_some_and(|c| c.proxy_takeover);
    if !present {
        return None;
    }

    let (provider_name, is_routed) = resolve_url_or_cc(
        "claude",
        live_url,
        cc,
        ctx,
        "Anthropic Official",
        "Custom Gateway",
        &[],
    );
    Some(ClientRouteSummary {
        agent_id: "claude_code".to_string(),
        title: "Claude".to_string(),
        provider_name,
        is_routed,
    })
}

fn discover_claude_desktop_summary(ctx: &RouteDiscoveryContext) -> Option<ClientRouteSummary> {
    let applied = inspect_applied_config_library(&ctx.claude_roots);
    let host_creds = discover_host_creds(&ctx.claude_roots);
    let has_desktop_dir = ctx.claude_roots.iter().any(|r| {
        r.join("claude_desktop_config.json").is_file() || r.join("configLibrary").is_dir()
    }) || ctx
        .local_appdata
        .as_ref()
        .is_some_and(|l| l.join("AIClaude").is_dir())
        || ctx
            .home
            .as_ref()
            .is_some_and(|h| h.join("AppData").join("Local").join("AIClaude").is_dir());
    let cc = find_cc(&ctx.cc_providers, &["claude-desktop", "claude_desktop"]);

    let present = applied.is_some()
        || host_creds.is_some()
        || has_desktop_dir
        || cc.is_some_and(|c| c.proxy_takeover);
    if !present {
        return None;
    }

    let live_url = applied
        .as_ref()
        .and_then(|a| a.base_url.as_deref())
        .or_else(|| host_creds.as_ref().map(|(u, _, _)| u.as_str()));
    let lib_name = applied
        .as_ref()
        .and_then(|a| a.display_name.as_deref())
        .filter(|n| !n.eq_ignore_ascii_case("default"));

    let (provider_name, is_routed) = resolve_url_or_cc(
        "claude-desktop",
        live_url,
        cc,
        ctx,
        "Anthropic Official",
        "Claude Gateway",
        &[lib_name],
    );
    Some(ClientRouteSummary {
        agent_id: "claude_desktop".to_string(),
        title: "Claude Desktop".to_string(),
        provider_name,
        is_routed,
    })
}

fn discover_codex_summary(ctx: &RouteDiscoveryContext) -> Option<ClientRouteSummary> {
    let toml_path = ctx
        .home
        .as_ref()
        .map(|h| h.join(".codex").join("config.toml"));
    let toml_exists = toml_path.as_ref().is_some_and(|p| p.is_file());
    let toml_route = toml_path
        .as_deref()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| parse_codex_toml_route(&s))
        .unwrap_or_default();
    let has_sessions = ctx
        .home
        .as_ref()
        .is_some_and(|h| has_non_empty_dir(&h.join(".codex").join("sessions")));
    let cc = find_cc(&ctx.cc_providers, &["codex"]);

    let live_url = ctx
        .env
        .openai_base_url
        .as_deref()
        .or(toml_route.active_base_url.as_deref());
    let custom_provider = toml_route
        .active_provider_key
        .as_deref()
        .filter(|k| !is_official_or_default_provider_name(k, ""));

    let present = live_url.is_some()
        || custom_provider.is_some()
        || ctx.installed.codex
        || toml_exists
        || has_sessions
        || cc.is_some_and(|c| c.proxy_takeover);
    if !present {
        return None;
    }

    let toml_label = toml_route
        .active_provider_name
        .as_deref()
        .or(toml_route.active_provider_key.as_deref())
        .filter(|n| !is_official_or_default_provider_name(n, ""));

    let (provider_name, is_routed) = if let Some(url) = live_url {
        if let Some(c) = cc.filter(|c| {
            cc_switch_manages_live_url(c, Some(url))
                && (!is_official_provider_url("codex", url) || c.proxy_takeover)
        }) {
            (clean_cc_name(c, Some(url), ctx, "CC Switch"), c.is_routed)
        } else if is_official_provider_url("codex", url) {
            ("OpenAI Official".to_string(), false)
        } else {
            let label =
                discover_local_gateway_label(url, ctx.home.as_deref(), ctx.appdata.as_deref())
                    .or_else(|| toml_label.map(str::to_string))
                    .or_else(|| host_label_from_url(url))
                    .unwrap_or_else(|| "Custom Gateway".to_string());
            (label, true)
        }
    } else if let Some(c) = cc.filter(|c| c.proxy_takeover) {
        (clean_cc_name(c, None, ctx, "CC Switch"), c.is_routed)
    } else if let Some(key) = custom_provider {
        let name = toml_route
            .active_provider_name
            .as_deref()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(key)
            .to_string();
        (name, true)
    } else {
        ("OpenAI Official".to_string(), false)
    };

    Some(ClientRouteSummary {
        agent_id: "codex".to_string(),
        title: "Codex".to_string(),
        provider_name,
        is_routed,
    })
}

fn discover_antigravity_summary(ctx: &RouteDiscoveryContext) -> Option<ClientRouteSummary> {
    let cfg = ctx
        .home
        .as_deref()
        .map(|h| crate::collectors::antigravity::load_antigravity_local_config_from(h, None))
        .unwrap_or_default();
    let cc = find_cc(&ctx.cc_providers, &["antigravity"]);

    let present = ctx.installed.antigravity
        || cfg.active_daemon.is_some()
        || cfg.installation_uuid.is_some()
        || cc.is_some_and(|c| c.proxy_takeover);
    if !present {
        return None;
    }

    let (provider_name, is_routed) =
        if let Some(c) = cc.filter(|c| cc_switch_manages_live_url(c, None)) {
            (clean_cc_name(c, None, ctx, "Google Official"), c.is_routed)
        } else {
            ("Google Official".to_string(), false)
        };
    Some(ClientRouteSummary {
        agent_id: "antigravity".to_string(),
        title: "Antigravity".to_string(),
        provider_name,
        is_routed,
    })
}

/// Resolves a client's provider label with priority:
/// 1) CC Switch only when actively managing `live_url` (or `proxy_takeover == true`);
/// 2) Preferred explicit name (such as `configLibrary` entry name);
/// 3) Local gateway discovery (`magpie/gateway.json`);
/// 4) URL host label / generic routed fallback.
fn resolve_url_or_cc(
    app_type: &str,
    live_url: Option<&str>,
    cc: Option<&CcSwitchActiveProvider>,
    ctx: &RouteDiscoveryContext,
    official_label: &str,
    routed_fallback: &str,
    preferred_names: &[Option<&str>],
) -> (String, bool) {
    if let Some(url) = live_url.map(str::trim).filter(|u| !u.is_empty()) {
        if let Some(c) = cc.filter(|c| {
            cc_switch_manages_live_url(c, Some(url))
                && (!is_official_provider_url(app_type, url) || c.proxy_takeover)
        }) {
            return (
                clean_cc_name(c, Some(url), ctx, routed_fallback),
                c.is_routed,
            );
        }
        if is_official_provider_url(app_type, url) {
            return (official_label.to_string(), false);
        }
        let preferred = preferred_names
            .iter()
            .flatten()
            .map(|s| s.trim())
            .find(|s| !s.is_empty())
            .map(str::to_string);
        let label = preferred
            .or_else(|| {
                discover_local_gateway_label(url, ctx.home.as_deref(), ctx.appdata.as_deref())
            })
            .or_else(|| host_label_from_url(url))
            .unwrap_or_else(|| routed_fallback.to_string());
        return (label, true);
    }

    if let Some(c) = cc.filter(|c| c.proxy_takeover) {
        return (clean_cc_name(c, None, ctx, routed_fallback), c.is_routed);
    }

    (official_label.to_string(), false)
}

fn clean_cc_name(
    cc: &CcSwitchActiveProvider,
    live_url: Option<&str>,
    ctx: &RouteDiscoveryContext,
    fallback: &str,
) -> String {
    let name = cc.name.trim();
    if !name.is_empty() && !name.eq_ignore_ascii_case("default") {
        return name.to_string();
    }
    let url = live_url.or(cc.upstream_base_url.as_deref());
    url.and_then(|u| discover_local_gateway_label(u, ctx.home.as_deref(), ctx.appdata.as_deref()))
        .or_else(|| url.and_then(host_label_from_url))
        .unwrap_or_else(|| fallback.to_string())
}

fn find_cc<'a>(
    providers: &'a [CcSwitchActiveProvider],
    aliases: &[&str],
) -> Option<&'a CcSwitchActiveProvider> {
    providers.iter().find(|p| {
        aliases
            .iter()
            .any(|alias| p.app_type.eq_ignore_ascii_case(alias))
    })
}

fn canonical_agent_id(app_type: &str) -> String {
    match app_type.trim().to_ascii_lowercase().as_str() {
        "claude" | "claude_code" => "claude_code".to_string(),
        "claude-desktop" | "claude_desktop" => "claude_desktop".to_string(),
        "codex" => "codex".to_string(),
        "gemini" | "gemini_cli" => "gemini_cli".to_string(),
        "opencode" | "zcode" => "zcode".to_string(),
        "antigravity" => "antigravity".to_string(),
        "grokbuild" | "grok" | "grok_build" => "grok_build".to_string(),
        other => other.replace('-', "_"),
    }
}

fn format_dynamic_title(raw: &str) -> String {
    raw.split(['-', '_'])
        .filter(|seg| !seg.trim().is_empty())
        .map(|seg| {
            let mut chars = seg.trim().chars();
            match chars.next() {
                Some(first) => {
                    let mut out = first.to_uppercase().to_string();
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn has_non_empty_dir(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .is_some_and(|mut it| it.next().is_some())
}
