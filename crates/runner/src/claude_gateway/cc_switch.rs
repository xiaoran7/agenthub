use super::*;

/// Discovered active provider from `~/.cc-switch/cc-switch.db`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CcSwitchActiveProvider {
    pub id: String,
    pub name: String,
    pub app_type: String,
    pub upstream_base_url: Option<String>,
    pub upstream_auth_token: Option<String>,
    pub models: Vec<ClaudeGatewayModel>,
    #[serde(default)]
    pub upstream_models: Vec<ClaudeGatewayModel>,
    #[serde(default)]
    pub is_routed: bool,
    #[serde(default)]
    pub proxy_takeover: bool,
}

/// Summary of one local agent client's active provider & routing mode (for tray / diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRouteSummary {
    pub agent_id: String,
    pub title: String,
    pub provider_name: String,
    pub is_routed: bool,
}

impl ClientRouteSummary {
    pub fn subtitle(&self) -> String {
        let mode = if self.is_routed { "路由" } else { "直连" };
        format!("{} · {mode}", self.provider_name)
    }
}

/// Discovered active gateway configuration for OpenAI Codex CLI (`codex`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexGatewayProfile {
    pub base_url: String,
    pub api_key: String,
    pub source: String,
    pub models: Vec<ClaudeGatewayModel>,
}

impl CodexGatewayProfile {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        source: impl Into<String>,
        models: Vec<ClaudeGatewayModel>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            source: source.into(),
            models,
        }
    }

    pub fn inject_into_command(&self, command: &mut Command, model_override: &mut Option<String>) {
        self.inject_into_command_with_session(command, model_override, None);
    }

    pub fn inject_into_command_with_session(
        &self,
        command: &mut Command,
        model_override: &mut Option<String>,
        session_id: Option<&str>,
    ) {
        command.env("OPENAI_BASE_URL", &self.base_url);
        command.env("OPENAI_API_KEY", &self.api_key);

        if let Some(sid) = session_id.map(str::trim).filter(|s| !s.is_empty()) {
            let clean_sid = sid.strip_prefix("codex_").unwrap_or(sid);
            command.env("X_MAGPIE_SESSION", clean_sid);
            command.env("X_CONVERSATION_ID", clean_sid);
        }

        if let Some(m) = model_override.as_deref() {
            if let Some(matched) = self.models.iter().find(|gm| {
                gm.name.eq_ignore_ascii_case(m)
                    || gm
                        .label
                        .as_deref()
                        .is_some_and(|l| l.eq_ignore_ascii_case(m))
            }) {
                *model_override = Some(matched.name.clone());
            }
        }
    }
}

/// Discovered active gateway configuration for Google Antigravity CLI (`antigravity`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AntigravityGatewayProfile {
    pub base_url: String,
    pub api_key: String,
    pub source: String,
    pub models: Vec<ClaudeGatewayModel>,
}

impl AntigravityGatewayProfile {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        source: impl Into<String>,
        models: Vec<ClaudeGatewayModel>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            source: source.into(),
            models,
        }
    }

    pub fn inject_into_command(&self, command: &mut Command, model_override: &mut Option<String>) {
        self.inject_into_command_with_session(command, model_override, None);
    }

    pub fn inject_into_command_with_session(
        &self,
        command: &mut Command,
        model_override: &mut Option<String>,
        session_id: Option<&str>,
    ) {
        command.env("GOOGLE_GEMINI_BASE_URL", &self.base_url);
        command.env("GOOGLE_BASE_URL", &self.base_url);
        command.env("GEMINI_BASE_URL", &self.base_url);
        command.env("GEMINI_API_KEY", &self.api_key);
        command.env("GOOGLE_API_KEY", &self.api_key);

        if let Some(sid) = session_id.map(str::trim).filter(|s| !s.is_empty()) {
            let clean_sid = sid.strip_prefix("antigravity_").unwrap_or(sid);
            command.env("X_MAGPIE_SESSION", clean_sid);
            command.env("X_CONVERSATION_ID", clean_sid);
        }

        if let Some(m) = model_override.as_deref() {
            let target_model = if let Some(matched) = self.models.iter().find(|gm| {
                gm.name.eq_ignore_ascii_case(m)
                    || gm
                        .label
                        .as_deref()
                        .is_some_and(|l| l.eq_ignore_ascii_case(m))
            }) {
                *model_override = Some(matched.name.clone());
                matched.name.clone()
            } else {
                m.to_string()
            };
            command.env("GEMINI_MODEL", target_model);
        }
    }
}

/// Locates the local CC Switch SQLite database (`~/.cc-switch/cc-switch.db`).
pub fn cc_switch_db_path() -> Option<PathBuf> {
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)?;
    let p = home.join(".cc-switch").join("cc-switch.db");
    if p.is_file() { Some(p) } else { None }
}

pub(crate) fn query_cc_switch_proxy_enabled(conn: &rusqlite::Connection, app_type: &str) -> bool {
    if let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(enabled, 0), COALESCE(proxy_enabled, 0), COALESCE(live_takeover_active, 0) \
         FROM proxy_config WHERE app_type = ?1 LIMIT 1",
    ) {
        if let Ok((enabled, proxy_enabled, live_takeover)) = stmt.query_row([app_type], |row| {
            Ok((
                row.get::<_, i64>(0).unwrap_or(0),
                row.get::<_, i64>(1).unwrap_or(0),
                row.get::<_, i64>(2).unwrap_or(0),
            ))
        }) && (enabled != 0 || proxy_enabled != 0 || live_takeover != 0)
        {
            return true;
        }
    } else if let Ok(mut stmt) =
        conn.prepare("SELECT enabled FROM proxy_config WHERE app_type = ?1 LIMIT 1")
        && let Ok(enabled) = stmt.query_row([app_type], |row| row.get::<_, i64>(0))
        && enabled != 0
    {
        return true;
    }

    if let Ok(mut stmt) =
        conn.prepare("SELECT 1 FROM proxy_live_backup WHERE app_type = ?1 LIMIT 1")
        && stmt.query_row([app_type], |_| Ok(())).is_ok()
    {
        return true;
    }

    if let Ok(mut stmt) = conn.prepare("SELECT value FROM settings WHERE key = 'proxy_takeover'")
        && let Ok(val) = stmt.query_row([], |row| row.get::<_, String>(0))
    {
        return val.contains(app_type) || val == "true";
    }
    false
}

pub(crate) fn extract_toml_quoted_value(toml_text: &str, key: &str) -> Option<String> {
    for line in toml_text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let val = rest.trim();
        if let Some(stripped) = val.strip_prefix('"').and_then(|s| s.split('"').next()) {
            let clean = stripped.trim();
            if !clean.is_empty() {
                return Some(clean.to_string());
            }
        }
        if let Some(stripped) = val.strip_prefix('\'').and_then(|s| s.split('\'').next()) {
            let clean = stripped.trim();
            if !clean.is_empty() {
                return Some(clean.to_string());
            }
        }
    }
    None
}

/// Discovers the configured Claude Desktop gateway token (`claude_desktop_gateway_token`) from `cc-switch.db`.
pub fn discover_cc_switch_gateway_token() -> Option<String> {
    let db_path = cc_switch_db_path()?;
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = 'claude_desktop_gateway_token' LIMIT 1")
        .ok()?;

    let mut rows = stmt.query([]).ok()?;
    let row = rows.next().ok()??;
    let token: String = row.get(0).ok()?;
    let trimmed = token.trim();
    if is_usable_gateway_token(trimmed) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Discovers all currently active provider rows across every `app_type` in `cc-switch.db`.
pub fn discover_all_cc_switch_active_providers() -> Vec<CcSwitchActiveProvider> {
    let Some(db_path) = cc_switch_db_path() else {
        return Vec::new();
    };
    discover_all_cc_switch_active_providers_in(&db_path)
}

pub(crate) fn discover_all_cc_switch_active_providers_in(
    db_path: &Path,
) -> Vec<CcSwitchActiveProvider> {
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, name, app_type, settings_config, meta \
         FROM providers \
         WHERE is_current = 1 \
         ORDER BY app_type",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0).unwrap_or_default(),
            row.get::<_, String>(1).unwrap_or_default(),
            row.get::<_, String>(2).unwrap_or_default(),
            row.get::<_, String>(3).unwrap_or_default(),
            row.get::<_, String>(4).unwrap_or_default(),
        ))
    }) else {
        return Vec::new();
    };
    rows.flatten()
        .map(|(id, name, row_app_type, settings_str, meta_str)| {
            parse_cc_switch_provider_row(&conn, id, name, row_app_type, settings_str, meta_str)
        })
        .collect()
}

/// Discovers the active provider for a specific `app_type` (`claude`, `claude-desktop`, `codex`, `gemini`, `opencode`) from `cc-switch.db`.
pub fn discover_cc_switch_provider_for_app(app_type: &str) -> Option<CcSwitchActiveProvider> {
    let db_path = cc_switch_db_path()?;
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, app_type, settings_config, meta \
             FROM providers \
             WHERE is_current = 1 AND app_type = ?1 \
             LIMIT 1",
        )
        .ok()?;

    let mut rows = stmt.query([app_type]).ok()?;
    let row = rows.next().ok()??;

    let id: String = row.get(0).unwrap_or_default();
    let name: String = row.get(1).unwrap_or_default();
    let row_app_type: String = row.get(2).unwrap_or_default();
    let settings_str: String = row.get(3).unwrap_or_default();
    let meta_str: String = row.get(4).unwrap_or_default();
    Some(parse_cc_switch_provider_row(
        &conn,
        id,
        name,
        if row_app_type.is_empty() {
            app_type.to_string()
        } else {
            row_app_type
        },
        settings_str,
        meta_str,
    ))
}

fn parse_cc_switch_provider_row(
    conn: &rusqlite::Connection,
    id: String,
    name: String,
    row_app_type: String,
    settings_str: String,
    meta_str: String,
) -> CcSwitchActiveProvider {
    let settings: Value = serde_json::from_str(&settings_str).unwrap_or(Value::Null);
    let meta: Value = serde_json::from_str(&meta_str).unwrap_or(Value::Null);

    let env_obj = settings.get("env");
    let config_toml = settings
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let upstream_base_url = env_obj
        .and_then(|e| {
            e.get("ANTHROPIC_BASE_URL")
                .or_else(|| e.get("OPENAI_BASE_URL"))
                .or_else(|| e.get("GOOGLE_GEMINI_BASE_URL"))
                .or_else(|| e.get("GEMINI_BASE_URL"))
                .or_else(|| e.get("GOOGLE_BASE_URL"))
        })
        .or_else(|| settings.get("base_url"))
        .or_else(|| settings.get("baseUrl"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| extract_toml_quoted_value(config_toml, "base_url"));

    let upstream_auth_token = env_obj
        .and_then(|e| {
            e.get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| e.get("ANTHROPIC_API_KEY"))
                .or_else(|| e.get("OPENAI_API_KEY"))
                .or_else(|| e.get("GEMINI_API_KEY"))
                .or_else(|| e.get("GOOGLE_API_KEY"))
        })
        .or_else(|| settings.get("auth_token"))
        .or_else(|| settings.get("apiKey"))
        .or_else(|| {
            settings.get("auth").and_then(|a| {
                a.get("OPENAI_API_KEY")
                    .or_else(|| a.get("ANTHROPIC_API_KEY"))
                    .or_else(|| a.get("GEMINI_API_KEY"))
                    .or_else(|| a.get("GOOGLE_API_KEY"))
            })
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| is_usable_gateway_token(s))
        .map(str::to_string);

    let mut models = Vec::new();
    let mut upstream_models = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    let mut seen_upstream = std::collections::HashSet::new();
    let mut has_desktop_routes = false;

    // 1. Configured routes in meta.claudeDesktopModelRoutes (only for claude-desktop)
    if row_app_type == "claude-desktop"
        && let Some(routes) = meta
            .get("claudeDesktopModelRoutes")
            .and_then(Value::as_object)
    {
        for (route_name, route_val) in routes {
            if route_name.is_empty() || !seen_names.insert(route_name.to_ascii_lowercase()) {
                continue;
            }
            has_desktop_routes = true;
            let raw_model = route_val
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let target = route_val
                .get("labelOverride")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| raw_model.clone());
            let supports_1m = route_val
                .get("supports1m")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            models.push(ClaudeGatewayModel {
                name: route_name.clone(),
                label: target,
                supports_1m,
            });
            if let Some(m_id) = raw_model
                && seen_upstream.insert(m_id.to_ascii_lowercase())
            {
                upstream_models.push(ClaudeGatewayModel {
                    name: m_id,
                    label: Some(route_name.clone()),
                    supports_1m,
                });
            }
        }
    }

    // 2. Models in modelCatalog
    let catalog = meta
        .get("modelCatalog")
        .or_else(|| settings.get("modelCatalog"));
    if let Some(arr) = catalog
        .and_then(|c| c.get("models"))
        .and_then(Value::as_array)
    {
        for item in arr {
            if let Some(m_name) = item
                .get("model")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|m| !m.is_empty())
            {
                let display_name = item
                    .get("displayName")
                    .or_else(|| item.get("label"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let supports_1m = item
                    .get("supports1m")
                    .and_then(Value::as_bool)
                    .unwrap_or_else(|| m_name.to_ascii_lowercase().contains("1m"));
                if !has_desktop_routes && seen_names.insert(m_name.to_ascii_lowercase()) {
                    models.push(ClaudeGatewayModel {
                        name: m_name.to_string(),
                        label: display_name.clone(),
                        supports_1m,
                    });
                }
                if seen_upstream.insert(m_name.to_ascii_lowercase()) {
                    upstream_models.push(ClaudeGatewayModel {
                        name: m_name.to_string(),
                        label: display_name,
                        supports_1m,
                    });
                }
            }
        }
    }

    // 3. Explicit single-model keys in env / settings / TOML
    let mut single_models = Vec::new();
    if let Some(env_map) = env_obj.and_then(Value::as_object) {
        for key in [
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "GEMINI_MODEL",
            "OPENAI_MODEL",
        ] {
            if let Some(m) = env_map.get(key).and_then(Value::as_str) {
                single_models.push(m.trim().to_string());
            }
        }
    }
    if let Some(m) = settings.get("model").and_then(Value::as_str) {
        single_models.push(m.trim().to_string());
    }
    if let Some(m) = extract_toml_quoted_value(config_toml, "model") {
        single_models.push(m);
    }
    for m_name in single_models {
        if m_name.is_empty() {
            continue;
        }
        if !has_desktop_routes && seen_names.insert(m_name.to_ascii_lowercase()) {
            models.push(ClaudeGatewayModel {
                name: m_name.clone(),
                label: None,
                supports_1m: m_name.to_ascii_lowercase().contains("1m"),
            });
        }
        if seen_upstream.insert(m_name.to_ascii_lowercase()) {
            upstream_models.push(ClaudeGatewayModel {
                name: m_name,
                label: None,
                supports_1m: false,
            });
        }
    }

    let sort_key = |m: &ClaudeGatewayModel| {
        let name_lower = m.name.to_ascii_lowercase();
        if name_lower.contains("opus") {
            0
        } else if name_lower.contains("sonnet") {
            1
        } else if name_lower.contains("fable") {
            2
        } else if name_lower.contains("haiku") {
            3
        } else {
            4
        }
    };
    models.sort_by_key(sort_key);
    upstream_models.sort_by_key(sort_key);

    let proxy_takeover = query_cc_switch_proxy_enabled(conn, &row_app_type);
    let is_routed = proxy_takeover
        || has_desktop_routes
        || upstream_base_url
            .as_deref()
            .is_some_and(|u| !is_official_provider_url(&row_app_type, u))
        || !is_official_or_default_provider_name(&name, &id);

    CcSwitchActiveProvider {
        id,
        name,
        app_type: row_app_type,
        upstream_base_url,
        upstream_auth_token,
        models,
        upstream_models,
        is_routed,
        proxy_takeover,
    }
}

fn normalize_origin_host(host: &str) -> String {
    if is_loopback_host(host) {
        "__loopback__".to_string()
    } else {
        host.trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase()
    }
}

pub(crate) fn urls_share_origin(a: &str, b: &str) -> bool {
    let Some((pa, ha, port_a)) = parse_http_url(a) else {
        return false;
    };
    let Some((pb, hb, port_b)) = parse_http_url(b) else {
        return false;
    };
    pa.scheme().eq_ignore_ascii_case(pb.scheme())
        && port_a == port_b
        && normalize_origin_host(&ha) == normalize_origin_host(&hb)
}

pub(crate) fn cc_switch_manages_live_url(
    cc: &CcSwitchActiveProvider,
    live_url: Option<&str>,
) -> bool {
    if cc.proxy_takeover {
        return true;
    }
    let Some(live) = live_url.map(str::trim).filter(|u| !u.is_empty()) else {
        return false;
    };
    if is_cc_switch_endpoint(live) {
        return true;
    }
    cc.upstream_base_url
        .as_deref()
        .is_some_and(|cu| urls_share_origin(live, cu))
}

pub(crate) fn is_official_provider_url(app_type: &str, url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return true;
    }
    let Some((host, _)) = parse_url_host_port(trimmed) else {
        return false;
    };
    let h = host.to_ascii_lowercase();
    match app_type {
        "claude" | "claude-desktop" | "claude_code" | "claude_desktop" => {
            matches!(h.as_str(), "api.anthropic.com" | "claude.ai")
        }
        "codex" => matches!(h.as_str(), "api.openai.com" | "chatgpt.com"),
        "gemini" | "gemini_cli" | "antigravity" => matches!(
            h.as_str(),
            "generativelanguage.googleapis.com"
                | "aiplatform.googleapis.com"
                | "cloudcode-pa.googleapis.com"
        ),
        "opencode" | "zcode" => matches!(h.as_str(), "open.bigmodel.cn"),
        "grok" | "grokbuild" | "grok_build" => matches!(h.as_str(), "api.x.ai" | "x.ai"),
        _ => false,
    }
}

pub(crate) fn is_official_or_default_provider_name(name: &str, id: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    let i = id.trim().to_ascii_lowercase();
    let check = if !n.is_empty() {
        n.as_str()
    } else {
        i.as_str()
    };
    matches!(
        check,
        "" | "default"
            | "official"
            | "anthropic"
            | "anthropic official"
            | "openai"
            | "openai official"
            | "google"
            | "google official"
            | "gemini official"
            | "zhipu"
            | "zhipu official"
    )
}

pub(crate) fn is_cc_switch_default_placeholder(cc: &CcSwitchActiveProvider) -> bool {
    !cc.is_routed
        && is_official_or_default_provider_name(&cc.name, &cc.id)
        && cc
            .upstream_base_url
            .as_deref()
            .is_none_or(|u| is_official_provider_url(&cc.app_type, u))
}

/// Discovers the currently active Claude Desktop or Claude provider from `cc-switch.db`.
pub fn discover_cc_switch_active_provider() -> Option<CcSwitchActiveProvider> {
    discover_cc_switch_provider_for_app("claude-desktop")
        .or_else(|| discover_cc_switch_provider_for_app("claude"))
}
