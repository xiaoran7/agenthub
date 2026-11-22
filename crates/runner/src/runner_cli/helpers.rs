use std::{env, fs, path::PathBuf, time::Duration};

use agenthub_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
    },
    device::DeviceIdentity,
    domain::{ObservationMeta, SessionObservation},
    gateway::GatewayClient,
    outbox::Outbox,
    privacy::ProjectRedactor,
};
use anyhow::{Context, Result, bail};

use super::args::InputFormat;

pub fn gateway_client(
    server: &str,
    timeout: Duration,
    ca_certificate_path: Option<&std::path::Path>,
) -> Result<GatewayClient> {
    let ca_certificate = ca_certificate_path
        .map(fs::read)
        .transpose()
        .map_err(|_| anyhow::anyhow!("gateway_ca_cert_unreadable"))?;
    GatewayClient::new_with_ca_certificate(server, timeout, ca_certificate.as_deref())
        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))
}

pub fn read_json_file<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("invalid JSON in {}", path.display()))
}

pub fn nonzero_duration(name: &str, seconds: u64) -> Result<Duration> {
    if seconds == 0 {
        bail!("{name} must be greater than zero");
    }
    Ok(Duration::from_secs(seconds))
}

pub fn load_redactor(
    environment_variable: &str,
    identity: Option<&DeviceIdentity>,
) -> Result<ProjectRedactor> {
    if let Some(identity) = identity {
        return Ok(ProjectRedactor::new(identity.fingerprint_key())?);
    }
    let key = env::var(environment_variable).with_context(|| {
        format!("missing fingerprint key environment variable {environment_variable}")
    })?;
    Ok(ProjectRedactor::new(key.as_bytes())?)
}

pub fn enqueue_all(outbox: &mut Outbox, observations: &[SessionObservation]) -> Result<Vec<i64>> {
    observations
        .iter()
        .map(|observation| outbox.enqueue(observation).map_err(Into::into))
        .collect()
}

pub fn normalize(
    format: InputFormat,
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<Vec<SessionObservation>> {
    match format {
        InputFormat::ClaudeAgents => Ok(claude::normalize_agents_json(input, meta, redactor)?),
        InputFormat::ClaudeHook => Ok(vec![claude::normalize_hook(input, meta, redactor)?]),
        InputFormat::CodexMessage => {
            let mut context = CodexThreadContext::default();
            Ok(
                codex::normalize_app_server_message(input, meta, redactor, &mut context)?
                    .into_iter()
                    .collect(),
            )
        }
        InputFormat::CodexThreadList => {
            let mut context = CodexThreadContext::default();
            Ok(codex::normalize_thread_list_result(
                input,
                meta,
                redactor,
                &mut context,
            )?)
        }
        InputFormat::CodexHook => Ok(vec![codex::normalize_hook(input, meta, redactor)?]),
        InputFormat::AntigravityBrain => {
            let steps = agenthub_runner::adapters::antigravity::parse_transcript_lines(input)?;
            let meta_session = agenthub_runner::adapters::antigravity::AntigravitySessionMetadata {
                session_id: "preview-session".into(),
                cwd: ".".into(),
                is_process_alive: true,
                started_at: None,
                last_activity_at: None,
            };
            Ok(
                agenthub_runner::adapters::antigravity::normalize_transcript_steps(
                    &steps,
                    &meta_session,
                    meta,
                    redactor,
                )
                .into_iter()
                .collect(),
            )
        }
        InputFormat::AntigravityHook => Ok(vec![
            agenthub_runner::adapters::antigravity::normalize_hook(input, meta, redactor)?,
        ]),
    }
    .and_then(|observations| {
        if observations.is_empty() {
            bail!("input produced no session observations");
        }
        Ok(observations)
    })
}

pub fn resolve_bound_gateway_server() -> Option<String> {
    let home_dir = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)?;
    let binding_path = home_dir.join(".agenthub").join("binding.json");
    if !binding_path.exists() {
        return None;
    }
    let content = fs::read_to_string(&binding_path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    let registered = val
        .get("registered")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !registered {
        return None;
    }
    let gateway = val.get("gateway").and_then(|v| v.as_str())?.trim();
    if gateway.is_empty() {
        None
    } else {
        Some(gateway.to_string())
    }
}

pub fn resolve_or_create_default_identity() -> DeviceIdentity {
    let identity_path = default_identity_path();
    let agenthub_dir = identity_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if identity_path.exists() {
        if let Ok(identity) = DeviceIdentity::load(&identity_path) {
            return identity;
        }
    }

    let identity = DeviceIdentity::generate();
    if fs::create_dir_all(&agenthub_dir).is_ok() {
        let _ = identity.save_new(&identity_path);
    }
    identity
}

/// The identity file the Runner uses when no path is given on the command line.
pub fn default_identity_path() -> PathBuf {
    let home_dir = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home_dir.join(".agenthub").join("identity.json")
}

pub fn resolve_default_db_path() -> PathBuf {
    let home_dir = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let agenthub_dir = home_dir.join(".agenthub");
    let _ = fs::create_dir_all(&agenthub_dir);
    let target_db = agenthub_dir.join("agenthub.db");

    // 平滑迁移与自动清理：如果当前工作目录存在历史遗留的 agenthub.db，且 ~/.agenthub/ 下尚无数据库
    // 则完整将本地的 db 及 wal/shm 复制到 ~/.agenthub/ 并安全清除本地文件，保证桌面绝对干净
    let local_db = PathBuf::from("agenthub.db");
    if local_db.is_file() {
        if !target_db.exists() {
            if fs::copy(&local_db, &target_db).is_ok() {
                let _ = fs::remove_file(&local_db);
                let local_wal = PathBuf::from("agenthub.db-wal");
                let target_wal = agenthub_dir.join("agenthub.db-wal");
                if local_wal.is_file() {
                    let _ = fs::copy(&local_wal, &target_wal);
                    let _ = fs::remove_file(&local_wal);
                }
                let local_shm = PathBuf::from("agenthub.db-shm");
                let target_shm = agenthub_dir.join("agenthub.db-shm");
                if local_shm.is_file() {
                    let _ = fs::copy(&local_shm, &target_shm);
                    let _ = fs::remove_file(&local_shm);
                }
            }
        }
    }

    target_db
}

pub fn hide_windows_console() {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Console::FreeConsole;

        // Detach only this daemon process; never call `ShowWindow(hwnd, SW_HIDE)` on
        // a shared parent terminal window.
        let _ = FreeConsole();
    }
}

pub fn collect_once_observations(
    agent: super::args::Agent,
    binary: Option<PathBuf>,
    include_completed: bool,
    limit: u32,
    timeout: Duration,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<Vec<SessionObservation>> {
    use super::args::Agent as CliAgent;
    use agenthub_runner::adapters::antigravity::{
        AntigravitySessionMetadata, AntigravityStep, normalize_transcript_steps,
    };
    use agenthub_runner::collectors::{
        CodexAppServer, collect_antigravity_sessions, collect_claude_transcripts,
        collect_codex_local_threads,
    };

    match agent {
        CliAgent::Claude => {
            let _ = binary;
            let payload = collect_claude_transcripts(include_completed)?;
            Ok(claude::normalize_agents_json(&payload, meta, redactor)?)
        }
        CliAgent::Codex => {
            let binary = binary.unwrap_or_else(|| PathBuf::from("codex"));
            let (result, local) = match CodexAppServer::start_with_timeout(binary, timeout)
                .and_then(|mut server| server.list_threads(limit))
            {
                Ok(result) => (result, false),
                Err(_) => (collect_codex_local_threads(limit)?, true),
            };
            let payload = serde_json::to_string(&result)?;
            let mut context = CodexThreadContext::default();
            if local {
                Ok(codex::normalize_local_thread_list_result(
                    &payload,
                    meta,
                    redactor,
                    &mut context,
                )?)
            } else {
                Ok(codex::normalize_thread_list_result(
                    &payload,
                    meta,
                    redactor,
                    &mut context,
                )?)
            }
        }
        CliAgent::Antigravity => {
            let _ = binary;
            let payload = collect_antigravity_sessions(include_completed)?;
            let sessions: Vec<serde_json::Value> = serde_json::from_str(&payload)?;
            let mut observations = Vec::new();
            for session in sessions {
                let session_id = session
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let cwd = session
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let started_at = session
                    .get("startedAt")
                    .and_then(|v| v.as_i64())
                    .and_then(chrono::DateTime::from_timestamp_millis);
                let last_activity_at = session
                    .get("lastActivityAt")
                    .and_then(|v| v.as_i64())
                    .and_then(chrono::DateTime::from_timestamp_millis);
                let steps_val = session
                    .get("steps")
                    .cloned()
                    .unwrap_or(serde_json::json!([]));
                let steps: Vec<AntigravityStep> =
                    serde_json::from_value(steps_val).unwrap_or_default();
                let session_meta = AntigravitySessionMetadata {
                    session_id,
                    cwd,
                    is_process_alive: true,
                    started_at,
                    last_activity_at,
                };
                if let Some(obs) = normalize_transcript_steps(&steps, &session_meta, meta, redactor)
                {
                    observations.push(obs);
                }
            }
            Ok(observations)
        }
    }
}
