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
    time::{SystemTime, UNIX_EPOCH},
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
            // No model requested: default to the gateway's primary configured model.
            return self.models.first().map(|m| m.name.clone());
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
        // use the gateway's first configured model to avoid router 400/502 errors.
        self.models.first().map(|m| m.name.clone())
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

/// Discovers active Claude gateway configuration from environment, `Claude-3p`, or `Claude` profiles.
pub fn discover_claude_gateway() -> Option<ClaudeGatewayProfile> {
    // 1. Environment variables take precedence
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
        let models = discover_config_library_models();
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            "environment",
            models,
        ));
    }

    let roots = gateway_search_roots();

    // 2. Discover from host-creds-*.json
    if let Some((base_url, auth_token, source_name)) = discover_host_creds(&roots) {
        let models = discover_config_library_models_in_roots(&roots);
        return Some(ClaudeGatewayProfile::new(
            base_url,
            auth_token,
            source_name,
            models,
        ));
    }

    // 3. Discover from configLibrary
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

        // 5. Default fallback on empty/none
        assert_eq!(profile.resolve_model(None), Some("claude-fable-5".into()));

        // 6. Unknown model fallback to primary
        assert_eq!(
            profile.resolve_model(Some("unrecognized-custom-model")),
            Some("claude-fable-5".into())
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
}
