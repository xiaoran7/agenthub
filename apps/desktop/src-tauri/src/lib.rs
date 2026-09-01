use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use agent_pulse_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
        grok,
    },
    collectors::{
        AgentTurnUpdate, CodexAppServer, CollectorError, LocalSessionContext,
        collect_antigravity_session_contexts, collect_claude_session_contexts,
        collect_claude_transcripts, collect_codex_local_threads, collect_codex_session_contexts,
        collect_grok_session_contexts, collect_grok_sessions, find_antigravity_session_cwd,
        find_grok_session_cwd, run_agent_session_turn_streaming,
    },
    device::DeviceIdentity,
    domain::{AgentType, ObservationMeta, SessionObservation},
    gateway::GatewayClient,
    outbox::Outbox,
    privacy::ProjectRedactor,
    runtime::{
        AntigravitySessionSource, ClaudeSessionSource, CodexSessionSource, GrokSessionSource,
        PollPolicy, RunnerRuntime, SessionSource,
    },
    sync::MAX_SYNC_BATCH_EVENTS,
    sync_worker::{SyncPolicy, SyncWorker},
};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State, ipc::Channel};
use thiserror::Error;
use url::{Host, Url};

const DEFAULT_SERVER_URL: &str = "https://platform.tannerlab.xyz";
const KEYRING_SERVICE: &str = "AgentHub Desktop";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const LOCAL_SESSION_LIMIT: u32 = 1_000;
const DESKTOP_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(5);
const AGENT_TURN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_PROMPT_CHARS: usize = 16_000;
const MAX_SESSION_ID_CHARS: usize = 160;
const MAX_SSE_LINE_BYTES: usize = 64 * 1024;
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(45);
const STREAM_MAX_BACKOFF: Duration = Duration::from_secs(15);
const SESSION_CONTEXT_CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Error)]
enum DesktopError {
    #[error("服务器地址无效")]
    InvalidServer,
    #[error("无法连接 AgentHub 服务器")]
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
    #[error("这个会话已有一个回合正在运行")]
    AgentBusy,
    #[error("本机会话已停止")]
    AgentCancelled,
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
    #[serde(default = "default_grok_binary")]
    grok_binary: String,
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
struct LaunchSessionInput {
    agent_type: AgentType,
    #[allow(dead_code)]
    cwd: Option<String>,
    prompt: String,
}

#[derive(Debug, Deserialize)]
struct SessionTurnRef {
    agent_type: AgentType,
    agent_session_id: String,
}

#[derive(Debug, Deserialize)]
struct SessionTimelineInput {
    device_id: String,
    agent_type: AgentType,
    agent_session_id: String,
}

#[derive(Debug, Deserialize)]
struct SessionConversationInput {
    agent_type: AgentType,
    agent_session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionTurnEvent {
    Started,
    AssistantDelta { text: String },
    AssistantMessage { text: String },
    Activity { label: &'static str },
}

#[derive(Debug, Clone, Serialize)]
struct DashboardRealtimeEvent {
    connected: bool,
    changed: bool,
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
    grok_binary: String,
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

fn default_grok_binary() -> String {
    "grok".to_owned()
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

struct RealtimeHandle {
    stop: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
}

#[derive(Default)]
struct SessionContextCache {
    refreshed_at: Option<Instant>,
    entries: HashMap<String, LocalSessionContext>,
}

impl RealtimeHandle {
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.connected.store(false, Ordering::Relaxed);
    }
}

struct DesktopState {
    data_dir: PathBuf,
    engine: Mutex<Option<EngineHandle>>,
    realtime: Mutex<Option<RealtimeHandle>>,
    active_turns: Mutex<HashMap<String, Arc<AtomicBool>>>,
    session_contexts: Mutex<SessionContextCache>,
}

#[tauri::command]
async fn authenticate(
    input: AuthInput,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let snapshot = authenticate_inner(input, &state).map_err(|error| error.to_string())?;
    start_dashboard_realtime(&state, &app).map_err(|error| error.to_string())?;
    Ok(snapshot)
}

#[tauri::command]
async fn restore_session(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Option<Value>, String> {
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
    start_dashboard_realtime(&state, &app).map_err(|error| error.to_string())?;
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
async fn load_session_timeline(
    input: SessionTimelineInput,
    state: State<'_, DesktopState>,
) -> Result<Vec<Value>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || load_session_timeline_inner(input, &data_dir))
        .await
        .map_err(|_| DesktopError::Unreachable.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn load_session_conversation(input: SessionConversationInput) -> Result<Vec<Value>, String> {
    tauri::async_runtime::spawn_blocking(move || load_session_conversation_inner(input))
        .await
        .map_err(|_| DesktopError::Unreachable.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn send_session_message(
    input: SessionMessageInput,
    on_event: Channel<SessionTurnEvent>,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    validate_session_id(&input.agent_session_id).map_err(|error| error.to_string())?;
    if input.prompt.trim().is_empty() || input.prompt.trim().chars().count() > MAX_PROMPT_CHARS {
        return Err(DesktopError::InvalidAction.to_string());
    }
    let key = session_turn_key(input.agent_type, &input.agent_session_id);
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active_turns = state
            .active_turns
            .lock()
            .map_err(|_| DesktopError::AgentFailed.to_string())?;
        if active_turns.contains_key(&key) {
            return Err(DesktopError::AgentBusy.to_string());
        }
        active_turns.insert(key.clone(), cancelled.clone());
    }
    let _ = on_event.send(SessionTurnEvent::Started);
    let data_dir = state.data_dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        send_session_message_inner(input, &data_dir, &cancelled, on_event)
    })
    .await;
    if let Ok(mut active_turns) = state.active_turns.lock() {
        active_turns.remove(&key);
    }
    result
        .map_err(|_| DesktopError::AgentFailed.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn launch_new_session(
    input: LaunchSessionInput,
    on_event: Channel<SessionTurnEvent>,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let prompt = input.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(DesktopError::InvalidAction.to_string());
    }
    let session_id = format!("session-{}", chrono::Utc::now().timestamp_millis());
    let key = session_turn_key(input.agent_type, &session_id);
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active_turns = state
            .active_turns
            .lock()
            .map_err(|_| DesktopError::AgentFailed.to_string())?;
        active_turns.insert(key.clone(), cancelled.clone());
    }
    let _ = on_event.send(SessionTurnEvent::Started);
    let data_dir = state.data_dir.clone();
    let agent_type = input.agent_type;
    let agent_session_id = session_id.clone();
    let input_msg = SessionMessageInput {
        agent_type,
        agent_session_id: agent_session_id.clone(),
        prompt: prompt.to_owned(),
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        send_session_message_inner(input_msg, &data_dir, &cancelled, on_event)
    })
    .await;
    if let Ok(mut active_turns) = state.active_turns.lock() {
        active_turns.remove(&key);
    }
    let val = result
        .map_err(|_| DesktopError::AgentFailed.to_string())?
        .map_err(|error| error.to_string())?;

    Ok(json!({
        "agent_session_id": session_id,
        "agent_type": agent_type_name(agent_type),
        "response": val["response"],
        "completed_at": val["completed_at"]
    }))
}

#[tauri::command]
fn cancel_session_turn(
    input: SessionTurnRef,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    validate_session_id(&input.agent_session_id).map_err(|error| error.to_string())?;
    let key = session_turn_key(input.agent_type, &input.agent_session_id);
    let cancelled = state
        .active_turns
        .lock()
        .map_err(|_| DesktopError::AgentFailed.to_string())?
        .get(&key)
        .cloned();
    if let Some(cancelled) = cancelled {
        cancelled.store(true, Ordering::Relaxed);
        return Ok(json!({ "cancelled": true }));
    }
    Ok(json!({ "cancelled": false }))
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
        || !valid_binary(&input.grok_binary)
    {
        return Err(DesktopError::InvalidAction.to_string());
    }
    config.refresh_interval_seconds = input.refresh_interval_seconds;
    config.claude_binary = input.claude_binary.trim().to_owned();
    config.codex_binary = input.codex_binary.trim().to_owned();
    config.grok_binary = input.grok_binary.trim().to_owned();
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
    stop_dashboard_realtime(&state);
    if let Ok(active_turns) = state.active_turns.lock() {
        for cancelled in active_turns.values() {
            cancelled.store(true, Ordering::Relaxed);
        }
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

fn finish_login(
    server_url: &str,
    cookie: &str,
    account: Value,
    state: &DesktopState,
) -> Result<Value, DesktopError> {
    store_cookie(server_url, cookie)?;
    write_config(
        &state.data_dir,
        &DesktopConfig {
            server_url: server_url.to_owned(),
            refresh_interval_seconds: default_refresh_interval_seconds(),
            claude_binary: default_claude_binary(),
            codex_binary: default_codex_binary(),
            grok_binary: default_grok_binary(),
            archived_sessions: Vec::new(),
        },
    )?;

    let identity = load_or_create_identity(&state.data_dir)?;
    ensure_device_bound(server_url, cookie, &identity)?;
    start_engine(state, server_url, &identity)?;
    let mut snapshot = dashboard_snapshot(server_url, cookie, &identity, state)?;
    snapshot["account"] = account;
    Ok(snapshot)
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
    finish_login(&server_url, &cookie, account, state)
}

#[tauri::command]
async fn open_external_url(url: String) -> Result<(), String> {
    let target = url.trim().to_owned();
    if !target.starts_with("https://") && !target.starts_with("http://") {
        return Err("invalid_url_scheme".to_owned());
    }
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                lpOperation: *const u16,
                lpFile: *const u16,
                lpParameters: *const u16,
                lpDirectory: *const u16,
                nShowCmd: i32,
            ) -> isize;
        }

        let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = OsStr::new(&target).encode_wide().chain(Some(0)).collect();

        let res = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };

        if res <= 32 {
            return Err(format!("ShellExecuteW failed with code {res}"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&target).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&target).spawn();
    }
    Ok(())
}

#[tauri::command]
async fn start_tg_browser_oauth(
    server_url: Option<String>,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let configured_server = server_url.or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))
            .map_err(|e| e.to_string())?;

    // Bind ephemeral loopback port for RFC 8252 PKCE loopback redirect
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind local loopback port: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local port: {e}"))?
        .port();

    let redirect_uri = format!("http://127.0.0.1:{local_port}/callback");

    // Request authorize URL and PKCE verifier from Gateway API
    let init_res = request_json(
        "POST",
        &server_url,
        "/v1/auth/oauth/tg/authorize-url",
        None,
        Some(&json!({
            "redirect_uri": redirect_uri,
            "scope": "openid profile email offline_access"
        })),
    )
    .map_err(|e| format!("Failed to initialize OAuth: {e}"))?;

    let authorize_url = init_res["authorize_url"]
        .as_str()
        .ok_or_else(|| "Missing authorize_url in response".to_owned())?
        .to_owned();
    let code_verifier = init_res["code_verifier"]
        .as_str()
        .ok_or_else(|| "Missing code_verifier in response".to_owned())?
        .to_owned();
    let expected_state = init_res["state"]
        .as_str()
        .ok_or_else(|| "Missing state in response".to_owned())?
        .to_owned();

    // Emit event so frontend has the URL immediately for retry/reopen button
    let _ = app.emit("oauth_url_ready", json!({ "url": &authorize_url }));

    // Open user's system default browser
    let _ = open_external_url(authorize_url).await;

    // Accept redirect connection in a blocking thread with timeout
    let (code, returned_state) = tauri::async_runtime::spawn_blocking(move || {
        use std::io::{Read, Write};

        listener
            .incoming()
            .next()
            .ok_or_else(|| "No connection received".to_owned())
            .and_then(|stream_res| {
                let mut stream = stream_res.map_err(|e| format!("Connection failed: {e}"))?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(120)))
                    .map_err(|e| format!("Timeout config error: {e}"))?;
                let mut buffer = [0u8; 4096];
                let n = stream
                    .read(&mut buffer)
                    .map_err(|e| format!("Failed to read callback request: {e}"))?;
                let req_text = String::from_utf8_lossy(&buffer[..n]);

                let first_line = req_text.lines().next().unwrap_or_default();
                let path_and_query = first_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default();

                let callback_url = format!("http://127.0.0.1{path_and_query}");
                let parsed_url =
                    Url::parse(&callback_url).map_err(|e| format!("Malformed callback: {e}"))?;

                let code = parsed_url
                    .query_pairs()
                    .find(|(k, _)| k == "code")
                    .map(|(_, v)| v.to_string());
                let state = parsed_url
                    .query_pairs()
                    .find(|(k, _)| k == "state")
                    .map(|(_, v)| v.to_string());
                let error = parsed_url
                    .query_pairs()
                    .find(|(k, _)| k == "error")
                    .map(|(_, v)| v.to_string());

                let html_body = if code.is_some() {
                    r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <title>AgentHub 授权成功</title>
  <style>
    body { background: #0b0d0f; color: #fff; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
    .card { text-align: center; padding: 48px; background: #14171c; border-radius: 16px; border: 1px solid rgba(255,255,255,0.1); box-shadow: 0 16px 48px rgba(0,0,0,0.6); max-width: 420px; }
    .icon { font-size: 56px; margin-bottom: 16px; }
    h2 { color: #7ed5a0; margin: 0 0 12px 0; font-size: 24px; font-weight: 700; }
    p { color: #8a99a8; margin: 0; font-size: 14px; line-height: 1.6; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✅</div>
    <h2>授权成功</h2>
    <p>已成功连接到 AgentHub 桌面客户端。<br>您可以关闭此标签页并返回桌面端继续操作。</p>
  </div>
  <script>setTimeout(function(){ window.close(); }, 2000);</script>
</body>
</html>"#
                } else {
                    r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <title>AgentHub 授权未完成</title>
  <style>
    body { background: #0b0d0f; color: #fff; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
    .card { text-align: center; padding: 48px; background: #14171c; border-radius: 16px; border: 1px solid rgba(255,255,255,0.1); box-shadow: 0 16px 48px rgba(0,0,0,0.6); max-width: 420px; }
    .icon { font-size: 56px; margin-bottom: 16px; }
    h2 { color: #ff6b6b; margin: 0 0 12px 0; font-size: 24px; font-weight: 700; }
    p { color: #8a99a8; margin: 0; font-size: 14px; line-height: 1.6; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">⚠️</div>
    <h2>授权未完成</h2>
    <p>未能获取有效授权代码，请返回客户端重新发起。</p>
  </div>
</body>
</html>"#
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html_body.len(),
                    html_body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();

                if let Some(err) = error {
                    return Err(format!("Authorization denied: {err}"));
                }
                let code =
                    code.ok_or_else(|| "No authorization code found in callback".to_owned())?;
                Ok((code, state))
            })
    })
    .await
    .map_err(|e| format!("Async task failed: {e}"))??;

    if let Some(st) = returned_state
        && st != expected_state
    {
        return Err("OAuth state verification mismatch".to_owned());
    }

    // Exchange code via Gateway API
    let agent = http_agent();
    let callback_url = format!("{server_url}/v1/auth/oauth/tg/callback");
    let mut response = agent
        .post(&callback_url)
        .send_json(json!({
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri
        }))
        .map_err(map_http_error)
        .map_err(|e| e.to_string())?;

    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(extract_session_cookie)
        .ok_or_else(|| DesktopError::InvalidResponse.to_string())?;

    let account = response
        .body_mut()
        .read_json::<Value>()
        .map_err(|_| DesktopError::InvalidResponse.to_string())?;

    let snapshot =
        finish_login(&server_url, &cookie, account, &state).map_err(|e| e.to_string())?;
    start_dashboard_realtime(&state, &app).map_err(|error| error.to_string())?;
    Ok(snapshot)
}

#[tauri::command]
async fn start_tg_device_auth(server_url: Option<String>) -> Result<Value, String> {
    let configured_server = server_url.or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))
            .map_err(|e| e.to_string())?;
    request_json(
        "POST",
        &server_url,
        "/v1/auth/oauth/tg/device/code",
        None,
        Some(&json!({})),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn poll_tg_device_auth(
    server_url: Option<String>,
    device_code: String,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let configured_server = server_url.or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))
            .map_err(|e| e.to_string())?;
    let agent = http_agent();
    let url = format!("{server_url}/v1/auth/oauth/tg/device/poll");
    let mut response = agent
        .post(&url)
        .send_json(json!({ "device_code": device_code }))
        .map_err(map_http_error)
        .map_err(|e| e.to_string())?;

    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(extract_session_cookie);

    let body = response
        .body_mut()
        .read_json::<Value>()
        .map_err(|_| DesktopError::InvalidResponse.to_string())?;

    if body["status"].as_str() == Some("pending") {
        return Ok(body);
    }

    if body["status"].as_str() == Some("success") {
        let cookie = cookie.ok_or_else(|| DesktopError::InvalidResponse.to_string())?;
        let account = body["account"].clone();
        let snapshot =
            finish_login(&server_url, &cookie, account, &state).map_err(|e| e.to_string())?;
        start_dashboard_realtime(&state, &app).map_err(|error| error.to_string())?;
        return Ok(json!({ "status": "success", "snapshot": snapshot }));
    }

    Err(DesktopError::InvalidResponse.to_string())
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
    let antigravity_health = outbox
        .adapter_health(AgentType::Antigravity)
        .map_err(|_| DesktopError::Sync)?;
    let grok_health = outbox
        .adapter_health(AgentType::GrokBuild)
        .map_err(|_| DesktopError::Sync)?;
    let local_contexts = local_session_contexts(state);
    let sessions = merge_dashboard_sessions(
        &remote_sessions["sessions"],
        &local_sessions,
        &desktop_config.archived_sessions,
        identity.device_id(),
        &local_contexts,
    );
    let running = state
        .engine
        .lock()
        .map(|engine| engine.is_some())
        .unwrap_or(false);
    let realtime_connected = state
        .realtime
        .lock()
        .ok()
        .and_then(|realtime| {
            realtime
                .as_ref()
                .map(|handle| handle.connected.load(Ordering::Relaxed))
        })
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
                local_source_summary(AgentType::Codex, &local_sessions, codex_health),
                local_source_summary(AgentType::Antigravity, &local_sessions, antigravity_health),
                local_source_summary(AgentType::GrokBuild, &local_sessions, grok_health)
            ],
            "memory": local_memory_summary(),
            "refreshed_at": chrono::Utc::now()
        },
        "sync_running": running,
        "realtime_connected": realtime_connected,
        "server_url": server_url,
        "preferences": preferences_json(&desktop_config)
    }))
}

fn send_session_message_inner(
    input: SessionMessageInput,
    data_dir: &Path,
    cancelled: &AtomicBool,
    on_event: Channel<SessionTurnEvent>,
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
        AgentType::Antigravity => &config.claude_binary,
        AgentType::GrokBuild => &config.grok_binary,
    };
    let project_path = match input.agent_type {
        AgentType::ClaudeCode => Some(
            find_claude_session_cwd(&input.agent_session_id)?
                .ok_or(DesktopError::SessionUnavailable)?,
        ),
        AgentType::Codex => None,
        AgentType::Antigravity => find_antigravity_session_cwd(&input.agent_session_id)
            .map_err(|_| DesktopError::SessionUnavailable)?,
        AgentType::GrokBuild => Some(
            find_grok_session_cwd(&input.agent_session_id)
                .map_err(|_| DesktopError::SessionUnavailable)?
                .ok_or(DesktopError::SessionUnavailable)?,
        ),
    };
    let response = run_agent_session_turn_streaming(
        input.agent_type,
        binary,
        project_path.as_deref(),
        &input.agent_session_id,
        prompt,
        AGENT_TURN_TIMEOUT,
        cancelled,
        move |update| {
            let event = match update {
                AgentTurnUpdate::AssistantDelta(text) => SessionTurnEvent::AssistantDelta { text },
                AgentTurnUpdate::AssistantMessage(text) => {
                    SessionTurnEvent::AssistantMessage { text }
                }
                AgentTurnUpdate::Activity(label) => SessionTurnEvent::Activity { label },
            };
            let _ = on_event.send(event);
        },
    )
    .map_err(|error| match error {
        CollectorError::Cancelled { .. } => DesktopError::AgentCancelled,
        _ => DesktopError::AgentFailed,
    })?;
    if response.trim().is_empty() {
        return Err(DesktopError::AgentFailed);
    };
    Ok(json!({
        "response": response.trim(),
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

fn load_session_timeline_inner(
    input: SessionTimelineInput,
    data_dir: &Path,
) -> Result<Vec<Value>, DesktopError> {
    validate_identifier(&input.device_id)?;
    validate_session_id(&input.agent_session_id)?;
    let config = read_config(data_dir)?;
    let cookie = load_cookie(&config.server_url)?;
    let path = format!(
        "/v1/sessions/events?device_id={}&agent_type={}&agent_session_id={}&limit=100",
        input.device_id,
        agent_type_name(input.agent_type),
        input.agent_session_id
    );
    let response = request_json("GET", &config.server_url, &path, Some(&cookie), None)?;
    let events = response["events"]
        .as_array()
        .ok_or(DesktopError::InvalidResponse)?;
    Ok(events.iter().map(sanitize_timeline_event).collect())
}

fn load_session_conversation_inner(
    input: SessionConversationInput,
) -> Result<Vec<Value>, DesktopError> {
    validate_session_id(&input.agent_session_id)?;
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return Ok(Vec::new());
    };
    let home = PathBuf::from(home);
    let Some(path) = find_agent_transcript(&home, input.agent_type, &input.agent_session_id) else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| DesktopError::InvalidResponse)?
        .take(32 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|_| DesktopError::InvalidResponse)?;
    let content = decode_local_text(&bytes);
    let mut messages = Vec::new();
    for line in content.lines().take(2_000) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some((role, text)) = local_conversation_message(&value) else {
            continue;
        };
        let text = text
            .replace("<USER_REQUEST>", "")
            .replace("</USER_REQUEST>", "");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        messages.push(
            json!({ "role": role, "content": text.chars().take(16_000).collect::<String>() }),
        );
        if messages.len() >= 200 {
            break;
        }
    }
    Ok(messages)
}

fn find_agent_transcript(home: &Path, agent_type: AgentType, session_id: &str) -> Option<PathBuf> {
    match agent_type {
        AgentType::ClaudeCode => {
            find_transcript_file(&home.join(".claude").join("projects"), session_id, 0)
        }
        AgentType::Codex => find_provider_transcript(&home.join(".codex"), session_id, 0),
        AgentType::Antigravity => {
            let roots = [
                home.join(".gemini").join("antigravity"),
                home.join(".antigravity"),
            ];
            roots.into_iter().find_map(|root| {
                let direct = root
                    .join("brain")
                    .join(session_id)
                    .join(".system_generated")
                    .join("logs")
                    .join("transcript.jsonl");
                direct.is_file().then_some(direct)
            })
        }
        AgentType::GrokBuild => {
            find_provider_transcript(&home.join(".grok").join("sessions"), session_id, 0)
        }
    }
}

fn find_provider_transcript(root: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    if depth > 7 {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let parent_matches = path
                .ancestors()
                .any(|part| part.file_name().and_then(|value| value.to_str()) == Some(session_id));
            if name.contains(session_id) || parent_matches {
                return Some(path);
            }
        }
        if path.is_dir()
            && let Some(found) = find_provider_transcript(&path, session_id, depth + 1)
        {
            return Some(found);
        }
    }
    None
}

fn decode_local_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)).into_owned()
}

fn local_conversation_message(value: &Value) -> Option<(&'static str, String)> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload_type = value
        .pointer("/payload/type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let raw_role = value
        .get("role")
        .or_else(|| value.pointer("/message/role"))
        .or_else(|| value.pointer("/payload/role"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| match (event_type, payload_type) {
            ("user", _) | ("USER_INPUT", _) | (_, "user_message") => "user",
            ("assistant", _) | ("ASSISTANT", _) | (_, "agent_message") => "assistant",
            (_, "message") => value
                .pointer("/payload/role")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            _ => "",
        });
    let role = match raw_role.to_ascii_lowercase().as_str() {
        "user" | "human" => "user",
        "assistant" | "agent" | "model" => "assistant",
        _ => return None,
    };
    let content = value
        .pointer("/message/content")
        .or_else(|| value.pointer("/payload/content"))
        .or_else(|| value.pointer("/payload/message"))
        .or_else(|| value.get("content"))
        .or_else(|| value.get("text"))?;
    let text = extract_local_text(content);
    (!text.trim().is_empty()).then_some((role, text))
}

fn extract_local_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .filter(|item| {
                !matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("thinking" | "reasoning" | "tool_call" | "tool_result")
                )
            })
            .map(extract_local_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    ["text", "content", "output_text", "input_text", "message"]
        .into_iter()
        .find_map(|field| value.get(field).map(extract_local_text))
        .unwrap_or_default()
}

fn find_transcript_file(root: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    if depth > 3 {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && path.file_stem().and_then(|value| value.to_str()) == Some(session_id)
        {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_transcript_file(&path, session_id, depth + 1)
        {
            return Some(found);
        }
    }
    None
}

fn sanitize_timeline_event(event: &Value) -> Value {
    json!({
        "event_id": event["event_id"],
        "device_id": event["device_id"],
        "agent_type": event["agent_type"],
        "agent_session_id": event["agent_session_id"],
        "sequence": event["sequence"],
        "observed_at": event["observed_at"],
        "received_at": event["received_at"],
        "payload": {
            "project": event["payload"]["project"],
            "state": {
                "presence": event["payload"]["state"]["presence"],
                "activity_status": event["payload"]["state"]["activity_status"]
            },
            "last_activity_at": event["payload"]["last_activity_at"]
        }
    })
}

fn preferences_json(config: &DesktopConfig) -> Value {
    json!({
        "refresh_interval_seconds": config.refresh_interval_seconds,
        "claude_binary": config.claude_binary,
        "codex_binary": config.codex_binary,
        "grok_binary": config.grok_binary,
    })
}

fn valid_binary(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.chars().count() <= 260 && !value.chars().any(char::is_control)
}

fn validate_session_id(value: &str) -> Result<(), DesktopError> {
    validate_identifier(value)
}

fn validate_identifier(value: &str) -> Result<(), DesktopError> {
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

fn session_turn_key(agent_type: AgentType, session_id: &str) -> String {
    format!("{}:{session_id}", agent_type_name(agent_type))
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
        Box::new(AntigravitySessionSource::new(true)),
        Box::new(GrokSessionSource::new(true)),
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

    let mut antigravity_source = AntigravitySessionSource::new(true);
    match antigravity_source.collect(&meta, redactor) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::Antigravity,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(error) => {
            outbox
                .record_adapter_health(
                    AgentType::Antigravity,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(error.code()),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }

    match collect_grok_sessions(true)
        .map_err(|error| error.code().to_owned())
        .and_then(|payload| {
            grok::normalize_sessions_json(&payload, &meta, redactor)
                .map_err(|_| "grok_payload_invalid".to_owned())
        }) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::GrokBuild,
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
                    AgentType::GrokBuild,
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
    contexts: &HashMap<String, LocalSessionContext>,
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
        let context = contexts.get(&session_context_key(
            observation.agent_type,
            &observation.agent_session_id,
        ));
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
                    "last_activity_at": observation.last_activity_at,
                    "runtime": context
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
            let context = contexts.get(&session_context_key(
                item.agent_type,
                &item.agent_session_id,
            ));
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
                    "runtime": context,
                },
                "sync_state": "synced",
                "archived": true,
            })
        });
    }
    sessions.into_values().collect()
}

fn local_session_contexts(state: &DesktopState) -> HashMap<String, LocalSessionContext> {
    let Ok(mut cache) = state.session_contexts.lock() else {
        return HashMap::new();
    };
    if cache
        .refreshed_at
        .is_some_and(|updated| updated.elapsed() < SESSION_CONTEXT_CACHE_TTL)
    {
        return cache.entries.clone();
    }

    let claude = collect_claude_session_contexts(LOCAL_SESSION_LIMIT as usize);
    let codex = collect_codex_session_contexts(LOCAL_SESSION_LIMIT);
    let antigravity = collect_antigravity_session_contexts(LOCAL_SESSION_LIMIT as usize);
    let grok = collect_grok_session_contexts(LOCAL_SESSION_LIMIT as usize);
    let mut next = HashMap::new();
    match claude {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::ClaudeCode)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    match codex {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::Codex)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    match antigravity {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::Antigravity)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    match grok {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::GrokBuild)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    cache.entries = next.clone();
    cache.refreshed_at = Some(Instant::now());
    next
}

fn insert_session_contexts(
    target: &mut HashMap<String, LocalSessionContext>,
    contexts: Vec<LocalSessionContext>,
) {
    for context in contexts {
        target.insert(
            session_context_key(context.agent_type, &context.agent_session_id),
            context,
        );
    }
}

fn session_context_key(agent_type: AgentType, session_id: &str) -> String {
    format!("{}:{session_id}", agent_type_name(agent_type))
}

fn agent_type_name(agent_type: AgentType) -> &'static str {
    match agent_type {
        AgentType::ClaudeCode => "claude_code",
        AgentType::Codex => "codex",
        AgentType::Antigravity => "antigravity",
        AgentType::GrokBuild => "grok_build",
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
        return json!({
            "claude_files": 0,
            "claude_bytes": 0,
            "codex_indexes": 0,
            "codex_bytes": 0,
            "grok_sessions": 0,
            "grok_bytes": 0
        });
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
    let grok_home = std::env::var_os("GROK_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".grok"));
    let mut grok_sessions = 0_u64;
    let mut grok_bytes = 0_u64;
    let grok_sessions_dir = grok_home.join("sessions");
    if let Ok(groups) = fs::read_dir(grok_sessions_dir) {
        for group in groups.flatten() {
            if !group.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Ok(children) = fs::read_dir(group.path()) else {
                continue;
            };
            for child in children.flatten() {
                if !child.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let summary = child.path().join("summary.json");
                if let Ok(metadata) = fs::metadata(summary) {
                    grok_sessions = grok_sessions.saturating_add(1);
                    grok_bytes = grok_bytes.saturating_add(metadata.len());
                }
            }
        }
    }
    json!({
        "claude_files": claude_files,
        "claude_bytes": claude_bytes,
        "codex_indexes": codex_indexes,
        "codex_bytes": codex_bytes,
        "grok_sessions": grok_sessions,
        "grok_bytes": grok_bytes
    })
}

fn start_dashboard_realtime(state: &DesktopState, app: &AppHandle) -> Result<(), DesktopError> {
    let config = read_config(&state.data_dir)?;
    let cookie = load_cookie(&config.server_url)?;
    let mut realtime = state.realtime.lock().map_err(|_| DesktopError::Sync)?;
    if realtime.is_some() {
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let connected = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread_connected = connected.clone();
    let app = app.clone();
    thread::Builder::new()
        .name("agent-pulse-desktop-realtime".to_owned())
        .spawn(move || {
            dashboard_realtime_loop(
                &app,
                &config.server_url,
                &cookie,
                &thread_stop,
                &thread_connected,
            );
        })
        .map_err(|_| DesktopError::Sync)?;
    *realtime = Some(RealtimeHandle { stop, connected });
    Ok(())
}

fn stop_dashboard_realtime(state: &DesktopState) {
    if let Ok(mut realtime) = state.realtime.lock()
        && let Some(handle) = realtime.take()
    {
        handle.stop();
    }
}

fn dashboard_realtime_loop(
    app: &AppHandle,
    server_url: &str,
    cookie: &str,
    stop: &AtomicBool,
    connected: &AtomicBool,
) {
    let mut attempt = 0_u32;
    while !stop.load(Ordering::Relaxed) {
        let result = read_dashboard_stream(app, server_url, cookie, stop, connected);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        publish_realtime_state(app, connected, false, false);
        attempt = if result.is_ok() {
            1
        } else {
            attempt.saturating_add(1)
        };
        let delay =
            Duration::from_secs(2_u64.saturating_pow(attempt.min(4))).min(STREAM_MAX_BACKOFF);
        sleep_until_cancelled(stop, delay);
    }
    connected.store(false, Ordering::Relaxed);
}

fn read_dashboard_stream(
    app: &AppHandle,
    server_url: &str,
    cookie: &str,
    stop: &AtomicBool,
    connected: &AtomicBool,
) -> Result<(), DesktopError> {
    let url = format!("{server_url}/v1/stream");
    let response = dashboard_stream_agent()
        .get(&url)
        .header("cookie", cookie)
        .header("accept", "text/event-stream")
        .call()
        .map_err(map_http_error)?;
    if !response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        return Err(DesktopError::InvalidResponse);
    }
    let mut reader = BufReader::new(response.into_parts().1.into_reader());
    let mut event = String::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let line = read_sse_line(&mut reader);
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match line {
            SseLine::Line(line) if line.is_empty() => {
                match event.as_str() {
                    "ready" => publish_realtime_state(app, connected, true, false),
                    "sessions" => publish_realtime_state(app, connected, true, true),
                    _ => {}
                }
                event.clear();
            }
            SseLine::Line(line) => {
                if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim_start().to_owned();
                }
            }
            SseLine::Eof => return Ok(()),
            SseLine::TooLarge | SseLine::InvalidUtf8 | SseLine::Io => {
                return Err(DesktopError::InvalidResponse);
            }
        }
    }
}

fn publish_realtime_state(
    app: &AppHandle,
    connected: &AtomicBool,
    next_connected: bool,
    changed: bool,
) {
    let state_changed = connected.swap(next_connected, Ordering::Relaxed) != next_connected;
    if state_changed || changed {
        let _ = app.emit(
            "dashboard-realtime",
            DashboardRealtimeEvent {
                connected: next_connected,
                changed,
            },
        );
    }
}

fn sleep_until_cancelled(stop: &AtomicBool, duration: Duration) {
    let deadline = std::time::Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
}

enum SseLine {
    Line(String),
    TooLarge,
    InvalidUtf8,
    Io,
    Eof,
}

fn read_sse_line(reader: &mut impl BufRead) -> SseLine {
    let mut line = Vec::new();
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(_) => return SseLine::Io,
        };
        if buffer.is_empty() {
            if line.is_empty() {
                return SseLine::Eof;
            }
            break;
        }
        let (take, complete) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (buffer.len(), false),
        };
        if line.len() + take > MAX_SSE_LINE_BYTES {
            return SseLine::TooLarge;
        }
        line.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if complete {
            break;
        }
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line)
        .map(SseLine::Line)
        .unwrap_or(SseLine::InvalidUtf8)
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
        .user_agent(concat!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AgentHub/",
            env!("CARGO_PKG_VERSION")
        ))
        .max_redirects(0)
        .build()
        .new_agent()
}

fn dashboard_stream_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(REQUEST_TIMEOUT))
        .timeout_recv_response(Some(REQUEST_TIMEOUT))
        .timeout_recv_body(Some(STREAM_READ_TIMEOUT))
        .user_agent(concat!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AgentHub/",
            env!("CARGO_PKG_VERSION")
        ))
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
                realtime: Mutex::new(None),
                active_turns: Mutex::new(HashMap::new()),
                session_contexts: Mutex::new(SessionContextCache::default()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            authenticate,
            open_external_url,
            start_tg_browser_oauth,
            start_tg_device_auth,
            poll_tg_device_auth,
            restore_session,
            refresh_dashboard,
            send_session_message,
            launch_new_session,
            cancel_session_turn,
            set_session_archived,
            save_desktop_preferences,
            set_sync_paused,
            load_session_timeline,
            load_session_conversation,
            logout
        ])
        .build(tauri::generate_context!())
        .expect("failed to build AgentHub desktop app")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app_handle.try_state::<DesktopState>()
            {
                stop_dashboard_realtime(&state);
                if let Ok(active_turns) = state.active_turns.lock() {
                    for cancelled in active_turns.values() {
                        cancelled.store(true, Ordering::Relaxed);
                    }
                }
                if let Ok(mut engine) = state.engine.lock()
                    && let Some(handle) = engine.take()
                {
                    handle.stop();
                }
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
    fn local_conversation_parser_supports_provider_native_message_shapes() {
        let claude = json!({"type":"user","message":{"content":[{"type":"text","text":"你好"}]}});
        let codex = json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"已完成"}]}});
        let antigravity =
            json!({"type":"USER_INPUT","content":"<USER_REQUEST>修复乱码</USER_REQUEST>"});

        assert_eq!(
            local_conversation_message(&claude),
            Some(("user", "你好".into()))
        );
        assert_eq!(
            local_conversation_message(&codex),
            Some(("assistant", "已完成".into()))
        );
        assert_eq!(
            local_conversation_message(&antigravity),
            Some(("user", "<USER_REQUEST>修复乱码</USER_REQUEST>".into()))
        );
    }

    #[test]
    fn local_text_decoder_handles_utf8_and_utf16_without_mojibake() {
        assert_eq!(decode_local_text("会话内容".as_bytes()), "会话内容");
        let mut utf16 = vec![0xff, 0xfe];
        for unit in "会话内容".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_local_text(&utf16), "会话内容");
    }

    #[test]
    fn rejects_insecure_remote_servers() {
        assert!(normalize_server_url("https://platform.tannerlab.xyz/").is_ok());
        assert!(normalize_server_url("http://localhost:3000").is_ok());
        assert!(normalize_server_url("http://example.com").is_err());
    }

    #[test]
    fn sse_line_reader_handles_crlf_and_blank_frames() {
        let mut input = std::io::Cursor::new(b"event: ready\r\n\r\n".to_vec());
        assert!(matches!(
            read_sse_line(&mut input),
            SseLine::Line(line) if line == "event: ready"
        ));
        assert!(matches!(
            read_sse_line(&mut input),
            SseLine::Line(line) if line.is_empty()
        ));
    }

    #[test]
    fn sse_line_reader_rejects_unbounded_lines() {
        let mut input = std::io::Cursor::new(vec![b'x'; MAX_SSE_LINE_BYTES + 1]);
        assert!(matches!(read_sse_line(&mut input), SseLine::TooLarge));
    }

    #[test]
    fn session_ids_reject_paths_and_control_characters() {
        assert!(validate_session_id("019abc-session_1").is_ok());
        assert!(validate_session_id("../auth.json").is_err());
        assert!(validate_session_id("bad\nvalue").is_err());
        assert!(validate_identifier("device-remote_1").is_ok());
        assert!(validate_identifier("device/remote").is_err());
    }

    #[test]
    fn timeline_sanitizer_keeps_status_metadata_only() {
        let event = json!({
            "event_id": "event-1",
            "device_id": "device-1",
            "agent_type": "codex",
            "agent_session_id": "session-1",
            "sequence": 7,
            "observed_at": "2026-08-15T05:00:00Z",
            "received_at": "2026-08-15T05:00:01Z",
            "payload": {
                "project": {"display_name": "demo", "fingerprint": "fp"},
                "state": {
                    "presence": "LIVE",
                    "activity_status": "WAITING_INPUT",
                    "raw_status": "secret raw status",
                    "raw_status_detail": {"prompt": "do not expose"}
                },
                "last_activity_at": "2026-08-15T05:00:00Z",
                "prompt": "never return this"
            }
        });
        let sanitized = sanitize_timeline_event(&event);
        assert_eq!(
            sanitized["payload"]["state"]["activity_status"],
            "WAITING_INPUT"
        );
        assert!(sanitized["payload"]["state"].get("raw_status").is_none());
        assert!(sanitized["payload"].get("prompt").is_none());
    }

    #[test]
    fn dashboard_attaches_runtime_context_only_to_local_sessions() {
        let redactor = ProjectRedactor::new(b"desktop-test-key").unwrap();
        let observations = claude::normalize_agents_json(
            r#"[{"sessionId":"session-1","cwd":"/work/project","state":"done"}]"#,
            &ObservationMeta::now("device-local"),
            &redactor,
        )
        .unwrap();
        let context = LocalSessionContext {
            agent_type: AgentType::ClaudeCode,
            agent_session_id: "session-1".to_owned(),
            title: Some("Explicit local title".to_owned()),
            model: Some("claude-opus-4-7".to_owned()),
            reasoning_effort: None,
            permission_mode: Some("acceptEdits".to_owned()),
            approval_policy: None,
            sandbox_mode: None,
            context_window_tokens: Some(200_000),
            context_used_tokens: Some(40_000),
            session_total_tokens: None,
        };
        let contexts = HashMap::from([("claude_code:session-1".to_owned(), context)]);
        let remote = json!([{
            "agent_type": "codex",
            "device_id": "device-remote",
            "agent_session_id": "session-1",
            "payload": {"project": {"display_name": "remote", "fingerprint": "fp"}}
        }]);

        let merged =
            merge_dashboard_sessions(&remote, &observations, &[], "device-local", &contexts);
        let local = merged
            .iter()
            .find(|session| session["device_id"] == "device-local")
            .unwrap();
        let remote = merged
            .iter()
            .find(|session| session["device_id"] == "device-remote")
            .unwrap();

        assert_eq!(local["payload"]["runtime"]["title"], "Explicit local title");
        assert!(remote["payload"].get("runtime").is_none());
    }
}
