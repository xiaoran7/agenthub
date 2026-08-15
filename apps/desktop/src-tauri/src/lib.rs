use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use agent_pulse_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
    },
    collectors::{
        CodexAppServer, collect_claude_transcripts, collect_codex_local_threads,
        run_agent_session_turn,
    },
    device::DeviceIdentity,
    domain::{AgentType, ObservationMeta, SessionObservation},
    gateway::GatewayClient,
    outbox::Outbox,
    privacy::ProjectRedactor,
    runtime::{ClaudeSessionSource, CodexSessionSource, PollPolicy, RunnerRuntime, SessionSource},
    sync::MAX_SYNC_BATCH_EVENTS,
    sync_worker::{SyncPolicy, SyncWorker},
};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{Manager, State};
use thiserror::Error;
use url::{Host, Url};

const DEFAULT_SERVER_URL: &str = "https://platform.tannerlab.xyz";
const KEYRING_SERVICE: &str = "AgentPulse Desktop";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const LOCAL_SESSION_LIMIT: u32 = 1_000;
const DESKTOP_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(5);
const AGENT_TURN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_PROMPT_CHARS: usize = 16_000;
const MAX_SESSION_ID_CHARS: usize = 160;

#[derive(Debug, Error)]
enum DesktopError {
    #[error("服务器地址无效")]
    InvalidServer,
    #[error("无法连接 AgentPulse 服务器")]
    Unreachable,
    #[error("服务器返回了无效响应")]
    InvalidResponse,
    #[error("登录信息不正确")]
    InvalidCredentials,
    #[error("登录尝试过多，请稍后重试")]
    LoginRateLimited,
    #[error("请先验证邮箱，然后重试自动连接")]
    EmailVerificationRequired,
    #[error("这台设备已绑定到另一个账号")]
    DeviceConflict,
    #[error("本机安全存储不可用")]
    SecureStorage,
    #[error("无法初始化本机设备身份")]
    Identity,
    #[error("无法启动本地同步")]
    Sync,
    #[error("当前没有登录")]
    NotAuthenticated,
    #[error("会话参数无效")]
    InvalidAction,
    #[error("本机会话已不可用，请刷新后重试")]
    SessionUnavailable,
    #[error("Agent 执行失败；请检查 CLI 登录状态和控制路径")]
    AgentFailed,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct AuthInput {
    mode: AuthMode,
    email: String,
    password: String,
    display_name: Option<String>,
    server_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthMode {
    Login,
    Register,
}

#[derive(Debug, Serialize, Deserialize)]
struct DesktopConfig {
    server_url: String,
    #[serde(default = "default_refresh_interval_seconds")]
    refresh_interval_seconds: u64,
    #[serde(default = "default_claude_binary")]
    claude_binary: String,
    #[serde(default = "default_codex_binary")]
    codex_binary: String,
    #[serde(default)]
    archived_sessions: Vec<ArchivedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchivedSession {
    agent_type: AgentType,
    agent_session_id: String,
    project_display_name: String,
    project_fingerprint: String,
    observed_at: String,
}

#[derive(Debug, Deserialize)]
struct SessionMessageInput {
    agent_type: AgentType,
    agent_session_id: String,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct ArchiveSessionInput {
    agent_type: AgentType,
    agent_session_id: String,
    project_display_name: String,
    project_fingerprint: String,
    observed_at: String,
    archived: bool,
}

#[derive(Debug, Deserialize)]
struct DesktopPreferencesInput {
    refresh_interval_seconds: u64,
    claude_binary: String,
    codex_binary: String,
}

fn default_refresh_interval_seconds() -> u64 {
    4
}

fn default_claude_binary() -> String {
    "claude".to_owned()
}

fn default_codex_binary() -> String {
    "codex".to_owned()
}

struct EngineHandle {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl EngineHandle {
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

struct DesktopState {
    data_dir: PathBuf,
    engine: Mutex<Option<EngineHandle>>,
}

#[tauri::command]
async fn authenticate(input: AuthInput, state: State<'_, DesktopState>) -> Result<Value, String> {
    authenticate_inner(input, &state).map_err(|error| error.to_string())
}

#[tauri::command]
async fn restore_session(state: State<'_, DesktopState>) -> Result<Option<Value>, String> {
    let config = match read_config(&state.data_dir) {
        Ok(config) => config,
        Err(DesktopError::NotAuthenticated) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let cookie = match load_cookie(&config.server_url) {
        Ok(cookie) => cookie,
        Err(DesktopError::NotAuthenticated) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let identity = load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
    ensure_device_bound(&config.server_url, &cookie, &identity)
        .map_err(|error| error.to_string())?;
    start_engine(&state, &config.server_url, &identity).map_err(|error| error.to_string())?;
    let mut snapshot = dashboard_snapshot(&config.server_url, &cookie, &identity, &state)
        .map_err(|error| error.to_string())?;
    snapshot["sync_running"] = Value::Bool(true);
    Ok(Some(snapshot))
}

#[tauri::command]
async fn refresh_dashboard(state: State<'_, DesktopState>) -> Result<Value, String> {
    let config = read_config(&state.data_dir).map_err(|error| error.to_string())?;
    let cookie = load_cookie(&config.server_url).map_err(|error| error.to_string())?;
    let identity = load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
    dashboard_snapshot(&config.server_url, &cookie, &identity, &state)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn send_session_message(
    input: SessionMessageInput,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || send_session_message_inner(input, &data_dir))
        .await
        .map_err(|_| DesktopError::AgentFailed.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn set_session_archived(
    input: ArchiveSessionInput,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || set_session_archived_inner(input, &data_dir))
        .await
        .map_err(|_| DesktopError::AgentFailed.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_desktop_preferences(
    input: DesktopPreferencesInput,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let mut config = read_config(&state.data_dir).map_err(|error| error.to_string())?;
    if !(2..=60).contains(&input.refresh_interval_seconds)
        || !valid_binary(&input.claude_binary)
        || !valid_binary(&input.codex_binary)
    {
        return Err(DesktopError::InvalidAction.to_string());
    }
    config.refresh_interval_seconds = input.refresh_interval_seconds;
    config.claude_binary = input.claude_binary.trim().to_owned();
    config.codex_binary = input.codex_binary.trim().to_owned();
    write_config(&state.data_dir, &config).map_err(|error| error.to_string())?;
    Ok(preferences_json(&config))
}

#[tauri::command]
async fn set_sync_paused(paused: bool, state: State<'_, DesktopState>) -> Result<Value, String> {
    let mut engine = state
        .engine
        .lock()
        .map_err(|_| DesktopError::Sync.to_string())?;
    if paused {
        if let Some(handle) = engine.take() {
            handle.stop();
        }
    } else if engine.is_none() {
        let config = read_config(&state.data_dir).map_err(|error| error.to_string())?;
        let identity =
            load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
        *engine = Some(
            build_engine(&state.data_dir, &config.server_url, &identity)
                .map_err(|error| error.to_string())?,
        );
    }
    Ok(json!({ "paused": paused }))
}

#[tauri::command]
async fn logout(state: State<'_, DesktopState>) -> Result<(), String> {
    let config = read_config(&state.data_dir).ok();
    if let Ok(mut engine) = state.engine.lock()
        && let Some(handle) = engine.take()
    {
        handle.stop();
    }
    if let Some(config) = config {
        if let Ok(cookie) = load_cookie(&config.server_url) {
            let _ = request_json(
                "POST",
                &config.server_url,
                "/v1/auth/logout",
                Some(&cookie),
                Some(&json!({})),
            );
        }
        let _ = delete_cookie(&config.server_url);
    }
    let _ = fs::remove_file(config_path(&state.data_dir));
    Ok(())
}

fn authenticate_inner(input: AuthInput, state: &DesktopState) -> Result<Value, DesktopError> {
    let configured_server = input
        .server_url
        .or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))?;
    let path = match input.mode {
        AuthMode::Login => "/v1/auth/login",
        AuthMode::Register => "/v1/auth/register",
    };
    let body = match input.mode {
        AuthMode::Login => json!({ "email": input.email, "password": input.password }),
        AuthMode::Register => json!({
            "email": input.email,
            "password": input.password,
            "display_name": input.display_name.unwrap_or_default()
        }),
    };
    let (account, cookie) = authenticate_request(&server_url, path, &body)?;
    store_cookie(&server_url, &cookie)?;
    write_config(
        &state.data_dir,
        &DesktopConfig {
            server_url: server_url.clone(),
            refresh_interval_seconds: default_refresh_interval_seconds(),
            claude_binary: default_claude_binary(),
            codex_binary: default_codex_binary(),
            archived_sessions: Vec::new(),
        },
    )?;

    let identity = load_or_create_identity(&state.data_dir)?;
    ensure_device_bound(&server_url, &cookie, &identity)?;
    start_engine(state, &server_url, &identity)?;
    let mut snapshot = dashboard_snapshot(&server_url, &cookie, &identity, state)?;
    snapshot["account"] = account;
    Ok(snapshot)
}

fn authenticate_request(
    server_url: &str,
    path: &str,
    body: &Value,
) -> Result<(Value, String), DesktopError> {
    let agent = http_agent();
    let url = format!("{server_url}{path}");
    let mut response = agent.post(&url).send_json(body).map_err(map_http_error)?;
    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(extract_session_cookie)
        .ok_or(DesktopError::InvalidResponse)?;
    let account = response
        .body_mut()
        .read_json::<Value>()
        .map_err(|_| DesktopError::InvalidResponse)?;
    Ok((account, cookie))
}

fn ensure_device_bound(
    server_url: &str,
    cookie: &str,
    identity: &DeviceIdentity,
) -> Result<(), DesktopError> {
    let devices = request_json("GET", server_url, "/v1/devices", Some(cookie), None)?;
    let already_bound = devices["devices"].as_array().is_some_and(|devices| {
        devices.iter().any(|device| {
            device["device_id"].as_str() == Some(identity.device_id())
                && device["revoked"].as_bool() != Some(true)
        })
    });
    if already_bound {
        return Ok(());
    }
    let binding = request_json(
        "POST",
        server_url,
        "/v1/device-bindings",
        Some(cookie),
        Some(&json!({})),
    )?;
    let code = binding["binding_code"]
        .as_str()
        .ok_or(DesktopError::InvalidResponse)?;
    GatewayClient::new(server_url, REQUEST_TIMEOUT)
        .and_then(|client| client.bind(identity, code))
        .map_err(|error| match error {
            agent_pulse_runner::gateway::GatewayError::Rejected { status: 409 } => {
                DesktopError::DeviceConflict
            }
            _ => DesktopError::Sync,
        })?;
    Ok(())
}

fn dashboard_snapshot(
    server_url: &str,
    cookie: &str,
    identity: &DeviceIdentity,
    state: &DesktopState,
) -> Result<Value, DesktopError> {
    let desktop_config = read_config(&state.data_dir)?;
    let (account, devices, remote_sessions) = thread::scope(|scope| {
        let account =
            scope.spawn(|| request_json("GET", server_url, "/v1/auth/me", Some(cookie), None));
        let devices =
            scope.spawn(|| request_json("GET", server_url, "/v1/devices", Some(cookie), None));
        let sessions =
            scope.spawn(|| request_json("GET", server_url, "/v1/sessions", Some(cookie), None));
        Ok::<_, DesktopError>((
            account
                .join()
                .map_err(|_| DesktopError::InvalidResponse)??,
            devices
                .join()
                .map_err(|_| DesktopError::InvalidResponse)??,
            sessions
                .join()
                .map_err(|_| DesktopError::InvalidResponse)??,
        ))
    })?;
    let outbox =
        Outbox::open(state.data_dir.join("outbox.sqlite")).map_err(|_| DesktopError::Sync)?;
    let stats = outbox.stats().map_err(|_| DesktopError::Sync)?;
    let local_sessions = outbox.snapshots().map_err(|_| DesktopError::Sync)?;
    let claude_health = outbox
        .adapter_health(AgentType::ClaudeCode)
        .map_err(|_| DesktopError::Sync)?;
    let codex_health = outbox
        .adapter_health(AgentType::Codex)
        .map_err(|_| DesktopError::Sync)?;
    let sessions = merge_dashboard_sessions(
        &remote_sessions["sessions"],
        &local_sessions,
        &desktop_config.archived_sessions,
        identity.device_id(),
    );
    let running = state
        .engine
        .lock()
        .map(|engine| engine.is_some())
        .unwrap_or(false);
    Ok(json!({
        "account": account,
        "devices": devices["devices"],
        "sessions": sessions,
        "device": {
            "device_id": identity.device_id(),
            "name": machine_name()
        },
        "local": {
            "pending_events": stats.pending_events,
            "session_count": local_sessions.len(),
            "sources": [
                local_source_summary(AgentType::ClaudeCode, &local_sessions, claude_health),
                local_source_summary(AgentType::Codex, &local_sessions, codex_health)
            ],
            "memory": local_memory_summary(),
            "refreshed_at": chrono::Utc::now()
        },
        "sync_running": running,
        "server_url": server_url,
        "preferences": preferences_json(&desktop_config)
    }))
}

fn send_session_message_inner(
    input: SessionMessageInput,
    data_dir: &Path,
) -> Result<Value, DesktopError> {
    validate_session_id(&input.agent_session_id)?;
    let prompt = input.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(DesktopError::InvalidAction);
    }
    let config = read_config(data_dir)?;
    let binary = match input.agent_type {
        AgentType::ClaudeCode => &config.claude_binary,
        AgentType::Codex => &config.codex_binary,
    };
    let project_path = match input.agent_type {
        AgentType::ClaudeCode => Some(
            find_claude_session_cwd(&input.agent_session_id)?
                .ok_or(DesktopError::SessionUnavailable)?,
        ),
        AgentType::Codex => None,
    };
    let output = run_agent_session_turn(
        input.agent_type,
        binary,
        project_path.as_deref(),
        &input.agent_session_id,
        prompt,
        AGENT_TURN_TIMEOUT,
    )
    .map_err(|_| DesktopError::AgentFailed)?;
    let response = match input.agent_type {
        AgentType::ClaudeCode => output.trim().to_owned(),
        AgentType::Codex => codex_final_message(&output).ok_or(DesktopError::AgentFailed)?,
    };
    if response.is_empty() {
        return Err(DesktopError::AgentFailed);
    }
    Ok(json!({
        "response": response,
        "completed_at": chrono::Utc::now(),
    }))
}

fn set_session_archived_inner(
    input: ArchiveSessionInput,
    data_dir: &Path,
) -> Result<Value, DesktopError> {
    validate_session_id(&input.agent_session_id)?;
    if input.project_display_name.chars().count() > 160
        || input.project_fingerprint.chars().count() > 160
        || chrono::DateTime::parse_from_rfc3339(&input.observed_at).is_err()
    {
        return Err(DesktopError::InvalidAction);
    }
    let mut config = read_config(data_dir)?;
    if input.agent_type == AgentType::Codex {
        let mut server =
            CodexAppServer::start_with_timeout(&config.codex_binary, DESKTOP_COLLECTOR_TIMEOUT)
                .map_err(|_| DesktopError::AgentFailed)?;
        server
            .request(
                if input.archived {
                    "thread/archive"
                } else {
                    "thread/unarchive"
                },
                json!({ "threadId": input.agent_session_id }),
            )
            .map_err(|_| DesktopError::AgentFailed)?;
    }
    config.archived_sessions.retain(|session| {
        session.agent_type != input.agent_type || session.agent_session_id != input.agent_session_id
    });
    if input.archived {
        config.archived_sessions.push(ArchivedSession {
            agent_type: input.agent_type,
            agent_session_id: input.agent_session_id,
            project_display_name: input.project_display_name,
            project_fingerprint: input.project_fingerprint,
            observed_at: input.observed_at,
        });
    }
    write_config(data_dir, &config)?;
    Ok(json!({ "archived": input.archived }))
}

fn preferences_json(config: &DesktopConfig) -> Value {
    json!({
        "refresh_interval_seconds": config.refresh_interval_seconds,
        "claude_binary": config.claude_binary,
        "codex_binary": config.codex_binary,
    })
}

fn valid_binary(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.chars().count() <= 260 && !value.chars().any(char::is_control)
}

fn validate_session_id(value: &str) -> Result<(), DesktopError> {
    if value.is_empty()
        || value.chars().count() > MAX_SESSION_ID_CHARS
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(DesktopError::InvalidAction);
    }
    Ok(())
}

fn find_claude_session_cwd(session_id: &str) -> Result<Option<PathBuf>, DesktopError> {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    let projects = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("projects");
    let filename = format!("{session_id}.jsonl");
    let entries = fs::read_dir(projects).map_err(|_| DesktopError::SessionUnavailable)?;
    for project in entries.flatten() {
        let path = project.path().join(&filename);
        let Ok(file) = fs::File::open(path) else {
            continue;
        };
        for line in BufReader::new(file).lines().take(128).map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let Some(cwd) = value.get("cwd").and_then(Value::as_str) else {
                continue;
            };
            let cwd = PathBuf::from(cwd);
            if cwd.is_absolute() && cwd.is_dir() {
                return Ok(Some(cwd));
            }
        }
    }
    Ok(None)
}

fn codex_final_message(output: &str) -> Option<String> {
    output
        .lines()
        .filter_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            let item = value.get("item")?;
            if value.get("type")?.as_str()? != "item.completed"
                || item.get("type")?.as_str()? != "agent_message"
            {
                return None;
            }
            Some(item.get("text")?.as_str()?.trim().to_owned())
        })
        .rfind(|message| !message.is_empty())
}

fn start_engine(
    state: &DesktopState,
    server_url: &str,
    identity: &DeviceIdentity,
) -> Result<(), DesktopError> {
    let mut engine = state.engine.lock().map_err(|_| DesktopError::Sync)?;
    if engine.is_none() {
        *engine = Some(build_engine(&state.data_dir, server_url, identity)?);
    }
    Ok(())
}

fn build_engine(
    data_dir: &Path,
    server_url: &str,
    identity: &DeviceIdentity,
) -> Result<EngineHandle, DesktopError> {
    let database_path = data_dir.join("outbox.sqlite");
    let identity_path = data_dir.join("device-identity.json");
    let sync_identity = DeviceIdentity::load(&identity_path).map_err(|_| DesktopError::Identity)?;
    let stop = Arc::new(AtomicBool::new(false));
    let client = GatewayClient::new(server_url, REQUEST_TIMEOUT).map_err(|_| DesktopError::Sync)?;
    let sources: Vec<Box<dyn SessionSource>> = vec![
        Box::new(ClaudeSessionSource::new(true)),
        Box::new(CodexSessionSource::new_with_timeout(
            "codex",
            LOCAL_SESSION_LIMIT,
            DESKTOP_COLLECTOR_TIMEOUT,
        )),
    ];
    let redactor =
        ProjectRedactor::new(identity.fingerprint_key()).map_err(|_| DesktopError::Sync)?;
    let mut outbox = Outbox::open(&database_path).map_err(|_| DesktopError::Sync)?;
    bootstrap_local_sessions(identity, &redactor, &mut outbox)?;
    let sync_worker = SyncWorker::start(
        database_path.clone(),
        client,
        sync_identity,
        SyncPolicy::default(),
        MAX_SYNC_BATCH_EVENTS.min(100),
        20,
        Arc::clone(&stop),
    )
    .map_err(|_| DesktopError::Sync)?;
    let mut runtime = RunnerRuntime::new(
        identity.device_id(),
        redactor,
        outbox,
        PollPolicy::default(),
        sources,
    )
    .with_sync_trigger(sync_worker.trigger_sender());
    let thread_stop = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("agent-pulse-desktop-engine".into())
        .spawn(move || {
            let _ = runtime.run(Arc::clone(&thread_stop), None);
            thread_stop.store(true, Ordering::Relaxed);
            let _ = sync_worker.join();
        })
        .map_err(|_| DesktopError::Sync)?;
    Ok(EngineHandle { stop, thread })
}

fn bootstrap_local_sessions(
    identity: &DeviceIdentity,
    redactor: &ProjectRedactor,
    outbox: &mut Outbox,
) -> Result<(), DesktopError> {
    let meta = ObservationMeta::now(identity.device_id());
    let next_attempt = meta.observed_at + chrono::Duration::seconds(10);

    match collect_claude_transcripts(true)
        .map_err(|error| error.code().to_owned())
        .and_then(|payload| {
            claude::normalize_agents_json(&payload, &meta, redactor)
                .map_err(|_| "claude_payload_invalid".to_owned())
        }) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::ClaudeCode,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(code) => {
            outbox
                .record_adapter_health(
                    AgentType::ClaudeCode,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(&code),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }

    match collect_codex_local_threads(LOCAL_SESSION_LIMIT)
        .map_err(|error| error.code().to_owned())
        .and_then(|value| {
            let payload =
                serde_json::to_string(&value).map_err(|_| "codex_payload_invalid".to_owned())?;
            codex::normalize_local_thread_list_result(
                &payload,
                &meta,
                redactor,
                &mut CodexThreadContext::default(),
            )
            .map_err(|_| "codex_payload_invalid".to_owned())
        }) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::Codex,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(code) => {
            outbox
                .record_adapter_health(
                    AgentType::Codex,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(&code),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }
    Ok(())
}

fn merge_dashboard_sessions(
    remote: &Value,
    local: &[SessionObservation],
    archived: &[ArchivedSession],
    device_id: &str,
) -> Vec<Value> {
    let mut sessions = std::collections::BTreeMap::new();
    if let Some(remote) = remote.as_array() {
        for session in remote {
            if let Some(key) = dashboard_session_key(session) {
                let mut session = session.clone();
                session["archived"] = Value::Bool(archived.iter().any(|item| {
                    session["device_id"].as_str() == Some(device_id)
                        && agent_type_name(item.agent_type)
                            == session["agent_type"].as_str().unwrap_or_default()
                        && item.agent_session_id
                            == session["agent_session_id"].as_str().unwrap_or_default()
                }));
                sessions.insert(key, session);
            }
        }
    }
    for observation in local {
        let key = observation.session_key();
        let sync_state = if sessions.contains_key(&key) {
            "synced"
        } else {
            "local"
        };
        sessions.insert(
            key,
            json!({
                "event_id": observation.event_id,
                "device_id": observation.device_id,
                "agent_type": observation.agent_type,
                "agent_session_id": observation.agent_session_id,
                "observed_at": observation.observed_at,
                "payload": {
                    "project": observation.project,
                    "state": observation.state,
                    "source": observation.source,
                    "confidence": observation.confidence,
                    "started_at": observation.started_at,
                    "last_activity_at": observation.last_activity_at
                },
                "sync_state": sync_state
                ,"archived": archived.iter().any(|item| {
                    item.agent_type == observation.agent_type
                        && item.agent_session_id == observation.agent_session_id
                })
            }),
        );
    }
    for item in archived {
        let key = format!(
            "{}:{}:{}",
            agent_type_name(item.agent_type),
            device_id,
            item.agent_session_id
        );
        sessions.entry(key).or_insert_with(|| {
            json!({
                "event_id": format!("archived:{}:{}", agent_type_name(item.agent_type), item.agent_session_id),
                "device_id": device_id,
                "agent_type": item.agent_type,
                "agent_session_id": item.agent_session_id,
                "observed_at": item.observed_at,
                "payload": {
                    "project": {
                        "display_name": item.project_display_name,
                        "fingerprint": item.project_fingerprint,
                    },
                    "state": { "activity_status": "UNKNOWN", "presence": "PERSISTED" },
                    "last_activity_at": item.observed_at,
                },
                "sync_state": "synced",
                "archived": true,
            })
        });
    }
    sessions.into_values().collect()
}

fn agent_type_name(agent_type: AgentType) -> &'static str {
    match agent_type {
        AgentType::ClaudeCode => "claude_code",
        AgentType::Codex => "codex",
    }
}

fn dashboard_session_key(session: &Value) -> Option<String> {
    Some(format!(
        "{}:{}:{}",
        session.get("agent_type")?.as_str()?,
        session.get("device_id")?.as_str()?,
        session.get("agent_session_id")?.as_str()?
    ))
}

fn local_source_summary(
    agent_type: AgentType,
    sessions: &[SessionObservation],
    health: Option<agent_pulse_runner::outbox::AdapterHealth>,
) -> Value {
    let session_count = sessions
        .iter()
        .filter(|session| session.agent_type == agent_type)
        .count();
    json!({
        "agent_type": agent_type,
        "session_count": session_count,
        "detected": session_count > 0 || health.as_ref().is_some_and(|value| value.status == "healthy"),
        "status": health.as_ref().map(|value| value.status.as_str()).unwrap_or("scanning"),
        "last_success_at": health.as_ref().and_then(|value| value.last_success_at),
        "error_code": health.and_then(|value| value.last_error_code)
    })
}

fn local_memory_summary() -> Value {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return json!({ "claude_files": 0, "claude_bytes": 0, "codex_indexes": 0, "codex_bytes": 0 });
    };
    let claude_projects = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
        .join("projects");
    let mut claude_files = 0_u64;
    let mut claude_bytes = 0_u64;
    if let Ok(projects) = fs::read_dir(claude_projects) {
        for project in projects.flatten() {
            let memory = project.path().join("memory");
            if let Ok(files) = fs::read_dir(memory) {
                for file in files.flatten() {
                    if file.file_type().is_ok_and(|kind| kind.is_file()) {
                        claude_files = claude_files.saturating_add(1);
                        claude_bytes = claude_bytes.saturating_add(
                            file.metadata().map(|value| value.len()).unwrap_or_default(),
                        );
                    }
                }
            }
        }
    }
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let mut codex_indexes = 0_u64;
    let mut codex_bytes = 0_u64;
    if let Ok(files) = fs::read_dir(codex_home) {
        for file in files.flatten() {
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("memories_")
                && name.ends_with(".sqlite")
                && file.file_type().is_ok_and(|kind| kind.is_file())
            {
                codex_indexes = codex_indexes.saturating_add(1);
                codex_bytes = codex_bytes
                    .saturating_add(file.metadata().map(|value| value.len()).unwrap_or_default());
            }
        }
    }
    json!({
        "claude_files": claude_files,
        "claude_bytes": claude_bytes,
        "codex_indexes": codex_indexes,
        "codex_bytes": codex_bytes
    })
}

fn request_json(
    method: &str,
    server_url: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<&Value>,
) -> Result<Value, DesktopError> {
    let url = format!("{server_url}{path}");
    let agent = http_agent();
    let mut response = match method {
        "GET" => {
            let mut request = agent.get(&url);
            if let Some(cookie) = cookie {
                request = request.header("cookie", cookie);
            }
            request.call()
        }
        "POST" => {
            let mut request = agent.post(&url);
            if let Some(cookie) = cookie {
                request = request.header("cookie", cookie);
            }
            request.send_json(body.unwrap_or(&Value::Null))
        }
        _ => return Err(DesktopError::InvalidResponse),
    }
    .map_err(map_http_error)?;
    if response.status().as_u16() == 204 {
        return Ok(json!({}));
    }
    response
        .body_mut()
        .read_json::<Value>()
        .map_err(|_| DesktopError::InvalidResponse)
}

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .user_agent(concat!("agent-pulse-desktop/", env!("CARGO_PKG_VERSION")))
        .max_redirects(0)
        .build()
        .new_agent()
}

fn map_http_error(error: ureq::Error) -> DesktopError {
    match error {
        ureq::Error::StatusCode(401) => DesktopError::InvalidCredentials,
        ureq::Error::StatusCode(429) => DesktopError::LoginRateLimited,
        ureq::Error::StatusCode(403) => DesktopError::EmailVerificationRequired,
        ureq::Error::StatusCode(409) => DesktopError::DeviceConflict,
        ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Io(_)
        | ureq::Error::Tls(_)
        | ureq::Error::Rustls(_) => DesktopError::Unreachable,
        _ => DesktopError::InvalidResponse,
    }
}

fn normalize_server_url(value: &str) -> Result<String, DesktopError> {
    let url = Url::parse(value).map_err(|_| DesktopError::InvalidServer)?;
    let loopback = match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
    {
        return Err(DesktopError::InvalidServer);
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn extract_session_cookie(header: &str) -> Option<String> {
    let first = header.split(';').next()?.trim();
    let (name, token) = first.split_once('=')?;
    if !matches!(name, "agent_pulse_session" | "__Host-agent_pulse_session")
        || token.len() != 43
        || !token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return None;
    }
    Some(format!("{name}={token}"))
}

fn credential_entry(server_url: &str) -> Result<Entry, DesktopError> {
    let digest = Sha256::digest(server_url.as_bytes());
    Entry::new(KEYRING_SERVICE, &format!("server-{}", hex_prefix(&digest)))
        .map_err(|_| DesktopError::SecureStorage)
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn store_cookie(server_url: &str, cookie: &str) -> Result<(), DesktopError> {
    credential_entry(server_url)?
        .set_password(cookie)
        .map_err(|_| DesktopError::SecureStorage)
}

fn load_cookie(server_url: &str) -> Result<String, DesktopError> {
    credential_entry(server_url)?
        .get_password()
        .map_err(|_| DesktopError::NotAuthenticated)
}

fn delete_cookie(server_url: &str) -> Result<(), DesktopError> {
    credential_entry(server_url)?
        .delete_credential()
        .map_err(|_| DesktopError::SecureStorage)
}

fn load_or_create_identity(data_dir: &Path) -> Result<DeviceIdentity, DesktopError> {
    fs::create_dir_all(data_dir).map_err(|_| DesktopError::Identity)?;
    let path = data_dir.join("device-identity.json");
    if path.exists() {
        return DeviceIdentity::load(path).map_err(|_| DesktopError::Identity);
    }
    let identity = DeviceIdentity::generate();
    identity
        .save_new(path)
        .map_err(|_| DesktopError::Identity)?;
    Ok(identity)
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("desktop-config.json")
}

fn write_config(data_dir: &Path, config: &DesktopConfig) -> Result<(), DesktopError> {
    fs::create_dir_all(data_dir).map_err(|_| DesktopError::Identity)?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|_| DesktopError::InvalidResponse)?;
    fs::write(config_path(data_dir), bytes).map_err(|_| DesktopError::Identity)
}

fn read_config(data_dir: &Path) -> Result<DesktopConfig, DesktopError> {
    let bytes = fs::read(config_path(data_dir)).map_err(|_| DesktopError::NotAuthenticated)?;
    serde_json::from_slice(&bytes).map_err(|_| DesktopError::NotAuthenticated)
}

fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "这台电脑".to_owned())
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            app.manage(DesktopState {
                data_dir,
                engine: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            authenticate,
            restore_session,
            refresh_dashboard,
            send_session_message,
            set_session_archived,
            save_desktop_preferences,
            set_sync_paused,
            logout
        ])
        .build(tauri::generate_context!())
        .expect("failed to build AgentPulse desktop app")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app_handle.try_state::<DesktopState>()
                && let Ok(mut engine) = state.engine.lock()
                && let Some(handle) = engine.take()
            {
                handle.stop();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_the_session_cookie_pair() {
        let token = "A".repeat(43);
        let header = format!("__Host-agent_pulse_session={token}; Path=/; Secure; HttpOnly");
        assert_eq!(
            extract_session_cookie(&header),
            Some(format!("__Host-agent_pulse_session={token}"))
        );
        assert!(extract_session_cookie("other=secret; Path=/").is_none());
    }

    #[test]
    fn rejects_insecure_remote_servers() {
        assert!(normalize_server_url("https://platform.tannerlab.xyz/").is_ok());
        assert!(normalize_server_url("http://localhost:3000").is_ok());
        assert!(normalize_server_url("http://example.com").is_err());
    }

    #[test]
    fn extracts_the_last_codex_agent_message() {
        let output = concat!(
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"first\"}}\n",
            "{\"type\":\"turn.completed\"}\n",
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\" final answer \"}}\n",
        );
        assert_eq!(codex_final_message(output).as_deref(), Some("final answer"));
    }

    #[test]
    fn session_ids_reject_paths_and_control_characters() {
        assert!(validate_session_id("019abc-session_1").is_ok());
        assert!(validate_session_id("../auth.json").is_err());
        assert!(validate_session_id("bad\nvalue").is_err());
    }
}
