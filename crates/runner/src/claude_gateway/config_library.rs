use super::*;

/// Candidate application data roots where Claude Desktop profiles live.
#[cfg(any(windows, test))]
pub(crate) fn windows_msix_claude_roots_in(local_appdata: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let packages = local_appdata.join("Packages");
    let Ok(entries) = fs::read_dir(&packages) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("Claude_") || name.starts_with("AnthropicPBC.Claude_") {
            let cache = entry.path().join("LocalCache");
            out.push(cache.join("Local").join("Claude-3p"));
            out.push(cache.join("Local").join("Claude"));
            out.push(cache.join("Roaming").join("Claude-3p"));
            out.push(cache.join("Roaming").join("Claude"));
        }
    }
    out
}

pub fn gateway_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        if let Some(local) = env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| crate::collectors::home_dir().map(|h| h.join("AppData").join("Local")))
        {
            roots.push(local.join("Claude-3p"));
            roots.push(local.join("Claude"));
            roots.extend(windows_msix_claude_roots_in(&local));
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

pub(crate) fn discover_host_creds(roots: &[PathBuf]) -> Option<(String, String, String)> {
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
                if !is_usable_gateway_token(auth_token) {
                    continue;
                }
                let expires_at = val.get("expiresAt").and_then(Value::as_u64).unwrap_or(0);
                let proc_start = val.get("procStart").and_then(Value::as_f64).unwrap_or(0.0) as u64;
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                // Never inject expired ephemeral host-creds or dead loopback endpoints.
                if expires_at > 0 && expires_at <= now_ms {
                    continue;
                }
                if expires_at == 0 && (mtime == 0 || now_ms.saturating_sub(mtime) > 43_200_000) {
                    continue;
                }
                if !is_gateway_url_reachable(base_url) {
                    continue;
                }

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

fn resolve_safe_config_library_file(root: &Path, applied_id: &str) -> Option<PathBuf> {
    if applied_id.is_empty()
        || applied_id.len() > 128
        || applied_id == "."
        || applied_id == ".."
        || !applied_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return None;
    }
    let lib_dir = root.join("configLibrary");
    let cfg_file = lib_dir.join(format!("{applied_id}.json"));
    let canon_dir = dunce::canonicalize(&lib_dir).ok()?;
    let canon_cfg = dunce::canonicalize(&cfg_file).ok()?;
    if canon_cfg.is_file() && canon_cfg.starts_with(&canon_dir) {
        Some(canon_cfg)
    } else {
        None
    }
}

pub(crate) fn discover_config_library(
    roots: &[PathBuf],
) -> Option<(String, String, Vec<ClaudeGatewayModel>, String)> {
    discover_config_library_with(roots, discover_cc_switch_gateway_token)
}

pub(crate) fn discover_config_library_with<F>(
    roots: &[PathBuf],
    ccs_token_lookup: F,
) -> Option<(String, String, Vec<ClaudeGatewayModel>, String)>
where
    F: Fn() -> Option<String>,
{
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
        let Some(cfg_file) = resolve_safe_config_library_file(root, applied_id) else {
            continue;
        };
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
            let mut resolved_token = auth_token.to_string();
            if resolved_token.starts_with("__sealed__:") {
                if !is_cc_switch_endpoint(base_url) {
                    continue;
                }
                if let Some(token) = ccs_token_lookup().filter(|t| is_usable_gateway_token(t)) {
                    resolved_token = token;
                } else {
                    // Sealed DPAPI token cannot be unsealed without CC Switch gateway token;
                    // never transplant another upstream's key onto the CCS endpoint.
                    continue;
                }
            }
            if !is_usable_gateway_token(&resolved_token) {
                continue;
            }
            let models = parse_models_from_config(&cfg_val);
            return Some((
                base_url.to_string(),
                resolved_token,
                models,
                format!("configLibrary:{}", applied_id),
            ));
        }
    }
    None
}

pub fn discover_config_library_models() -> Vec<ClaudeGatewayModel> {
    let roots = gateway_search_roots();
    discover_config_library_models_in_roots(&roots)
}

pub(crate) fn discover_config_library_models_in_roots(
    roots: &[PathBuf],
) -> Vec<ClaudeGatewayModel> {
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
        let Some(cfg_file) = resolve_safe_config_library_file(root, applied_id) else {
            continue;
        };
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

pub(crate) fn parse_models_from_config(cfg: &Value) -> Vec<ClaudeGatewayModel> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppliedConfigLibrarySummary {
    pub applied_id: String,
    pub display_name: Option<String>,
    pub base_url: Option<String>,
    pub inference_provider: Option<String>,
    pub models: Vec<ClaudeGatewayModel>,
    pub source: String,
}

pub(crate) fn inspect_applied_config_library(
    roots: &[PathBuf],
) -> Option<AppliedConfigLibrarySummary> {
    for root in roots {
        let meta_file = root.join("configLibrary").join("_meta.json");
        let Ok(meta_str) = fs::read_to_string(&meta_file) else {
            continue;
        };
        let Ok(meta_val) = serde_json::from_str::<Value>(&meta_str) else {
            continue;
        };
        let Some(applied_id) = meta_val
            .get("appliedId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(cfg_file) = resolve_safe_config_library_file(root, applied_id) else {
            continue;
        };
        let Ok(cfg_str) = fs::read_to_string(&cfg_file) else {
            continue;
        };
        let Ok(cfg_val) = serde_json::from_str::<Value>(&cfg_str) else {
            continue;
        };
        let display_name = meta_val
            .get("entries")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries.iter().find_map(|entry| {
                    let id = entry.get("id").and_then(Value::as_str)?;
                    if id == applied_id {
                        entry
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    } else {
                        None
                    }
                })
            });
        let base_url = cfg_val
            .get("inferenceGatewayBaseUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let inference_provider = cfg_val
            .get("inferenceProvider")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let models = parse_models_from_config(&cfg_val);
        return Some(AppliedConfigLibrarySummary {
            applied_id: applied_id.to_string(),
            display_name,
            base_url,
            inference_provider,
            models,
            source: format!("configLibrary:{applied_id}"),
        });
    }
    None
}
