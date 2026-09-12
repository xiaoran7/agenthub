use super::*;
use agent_pulse_runner::{
    daemon_ipc,
    daemon_protocol::{
        DAEMON_PROTOCOL_VERSION, DaemonRequest, DaemonRequestEnvelope, DaemonResponse,
    },
    remote_host::{CommandReceipt, HostEvent, RemoteCommand, SessionBinding},
    remote_journal::{EventsAfter, JournalSnapshot},
};
use std::{
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};

const DAEMON_EVENT_HEALTH_WINDOW: Duration = Duration::from_secs(35);
const DAEMON_EXECUTE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct DaemonProfile {
    pub workspace_paths: Vec<PathBuf>,
    pub codex_binary: String,
    #[serde(default)]
    pub remote_enabled: bool,
}

#[derive(Clone)]
pub(crate) struct DaemonClient {
    endpoint: String,
}

type DaemonStatus = (String, Vec<PathBuf>, bool, bool);

#[derive(Debug)]
enum DaemonProbe {
    Ready(DaemonStatus),
    Starting,
    Absent,
    Failed(String),
}

pub(crate) struct DaemonEventSubscription {
    stopped: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    last_seen: Arc<Mutex<Instant>>,
}

impl DaemonEventSubscription {
    pub(crate) fn is_healthy(&self) -> bool {
        !self.failed.load(Ordering::Relaxed)
            && self
                .last_seen
                .lock()
                .is_ok_and(|last_seen| last_seen.elapsed() <= DAEMON_EVENT_HEALTH_WINDOW)
    }
}

impl Drop for DaemonEventSubscription {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

impl DaemonClient {
    fn call_with_timeout(
        &self,
        request: DaemonRequest,
        timeout: Duration,
    ) -> Result<DaemonResponse, String> {
        let request_id = format!("desktop-{}", uuid::Uuid::new_v4());
        let response = daemon_ipc::request_with_timeout(
            &self.endpoint,
            &DaemonRequestEnvelope {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                request_id: request_id.clone(),
                request,
            },
            timeout,
        )
        .map_err(|error| error.to_string())?;
        if response.protocol_version != DAEMON_PROTOCOL_VERSION || response.request_id != request_id
        {
            return Err("daemon_protocol_mismatch".into());
        }
        match response.response {
            DaemonResponse::Error { code } => Err(code),
            response => Ok(response),
        }
    }

    fn call(&self, request: DaemonRequest) -> Result<DaemonResponse, String> {
        self.call_with_timeout(request, daemon_ipc::DAEMON_REQUEST_TIMEOUT)
    }

    pub(crate) fn execute(&self, command: RemoteCommand) -> Result<CommandReceipt, String> {
        execute_command_with_retry(
            command,
            Instant::now() + DAEMON_EXECUTE_TIMEOUT,
            |command, remaining| {
                self.call_with_timeout(DaemonRequest::Execute { command }, remaining)
            },
        )
    }

    pub(crate) fn snapshot(&self) -> Result<JournalSnapshot, String> {
        match self.call(DaemonRequest::GetSnapshot)? {
            DaemonResponse::Snapshot(snapshot) => Ok(snapshot),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn query_sessions(&self) -> Result<Vec<SessionBinding>, String> {
        self.snapshot().map(|snapshot| snapshot.sessions)
    }

    pub(crate) fn query_generation(&self) -> Result<String, String> {
        match self.call(DaemonRequest::GetStatus)? {
            DaemonResponse::Status {
                host_generation, ..
            } => Ok(host_generation),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn query_models(&self) -> Result<Value, String> {
        match self.call(DaemonRequest::GetModels)? {
            DaemonResponse::Models(models) => Ok(models),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn discover(&self) -> Result<Value, String> {
        match self.call(DaemonRequest::Discover)? {
            DaemonResponse::Discovery(discovery) => Ok(discovery),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn session_conversation(
        &self,
        agent_type: AgentType,
        session_id: &str,
    ) -> Result<Vec<Value>, String> {
        match self.call(DaemonRequest::GetSessionConversation {
            agent_type,
            session_id: session_id.to_owned(),
        })? {
            DaemonResponse::SessionConversation(messages) => Ok(messages),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn authorize_workspace(&self, workspace: PathBuf) -> Result<PathBuf, String> {
        match self.call(DaemonRequest::AuthorizeWorkspace { workspace })? {
            DaemonResponse::Workspace(workspace) => Ok(workspace),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn set_remote_enabled(
        &self,
        server_url: Option<String>,
    ) -> Result<(bool, bool), String> {
        match self.call(DaemonRequest::SetRemoteEnabled { server_url })? {
            DaemonResponse::RemoteState { enabled, connected } => Ok((enabled, connected)),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn status(&self) -> Result<(String, Vec<PathBuf>, bool, bool), String> {
        match self.call(DaemonRequest::GetStatus)? {
            DaemonResponse::Status {
                host_generation,
                workspaces,
                remote_enabled,
                remote_connected,
            } => Ok((
                host_generation,
                workspaces,
                remote_enabled,
                remote_connected,
            )),
            _ => Err("daemon_invalid_response".into()),
        }
    }

    pub(crate) fn subscribe(
        &self,
        capacity: usize,
    ) -> Result<
        (
            DaemonEventSubscription,
            Receiver<HostEvent>,
            JournalSnapshot,
        ),
        String,
    > {
        let endpoint = self.endpoint.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let last_seen = Arc::new(Mutex::new(Instant::now()));
        let worker_stopped = stopped.clone();
        let worker_failed = failed.clone();
        let worker_last_seen = last_seen.clone();
        let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
        let initial_snapshot = self.snapshot()?;
        let cursor = initial_snapshot.cursor;
        let (mut cursor, mut request_id, mut subscription) =
            open_subscription(&endpoint, cursor, &sender, Duration::from_secs(30))?;
        if let Ok(mut last_seen) = last_seen.lock() {
            *last_seen = Instant::now();
        }
        thread::spawn(move || {
            let result = (|| -> Result<(), ()> {
                loop {
                    while !worker_stopped.load(Ordering::Relaxed) {
                        let event = match subscription.next_event() {
                            Ok(Some(event)) => event,
                            Ok(None) | Err(_) => break,
                        };
                        if event.protocol_version != DAEMON_PROTOCOL_VERSION
                            || event.request_id != request_id
                            || event.cursor < cursor
                        {
                            return Err(());
                        }
                        match event.event["type"].as_str() {
                            Some("heartbeat") if event.cursor == cursor => {}
                            Some("event") if event.cursor > cursor => {
                                cursor = event.cursor;
                                let host_event = serde_json::from_value::<HostEvent>(
                                    event.event["event"].clone(),
                                )
                                .map_err(|_| ())?;
                                sender.try_send(host_event).map_err(|_| ())?;
                            }
                            Some("receipt" | "host_lost") if event.cursor > cursor => {
                                cursor = event.cursor;
                            }
                            Some("snapshot_required") => return Err(()),
                            Some("error")
                                if event.event["code"]
                                    .as_str()
                                    .is_some_and(transient_subscription_error) =>
                            {
                                break;
                            }
                            Some("error") => return Err(()),
                            _ => return Err(()),
                        }
                        *worker_last_seen.lock().map_err(|_| ())? = Instant::now();
                    }
                    if worker_stopped.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    match open_subscription(&endpoint, cursor, &sender, Duration::from_secs(30)) {
                        Ok((next_cursor, next_request_id, next_subscription)) => {
                            cursor = next_cursor;
                            request_id = next_request_id;
                            subscription = next_subscription;
                            *worker_last_seen.lock().map_err(|_| ())? = Instant::now();
                        }
                        Err(_) => return Err(()),
                    }
                }
            })();
            if result.is_err() && !worker_stopped.load(Ordering::Relaxed) {
                worker_failed.store(true, Ordering::Relaxed);
            }
        });
        Ok((
            DaemonEventSubscription {
                stopped,
                failed,
                last_seen,
            },
            receiver,
            initial_snapshot,
        ))
    }
}

fn open_subscription(
    endpoint: &str,
    mut cursor: i64,
    sender: &mpsc::SyncSender<HostEvent>,
    retry_for: Duration,
) -> Result<(i64, String, daemon_ipc::DaemonSubscription), String> {
    let deadline = Instant::now() + retry_for;
    loop {
        let request_id = format!("desktop-sub-{}", uuid::Uuid::new_v4());
        let request = DaemonRequestEnvelope {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            request_id: request_id.clone(),
            request: DaemonRequest::Subscribe {
                after_cursor: cursor,
            },
        };
        match daemon_ipc::subscribe(endpoint, &request) {
            Ok((initial, subscription))
                if initial.protocol_version == DAEMON_PROTOCOL_VERSION
                    && initial.request_id == request_id =>
            {
                match initial.response {
                    DaemonResponse::Events(EventsAfter::Events {
                        events,
                        cursor: next_cursor,
                    }) => {
                        for event in events {
                            if event.cursor <= cursor {
                                return Err("daemon_cursor_regression".into());
                            }
                            cursor = event.cursor;
                            match event.payload["type"].as_str() {
                                Some("event") => {
                                    let host_event = serde_json::from_value::<HostEvent>(
                                        event.payload["event"].clone(),
                                    )
                                    .map_err(|_| "daemon_invalid_event".to_owned())?;
                                    sender
                                        .try_send(host_event)
                                        .map_err(|_| "daemon_event_channel_overflow".to_owned())?;
                                }
                                Some("receipt" | "host_lost") => {}
                                _ => return Err("daemon_invalid_event".into()),
                            }
                        }
                        if next_cursor != cursor {
                            return Err("daemon_cursor_mismatch".into());
                        }
                        return Ok((cursor, request_id, subscription));
                    }
                    DaemonResponse::Events(EventsAfter::SnapshotRequired { .. }) => {
                        return Err("daemon_snapshot_required".into());
                    }
                    DaemonResponse::Error { code } => {
                        if code != "daemon_starting" {
                            return Err(code);
                        }
                    }
                    _ => return Err("daemon_invalid_response".into()),
                }
            }
            Ok(_) => return Err("daemon_protocol_mismatch".into()),
            Err(error) if Instant::now() >= deadline => return Err(error.to_string()),
            Err(_) => {}
        }
        if Instant::now() >= deadline {
            return Err("daemon_subscription_timeout".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub(crate) fn host_database_path(data_dir: &Path, device: &str) -> (PathBuf, PathBuf) {
    let scope = format!(
        "{:x}",
        Sha256::digest(format!("agenthub_native_host_v1\n{device}").as_bytes())
    );
    (
        data_dir.join(format!("remote-{}.sqlite", &scope[..16])),
        data_dir.join(format!("remote-{}.json", &scope[..16])),
    )
}

fn client_for_state(state: &DesktopState) -> Result<DaemonClient, DesktopError> {
    let identity = load_or_create_identity(&state.data_dir).map_err(|_| DesktopError::Identity)?;
    Ok(DaemonClient {
        endpoint: daemon_ipc::default_endpoint(identity.device_id()),
    })
}

fn execute_command_with_retry<F>(
    command: RemoteCommand,
    deadline: Instant,
    mut call: F,
) -> Result<CommandReceipt, String>
where
    F: FnMut(RemoteCommand, Duration) -> Result<DaemonResponse, String>,
{
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("command_outcome_unknown".into());
        }
        match call(command.clone(), remaining) {
            Ok(DaemonResponse::Receipt(receipt)) => {
                if receipt.client_cmd_id != command.client_cmd_id
                    || receipt.hub_session_id != command.hub_session_id
                {
                    return Err("daemon_command_receipt_mismatch".into());
                }
                return Ok(receipt);
            }
            Ok(_) => return Err("daemon_invalid_response".into()),
            Err(error) if transient_execute_error(&error) && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) if transient_execute_error(&error) => {
                return Err("command_outcome_unknown".into());
            }
            Err(error) => return Err(error),
        }
    }
}

fn transient_execute_error(error: &str) -> bool {
    error == "daemon_starting"
        || error == "command_timeout"
        || error == "host_queue_timeout"
        || error.contains("ipc_connect_failed")
        || error.contains("ipc_response_timeout")
        || error.contains("ipc_response_missing")
        || error.contains("ipc_read_failed")
        || error.contains("ipc_write_failed")
        || error.contains("ipc_truncated_frame")
}

fn transient_subscription_error(error: &str) -> bool {
    error == "query_timeout" || error == "host_queue_timeout" || error == "host_not_running"
}

fn transient_startup_error(error: &str) -> bool {
    error.contains("ipc_response_missing")
        || error.contains("ipc_read_failed")
        || error.contains("ipc_write_failed")
        || error.contains("ipc_truncated_frame")
}

fn probe_daemon(client: &DaemonClient) -> DaemonProbe {
    match client.status() {
        Ok(status) => DaemonProbe::Ready(status),
        Err(error) if error == "daemon_starting" => DaemonProbe::Starting,
        Err(error) if daemon_absent(&error) => DaemonProbe::Absent,
        Err(error) => DaemonProbe::Failed(error),
    }
}

fn daemon_absent(error: &str) -> bool {
    error.contains("ipc_connect_failed")
}

fn wait_for_daemon_ready<F>(mut probe: F, deadline: Instant) -> Result<DaemonStatus, String>
where
    F: FnMut() -> DaemonProbe,
{
    loop {
        match probe() {
            DaemonProbe::Ready(status) => return Ok(status),
            DaemonProbe::Starting | DaemonProbe::Absent if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            DaemonProbe::Failed(error)
                if transient_startup_error(&error) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(100));
            }
            DaemonProbe::Starting | DaemonProbe::Absent => {
                return Err("daemon_start_timeout".into());
            }
            DaemonProbe::Failed(error) => return Err(error),
        }
    }
}

fn ensure_daemon_ready_with<P, L>(
    mut probe: P,
    mut launch: L,
    deadline: Instant,
) -> Result<DaemonStatus, String>
where
    P: FnMut() -> DaemonProbe,
    L: FnMut() -> Result<(), String>,
{
    match probe() {
        DaemonProbe::Ready(status) => Ok(status),
        DaemonProbe::Starting => wait_for_daemon_ready(probe, deadline),
        DaemonProbe::Absent => {
            launch()?;
            wait_for_daemon_ready(probe, deadline)
        }
        DaemonProbe::Failed(error) => Err(error),
    }
}

fn ensure_daemon_ready<F>(client: &DaemonClient, launch: F) -> Result<DaemonStatus, String>
where
    F: FnMut() -> Result<(), String>,
{
    ensure_daemon_ready_with(
        || probe_daemon(client),
        launch,
        Instant::now() + Duration::from_secs(30),
    )
}

pub(crate) fn ensure_daemon_started(data_dir: PathBuf) {
    thread::spawn(move || {
        let Ok(identity) = load_or_create_identity(&data_dir) else {
            return;
        };
        let (_, profile_path) = host_database_path(&data_dir, identity.device_id());
        let valid_profile = fs::read(profile_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<DaemonProfile>(&bytes).ok())
            .is_some_and(|profile| {
                !profile.workspace_paths.is_empty()
                    && profile.workspace_paths.len() <= 32
                    && valid_binary(&profile.codex_binary)
                    && profile.workspace_paths.iter().all(|path| path.is_dir())
            });
        if !valid_profile {
            return;
        }
        let client = DaemonClient {
            endpoint: daemon_ipc::default_endpoint(identity.device_id()),
        };
        let Ok(_launch_guard) = LAUNCH_LOCK.lock() else {
            return;
        };
        let _ = ensure_daemon_ready(&client, || {
            start_daemon_for_data(&data_dir, None).map_err(|error| error.to_string())
        });
    });
}

pub(crate) fn get_daemon_client(state: &DesktopState) -> Option<DaemonClient> {
    let client = client_for_state(state).ok()?;
    client.status().ok()?;
    Some(client)
}

static LAUNCH_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn ensure_daemon_client(
    state: &DesktopState,
    required_workspace: Option<&Path>,
) -> Result<DaemonClient, DesktopError> {
    let _launch_guard = LAUNCH_LOCK.lock().map_err(|_| DesktopError::AgentFailed)?;
    let identity = load_or_create_identity(&state.data_dir).map_err(|_| DesktopError::Identity)?;
    let client = DaemonClient {
        endpoint: daemon_ipc::default_endpoint(identity.device_id()),
    };
    ensure_daemon_ready(&client, || {
        start_daemon(state, required_workspace).map_err(|error| error.to_string())
    })
    .map_err(|_| DesktopError::AgentFailed)?;
    if let Some(workspace) = required_workspace {
        let canonical = workspace
            .canonicalize()
            .map_err(|_| DesktopError::InvalidAction)?;
        client
            .authorize_workspace(canonical.clone())
            .map_err(|_| DesktopError::AgentFailed)?;
        persist_workspace(&state.data_dir, &identity, canonical)?;
    }
    Ok(client)
}

fn start_daemon(
    state: &DesktopState,
    required_workspace: Option<&Path>,
) -> Result<(), DesktopError> {
    start_daemon_for_data(&state.data_dir, required_workspace)
}

fn start_daemon_for_data(
    data_dir: &Path,
    required_workspace: Option<&Path>,
) -> Result<(), DesktopError> {
    let identity = load_or_create_identity(data_dir).map_err(|_| DesktopError::Identity)?;
    let (_, profile_path) = host_database_path(data_dir, identity.device_id());
    let config = read_config(data_dir).ok();
    let mut profile = fs::read(&profile_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DaemonProfile>(&bytes).ok())
        .unwrap_or(DaemonProfile {
            workspace_paths: Vec::new(),
            codex_binary: config
                .as_ref()
                .map(|config| config.codex_binary.clone())
                .unwrap_or_else(default_codex_binary),
            remote_enabled: false,
        });
    if let Some(workspace) = required_workspace
        && let Ok(canonical) = workspace.canonicalize()
        && canonical.is_dir()
        && !profile.workspace_paths.contains(&canonical)
    {
        profile.workspace_paths.push(canonical);
    }
    profile.workspace_paths.retain(|path| path.is_dir());
    if profile.workspace_paths.is_empty()
        && let Ok(cwd) = std::env::current_dir().and_then(|path| path.canonicalize())
        && cwd.is_dir()
    {
        profile.workspace_paths.push(cwd);
    }
    if profile.workspace_paths.len() > 32 || !valid_binary(&profile.codex_binary) {
        return Err(DesktopError::InvalidAction);
    }
    fs::write(
        &profile_path,
        serde_json::to_vec(&profile).map_err(|_| DesktopError::Identity)?,
    )
    .map_err(|_| DesktopError::Identity)?;

    let mut command = Command::new(std::env::current_exe().map_err(|_| DesktopError::AgentFailed)?);
    command.arg("--agenthubd").arg(data_dir);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000 | 0x0000_0008 | 0x0000_0200);
    }
    command.spawn().map_err(|_| DesktopError::AgentFailed)?;
    Ok(())
}

fn persist_workspace(
    data_dir: &Path,
    identity: &DeviceIdentity,
    workspace: PathBuf,
) -> Result<(), DesktopError> {
    let (_, profile_path) = host_database_path(data_dir, identity.device_id());
    let config = read_config(data_dir).ok();
    let mut profile = fs::read(&profile_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DaemonProfile>(&bytes).ok())
        .unwrap_or(DaemonProfile {
            workspace_paths: Vec::new(),
            codex_binary: config
                .as_ref()
                .map(|config| config.codex_binary.clone())
                .unwrap_or_else(default_codex_binary),
            remote_enabled: false,
        });
    if !profile.workspace_paths.contains(&workspace) {
        if profile.workspace_paths.len() >= 32 {
            return Err(DesktopError::InvalidAction);
        }
        profile.workspace_paths.push(workspace);
    }
    fs::write(
        profile_path,
        serde_json::to_vec(&profile).map_err(|_| DesktopError::Identity)?,
    )
    .map_err(|_| DesktopError::Identity)
}

fn gateway_websocket_url(server_url: &str) -> Result<String, String> {
    let mut server = Url::parse(server_url).map_err(|_| "服务器地址无效".to_owned())?;
    let scheme = if server.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    server
        .set_scheme(scheme)
        .map_err(|_| "服务器地址无效".to_owned())?;
    server.set_path("/v3/remote/ws");
    Ok(server.to_string())
}

#[tauri::command]
pub(crate) async fn start_remote_host(
    input: DaemonProfile,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    if input.workspace_paths.is_empty()
        || input.workspace_paths.len() > 32
        || !valid_binary(&input.codex_binary)
    {
        return Err("请选择工作目录和有效的 Codex 可执行文件".to_owned());
    }
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _transition_guard = LAUNCH_LOCK
            .lock()
            .map_err(|_| DesktopError::AgentFailed.to_string())?;
        let config = read_config(&data_dir).map_err(|_| "请先登录".to_owned())?;
        let cookie = load_cookie(&config.server_url).map_err(|_| "请重新登录".to_owned())?;
        request_json(
            "GET",
            &config.server_url,
            "/v3/remote/sessions",
            Some(&cookie),
            None,
        )
        .map_err(|_| "Gateway 尚未启用远程连接，或当前登录已失效".to_owned())?;
        let identity =
            load_or_create_identity(&data_dir).map_err(|_| "本机设备身份不可用".to_owned())?;
        ensure_device_bound(&config.server_url, &cookie, &identity)
            .map_err(|_| "设备绑定已失效".to_owned())?;
        let (_, profile_path) = host_database_path(&data_dir, identity.device_id());
        let workspace_paths = input
            .workspace_paths
            .into_iter()
            .map(|path| {
                path.canonicalize()
                    .ok()
                    .filter(|path| path.is_dir())
                    .ok_or_else(|| "工作目录不存在".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let profile = DaemonProfile {
            workspace_paths: workspace_paths.clone(),
            codex_binary: input.codex_binary,
            remote_enabled: false,
        };
        let existing_profile = fs::read(&profile_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<DaemonProfile>(&bytes).ok());
        let direct_client = DaemonClient {
            endpoint: daemon_ipc::default_endpoint(identity.device_id()),
        };
        let mut launched = false;
        let (_, active_workspaces, was_remote_enabled, _) =
            ensure_daemon_ready(&direct_client, || {
                fs::write(
                    &profile_path,
                    serde_json::to_vec(&profile).map_err(|_| "无法保存连接设置".to_owned())?,
                )
                .map_err(|_| "无法保存连接设置".to_owned())?;
                launched = true;
                start_daemon_for_data(&data_dir, workspace_paths.first().map(PathBuf::as_path))
                    .map_err(|error| error.to_string())
            })?;
        if !launched {
            if existing_profile
                .as_ref()
                .is_some_and(|existing| existing.codex_binary != profile.codex_binary)
            {
                return Err(
                    "agenthubd 运行期间不能更换 Codex 可执行文件；请恢复当前设置后重试".to_owned(),
                );
            }
            if active_workspaces
                .iter()
                .any(|workspace| !workspace_paths.contains(workspace))
            {
                return Err(
                    "agenthubd 运行期间不能移除已授权工作目录；请保留现有目录后重试".to_owned(),
                );
            }
        }
        for workspace in &workspace_paths {
            direct_client.authorize_workspace(workspace.clone())?;
        }
        direct_client.set_remote_enabled(Some(gateway_websocket_url(&config.server_url)?))?;
        let mut enabled_profile = profile;
        enabled_profile.remote_enabled = true;
        let encoded =
            serde_json::to_vec(&enabled_profile).map_err(|_| "无法保存连接设置".to_owned())?;
        if fs::write(profile_path, encoded).is_err() {
            if !was_remote_enabled {
                let _ = direct_client.set_remote_enabled(None);
            }
            return Err("无法保存连接设置".to_owned());
        }
        Ok(())
    })
    .await
    .map_err(|_| "无法启动本机连接".to_owned())?
}

#[tauri::command]
pub(crate) async fn remote_host_status(state: State<'_, DesktopState>) -> Result<Value, String> {
    let config = read_config(&state.data_dir).ok();
    let identity =
        load_or_create_identity(&state.data_dir).map_err(|_| "设备身份不可用".to_owned())?;
    let (_, profile_path) = host_database_path(&state.data_dir, identity.device_id());
    let profile = fs::read(profile_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DaemonProfile>(&bytes).ok())
        .unwrap_or(DaemonProfile {
            workspace_paths: Vec::new(),
            codex_binary: config
                .map(|config| config.codex_binary)
                .unwrap_or_else(default_codex_binary),
            remote_enabled: false,
        });
    let client = client_for_state(&state).map_err(|error| error.to_string())?;
    let phase = match probe_daemon(&client) {
        DaemonProbe::Ready((_, _, true, true)) => "connected",
        DaemonProbe::Ready((_, _, true, false)) => "connecting",
        DaemonProbe::Ready(_) => "local",
        DaemonProbe::Starting => "starting",
        DaemonProbe::Absent => "stopped",
        DaemonProbe::Failed(error) => return Err(error),
    };
    Ok(json!({"phase":phase,"profile":profile}))
}

fn disable_remote_locked(data_dir: &Path) -> Result<(), String> {
    let identity = load_or_create_identity(data_dir).map_err(|error| error.to_string())?;
    let (_, profile_path) = host_database_path(data_dir, identity.device_id());
    let config = read_config(data_dir).ok();
    let client = DaemonClient {
        endpoint: daemon_ipc::default_endpoint(identity.device_id()),
    };
    let was_remote_enabled = match probe_daemon(&client) {
        DaemonProbe::Ready((_, _, enabled, _)) => enabled,
        DaemonProbe::Starting => {
            wait_for_daemon_ready(
                || probe_daemon(&client),
                Instant::now() + Duration::from_secs(30),
            )?
            .2
        }
        DaemonProbe::Absent => false,
        DaemonProbe::Failed(error) => return Err(error),
    };
    if was_remote_enabled {
        client.set_remote_enabled(None)?;
    }
    if let Ok(bytes) = fs::read(&profile_path)
        && let Ok(mut profile) = serde_json::from_slice::<DaemonProfile>(&bytes)
    {
        profile.remote_enabled = false;
        let encoded = serde_json::to_vec(&profile).map_err(|_| "无法保存连接设置".to_owned())?;
        if fs::write(&profile_path, encoded).is_err() {
            if was_remote_enabled
                && let Some(config) = config
                && let Ok(url) = gateway_websocket_url(&config.server_url)
            {
                let _ = client.set_remote_enabled(Some(url));
            }
            return Err("无法保存连接设置".to_owned());
        }
    }
    Ok(())
}

pub(crate) fn take_logout_authorization(
    data_dir: &Path,
) -> Result<Option<(String, String)>, String> {
    let _transition_guard = LAUNCH_LOCK
        .lock()
        .map_err(|_| DesktopError::AgentFailed.to_string())?;
    disable_remote_locked(data_dir)?;
    let config = read_config(data_dir).ok();
    let authorization = config.as_ref().and_then(|config| {
        load_cookie(&config.server_url)
            .ok()
            .map(|cookie| (config.server_url.clone(), cookie))
    });
    let path = config_path(data_dir);
    if path.exists() {
        fs::remove_file(path).map_err(|_| "无法清除本机登录配置".to_owned())?;
    }
    if let Some(config) = config {
        delete_cookie(&config.server_url).map_err(|error| error.to_string())?;
    }
    Ok(authorization)
}

#[tauri::command]
pub(crate) async fn stop_remote_host(state: State<'_, DesktopState>) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _transition_guard = LAUNCH_LOCK
            .lock()
            .map_err(|_| DesktopError::AgentFailed.to_string())?;
        disable_remote_locked(&data_dir)
    })
    .await
    .map_err(|_| "无法关闭 Remote".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_pulse_runner::{daemon_ipc::DaemonRequestHandler, remote_journal::JournalEvent};

    fn start_ipc_server(
        handler: Arc<dyn DaemonRequestHandler>,
    ) -> (
        String,
        Arc<AtomicBool>,
        thread::JoinHandle<Result<(), String>>,
    ) {
        let endpoint = daemon_ipc::default_endpoint(&format!(
            "desktop-subscription-test-{}",
            uuid::Uuid::new_v4()
        ));
        let stopped = Arc::new(AtomicBool::new(false));
        let server_endpoint = endpoint.clone();
        let server_stopped = stopped.clone();
        let server = thread::spawn(move || {
            daemon_ipc::serve(&server_endpoint, handler, server_stopped)
                .map_err(|error| error.to_string())
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let response = daemon_ipc::request(
                &endpoint,
                &DaemonRequestEnvelope {
                    protocol_version: DAEMON_PROTOCOL_VERSION,
                    request_id: "ready".into(),
                    request: DaemonRequest::GetStatus,
                },
            );
            if response.is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "IPC test server did not start");
            thread::sleep(Duration::from_millis(20));
        }
        (endpoint, stopped, server)
    }

    fn stop_ipc_server(
        endpoint: &str,
        stopped: Arc<AtomicBool>,
        server: thread::JoinHandle<Result<(), String>>,
    ) {
        stopped.store(true, Ordering::Relaxed);
        let _ = daemon_ipc::request(
            endpoint,
            &DaemonRequestEnvelope {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                request_id: "stop".into(),
                request: DaemonRequest::GetStatus,
            },
        );
        server.join().unwrap().unwrap();
    }

    fn test_host_event() -> HostEvent {
        HostEvent {
            event_id: "event-1".into(),
            host_generation: "generation-1".into(),
            hub_session_id: Some("hub-1".into()),
            provider_session_id: Some("provider-1".into()),
            kind: "turn/started".into(),
            raw: json!({"method":"turn/started"}),
            binding: None,
            approval_id: None,
            approval: None,
        }
    }

    fn ready_status() -> DaemonStatus {
        ("generation-1".into(), Vec::new(), false, false)
    }

    #[test]
    fn failed_runtime_disable_preserves_enabled_profile() {
        let dir = tempfile::tempdir().unwrap();
        let identity = DeviceIdentity::generate();
        identity
            .save_new(dir.path().join("device-identity.json"))
            .unwrap();
        let (_, profile_path) = host_database_path(dir.path(), identity.device_id());
        let profile = DaemonProfile {
            workspace_paths: vec![dir.path().to_owned()],
            codex_binary: "codex".into(),
            remote_enabled: true,
        };
        fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
        let handler: Arc<dyn DaemonRequestHandler> = Arc::new(|request| match request {
            DaemonRequest::GetStatus => DaemonResponse::Status {
                host_generation: "generation-1".into(),
                workspaces: vec![],
                remote_enabled: true,
                remote_connected: true,
            },
            DaemonRequest::SetRemoteEnabled { server_url: None } => DaemonResponse::Error {
                code: "disable_failed".into(),
            },
            _ => DaemonResponse::Error {
                code: "unexpected_request".into(),
            },
        });
        let endpoint = daemon_ipc::default_endpoint(identity.device_id());
        let stopped = Arc::new(AtomicBool::new(false));
        let server_endpoint = endpoint.clone();
        let server_stopped = stopped.clone();
        let server = thread::spawn(move || {
            daemon_ipc::serve(&server_endpoint, handler, server_stopped)
                .map_err(|error| error.to_string())
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if daemon_ipc::request(
                &endpoint,
                &DaemonRequestEnvelope {
                    protocol_version: DAEMON_PROTOCOL_VERSION,
                    request_id: "ready".into(),
                    request: DaemonRequest::GetStatus,
                },
            )
            .is_ok()
            {
                break;
            }
            assert!(Instant::now() < deadline, "IPC test server did not start");
            thread::sleep(Duration::from_millis(20));
        }

        assert_eq!(
            disable_remote_locked(dir.path()).unwrap_err(),
            "disable_failed"
        );
        let stored: DaemonProfile =
            serde_json::from_slice(&fs::read(profile_path).unwrap()).unwrap();
        assert!(stored.remote_enabled);

        stop_ipc_server(&endpoint, stopped, server);
    }

    #[test]
    fn starting_daemon_is_awaited_without_launching_another_process() {
        let mut probes = std::collections::VecDeque::from([
            DaemonProbe::Starting,
            DaemonProbe::Ready(ready_status()),
        ]);
        let mut launches = 0;

        let status = ensure_daemon_ready_with(
            || probes.pop_front().expect("bounded daemon probes"),
            || {
                launches += 1;
                Ok(())
            },
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(status.0, "generation-1");
        assert_eq!(launches, 0);
    }

    #[test]
    fn absent_daemon_is_launched_once_then_awaited() {
        let mut probes = std::collections::VecDeque::from([
            DaemonProbe::Absent,
            DaemonProbe::Absent,
            DaemonProbe::Starting,
            DaemonProbe::Ready(ready_status()),
        ]);
        let mut launches = 0;

        let status = ensure_daemon_ready_with(
            || probes.pop_front().expect("bounded daemon probes"),
            || {
                launches += 1;
                Ok(())
            },
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(status.0, "generation-1");
        assert_eq!(launches, 1);
    }

    #[test]
    fn transient_transport_failure_after_launch_is_awaited_without_relaunch() {
        let mut probes = std::collections::VecDeque::from([
            DaemonProbe::Absent,
            DaemonProbe::Failed("ipc_response_missing".into()),
            DaemonProbe::Ready(ready_status()),
        ]);
        let mut launches = 0;

        let status = ensure_daemon_ready_with(
            || probes.pop_front().expect("bounded daemon probes"),
            || {
                launches += 1;
                Ok(())
            },
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();

        assert_eq!(status.0, "generation-1");
        assert_eq!(launches, 1);
    }

    #[test]
    fn fatal_daemon_probe_error_does_not_launch() {
        let mut launches = 0;
        let result = ensure_daemon_ready_with(
            || DaemonProbe::Failed("daemon_protocol_mismatch".into()),
            || {
                launches += 1;
                Ok(())
            },
            Instant::now() + Duration::from_secs(1),
        );

        assert_eq!(result.unwrap_err(), "daemon_protocol_mismatch");
        assert_eq!(launches, 0);
    }

    #[test]
    fn starting_daemon_timeout_never_falls_back_to_launch() {
        let mut launches = 0;
        let result = ensure_daemon_ready_with(
            || DaemonProbe::Starting,
            || {
                launches += 1;
                Ok(())
            },
            Instant::now(),
        );

        assert_eq!(result.unwrap_err(), "daemon_start_timeout");
        assert_eq!(launches, 0);
    }

    #[test]
    fn execute_retries_transport_loss_with_the_same_durable_command_id() {
        let command = RemoteCommand {
            client_cmd_id: "command-1".into(),
            hub_session_id: "hub-1".into(),
            host_generation: Some("generation-1".into()),
            command: agent_pulse_runner::remote_host::RemoteAction::SendPrompt {
                text: "hello".into(),
            },
        };
        let receipt = CommandReceipt {
            client_cmd_id: command.client_cmd_id.clone(),
            hub_session_id: command.hub_session_id.clone(),
            status: "accepted".into(),
            code: None,
            result: None,
        };
        let mut attempts = Vec::new();
        let mut responses = std::collections::VecDeque::from([
            Err("ipc_response_missing".to_owned()),
            Ok(DaemonResponse::Receipt(receipt.clone())),
        ]);

        let result = execute_command_with_retry(
            command.clone(),
            Instant::now() + Duration::from_secs(1),
            |attempt, _remaining| {
                attempts.push(attempt);
                responses.pop_front().expect("bounded retry responses")
            },
        )
        .unwrap();

        assert_eq!(result, receipt);
        assert_eq!(attempts, vec![command.clone(), command]);
    }

    #[test]
    fn execute_rejects_a_receipt_for_another_command() {
        let command = RemoteCommand {
            client_cmd_id: "command-1".into(),
            hub_session_id: "hub-1".into(),
            host_generation: Some("generation-1".into()),
            command: agent_pulse_runner::remote_host::RemoteAction::Interrupt,
        };
        let result =
            execute_command_with_retry(command, Instant::now() + Duration::from_secs(1), |_, _| {
                Ok(DaemonResponse::Receipt(CommandReceipt {
                    client_cmd_id: "command-other".into(),
                    hub_session_id: "hub-1".into(),
                    status: "accepted".into(),
                    code: None,
                    result: None,
                }))
            });

        assert_eq!(result.unwrap_err(), "daemon_command_receipt_mismatch");
    }

    #[test]
    fn host_database_path_is_deterministic_and_distinct_per_device() {
        let dir = Path::new("test-dir");
        let first = host_database_path(dir, "dev-1");
        assert_eq!(first, host_database_path(dir, "dev-1"));
        assert_ne!(first, host_database_path(dir, "dev-2"));
    }

    #[test]
    fn subscription_handshake_replays_host_events_and_preserves_request_id() {
        let event = test_host_event();
        let replay_event = event.clone();
        let handler: Arc<dyn DaemonRequestHandler> = Arc::new(move |request| match request {
            DaemonRequest::GetStatus => DaemonResponse::Status {
                host_generation: "generation-1".into(),
                workspaces: vec![],
                remote_enabled: false,
                remote_connected: false,
            },
            DaemonRequest::GetEvents {
                after_cursor: 3, ..
            } => DaemonResponse::Events(EventsAfter::Events {
                events: vec![
                    JournalEvent {
                        cursor: 4,
                        payload: json!({"type":"receipt","receipt":{}}),
                    },
                    JournalEvent {
                        cursor: 5,
                        payload: json!({"type":"event","event":replay_event}),
                    },
                ],
                cursor: 5,
            }),
            _ => DaemonResponse::Error {
                code: "test_complete".into(),
            },
        });
        let (endpoint, stopped, server) = start_ipc_server(handler);
        let (sender, receiver) = mpsc::sync_channel(2);

        let (cursor, request_id, subscription) =
            open_subscription(&endpoint, 3, &sender, Duration::from_secs(1)).unwrap();

        assert_eq!(cursor, 5);
        assert!(request_id.starts_with("desktop-sub-"));
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .event_id,
            event.event_id
        );
        assert!(receiver.try_recv().is_err());
        drop(subscription);
        stop_ipc_server(&endpoint, stopped, server);
    }

    #[test]
    fn live_subscription_skips_receipts_before_delivering_host_events() {
        let event = test_host_event();
        let streamed_event = event.clone();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let handler_calls = calls.clone();
        let handler: Arc<dyn DaemonRequestHandler> = Arc::new(move |request| match request {
            DaemonRequest::GetStatus => DaemonResponse::Status {
                host_generation: "generation-1".into(),
                workspaces: vec![],
                remote_enabled: false,
                remote_connected: false,
            },
            DaemonRequest::GetSnapshot => DaemonResponse::Snapshot(JournalSnapshot {
                sessions: vec![],
                pending_approvals: vec![agent_pulse_runner::remote_host::PendingApprovalSnapshot {
                    approval_id: "approval-1".into(),
                    hub_session_id: "hub-1".into(),
                    host_generation: "generation-1".into(),
                    turn_id: "turn-1".into(),
                    can_approve: true,
                    can_decline: true,
                }],
                cursor: 3,
            }),
            DaemonRequest::GetEvents { after_cursor, .. } => {
                let call = handler_calls.fetch_add(1, Ordering::Relaxed);
                match call {
                    0 => DaemonResponse::Events(EventsAfter::Events {
                        events: vec![],
                        cursor: after_cursor,
                    }),
                    1 => DaemonResponse::Events(EventsAfter::Events {
                        events: vec![JournalEvent {
                            cursor: 4,
                            payload: json!({"type":"receipt","receipt":{}}),
                        }],
                        cursor: 4,
                    }),
                    _ => DaemonResponse::Events(EventsAfter::Events {
                        events: vec![JournalEvent {
                            cursor: 5,
                            payload: json!({"type":"event","event":streamed_event}),
                        }],
                        cursor: 5,
                    }),
                }
            }
            _ => DaemonResponse::Error {
                code: "unexpected_request".into(),
            },
        });
        let (endpoint, stopped, server) = start_ipc_server(handler);
        let client = DaemonClient {
            endpoint: endpoint.clone(),
        };

        let (subscription, receiver, snapshot) = client.subscribe(2).unwrap();

        assert_eq!(snapshot.cursor, 3);
        assert_eq!(snapshot.pending_approvals[0].approval_id, "approval-1");

        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .event_id,
            event.event_id
        );
        assert!(receiver.try_recv().is_err());
        drop(subscription);
        stop_ipc_server(&endpoint, stopped, server);
    }

    #[test]
    fn subscription_handshake_rejects_expired_cursor() {
        let handler: Arc<dyn DaemonRequestHandler> = Arc::new(|request| match request {
            DaemonRequest::GetStatus => DaemonResponse::Status {
                host_generation: "generation-1".into(),
                workspaces: vec![],
                remote_enabled: false,
                remote_connected: false,
            },
            DaemonRequest::GetEvents { .. } => {
                DaemonResponse::Events(EventsAfter::SnapshotRequired {
                    snapshot: JournalSnapshot {
                        sessions: vec![],
                        pending_approvals: vec![],
                        cursor: 9,
                    },
                })
            }
            _ => DaemonResponse::Error {
                code: "unexpected_request".into(),
            },
        });
        let (endpoint, stopped, server) = start_ipc_server(handler);
        let (sender, _receiver) = mpsc::sync_channel(1);

        let error = open_subscription(&endpoint, 0, &sender, Duration::from_secs(1))
            .err()
            .expect("expired cursor must fail");

        assert_eq!(error, "daemon_snapshot_required");
        stop_ipc_server(&endpoint, stopped, server);
    }
}
