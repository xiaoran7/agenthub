use std::{
    collections::HashMap,
    path::Path,
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    AntigravityDaemonEndpoint, DAEMON_QUERY_TIMEOUT, MAX_DAEMON_CANDIDATES,
    config::decode_file_uri_to_path, discover_antigravity_homes, read_daemon_endpoints,
    resolve_antigravity_model_reference, resolve_antigravity_rpc_model_enum,
};
use crate::collectors::{BoundedLruCache, CollectorError};

const DAEMON_RPC_TIMEOUT: Duration = Duration::from_secs(8);
const LIVE_TRAJECTORIES_TTL: Duration = Duration::from_secs(3);
const ENDPOINT_CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveTrajectorySummary {
    pub status: String,
    pub step_count: u64,
    pub workspace_cwd: Option<String>,
    pub is_running: bool,
}

type EndpointCache = Option<(Instant, Option<AntigravityDaemonEndpoint>)>;
type TrajectoriesCache = Option<(Instant, Option<HashMap<String, LiveTrajectorySummary>>)>;
type TrajectoryModelCache = BoundedLruCache<String, (u64, Option<String>)>;

fn daemon_endpoint_cache() -> &'static RwLock<EndpointCache> {
    static CACHE: OnceLock<RwLock<EndpointCache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn live_trajectories_cache() -> &'static RwLock<TrajectoriesCache> {
    static CACHE: OnceLock<RwLock<TrajectoriesCache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn trajectory_model_cache() -> &'static RwLock<TrajectoryModelCache> {
    static CACHE: OnceLock<RwLock<TrajectoryModelCache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BoundedLruCache::new(2_048)))
}

pub fn invalidate_antigravity_daemon_caches() {
    if let Ok(mut g) = daemon_endpoint_cache().write() {
        *g = None;
    }
    if let Ok(mut g) = live_trajectories_cache().write() {
        *g = None;
    }
}

/// Performs a ConnectRPC JSON POST against `exa.language_server_pb.LanguageServerService/<method>`.
pub fn call_daemon_rpc(
    endpoint: &AntigravityDaemonEndpoint,
    method: &str,
    payload: &Value,
    timeout: Duration,
) -> Result<Value, CollectorError> {
    let program = format!("Antigravity daemon ({method})");
    let url = format!(
        "{}/exa.language_server_pb.LanguageServerService/{method}",
        endpoint.base_url()
    );
    let agent = ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .new_agent();

    let mut resp = agent
        .post(&url)
        .header("X-Codeium-Csrf-Token", &endpoint.csrf_token)
        .header("Connect-Protocol-Version", "1")
        .header("Content-Type", "application/json")
        .send_json(payload)
        .map_err(|err| CollectorError::Io {
            program: program.clone(),
            source: std::io::Error::other(err.to_string()),
        })?;
    let status = resp.status().as_u16();
    let body_text = resp.body_mut().read_to_string().unwrap_or_default();
    if status != 200 {
        return Err(CollectorError::Protocol {
            program,
            code: format!("http_{status}"),
            message: body_text.chars().take(240).collect(),
        });
    }
    if body_text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str::<Value>(&body_text)
        .map_err(|source| CollectorError::InvalidJson { program, source })
}

/// Discovers a responsive local Antigravity 2.0 `language_server` endpoint.
pub fn discover_live_antigravity_daemon() -> Option<AntigravityDaemonEndpoint> {
    if let Ok(guard) = daemon_endpoint_cache().read()
        && let Some((ts, ref cached)) = *guard
        && ts.elapsed() < ENDPOINT_CACHE_TTL
    {
        return cached.clone();
    }

    let mut found = None;
    'outer: for ag_home in discover_antigravity_homes() {
        for ep in read_daemon_endpoints(&ag_home)
            .into_iter()
            .take(MAX_DAEMON_CANDIDATES)
        {
            if call_daemon_rpc(
                &ep,
                "GetAllCascadeTrajectories",
                &json!({}),
                DAEMON_QUERY_TIMEOUT,
            )
            .is_ok()
            {
                found = Some(ep);
                break 'outer;
            }
        }
    }

    if let Ok(mut guard) = daemon_endpoint_cache().write() {
        *guard = Some((Instant::now(), found.clone()));
    }
    found
}

pub fn parse_live_trajectories_response(val: &Value) -> HashMap<String, LiveTrajectorySummary> {
    let mut out = HashMap::new();
    let Some(map) = val.get("trajectorySummaries").and_then(Value::as_object) else {
        return out;
    };
    for (cid, item) in map {
        let status = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let step_count = item.get("stepCount").and_then(Value::as_u64).unwrap_or(0);
        let workspace_cwd = item
            .get("workspaces")
            .and_then(Value::as_array)
            .and_then(|arr| {
                arr.iter().find_map(|w| {
                    w.get("workspaceFolderAbsoluteUri")
                        .and_then(Value::as_str)
                        .and_then(decode_file_uri_to_path)
                })
            });
        let status_upper = status.to_ascii_uppercase();
        let is_running = status_upper.contains("RUNNING") || status_upper.contains("WAITING");
        out.insert(
            cid.clone(),
            LiveTrajectorySummary {
                status,
                step_count,
                workspace_cwd,
                is_running,
            },
        );
    }
    out
}

/// Queries live trajectory statuses from a running Antigravity daemon (`None` when daemon is offline).
pub fn query_all_cascade_trajectories() -> Option<HashMap<String, LiveTrajectorySummary>> {
    if let Ok(guard) = live_trajectories_cache().read()
        && let Some((ts, ref cached)) = *guard
        && ts.elapsed() < LIVE_TRAJECTORIES_TTL
    {
        return cached.clone();
    }

    let result = discover_live_antigravity_daemon().and_then(|ep| {
        let val = call_daemon_rpc(
            &ep,
            "GetAllCascadeTrajectories",
            &json!({}),
            DAEMON_QUERY_TIMEOUT,
        )
        .ok()?;
        Some(parse_live_trajectories_response(&val))
    });

    if let Ok(mut guard) = live_trajectories_cache().write() {
        *guard = Some((Instant::now(), result.clone()));
    }
    result
}

pub fn extract_generator_metadata_model(val: &Value) -> Option<String> {
    let arr = val.get("generatorMetadata").and_then(Value::as_array)?;
    for entry in arr.iter().rev() {
        let candidates = [
            entry
                .pointer("/chatModel/modelName")
                .and_then(Value::as_str),
            entry.pointer("/chatModel/model").and_then(Value::as_str),
            entry
                .pointer("/chatModel/planModel")
                .and_then(Value::as_str),
        ];
        for raw in candidates.into_iter().flatten() {
            if let Some(resolved) = resolve_antigravity_model_reference(raw) {
                return Some(resolved);
            }
        }
    }
    None
}

/// Queries `GetCascadeTrajectoryGeneratorMetadata` for exact per-conversation model attribution,
/// memoized by `(cascade_id, step_count_hint)` so unchanged sessions incur zero RPC overhead.
pub fn query_trajectory_model(cascade_id: &str, step_count_hint: u64) -> Option<String> {
    let raw_id = cascade_id
        .strip_prefix("antigravity_")
        .unwrap_or(cascade_id)
        .trim();
    if raw_id.is_empty() {
        return None;
    }
    if step_count_hint > 0
        && let Ok(guard) = trajectory_model_cache().read()
        && let Some((cached_steps, cached_model)) = guard.get(raw_id)
        && *cached_steps == step_count_hint
        && cached_model.is_some()
    {
        return cached_model.clone();
    }

    let ep = discover_live_antigravity_daemon()?;
    let val = call_daemon_rpc(
        &ep,
        "GetCascadeTrajectoryGeneratorMetadata",
        &json!({ "cascadeId": raw_id }),
        DAEMON_QUERY_TIMEOUT,
    )
    .ok()?;
    let model = extract_generator_metadata_model(&val);
    if model.is_some()
        && let Ok(mut guard) = trajectory_model_cache().write()
    {
        guard.insert(raw_id.to_owned(), (step_count_hint, model.clone()));
    }
    model
}

/// Encodes a local workspace path into a standard `file:///` URI accepted by `StartCascade`.
pub fn path_to_file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let stripped = raw.strip_prefix("//?/").unwrap_or(&raw);
    let mut encoded = String::with_capacity(stripped.len() + 16);
    if !stripped.starts_with('/') {
        encoded.push_str("file:///");
    } else {
        encoded.push_str("file://");
    }
    for b in stripped.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                encoded.push(b as char);
            }
            _ => encoded.push_str(&format!("%{b:02X}")),
        }
    }
    encoded
}

/// Starts a new Antigravity Desktop conversation via ConnectRPC `StartCascade`.
pub fn start_daemon_cascade(
    endpoint: &AntigravityDaemonEndpoint,
    workspace: Option<&Path>,
    model: Option<&str>,
) -> Result<String, CollectorError> {
    let mut body = json!({});
    if let Some(ws) = workspace.filter(|p| !p.as_os_str().is_empty() && *p != Path::new(".")) {
        let uri = path_to_file_uri(ws);
        body["workspaceUris"] = json!([uri]);
    }
    if let Some(m) = model.and_then(resolve_antigravity_rpc_model_enum) {
        body["requestedModel"] = json!(m);
    }
    let res = call_daemon_rpc(endpoint, "StartCascade", &body, DAEMON_RPC_TIMEOUT)?;
    res.get("cascadeId")
        .or_else(|| res.get("cascade_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| CollectorError::Protocol {
            program: "Antigravity StartCascade".to_owned(),
            code: "missing_cascade_id".to_owned(),
            message: "StartCascade did not return cascadeId".to_owned(),
        })
}

/// Sends a user turn (with optional inline images and model override) via `SendUserCascadeMessage`.
pub fn send_daemon_cascade_message(
    endpoint: &AntigravityDaemonEndpoint,
    cascade_id: &str,
    text: &str,
    image_blocks: &[Value],
    model: Option<&str>,
) -> Result<(), CollectorError> {
    let raw_id = cascade_id
        .strip_prefix("antigravity_")
        .unwrap_or(cascade_id);
    let mut body = json!({
        "cascadeId": raw_id,
        "items": [{ "text": text }],
        "blocking": false,
    });
    let images: Vec<Value> = image_blocks
        .iter()
        .filter_map(|blk| {
            let src = blk.get("source")?;
            let data = src.get("data").and_then(Value::as_str)?;
            let media_type = src
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            Some(json!({
                "base64Data": data,
                "mimeType": media_type,
            }))
        })
        .collect();
    if !images.is_empty() {
        body["images"] = Value::Array(images);
    }
    if let Some(m) = model.and_then(resolve_antigravity_rpc_model_enum) {
        body["cascadeConfig"] = json!({
            "plannerConfig": {
                "requestedModel": { "model": m }
            }
        });
    }
    call_daemon_rpc(
        endpoint,
        "SendUserCascadeMessage",
        &body,
        DAEMON_RPC_TIMEOUT,
    )?;
    invalidate_antigravity_daemon_caches();
    Ok(())
}

/// Cancels an active Antigravity cascade invocation via `CancelCascadeInvocation`.
pub fn cancel_daemon_cascade(cascade_id: &str) -> bool {
    let Some(ep) = discover_live_antigravity_daemon() else {
        return false;
    };
    let raw_id = cascade_id
        .strip_prefix("antigravity_")
        .unwrap_or(cascade_id);
    let ok = call_daemon_rpc(
        &ep,
        "CancelCascadeInvocation",
        &json!({
            "cascadeId": raw_id,
            "killBackgroundTasks": true
        }),
        Duration::from_secs(3),
    )
    .is_ok();
    invalidate_antigravity_daemon_caches();
    ok
}

/// Responds to a pending permission, command, or `ask_question` step on the Antigravity daemon.
pub fn respond_daemon_interaction(
    cascade_id: &str,
    step_index: u64,
    kind: &str,
    result: &Value,
) -> Result<(), CollectorError> {
    let ep = discover_live_antigravity_daemon().ok_or_else(|| CollectorError::Protocol {
        program: "Antigravity HandleCascadeUserInteraction".to_owned(),
        code: "daemon_unavailable".to_owned(),
        message: "Antigravity daemon is not reachable".to_owned(),
    })?;
    let raw_id = cascade_id
        .strip_prefix("antigravity_")
        .unwrap_or(cascade_id);
    if raw_id.len() < 2
        || raw_id.len() > 128
        || !raw_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CollectorError::Protocol {
            program: "Antigravity HandleCascadeUserInteraction".to_owned(),
            code: "invalid_cascade_id".to_owned(),
            message: "invalid Antigravity cascade_id".to_owned(),
        });
    }
    let decision = result.get("decision").and_then(Value::as_str);
    let approved = match decision {
        Some("accept") | Some("allow") => true,
        Some("decline") | Some("deny") | None => false,
        Some(_) => {
            return Err(CollectorError::Protocol {
                program: "Antigravity HandleCascadeUserInteraction".to_owned(),
                code: "invalid_decision".to_owned(),
                message: "invalid interaction decision".to_owned(),
            });
        }
    };

    let mut interaction = json!({
        "trajectoryId": raw_id,
        "stepIndex": step_index,
    });
    match kind {
        "run_command" => {
            interaction["runCommand"] = json!({ "confirm": approved });
        }
        "ask_question" => {
            let answer_text = result
                .get("response")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    result.get("response").and_then(Value::as_array).map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                })
                .unwrap_or_default();
            interaction["askQuestion"] = json!({
                "cancelled": false,
                "responses": [{
                    "question": "",
                    "options": [{ "text": answer_text }]
                }]
            });
        }
        "permission" | "command_status" | "file_permission" => {
            interaction["permission"] = json!({ "allow": approved });
        }
        _ => {
            return Err(CollectorError::Protocol {
                program: "Antigravity HandleCascadeUserInteraction".to_owned(),
                code: "unsupported_interaction_kind".to_owned(),
                message: format!("unsupported interaction kind: {kind}"),
            });
        }
    }

    call_daemon_rpc(
        &ep,
        "HandleCascadeUserInteraction",
        &json!({
            "cascadeId": raw_id,
            "interaction": interaction
        }),
        Duration::from_secs(5),
    )?;
    Ok(())
}

/// Deletes a trajectory from a running Antigravity daemon (`DeleteCascadeTrajectory`).
pub fn delete_daemon_trajectory(cascade_id: &str) -> bool {
    let Some(ep) = discover_live_antigravity_daemon() else {
        return false;
    };
    let raw_id = cascade_id
        .strip_prefix("antigravity_")
        .unwrap_or(cascade_id);
    let ok = call_daemon_rpc(
        &ep,
        "DeleteCascadeTrajectory",
        &json!({ "cascadeId": raw_id }),
        Duration::from_secs(3),
    )
    .is_ok();
    invalidate_antigravity_daemon_caches();
    ok
}
