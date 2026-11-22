use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::super::home_dir;

/// Strips a UTF-8 BOM if present (`~/.gemini/config/mcp_config.json` on Windows often carries one).
pub fn strip_utf8_bom(raw: &str) -> &str {
    raw.strip_prefix('\u{feff}').unwrap_or(raw)
}

/// Reads and parses a JSON file while stripping any leading UTF-8 BOM.
pub fn read_json_bom_safe(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(strip_utf8_bom(&raw)).ok()
}

/// Infers user home directory given an Antigravity root like `~/.gemini/antigravity` or `~/.antigravity`.
pub fn infer_user_home_from_ag_root(ag_home: &Path) -> Option<PathBuf> {
    let parent = ag_home.parent()?;
    if parent.file_name().and_then(|n| n.to_str()) == Some(".gemini") {
        return parent.parent().map(Path::to_path_buf);
    }
    Some(parent.to_path_buf())
}

/// Decodes `file:///c%3A/Projects/app` or `file:///Users/dev/app` into a normalized filesystem path string.
pub fn decode_file_uri_to_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed.starts_with("file://") {
        return Some(trimmed.to_owned());
    }

    let decoded = percent_decode_utf8(trimmed);
    let mut rest = decoded
        .strip_prefix("file://localhost")
        .or_else(|| decoded.strip_prefix("file://"))
        .unwrap_or(&decoded)
        .to_owned();

    // Windows drive URI: /c:/Projects/app -> C:/Projects/app (or C:\Projects\app on Windows)
    let bytes = rest.as_bytes();
    let is_windows_drive =
        bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    if is_windows_drive {
        let drive = (bytes[1] as char).to_ascii_uppercase();
        rest = format!("{drive}:{}", &rest[3..]);
    }

    #[cfg(windows)]
    let mut normalized = if is_windows_drive {
        rest.replace('/', "\\")
    } else {
        rest
    };
    #[cfg(not(windows))]
    let mut normalized = rest;

    // Trim trailing separator unless it's a bare root like "/" or "D:\" / "D:/"
    while normalized.len() > 1 && (normalized.ends_with('/') || normalized.ends_with('\\')) {
        if normalized.len() == 3
            && normalized.as_bytes()[0].is_ascii_alphabetic()
            && normalized.as_bytes()[1] == b':'
        {
            break;
        }
        normalized.pop();
    }

    (!normalized.is_empty()).then_some(normalized)
}

fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AntigravityDaemonInfo {
    pub pid: Option<u32>,
    pub http_port: Option<u16>,
    pub https_port: Option<u16>,
    pub ls_version: Option<String>,
    pub has_csrf_token: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AntigravityLocalConfig {
    pub resolved_model: Option<String>,
    pub state_model: Option<String>,
    pub configured_models: Vec<String>,
    pub auto_execution_policy: Option<String>,
    pub artifact_review_mode: Option<String>,
    pub remote_control_hostname: Option<String>,
    pub enabled_plugins_count: usize,
    pub installation_uuid: Option<String>,
    pub active_daemon: Option<AntigravityDaemonInfo>,
}

/// Loads Antigravity local configuration across `~/.gemini/config/config.json`, settings files,
/// `antigravity_state.pbtxt`, and `daemon/hub_*.json`.
pub fn load_antigravity_local_config() -> AntigravityLocalConfig {
    let Some(user_home) = home_dir() else {
        return AntigravityLocalConfig::default();
    };
    let ag_home = super::discover_antigravity_home().ok().flatten();
    load_antigravity_local_config_from(&user_home, ag_home.as_deref())
}

pub fn load_antigravity_local_config_from(
    user_home: &Path,
    ag_home: Option<&Path>,
) -> AntigravityLocalConfig {
    let mut cfg = AntigravityLocalConfig::default();
    let default_ag_home = user_home.join(".gemini").join("antigravity");
    let resolved_ag_home = ag_home.unwrap_or(default_ag_home.as_path());

    // 1. `~/.gemini/config/config.json` (plugins + userSettings)
    if let Some(val) =
        read_json_bom_safe(&user_home.join(".gemini").join("config").join("config.json"))
    {
        if let Some(plugins) = val.get("plugins").and_then(Value::as_object) {
            cfg.enabled_plugins_count = plugins
                .values()
                .filter(|v| v.get("enabled").and_then(Value::as_bool) == Some(true))
                .count();
        }
        if let Some(us) = val.get("userSettings").and_then(Value::as_object) {
            cfg.auto_execution_policy = us
                .get("autoExecutionPolicy")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            cfg.artifact_review_mode = us
                .get("artifactReviewMode")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            cfg.remote_control_hostname = us
                .get("remoteControlHostname")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
        }
        collect_models_from_value(&val, &mut cfg.configured_models);
    }

    // 2. Settings files across CLI & IDE dirs
    let candidate_paths = [
        resolved_ag_home.join("settings.json"),
        resolved_ag_home.join("config.json"),
        user_home
            .join(".gemini")
            .join("antigravity-cli")
            .join("settings.json"),
        user_home.join(".gemini").join("settings.json"),
        user_home.join(".antigravity").join("settings.json"),
        user_home.join(".antigravity").join("config.json"),
    ];
    for p in candidate_paths {
        if let Some(val) = read_json_bom_safe(&p) {
            collect_models_from_value(&val, &mut cfg.configured_models);
        }
    }

    // 3. `antigravity_state.pbtxt` + `installation_id`
    let pbtxt_path = resolved_ag_home.join("antigravity_state.pbtxt");
    if let Ok(raw) = fs::read_to_string(&pbtxt_path) {
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("last_selected_agent_model:") {
                let cleaned = val.trim().trim_matches('"').to_owned();
                if !cleaned.is_empty() {
                    cfg.state_model = Some(cleaned.clone());
                    if !is_model_placeholder(&cleaned)
                        && !cfg
                            .configured_models
                            .iter()
                            .any(|m| m.eq_ignore_ascii_case(&cleaned))
                    {
                        cfg.configured_models.push(cleaned);
                    }
                }
            } else if let Some(val) = trimmed.strip_prefix("installation_uuid:") {
                let cleaned = val.trim().trim_matches('"').to_owned();
                if !cleaned.is_empty() {
                    cfg.installation_uuid = Some(cleaned);
                }
            }
        }
    }
    if cfg.installation_uuid.is_none() {
        if let Ok(id) = fs::read_to_string(resolved_ag_home.join("installation_id")) {
            let trimmed = id.trim().to_owned();
            if !trimmed.is_empty() {
                cfg.installation_uuid = Some(trimmed);
            }
        }
    }

    // 4. `daemon/hub_*.json` (live or most recent daemon descriptor)
    let daemon_dir = resolved_ag_home.join("daemon");
    if let Ok(entries) = fs::read_dir(&daemon_dir) {
        let mut hubs: Vec<(i64, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if fname.starts_with("hub_") && fname.ends_with(".json") && p.is_file() {
                let mtime = super::super::file_modified_millis(&p).unwrap_or(0);
                hubs.push((mtime, p));
            }
        }
        hubs.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
        for (_, hub_path) in hubs {
            if let Some(val) = read_json_bom_safe(&hub_path) {
                cfg.active_daemon = Some(AntigravityDaemonInfo {
                    pid: val.get("pid").and_then(Value::as_u64).map(|v| v as u32),
                    http_port: val
                        .get("httpPort")
                        .and_then(Value::as_u64)
                        .and_then(|v| u16::try_from(v).ok()),
                    https_port: val
                        .get("httpsPort")
                        .and_then(Value::as_u64)
                        .and_then(|v| u16::try_from(v).ok()),
                    ls_version: val
                        .get("lsVersion")
                        .and_then(Value::as_str)
                        .map(|s| s.trim().to_owned())
                        .filter(|s| !s.is_empty()),
                    has_csrf_token: val
                        .get("csrfToken")
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.trim().is_empty()),
                });
                break;
            }
        }
    }

    cfg.resolved_model = cfg
        .configured_models
        .iter()
        .find(|m| !is_model_placeholder(m))
        .cloned();
    #[cfg(not(test))]
    if cfg.resolved_model.is_none() {
        if let Some(ref raw_state) = cfg.state_model
            && let Some(resolved) = super::resolve_antigravity_model_reference(raw_state)
        {
            if !cfg
                .configured_models
                .iter()
                .any(|m| m.eq_ignore_ascii_case(&resolved))
            {
                cfg.configured_models.insert(0, resolved.clone());
            }
            cfg.resolved_model = Some(resolved);
        } else if let Some(live) = super::discover_live_antigravity_models() {
            cfg.resolved_model = live
                .default_model_id
                .or_else(|| live.models.first().map(|m| m.model_id.clone()));
        }
    }
    cfg
}

pub fn is_model_placeholder(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.is_empty() || trimmed.starts_with("MODEL_PLACEHOLDER_")
}

fn collect_models_from_value(val: &Value, out: &mut Vec<String>) {
    let push_if_new = |s: &str, out: &mut Vec<String>| {
        let trimmed = s.trim();
        if !trimmed.is_empty()
            && !is_model_placeholder(trimmed)
            && !out
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            out.push(trimmed.to_owned());
        }
    };

    if let Some(model_val) = val.get("model") {
        if let Some(s) = model_val.as_str() {
            push_if_new(s, out);
        } else if let Some(s) = model_val.get("name").and_then(Value::as_str) {
            push_if_new(s, out);
        }
    }
    if let Some(s) = val.get("selectedModel").and_then(Value::as_str) {
        push_if_new(s, out);
    }
    if let Some(arr) = val.get("models").and_then(Value::as_array) {
        for item in arr {
            if let Some(s) = item.as_str() {
                push_if_new(s, out);
            } else if let Some(s) = item
                .get("id")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
            {
                push_if_new(s, out);
            }
        }
    }
}

/// Resolves the concrete model name for Antigravity ledger & session metrics.
pub fn resolve_antigravity_model_for_home(user_home: &Path, ag_home: Option<&Path>) -> String {
    let cfg = load_antigravity_local_config_from(user_home, ag_home);
    cfg.resolved_model
        .unwrap_or_else(|| "gemini-3.8-flash".to_owned())
}

/// Context window budget in tokens for the given Antigravity / Gemini model.
pub fn resolve_antigravity_context_window(model: &str) -> u64 {
    let m = model.to_ascii_lowercase();
    if m.contains("claude") || m.contains("sonnet") || m.contains("opus") || m.contains("haiku") {
        200_000
    } else if m.contains("gpt-5") || m.contains("codex") {
        400_000
    } else if m.contains("gpt-4") || m.contains("deepseek") || m.contains("grok") {
        128_000
    } else {
        1_000_000
    }
}

/// Discovers project UUID -> decoded workspace folder path mappings from `~/.gemini/config/projects/*.json`.
pub fn discover_antigravity_projects_map(user_home: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let projects_dir = user_home.join(".gemini").join("config").join("projects");
    let Ok(entries) = fs::read_dir(&projects_dir) else {
        return map;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") || !p.is_file() {
            continue;
        }
        let Some(val) = read_json_bom_safe(&p) else {
            continue;
        };
        let proj_id = val
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| p.file_stem().and_then(|s| s.to_str()).map(str::to_owned));
        let Some(id) = proj_id else {
            continue;
        };
        let folder_uri = val
            .pointer("/projectResources/resources")
            .and_then(Value::as_array)
            .and_then(|resources| {
                resources.iter().find_map(|res| {
                    res.get("folderUri")
                        .and_then(Value::as_str)
                        .or_else(|| res.pointer("/gitFolder/folderUri").and_then(Value::as_str))
                })
            });
        if let Some(uri) = folder_uri
            && let Some(decoded) = decode_file_uri_to_path(uri)
        {
            map.insert(id, decoded);
        }
    }
    map
}

/// Non-spawning installation check for Antigravity IDE / CLI / local runtime on Windows, macOS, and Linux.
pub fn check_antigravity_installed_at(home: Option<&Path>) -> (bool, Option<String>) {
    if let Some(override_bin) = env::var_os("ANTIGRAVITY_BINARY").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(override_bin);
        if p.is_file() {
            return (true, Some(p.to_string_lossy().into_owned()));
        }
    }

    if let Some(h) = home {
        let candidates = [
            h.join(".local").join("bin").join("agy"),
            h.join(".local").join("bin").join("antigravity"),
            h.join("AppData")
                .join("Local")
                .join("agy")
                .join("bin")
                .join("agy.exe"),
            h.join("AppData")
                .join("Local")
                .join("Programs")
                .join("antigravity")
                .join("Antigravity.exe"),
            h.join(".gemini")
                .join("antigravity")
                .join("bin")
                .join("agentapi.bat"),
            h.join(".gemini")
                .join("antigravity")
                .join("bin")
                .join("agentapi"),
        ];
        for cand in candidates {
            if cand.is_file() {
                return (true, Some(cand.to_string_lossy().into_owned()));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app) = env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            let local_root = PathBuf::from(local_app);
            for exe in [
                local_root.join("agy").join("bin").join("agy.exe"),
                local_root
                    .join("Programs")
                    .join("antigravity")
                    .join("Antigravity.exe"),
            ] {
                if exe.is_file() {
                    return (true, Some(exe.to_string_lossy().into_owned()));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let app = PathBuf::from("/Applications/Antigravity.app");
        if app.exists() {
            return (true, Some(app.to_string_lossy().into_owned()));
        }
    }

    if let Some(path_var) = env::var_os("PATH") {
        #[cfg(target_os = "windows")]
        let names = [
            "agy.cmd",
            "agy.exe",
            "agy",
            "antigravity.cmd",
            "antigravity.exe",
            "antigravity",
        ];
        #[cfg(not(target_os = "windows"))]
        let names = ["agy", "antigravity"];
        for dir in env::split_paths(&path_var) {
            for name in names {
                let p = dir.join(name);
                if p.is_file() {
                    return (true, Some(p.to_string_lossy().into_owned()));
                }
            }
        }
    }

    if let Some(h) = home {
        let ag_dir = h.join(".gemini").join("antigravity");
        if ag_dir.join("antigravity_state.pbtxt").is_file()
            || ag_dir.join("conversation_summaries.db").is_file()
        {
            return (true, Some(ag_dir.to_string_lossy().into_owned()));
        }
    }

    (false, None)
}
