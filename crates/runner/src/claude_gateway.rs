//! Automatic discovery and adaptation for third-party Claude Code gateways (e.g. CC Switch, Claude-3p).
//!
//! When a user runs Claude Code via a third-party gateway or router such as CC Switch,
//! the runtime credentials and proxy endpoints are not committed to standard user profiles or
//! global system environment variables. Instead, they are dynamically managed under
//! platform app data roots (`Claude-3p`, `Claude`, `.cc-switch`).
//!
//! Without these environment variables, `claude.exe` defaults to the official Anthropic API
//! and fails with `OAuth session expired`. Furthermore, third-party routers often map specific
//! model IDs (e.g. `claude-fable-5` -> `gemini-3.8-flash-high`); running without an explicit
//! model can cause the router to reject unknown or unmapped models with a 502/400 error.
//!
//! This module automatically detects active gateway environments, extracts valid base URLs and
//! API tokens, maps transcript/provider model IDs to configured router models, and injects
//! them when launching agent CLI subprocesses.

use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    sync::RwLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One configured model inside a third-party gateway profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeGatewayModel {
    pub name: String,
    pub label: Option<String>,
    pub supports_1m: bool,
}

/// A discovered gateway runtime environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeGatewayProfile {
    pub base_url: String,
    pub auth_token: String,
    pub source: String,
    pub models: Vec<ClaudeGatewayModel>,
}

impl ClaudeGatewayProfile {
    pub fn new(
        base_url: impl Into<String>,
        auth_token: impl Into<String>,
        source: impl Into<String>,
        models: Vec<ClaudeGatewayModel>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            auth_token: auth_token.into(),
            source: source.into(),
            models,
        }
    }

    /// Resolves an input model name to a model name understood by the gateway router.
    ///
    /// Router backends often report underlying model identifiers (such as `gemini-3.8-flash`
    /// or `glm-4`) into transcripts, but the router itself requires Claude-compatible aliases
    /// (`claude-fable-5`, `claude-opus-5`, etc.). This function translates:
    /// 1. Exact match with router model name (`name`).
    /// 2. Exact or prefix match with backend label override (`label`).
    /// 3. Family keyword heuristics (`gemini`, `fable`, `opus`, `sonnet`, `haiku`).
    /// 4. Fallback to the gateway's primary configured model.
    pub fn resolve_model(&self, requested: Option<&str>) -> Option<String> {
        let requested = requested.map(str::trim).filter(|s| !s.is_empty());

        if self.models.is_empty() {
            return requested.map(str::to_owned);
        }

        let Some(req) = requested else {
            // No model requested: default to the gateway's flagship configured model (opus > sonnet > first).
            return self
                .models
                .iter()
                .find(|m| m.name.to_ascii_lowercase().contains("opus"))
                .or_else(|| {
                    self.models
                        .iter()
                        .find(|m| m.name.to_ascii_lowercase().contains("sonnet"))
                })
                .or_else(|| self.models.first())
                .map(|m| m.name.clone());
        };

        // 1. Exact name match (case-insensitive)
        if let Some(m) = self
            .models
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(req))
        {
            return Some(m.name.clone());
        }

        // 2. Exact label match (case-insensitive)
        if let Some(m) = self.models.iter().find(|m| {
            m.label
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(req))
        }) {
            return Some(m.name.clone());
        }

        // 3. Prefix / Substring match on label (e.g. transcript says "gemini-3.8-flash"
        // and labelOverride is "gemini-3.8-flash-high" or vice-versa)
        let req_lower = req.to_ascii_lowercase();
        for m in &self.models {
            if let Some(label) = &m.label {
                let label_lower = label.to_ascii_lowercase();
                if label_lower.starts_with(&req_lower) || req_lower.starts_with(&label_lower) {
                    return Some(m.name.clone());
                }
            }
        }

        // 4. Family keyword heuristics
        for keyword in ["gemini", "fable", "opus", "sonnet", "haiku"] {
            if req_lower.contains(keyword)
                && let Some(m) = self.models.iter().find(|m| {
                    m.name.to_ascii_lowercase().contains(keyword)
                        || m.label
                            .as_deref()
                            .is_some_and(|l| l.to_ascii_lowercase().contains(keyword))
                })
            {
                return Some(m.name.clone());
            }
        }

        // 5. Fallback: if the requested model doesn't exist in the gateway's whitelist,
        // use the gateway's flagship model (opus > sonnet > first) to avoid router 400/502/429 errors.
        self.models
            .iter()
            .find(|m| m.name.to_ascii_lowercase().contains("opus"))
            .or_else(|| {
                self.models
                    .iter()
                    .find(|m| m.name.to_ascii_lowercase().contains("sonnet"))
            })
            .or_else(|| self.models.first())
            .map(|m| m.name.clone())
    }

    /// Injects the discovered gateway environment variables and resolved model into a child command.
    pub fn inject_into_command(&self, command: &mut Command, model_override: &mut Option<String>) {
        if env::var_os("ANTHROPIC_BASE_URL").is_none() {
            command.env("ANTHROPIC_BASE_URL", &self.base_url);
        }
        if env::var_os("ANTHROPIC_AUTH_TOKEN").is_none() {
            command.env("ANTHROPIC_AUTH_TOKEN", &self.auth_token);
            command.env("CLAUDE_CODE_HOST_AUTH_ENV_VAR", "ANTHROPIC_AUTH_TOKEN");
        }
        // Bound retries on third-party gateways to prevent 10-retry 5-minute freeze on 429
        if env::var_os("CLAUDE_CODE_MAX_RETRIES").is_none() {
            command.env("CLAUDE_CODE_MAX_RETRIES", "2");
        }
        if let Some(resolved) = self.resolve_model(model_override.as_deref()) {
            *model_override = Some(resolved);
        }
    }
}

static UPSTREAM_MODELS_CACHE: RwLock<Option<(Instant, String, Vec<ClaudeGatewayModel>)>> =
    RwLock::new(None);

/// Invalidate in-memory cached upstream models, forcing a fresh fetch on the next call.
pub fn invalidate_claude_gateway_cache() {
    if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write() {
        *guard = None;
    }
}

/// Discovered active provider from `~/.cc-switch/cc-switch.db`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CcSwitchActiveProvider {
    pub id: String,
    pub name: String,
    pub app_type: String,
    pub upstream_base_url: Option<String>,
    pub upstream_auth_token: Option<String>,
    pub models: Vec<ClaudeGatewayModel>,
}

/// Locates the local CC Switch SQLite database (`~/.cc-switch/cc-switch.db`).
pub fn cc_switch_db_path() -> Option<PathBuf> {
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)?;
    let p = home.join(".cc-switch").join("cc-switch.db");
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

/// Discovers the currently active provider and configured model routes from `cc-switch.db`.
pub fn discover_cc_switch_active_provider() -> Option<CcSwitchActiveProvider> {
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
             WHERE is_current = 1 AND app_type IN ('claude-desktop', 'claude') \
             ORDER BY CASE WHEN app_type = 'claude-desktop' THEN 0 ELSE 1 END \
             LIMIT 1",
        )
        .ok()?;

    let mut rows = stmt.query([]).ok()?;
    let row = rows.next().ok()??;

    let id: String = row.get(0).unwrap_or_default();
    let name: String = row.get(1).unwrap_or_default();
    let app_type: String = row.get(2).unwrap_or_default();
    let settings_str: String = row.get(3).unwrap_or_default();
    let meta_str: String = row.get(4).unwrap_or_default();

    let settings: Value = serde_json::from_str(&settings_str).unwrap_or(Value::Null);
    let meta: Value = serde_json::from_str(&meta_str).unwrap_or(Value::Null);

    let env_obj = settings.get("env");
    let upstream_base_url = env_obj
        .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
        .or_else(|| settings.get("base_url"))
        .or_else(|| settings.get("baseUrl"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let upstream_auth_token = env_obj
        .and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN").or_else(|| e.get("ANTHROPIC_API_KEY")))
        .or_else(|| settings.get("auth_token"))
        .or_else(|| settings.get("apiKey"))
        .or_else(|| settings.get("auth").and_then(|a| a.get("OPENAI_API_KEY")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mut models = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    // 1. Configured routes in meta.claudeDesktopModelRoutes
    if let Some(routes) = meta.get("claudeDesktopModelRoutes").and_then(Value::as_object) {
        for (route_name, route_val) in routes {
            if route_name.is_empty() || !seen_names.insert(route_name.to_ascii_lowercase()) {
                continue;
            }
            let target = route_val
                .get("labelOverride")
                .or_else(|| route_val.get("model"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let supports_1m = route_val
                .get("supports1m")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            models.push(ClaudeGatewayModel {
                name: route_name.clone(),
                label: target,
                supports_1m,
            });
        }
    }

    // 2. Models in modelCatalog
    let catalog = meta
        .get("modelCatalog")
        .or_else(|| settings.get("modelCatalog"));
    if let Some(arr) = catalog.and_then(|c| c.get("models")).and_then(Value::as_array) {
        for item in arr {
            if let Some(m_name) = item
                .get("model")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
            {
                if !m_name.is_empty() && seen_names.insert(m_name.to_ascii_lowercase()) {
                    let display_name = item
                        .get("displayName")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    models.push(ClaudeGatewayModel {
                        name: m_name.to_string(),
                        label: display_name,
                        supports_1m: false,
                    });
                }
            }
        }
    }

    // Sort routes: opus > sonnet > fable > haiku > others
    models.sort_by_key(|m| {
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
    });

    Some(CcSwitchActiveProvider {
        id,
        name,
        app_type,
        upstream_base_url,
        upstream_auth_token,
        models,
    })
}

/// Dynamically queries live models from an upstream OpenAI/Anthropic compatible gateway.
pub fn fetch_live_upstream_models(base_url: &str, token: &str) -> Vec<ClaudeGatewayModel> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() || token.trim().is_empty() {
        return Vec::new();
    }

    // Fast cache lookup (15s TTL)
    if let Ok(guard) = UPSTREAM_MODELS_CACHE.read()
        && let Some((cached_at, cached_url, cached_models)) = guard.as_ref()
        && cached_url == base_url
        && cached_at.elapsed() < Duration::from_secs(15)
    {
        return cached_models.clone();
    }

    let models_url = if base_url.ends_with("/v1") {
        format!("{base_url}/models")
    } else {
        format!("{base_url}/v1/models")
    };

    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(2500)))
        .build()
        .new_agent();

    let Ok(mut resp) = agent
        .get(&models_url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("x-api-key", token)
        .call()
    else {
        return Vec::new();
    };

    let Ok(val) = resp.body_mut().read_json::<Value>() else {
        return Vec::new();
    };

    let mut fetched = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let items = val
        .get("data")
        .or_else(|| val.get("models"))
        .and_then(Value::as_array);

    if let Some(items) = items {
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
            // Filter non-LLM models (image generation, audio/tts/asr, embedding, rerank)
            if id_lower.contains("image")
                || id_lower.contains("audio")
                || id_lower.contains("tts")
                || id_lower.contains("asr")
                || id_lower.contains("embed")
                || id_lower.contains("rerank")
                || id_lower.contains("whisper")
                || id_lower.contains("moderation")
            {
                continue;
            }
            if !seen.insert(id_lower.clone()) {
                continue;
            }
            let label = item
                .get("displayName")
                .or_else(|| item.get("display_name"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let supports_1m = id_lower.contains("1m")
                || id_lower.contains("high")
                || id_lower.contains("gemini");
            fetched.push(ClaudeGatewayModel {
                name: id.to_string(),
                label,
                supports_1m,
            });
        }
    }

    if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write() {
        *guard = Some((Instant::now(), base_url.to_string(), fetched.clone()));
    }

    fetched
}

fn merge_gateway_models(target: &mut Vec<ClaudeGatewayModel>, incoming: Vec<ClaudeGatewayModel>) {
    let mut seen: std::collections::HashSet<String> = target
        .iter()
        .map(|m| m.name.to_ascii_lowercase())
        .collect();
    for m in target.iter() {
        if let Some(lbl) = &m.label {
            seen.insert(lbl.to_ascii_lowercase());
        }
    }
    for m in incoming {
        if seen.insert(m.name.to_ascii_lowercase()) {
            target.push(m);
        }
    }
}

/// Discovers active Claude gateway configuration from environment, `cc-switch.db`, `Claude-3p`, or `Claude` profiles.
pub fn discover_claude_gateway() -> Option<ClaudeGatewayProfile> {
    let cc_switch = discover_cc_switch_active_provider();
    let roots = gateway_search_roots();

    // 1. Environment variables take precedence if explicitly set
    let env_base = env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let env_token = env::var("ANTHROPIC_AUTH_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|v| !v.trim().is_empty())
        });

    if let (Some(base_url), Some(auth_token)) = (env_base, env_token) {
        let mut models = if let Some(ref cc) = cc_switch {
            cc.models.clone()
        } else {
            discover_config_library_models()
        };
        let live_models = fetch_live_upstream_models(&base_url, &auth_token);
        merge_gateway_models(&mut models, live_models);
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            "environment",
            models,
        ));
    }

    // 2. Discover from host-creds-*.json (active CC Switch local proxy loopback)
    if let Some((proxy_url, proxy_token, source_name)) = discover_host_creds(&roots) {
        let mut models = if let Some(ref cc) = cc_switch {
            cc.models.clone()
        } else {
            discover_config_library_models_in_roots(&roots)
        };
        if let Some(ref cc) = cc_switch
            && let Some(ref upstream_url) = cc.upstream_base_url
            && let Some(ref upstream_token) = cc.upstream_auth_token
        {
            let live_models = fetch_live_upstream_models(upstream_url, upstream_token);
            merge_gateway_models(&mut models, live_models);
        }
        return Some(ClaudeGatewayProfile::new(
            proxy_url,
            proxy_token,
            source_name,
            models,
        ));
    }

    // 3. Discover directly from cc-switch.db if active provider has upstream base_url & token
    if let Some(cc) = cc_switch
        && let Some(upstream_url) = cc.upstream_base_url
        && let Some(upstream_token) = cc.upstream_auth_token
    {
        let mut models = cc.models;
        let live_models = fetch_live_upstream_models(&upstream_url, &upstream_token);
        merge_gateway_models(&mut models, live_models);
        return Some(ClaudeGatewayProfile::new(
            upstream_url,
            upstream_token,
            format!("cc-switch:{}", cc.name),
            models,
        ));
    }

    // 4. Discover from configLibrary static metadata as fallback
    if let Some((base_url, auth_token, models, source_name)) = discover_config_library(&roots) {
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            source_name,
            models,
        ));
    }

    None
}

/// Candidate application data roots where Claude Desktop profiles live.
pub fn gateway_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        if let Some(local) = env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
        {
            roots.push(local.join("Claude-3p"));
            roots.push(local.join("Claude"));
        }
        if let Some(appdata) = env::var_os("APPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
        {
            roots.push(appdata.join("Claude-3p"));
            roots.push(appdata.join("Claude"));
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = crate::collectors::home_dir() {
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Claude-3p"),
            );
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Claude"),
            );
            roots.push(home.join(".config").join("Claude-3p"));
            roots.push(home.join(".config").join("Claude"));
        }
    }
    roots
}

fn discover_host_creds(roots: &[PathBuf]) -> Option<(String, String, String)> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut best_entry = None;
    let mut best_score = 0_u64;

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !file_name.starts_with("host-creds-") || !file_name.ends_with(".json") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(val) = serde_json::from_str::<Value>(&content) else {
                continue;
            };
            let env_obj = val.get("env");
            let base_url = env_obj
                .and_then(|e| e.get("ANTHROPIC_BASE_URL"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let auth_token = env_obj
                .and_then(|e| e.get("ANTHROPIC_AUTH_TOKEN"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());

            if let (Some(base_url), Some(auth_token)) = (base_url, auth_token) {
                let expires_at = val.get("expiresAt").and_then(Value::as_u64).unwrap_or(0);
                let proc_start = val.get("procStart").and_then(Value::as_f64).unwrap_or(0.0) as u64;
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                // Score prioritizing unexpired, then latest procStart / mtime
                let score = if expires_at > now_ms {
                    1_000_000_000_000_000_000_u64 + expires_at
                } else {
                    proc_start.max(mtime)
                };

                if score > best_score {
                    best_score = score;
                    best_entry = Some((
                        base_url.to_string(),
                        auth_token.to_string(),
                        format!("host-creds:{}", file_name),
                    ));
                }
            }
        }
    }
    best_entry
}

fn discover_config_library(
    roots: &[PathBuf],
) -> Option<(String, String, Vec<ClaudeGatewayModel>, String)> {
    for root in roots {
        let meta_file = root.join("configLibrary").join("_meta.json");
        if !meta_file.is_file() {
            continue;
        }
        let Ok(meta_str) = fs::read_to_string(&meta_file) else {
            continue;
        };
        let Ok(meta_val) = serde_json::from_str::<Value>(&meta_str) else {
            continue;
        };
        let Some(applied_id) = meta_val.get("appliedId").and_then(Value::as_str) else {
            continue;
        };
        let cfg_file = root
            .join("configLibrary")
            .join(format!("{}.json", applied_id));
        if !cfg_file.is_file() {
            continue;
        }
        let Ok(cfg_str) = fs::read_to_string(&cfg_file) else {
            continue;
        };
        let Ok(cfg_val) = serde_json::from_str::<Value>(&cfg_str) else {
            continue;
        };

        let base_url = cfg_val
            .get("inferenceGatewayBaseUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let auth_token = cfg_val
            .get("inferenceGatewayApiKey")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if let (Some(base_url), Some(auth_token)) = (base_url, auth_token) {
            let models = parse_models_from_config(&cfg_val);
            return Some((
                base_url.to_string(),
                auth_token.to_string(),
                models,
                format!("configLibrary:{}", applied_id),
            ));
        }
    }
    None
}

fn discover_config_library_models() -> Vec<ClaudeGatewayModel> {
    let roots = gateway_search_roots();
    discover_config_library_models_in_roots(&roots)
}

fn discover_config_library_models_in_roots(roots: &[PathBuf]) -> Vec<ClaudeGatewayModel> {
    for root in roots {
        let meta_file = root.join("configLibrary").join("_meta.json");
        if !meta_file.is_file() {
            continue;
        }
        let Ok(meta_str) = fs::read_to_string(&meta_file) else {
            continue;
        };
        let Ok(meta_val) = serde_json::from_str::<Value>(&meta_str) else {
            continue;
        };
        let Some(applied_id) = meta_val.get("appliedId").and_then(Value::as_str) else {
            continue;
        };
        let cfg_file = root
            .join("configLibrary")
            .join(format!("{}.json", applied_id));
        if let Ok(cfg_str) = fs::read_to_string(&cfg_file)
            && let Ok(cfg_val) = serde_json::from_str::<Value>(&cfg_str)
        {
            let models = parse_models_from_config(&cfg_val);
            if !models.is_empty() {
                return models;
            }
        }
    }
    Vec::new()
}

fn parse_models_from_config(cfg: &Value) -> Vec<ClaudeGatewayModel> {
    let mut models = Vec::new();
    if let Some(arr) = cfg.get("inferenceModels").and_then(Value::as_array) {
        for item in arr {
            if let Some(name) = item.get("name").and_then(Value::as_str) {
                let label = item
                    .get("labelOverride")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let supports_1m = item
                    .get("supports1m")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                models.push(ClaudeGatewayModel {
                    name: name.to_string(),
                    label,
                    supports_1m,
                });
            }
        }
    }
    // Prioritize flagship/stable models: claude-opus-5 (Alpha), claude-sonnet-5, claude-fable-5, etc.
    models.sort_by_key(|m| {
        let name_lower = m.name.to_ascii_lowercase();
        let label_lower = m.label.as_deref().unwrap_or("").to_ascii_lowercase();
        if name_lower.contains("opus") || label_lower.contains("alpha") {
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
    });
    models
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_resolution_exact_and_fuzzy() {
        let profile = ClaudeGatewayProfile::new(
            "http://127.0.0.1:15721/claude-desktop",
            "ccs-test",
            "test",
            vec![
                ClaudeGatewayModel {
                    name: "claude-fable-5".into(),
                    label: Some("gemini-3.8-flash-high".into()),
                    supports_1m: true,
                },
                ClaudeGatewayModel {
                    name: "claude-opus-5".into(),
                    label: Some("alpha".into()),
                    supports_1m: true,
                },
                ClaudeGatewayModel {
                    name: "claude-sonnet-5".into(),
                    label: Some("claude-opus-4-6".into()),
                    supports_1m: true,
                },
            ],
        );

        // 1. Exact match on router model name
        assert_eq!(
            profile.resolve_model(Some("claude-fable-5")),
            Some("claude-fable-5".into())
        );

        // 2. Exact match on backend label
        assert_eq!(
            profile.resolve_model(Some("gemini-3.8-flash-high")),
            Some("claude-fable-5".into())
        );

        // 3. Prefix match on backend label from transcript ("gemini-3.8-flash")
        assert_eq!(
            profile.resolve_model(Some("gemini-3.8-flash")),
            Some("claude-fable-5".into())
        );

        // 4. Family keyword match ("alpha")
        assert_eq!(
            profile.resolve_model(Some("alpha")),
            Some("claude-opus-5".into())
        );

        // 5. Default fallback on empty/none prioritizes flagship opus
        assert_eq!(profile.resolve_model(None), Some("claude-opus-5".into()));

        // 6. Unknown model fallback to flagship opus
        assert_eq!(
            profile.resolve_model(Some("unrecognized-custom-model")),
            Some("claude-opus-5".into())
        );
    }

    #[test]
    fn inject_into_command_sets_env_and_resolves_model() {
        let profile = ClaudeGatewayProfile::new(
            "http://127.0.0.1:15721/claude-desktop",
            "ccs-token-xyz",
            "test",
            vec![ClaudeGatewayModel {
                name: "claude-fable-5".into(),
                label: Some("gemini-3.8-flash".into()),
                supports_1m: true,
            }],
        );

        let mut cmd = Command::new("claude");
        let mut model = Some("gemini-3.8-flash".to_string());
        profile.inject_into_command(&mut cmd, &mut model);

        assert_eq!(model, Some("claude-fable-5".to_string()));
    }

    #[test]
    fn merge_gateway_models_deduplicates_by_name_and_label() {
        let mut target = vec![
            ClaudeGatewayModel {
                name: "claude-opus-5".into(),
                label: Some("claude-opus-5-5-high".into()),
                supports_1m: true,
            },
            ClaudeGatewayModel {
                name: "claude-sonnet-5".into(),
                label: Some("claude-sonnet-5-5-high".into()),
                supports_1m: true,
            },
        ];

        let incoming = vec![
            ClaudeGatewayModel {
                name: "claude-opus-5-5-high".into(),
                label: None,
                supports_1m: true,
            },
            ClaudeGatewayModel {
                name: "claude-opus-4-6-thinking".into(),
                label: None,
                supports_1m: true,
            },
            ClaudeGatewayModel {
                name: "gemini-3.8-flash-high".into(),
                label: None,
                supports_1m: true,
            },
        ];

        merge_gateway_models(&mut target, incoming);

        // claude-opus-5-5-high should NOT be duplicated since claude-opus-5 routes to it
        assert_eq!(target.len(), 4);
        assert_eq!(target[0].name, "claude-opus-5");
        assert_eq!(target[1].name, "claude-sonnet-5");
        assert_eq!(target[2].name, "claude-opus-4-6-thinking");
        assert_eq!(target[3].name, "gemini-3.8-flash-high");
    }

    #[test]
    fn discover_cc_switch_reads_active_provider_if_present() {
        if cc_switch_db_path().is_some() {
            let active = discover_cc_switch_active_provider();
            if let Some(prov) = active {
                assert!(!prov.name.is_empty());
                assert!(!prov.models.is_empty());
            }
        }
    }
}
