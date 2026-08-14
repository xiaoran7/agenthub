use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use agent_pulse_runner::{
    device::DeviceIdentity,
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
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

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
fn authenticate(input: AuthInput, state: State<'_, DesktopState>) -> Result<Value, String> {
    authenticate_inner(input, &state).map_err(|error| error.to_string())
}

#[tauri::command]
fn restore_session(state: State<'_, DesktopState>) -> Result<Option<Value>, String> {
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
    let mut snapshot = dashboard_snapshot(&config.server_url, &cookie, &identity, &state)
        .map_err(|error| error.to_string())?;
    start_engine(&state, &config.server_url, &identity).map_err(|error| error.to_string())?;
    snapshot["sync_running"] = Value::Bool(true);
    Ok(Some(snapshot))
}

#[tauri::command]
fn refresh_dashboard(state: State<'_, DesktopState>) -> Result<Value, String> {
    let config = read_config(&state.data_dir).map_err(|error| error.to_string())?;
    let cookie = load_cookie(&config.server_url).map_err(|error| error.to_string())?;
    let identity = load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
    dashboard_snapshot(&config.server_url, &cookie, &identity, &state)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_sync_paused(paused: bool, state: State<'_, DesktopState>) -> Result<Value, String> {
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
fn logout(state: State<'_, DesktopState>) -> Result<(), String> {
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
    let server_url =
        normalize_server_url(input.server_url.as_deref().unwrap_or(DEFAULT_SERVER_URL))?;
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
    let account = request_json("GET", server_url, "/v1/auth/me", Some(cookie), None)?;
    let devices = request_json("GET", server_url, "/v1/devices", Some(cookie), None)?;
    let sessions = request_json("GET", server_url, "/v1/sessions", Some(cookie), None)?;
    let running = state
        .engine
        .lock()
        .map(|engine| engine.is_some())
        .unwrap_or(false);
    Ok(json!({
        "account": account,
        "devices": devices["devices"],
        "sessions": sessions["sessions"],
        "device": {
            "device_id": identity.device_id(),
            "name": machine_name()
        },
        "sync_running": running,
        "server_url": server_url
    }))
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
    let sync_worker = SyncWorker::start(
        database_path.clone(),
        client,
        sync_identity,
        SyncPolicy::default(),
        MAX_SYNC_BATCH_EVENTS.min(100),
        10,
        Arc::clone(&stop),
    )
    .map_err(|_| DesktopError::Sync)?;
    let sources: Vec<Box<dyn SessionSource>> = vec![
        Box::new(ClaudeSessionSource::new(true)),
        Box::new(CodexSessionSource::new("codex", 100)),
    ];
    let redactor =
        ProjectRedactor::new(identity.fingerprint_key()).map_err(|_| DesktopError::Sync)?;
    let outbox = Outbox::open(&database_path).map_err(|_| DesktopError::Sync)?;
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
}
