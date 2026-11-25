use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::collectors::AcpAppServer;

use super::types::{InstalledPluginRecord, PluginManifest, validate_relative_path};

const METADATA_CACHE_TTL: Duration = Duration::from_secs(10);
const NEGATIVE_METADATA_CACHE_TTL: Duration = Duration::from_secs(30);
const PLUGIN_RPC_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Default)]
pub struct PluginMetadataSnapshot {
    pub fetched_at: Option<Instant>,
    pub available: Option<bool>,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub error: Option<String>,
    pub sessions: Vec<Value>,
    pub models: Vec<Value>,
}

fn metadata_cache() -> &'static Mutex<HashMap<String, (String, PluginMetadataSnapshot)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (String, PluginMetadataSnapshot)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn invalidate_runtime_cache(plugin_id: Option<&str>) {
    if let Ok(mut guard) = metadata_cache().lock() {
        if let Some(id) = plugin_id {
            guard.remove(id);
        } else {
            guard.clear();
        }
    }
}

pub fn resolve_runtime_command(
    manifest: &PluginManifest,
    install_dir: &Path,
) -> Result<(PathBuf, Vec<String>), String> {
    super::installer::validate_runtime_spec(&manifest.runtime, install_dir)?;
    let cmd = manifest.runtime.command.trim();
    if cmd.eq_ignore_ascii_case("node") {
        let program = crate::collectors::resolve_node_executable();
        let mut args = manifest.runtime.args.clone();
        if let Some(first) = args.first_mut() {
            let rel = validate_relative_path(first)?;
            *first = install_dir.join(rel).to_string_lossy().to_string();
        }
        Ok((program, args))
    } else {
        let rel = validate_relative_path(cmd)?;
        Ok((install_dir.join(rel), manifest.runtime.args.clone()))
    }
}

pub fn spawn_plugin_server(
    record: &InstalledPluginRecord,
    timeout: Duration,
) -> Result<AcpAppServer, String> {
    let install_dir = Path::new(&record.install_dir);
    let (program, args) = resolve_runtime_command(&record.manifest, install_dir)?;
    let safe_env: std::collections::BTreeMap<String, String> = record
        .manifest
        .runtime
        .env
        .iter()
        .filter(|(k, _)| !super::installer::is_forbidden_plugin_env_key(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // Avoid holding an open CWD directory handle on Windows while running Node scripts,
    // which would otherwise block uninstall/reinstall with OS error 32 sharing violations.
    let working_dir = if record
        .manifest
        .runtime
        .command
        .trim()
        .eq_ignore_ascii_case("node")
    {
        None
    } else {
        Some(install_dir)
    };
    AcpAppServer::start_with_options(&program, &args, working_dir, &safe_env, false, timeout)
        .map_err(|e| format!("plugin_start_failed:{}: {e}", record.id))
}

pub fn cached_plugin_metadata(record: &InstalledPluginRecord) -> Option<PluginMetadataSnapshot> {
    let cache_key_version = format!("{}:{}", record.version, record.install_dir);
    metadata_cache().lock().ok().and_then(|guard| {
        guard
            .get(&record.id)
            .filter(|(ver, _)| *ver == cache_key_version)
            .map(|(_, snap)| snap.clone())
    })
}

fn is_method_not_found(err: &crate::collectors::CollectorError) -> bool {
    matches!(
        err,
        crate::collectors::CollectorError::Protocol { code, message, .. }
            if code == "-32601" || message.contains("Method not found")
    )
}

pub fn query_plugin_metadata(record: &InstalledPluginRecord) -> PluginMetadataSnapshot {
    let cache_key_version = format!("{}:{}", record.version, record.install_dir);
    if let Ok(guard) = metadata_cache().lock()
        && let Some((ver, snap)) = guard.get(&record.id)
        && *ver == cache_key_version
    {
        let ttl = if snap.available == Some(false) && snap.error.is_some() {
            NEGATIVE_METADATA_CACHE_TTL
        } else {
            METADATA_CACHE_TTL
        };
        if snap.fetched_at.is_some_and(|at| at.elapsed() < ttl) {
            return snap.clone();
        }
    }

    let mut snapshot = PluginMetadataSnapshot {
        fetched_at: Some(Instant::now()),
        ..Default::default()
    };

    let mut server = match spawn_plugin_server(record, PLUGIN_RPC_TIMEOUT) {
        Ok(srv) => srv,
        Err(err) => {
            snapshot.available = Some(false);
            snapshot.error = Some(err);
            if let Ok(mut guard) = metadata_cache().lock() {
                guard.insert(record.id.clone(), (cache_key_version, snapshot.clone()));
            }
            return snapshot;
        }
    };

    if record.manifest.capabilities.discover || record.manifest.capabilities.launch {
        match server.request_raw("agenthub/probe", json!({})) {
            Ok(val) => {
                snapshot.available = val.get("available").and_then(Value::as_bool);
                snapshot.binary_path = val
                    .get("binary_path")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                snapshot.version = val
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                snapshot.error = val
                    .get("error")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string);
            }
            Err(err) if is_method_not_found(&err) => {
                snapshot.available = Some(true);
            }
            Err(err) => {
                snapshot.available = Some(false);
                snapshot.error = Some(err.to_string());
            }
        }
    }

    if record.manifest.capabilities.discover {
        match server.request_raw("agenthub/discover", json!({})) {
            Ok(val) => {
                if let Some(arr) = val.get("sessions").and_then(Value::as_array) {
                    let prefix = record.manifest.effective_session_prefix();
                    let resumable_default = snapshot.available.unwrap_or(true);
                    snapshot.sessions = arr
                        .iter()
                        .filter_map(|raw| {
                            normalize_plugin_session(raw, &record.id, &prefix, resumable_default)
                        })
                        .collect();
                }
            }
            Err(err) if is_method_not_found(&err) => {}
            Err(_) => {}
        }
    }

    if record.manifest.capabilities.models {
        match server.request_raw("agenthub/models", json!({})) {
            Ok(val) => {
                if let Some(arr) = val.get("models").and_then(Value::as_array) {
                    let mut seen = HashSet::new();
                    for item in arr {
                        let Some(id) = item
                            .get("id")
                            .or_else(|| item.get("name"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        else {
                            continue;
                        };
                        if !seen.insert(id.to_ascii_lowercase()) {
                            continue;
                        }
                        let mut model = item.clone();
                        if let Some(obj) = model.as_object_mut() {
                            obj.entry("id").or_insert_with(|| json!(id));
                            obj.entry("name").or_insert_with(|| json!(id));
                            obj.entry("display_name").or_insert_with(|| json!(id));
                        }
                        snapshot.models.push(model);
                    }
                }
            }
            Err(err) if is_method_not_found(&err) => {}
            Err(_) => {}
        }
    }

    drop(server);

    if let Ok(mut guard) = metadata_cache().lock() {
        guard.insert(record.id.clone(), (cache_key_version, snapshot.clone()));
    }
    snapshot
}

fn normalize_plugin_session(
    raw: &Value,
    plugin_id: &str,
    prefix: &str,
    resumable_default: bool,
) -> Option<Value> {
    let raw_id = raw
        .get("id")
        .or_else(|| raw.get("session_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let id = if raw_id.starts_with(prefix) {
        raw_id.to_string()
    } else {
        format!("{prefix}{raw_id}")
    };
    let cwd = raw
        .get("cwd")
        .or_else(|| raw.get("workspace_path"))
        .and_then(Value::as_str)
        .unwrap_or(".");
    let mut item = raw.clone();
    if let Some(obj) = item.as_object_mut() {
        obj.insert("id".into(), json!(id));
        obj.insert("session_id".into(), json!(id));
        obj.insert("cwd".into(), json!(cwd));
        obj.insert("workspace_path".into(), json!(cwd));
        obj.insert("provider".into(), json!(plugin_id));
        obj.insert("agent_name".into(), json!(plugin_id));
        if !obj.contains_key("resumable") {
            obj.insert("resumable".into(), json!(resumable_default));
        }
        if !obj.contains_key("status") {
            obj.insert("status".into(), json!({ "type": "idle" }));
        }
    }
    Some(item)
}

pub fn query_plugin_history(
    record: &InstalledPluginRecord,
    session_id: &str,
    workspace: Option<&Path>,
) -> Vec<Value> {
    if !record.manifest.capabilities.history {
        return Vec::new();
    }
    let Ok(mut server) = spawn_plugin_server(record, PLUGIN_RPC_TIMEOUT) else {
        return Vec::new();
    };
    let params = json!({
        "sessionId": session_id,
        "cwd": workspace.map(|p| p.to_string_lossy().to_string()),
    });
    let Ok(resp) = server.request_raw("agenthub/history", params) else {
        return Vec::new();
    };
    let Some(items) = resp.get("items").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            if item.get("role").is_some() && item.get("kind").is_some() {
                return Some(item.clone());
            }
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            match item_type {
                "userMessage" => {
                    let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                    (!text.trim().is_empty()).then(|| {
                        crate::conversation::conversation_item(
                            "user",
                            "message",
                            "",
                            text,
                            "completed",
                        )
                    })
                }
                "agentMessage" => {
                    let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                    (!text.trim().is_empty()).then(|| {
                        crate::conversation::conversation_item(
                            "assistant",
                            "message",
                            "",
                            text,
                            "completed",
                        )
                    })
                }
                "thinking" => {
                    let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                    (!text.trim().is_empty()).then(|| {
                        crate::conversation::conversation_item(
                            "assistant",
                            "thinking",
                            "思考过程",
                            text,
                            "completed",
                        )
                    })
                }
                "commandExecution" => {
                    let cmd = item
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or("tool");
                    let output = item.get("output").and_then(Value::as_str).unwrap_or("");
                    let status = item
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("completed");
                    let content = if output.trim().is_empty() {
                        cmd.to_string()
                    } else {
                        format!("{cmd}\n{output}")
                    };
                    Some(crate::conversation::conversation_item(
                        "assistant",
                        "tool",
                        cmd,
                        &content,
                        status,
                    ))
                }
                _ => None,
            }
        })
        .collect()
}
