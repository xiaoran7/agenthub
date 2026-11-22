use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::claude_gateway::{
    CcSwitchActiveProvider, cc_switch_manages_live_url, discover_cc_switch_provider_for_app,
    discover_host_creds, gateway_search_roots, inspect_applied_config_library,
    is_cc_switch_default_placeholder, parse_codex_toml_route,
};

use super::*;

const MAX_VISIBLE_MODELS: usize = 3;

/// Formats an endpoint URL for tray display while stripping userinfo, query
/// parameters, and fragments so credentials can never appear in the card.
pub(crate) fn sanitize_endpoint_display(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "官方默认端点".to_string();
    }
    if let Ok(mut parsed) = url::Url::parse(trimmed) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_query(None);
        parsed.set_fragment(None);
        return parsed.to_string().trim_end_matches('/').to_string();
    }
    let clean = trimmed
        .split(['?', '#'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    if clean.contains('@') {
        clean.rsplit('@').next().unwrap_or(clean).to_string()
    } else {
        clean.to_string()
    }
}

fn cap_models(raw: Vec<String>) -> (Vec<String>, usize) {
    let mut unique = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_ascii_lowercase()) {
            unique.push(trimmed.to_string());
        }
    }
    let total = unique.len();
    if total > MAX_VISIBLE_MODELS {
        unique.truncate(MAX_VISIBLE_MODELS);
    }
    let extra = total.saturating_sub(unique.len());
    (unique, extra)
}

fn models_from_cc_switch(cc: &CcSwitchActiveProvider) -> Vec<String> {
    let src = if !cc.models.is_empty() {
        &cc.models
    } else {
        &cc.upstream_models
    };
    src.iter().map(|m| m.name.clone()).collect()
}

fn console_target(is_routed: bool) -> (usize, String) {
    if is_routed {
        (ACTION_OPEN_SETTINGS, "在主控台配置网关".to_string())
    } else {
        (ACTION_OPEN_AGENTS, "在主控台查看智能体".to_string())
    }
}

fn env_non_empty(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        std::env::var(k)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

fn read_json_bom_safe(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(raw.strip_prefix('\u{feff}').unwrap_or(&raw)).ok()
}

fn json_string_at(val: &Value, pointers: &[&str]) -> Option<String> {
    pointers.iter().find_map(|ptr| {
        val.pointer(ptr)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

fn make_detail(
    endpoint: String,
    source: impl Into<String>,
    raw_models: Vec<String>,
    console_action_id: usize,
    console_label: String,
) -> TrayAgentDetail {
    let (models, extra_model_count) = cap_models(raw_models);
    TrayAgentDetail {
        endpoint,
        source: source.into(),
        models,
        extra_model_count,
        console_action_id,
        console_label,
    }
}

fn cc_managed_detail(
    cc: Option<&CcSwitchActiveProvider>,
    live_url: Option<&str>,
    fallback_endpoint: &str,
    source: impl Into<String>,
    fallback_models: Vec<String>,
    console_action_id: usize,
    console_label: &str,
) -> Option<TrayAgentDetail> {
    let cc = cc.filter(|c| {
        !is_cc_switch_default_placeholder(c) && cc_switch_manages_live_url(c, live_url)
    })?;
    let endpoint = cc
        .upstream_base_url
        .as_deref()
        .map(sanitize_endpoint_display)
        .or_else(|| live_url.map(sanitize_endpoint_display))
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| fallback_endpoint.to_string());
    let mut raw_models = models_from_cc_switch(cc);
    if raw_models.is_empty() {
        raw_models = fallback_models;
    }
    Some(make_detail(
        endpoint,
        source,
        raw_models,
        console_action_id,
        console_label.to_string(),
    ))
}

pub(crate) fn inspect_agent_detail_local(agent_id: &str, is_routed: bool) -> TrayAgentDetail {
    let home = crate::collectors::home_dir();
    let roots = gateway_search_roots();
    let cc = match agent_id {
        "claude_code" => discover_cc_switch_provider_for_app("claude"),
        "claude_desktop" => discover_cc_switch_provider_for_app("claude-desktop"),
        "codex" => discover_cc_switch_provider_for_app("codex"),
        "antigravity" => discover_cc_switch_provider_for_app("antigravity"),
        other => discover_cc_switch_provider_for_app(other).or_else(|| {
            let dashed = other.replace('_', "-");
            (dashed != other)
                .then(|| discover_cc_switch_provider_for_app(&dashed))
                .flatten()
        }),
    };
    let env_base_url = match agent_id {
        "claude_code" => env_non_empty(&["ANTHROPIC_BASE_URL"]),
        "codex" => env_non_empty(&["OPENAI_BASE_URL"]),
        _ => None,
    };
    inspect_agent_detail_from_sources_with_env(
        agent_id,
        is_routed,
        home.as_deref(),
        &roots,
        cc.as_ref(),
        None,
        env_base_url.as_deref(),
    )
}

#[cfg(test)]
pub(crate) fn inspect_agent_detail_from_sources(
    agent_id: &str,
    is_routed: bool,
    home: Option<&Path>,
    roots: &[PathBuf],
    cc: Option<&CcSwitchActiveProvider>,
    zcode_providers: Option<&[Value]>,
) -> TrayAgentDetail {
    inspect_agent_detail_from_sources_with_env(
        agent_id,
        is_routed,
        home,
        roots,
        cc,
        zcode_providers,
        None,
    )
}

fn inspect_agent_detail_from_sources_with_env(
    agent_id: &str,
    is_routed: bool,
    home: Option<&Path>,
    roots: &[PathBuf],
    cc: Option<&CcSwitchActiveProvider>,
    _zcode_providers: Option<&[Value]>,
    env_base_url: Option<&str>,
) -> TrayAgentDetail {
    let (console_action_id, console_label) = console_target(is_routed);
    match agent_id {
        "claude_code" => {
            inspect_claude_code(home, cc, env_base_url, console_action_id, console_label)
        }
        "claude_desktop" => inspect_claude_desktop(roots, cc, console_action_id, console_label),
        "codex" => inspect_codex(home, cc, env_base_url, console_action_id, console_label),
        "antigravity" => inspect_antigravity(home, cc, console_action_id, console_label),
        _ => inspect_dynamic_agent(agent_id, home, cc, console_action_id, console_label),
    }
}

fn inspect_claude_code(
    home: Option<&Path>,
    cc: Option<&CcSwitchActiveProvider>,
    env_base_url: Option<&str>,
    console_action_id: usize,
    console_label: String,
) -> TrayAgentDetail {
    let settings_val =
        home.and_then(|h| read_json_bom_safe(&h.join(".claude").join("settings.json")));
    let env_obj = settings_val
        .as_ref()
        .and_then(|v| v.get("env"))
        .and_then(Value::as_object);
    let settings_url = env_obj
        .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut settings_models = Vec::new();
    if let Some(e) = env_obj {
        for key in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
        ] {
            if let Some(m) = e.get(key).and_then(Value::as_str) {
                settings_models.push(m.to_string());
            }
        }
    }

    let live_url = env_base_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(settings_url.as_deref());

    if let Some(detail) = cc_managed_detail(
        cc,
        live_url,
        "CC Switch 本地路由",
        "CC Switch (cc-switch.db)",
        settings_models.clone(),
        console_action_id,
        &console_label,
    ) {
        return detail;
    }

    if env_obj.is_some() || env_base_url.is_some() {
        let endpoint = live_url
            .map(sanitize_endpoint_display)
            .unwrap_or_else(|| "https://api.anthropic.com".to_string());
        let source = if env_base_url.is_some() {
            "ANTHROPIC_BASE_URL"
        } else {
            "~/.claude/settings.json"
        };
        return make_detail(
            endpoint,
            source,
            settings_models,
            console_action_id,
            console_label,
        );
    }

    make_detail(
        "https://api.anthropic.com".to_string(),
        "Anthropic 官方默认",
        Vec::new(),
        console_action_id,
        console_label,
    )
}

fn inspect_claude_desktop(
    roots: &[PathBuf],
    cc: Option<&CcSwitchActiveProvider>,
    console_action_id: usize,
    console_label: String,
) -> TrayAgentDetail {
    let lib = inspect_applied_config_library(roots);
    let host_creds = if lib.is_none() {
        discover_host_creds(roots)
    } else {
        None
    };
    let live_url = lib
        .as_ref()
        .and_then(|s| s.base_url.as_deref())
        .or_else(|| host_creds.as_ref().map(|(u, _, _)| u.as_str()));
    let lib_models = lib
        .as_ref()
        .map(|s| s.models.iter().map(|m| m.name.clone()).collect())
        .unwrap_or_default();

    if let Some(detail) = cc_managed_detail(
        cc,
        live_url,
        "CC Switch 本地路由",
        "CC Switch + Claude-3p",
        lib_models,
        console_action_id,
        &console_label,
    ) {
        return detail;
    }

    if let Some(lib_summary) = lib {
        let endpoint = lib_summary
            .base_url
            .as_deref()
            .map(sanitize_endpoint_display)
            .unwrap_or_else(|| "https://claude.ai".to_string());
        let raw_models = lib_summary.models.into_iter().map(|m| m.name).collect();
        return make_detail(
            endpoint,
            lib_summary.source,
            raw_models,
            console_action_id,
            console_label,
        );
    }

    if let Some((url, _, source)) = host_creds {
        return make_detail(
            sanitize_endpoint_display(&url),
            source,
            Vec::new(),
            console_action_id,
            console_label,
        );
    }

    make_detail(
        "https://claude.ai".to_string(),
        "Claude Desktop 官方",
        Vec::new(),
        console_action_id,
        console_label,
    )
}

fn inspect_codex(
    home: Option<&Path>,
    cc: Option<&CcSwitchActiveProvider>,
    env_base_url: Option<&str>,
    console_action_id: usize,
    console_label: String,
) -> TrayAgentDetail {
    let toml_route = home
        .and_then(|h| fs::read_to_string(h.join(".codex").join("config.toml")).ok())
        .map(|s| parse_codex_toml_route(&s));
    let live_url = env_base_url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            toml_route
                .as_ref()
                .and_then(|r| r.active_base_url.as_deref())
        });
    let toml_models: Vec<String> = toml_route
        .as_ref()
        .and_then(|r| r.model.clone())
        .into_iter()
        .collect();

    if let Some(detail) = cc_managed_detail(
        cc,
        live_url,
        "https://api.openai.com",
        "CC Switch (cc-switch.db)",
        toml_models.clone(),
        console_action_id,
        &console_label,
    ) {
        return detail;
    }

    if env_base_url.is_some() || toml_route.is_some() {
        let endpoint = live_url
            .map(sanitize_endpoint_display)
            .unwrap_or_else(|| "https://api.openai.com".to_string());
        let source = if env_base_url.is_some() {
            "OPENAI_BASE_URL"
        } else {
            "~/.codex/config.toml"
        };
        return make_detail(
            endpoint,
            source,
            toml_models,
            console_action_id,
            console_label,
        );
    }

    make_detail(
        "https://api.openai.com".to_string(),
        "OpenAI 官方默认",
        Vec::new(),
        console_action_id,
        console_label,
    )
}

fn inspect_antigravity(
    home: Option<&Path>,
    cc: Option<&CcSwitchActiveProvider>,
    console_action_id: usize,
    console_label: String,
) -> TrayAgentDetail {
    let cfg = home
        .map(|h| crate::collectors::antigravity::load_antigravity_local_config_from(h, None))
        .unwrap_or_default();
    if let Some(detail) = cc_managed_detail(
        cc,
        None,
        "CC Switch 本地路由",
        "CC Switch (cc-switch.db)",
        cfg.configured_models.clone(),
        console_action_id,
        &console_label,
    ) {
        return detail;
    }
    let endpoint = cfg
        .active_daemon
        .as_ref()
        .and_then(|d| {
            d.https_port
                .map(|p| format!("https://127.0.0.1:{p}"))
                .or_else(|| d.http_port.map(|p| format!("http://127.0.0.1:{p}")))
        })
        .unwrap_or_else(|| "https://generativelanguage.googleapis.com".to_string());
    let source = match cfg.active_daemon.as_ref() {
        Some(d) => d.ls_version.as_deref().map_or_else(
            || "Antigravity Daemon (hub_*.json)".to_string(),
            |v| format!("Antigravity Daemon ({v})"),
        ),
        None if cfg.resolved_model.is_some() || cfg.enabled_plugins_count > 0 => {
            "~/.gemini/config/config.json".to_string()
        }
        None => "Google Antigravity 默认".to_string(),
    };
    make_detail(
        endpoint,
        source,
        cfg.configured_models,
        console_action_id,
        console_label,
    )
}

fn inspect_dynamic_agent(
    _agent_id: &str,
    _home: Option<&Path>,
    cc: Option<&CcSwitchActiveProvider>,
    console_action_id: usize,
    console_label: String,
) -> TrayAgentDetail {
    if let Some(c) = cc {
        let src = format!("CC Switch ({})", c.app_type);
        if let Some(detail) = cc_managed_detail(
            Some(c),
            None,
            "CC Switch 本地路由",
            src,
            Vec::new(),
            console_action_id,
            &console_label,
        ) {
            return detail;
        }
    }

    make_detail(
        "本地默认配置".to_string(),
        "AgentHub 自动发现",
        Vec::new(),
        console_action_id,
        console_label,
    )
}
