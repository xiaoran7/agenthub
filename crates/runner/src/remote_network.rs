//! Single outbound v3 connection. Network retries never own or stop the native Host.
use crate::{
    collectors::CodexAppServer,
    device::DeviceIdentity,
    remote_host::{
        CommandReceipt, HostEvent, NativeSessionHost, NativeTransport, RemoteCommand,
        SessionBinding,
    },
    remote_journal::{DurableHost, EventsAfter, JournalSnapshot, RemoteJournal},
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::TcpStream,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

const LOCAL_HOST_SEND_TIMEOUT: Duration = Duration::from_secs(2);
const LOCAL_HOST_QUERY_TIMEOUT: Duration = Duration::from_secs(25);

/// How long a provider may take to answer one request.
///
/// The remote Host answers `thread/list` from the provider's own app-server,
/// which reads its state database and, on a busy machine, can take well over
/// three seconds. A short timeout turned a slow-but-healthy provider into a
/// hard failure that left the console with no sessions at all, so the budget
/// matches the collector default the local console already uses.
const PROVIDER_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// How often the Runner re-reads the provider's session list and republishes
/// it to the Gateway.
///
/// The `ready` frame is the only channel that carries discovered sessions, so
/// a discovery failure — or a session created after the last one — stayed
/// invisible in the web console for the life of the process.
const DISCOVERY_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

fn send_local_command(
    sender: &SyncSender<LocalHostCommand>,
    mut command: LocalHostCommand,
) -> Result<(), String> {
    let deadline = Instant::now() + LOCAL_HOST_SEND_TIMEOUT;
    loop {
        match sender.try_send(command) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(returned)) if Instant::now() < deadline => {
                command = returned;
                thread::sleep(Duration::from_millis(5));
            }
            Err(mpsc::TrySendError::Full(_)) => return Err("host_queue_timeout".into()),
            Err(mpsc::TrySendError::Disconnected(_)) => return Err("host_not_running".into()),
        }
    }
}

pub enum LocalHostCommand {
    Execute {
        command: RemoteCommand,
        respond_to: SyncSender<Result<CommandReceipt, String>>,
    },
    QuerySessions {
        respond_to: SyncSender<Vec<SessionBinding>>,
    },
    QuerySnapshot {
        respond_to: SyncSender<Result<JournalSnapshot, String>>,
    },
    QueryEvents {
        after_cursor: i64,
        limit: usize,
        respond_to: SyncSender<Result<EventsAfter, String>>,
    },
    QueryWorkspaces {
        respond_to: SyncSender<Vec<PathBuf>>,
    },
    AuthorizeWorkspace {
        workspace: PathBuf,
        respond_to: SyncSender<Result<PathBuf, String>>,
    },
    QueryGeneration {
        respond_to: SyncSender<String>,
    },
    QueryModels {
        respond_to: SyncSender<Result<Value, String>>,
    },
    Discover {
        respond_to: SyncSender<Result<Value, String>>,
    },
    SetRemoteEnabled {
        server_url: Option<String>,
        identity: DeviceIdentity,
        respond_to: SyncSender<Result<bool, String>>,
    },
    QueryStatus {
        respond_to: SyncSender<(String, Vec<PathBuf>, bool, bool)>,
    },
}

pub struct LocalHostChannel {
    pub receiver: Receiver<LocalHostCommand>,
    pub listeners: Arc<Mutex<HashMap<u64, SyncSender<HostEvent>>>>,
}

#[derive(Clone)]
pub struct LocalHostClient {
    sender: SyncSender<LocalHostCommand>,
    listeners: Arc<Mutex<HashMap<u64, SyncSender<HostEvent>>>>,
    next_listener_id: Arc<AtomicU64>,
}

pub struct LocalEventSubscription {
    listeners: Arc<Mutex<HashMap<u64, SyncSender<HostEvent>>>>,
    id: u64,
}

impl Drop for LocalEventSubscription {
    fn drop(&mut self) {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.remove(&self.id);
        }
    }
}

impl LocalHostClient {
    pub fn execute(&self, command: RemoteCommand) -> Result<CommandReceipt, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::Execute {
                command,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "command_timeout".to_string())?
    }

    pub fn query_sessions(&self) -> Result<Vec<SessionBinding>, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QuerySessions { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn snapshot(&self) -> Result<JournalSnapshot, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QuerySnapshot { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn events_after(&self, after_cursor: i64, limit: usize) -> Result<EventsAfter, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryEvents {
                after_cursor,
                limit,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn query_workspaces(&self) -> Result<Vec<PathBuf>, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryWorkspaces { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn authorize_workspace(&self, workspace: PathBuf) -> Result<PathBuf, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::AuthorizeWorkspace {
                workspace,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "authorize_timeout".to_string())?
    }

    pub fn query_generation(&self) -> Result<String, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryGeneration { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn query_models(&self) -> Result<Value, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryModels { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn discover(&self) -> Result<Value, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(&self.sender, LocalHostCommand::Discover { respond_to: tx })?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn set_remote_enabled(
        &self,
        server_url: Option<String>,
        identity: DeviceIdentity,
    ) -> Result<bool, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::SetRemoteEnabled {
                server_url,
                identity,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "set_remote_timeout".to_string())?
    }

    pub fn status(&self) -> Result<(String, Vec<PathBuf>, bool, bool), String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryStatus { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn enable_remote(
        &self,
        server_url: String,
        identity: DeviceIdentity,
    ) -> Result<(), String> {
        self.set_remote_enabled(Some(server_url), identity)
            .map(|_| ())
    }

    pub fn subscribe(&self, capacity: usize) -> (LocalEventSubscription, Receiver<HostEvent>) {
        let (tx, rx) = mpsc::sync_channel(capacity);
        let id = self.next_listener_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.insert(id, tx);
        }
        (
            LocalEventSubscription {
                listeners: self.listeners.clone(),
                id,
            },
            rx,
        )
    }
}

pub fn create_local_host_channel() -> (LocalHostClient, LocalHostChannel) {
    let (tx, rx) = mpsc::sync_channel(128);
    let listeners = Arc::new(Mutex::new(HashMap::new()));
    let client = LocalHostClient {
        sender: tx,
        listeners: listeners.clone(),
        next_listener_id: Arc::new(AtomicU64::new(1)),
    };
    let channel = LocalHostChannel {
        receiver: rx,
        listeners,
    };
    (client, channel)
}

enum NetworkEvent {
    Connected(u64, i64),
    Sent(u64, i64),
    Frame(u64, Value),
    Offline(u64),
}
enum NetworkSend {
    Data(u64, i64, Value),
    Ack(u64, i64),
}

#[derive(Default)]
pub struct RemoteControl {
    pub stopped: AtomicBool,
    pub connected: AtomicBool,
    pub ready: AtomicBool,
    rejected_epoch: AtomicU64,
}

struct StopOnExit(Arc<RemoteControl>);
impl Drop for StopOnExit {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Relaxed);
        self.0.connected.store(false, Ordering::Relaxed);
        self.0.ready.store(false, Ordering::Relaxed);
    }
}

pub fn run(
    binary: PathBuf,
    workspaces: Vec<PathBuf>,
    database: PathBuf,
    identity: DeviceIdentity,
    server: String,
) -> Result<()> {
    let control = Arc::new(RemoteControl::default());
    let signal = control.clone();
    ctrlc::set_handler(move || signal.stopped.store(true, Ordering::Relaxed))?;
    run_with_control(binary, workspaces, database, identity, server, control)
}

/// Embeddable Host; the owning application controls shutdown without installing
/// a second process-wide signal handler. Network connectivity is factual only.
pub fn run_with_control(
    binary: PathBuf,
    workspaces: Vec<PathBuf>,
    database: PathBuf,
    identity: DeviceIdentity,
    server: String,
    control: Arc<RemoteControl>,
) -> Result<()> {
    run_with_local_control(
        binary,
        workspaces,
        database,
        identity,
        Some(server),
        control,
        None,
        &crate::device_name::resolve(None, None),
    )
}

fn sanitize_remote_discovery(mut discovery: Option<Value>) -> Option<Value> {
    if let Some(candidates) = discovery
        .as_mut()
        .and_then(|value| value.get_mut("candidates"))
        .and_then(Value::as_array_mut)
    {
        for candidate in candidates {
            if let Some(path) = candidate.get("workspace_path").and_then(Value::as_str) {
                candidate["workspace_path"] = json!(crate::privacy::workspace_id_for_path(path));
            }
        }
    }
    discovery
}

/// Re-read the provider's session list and publish it to the Gateway.
///
/// The `ready` frame is the only channel that carries discovered sessions, so
/// every path that publishes one has to publish a *current* list. A discovery
/// failure leaves the previous list in place rather than blanking the console.
fn refresh_discovery_and_publish<T: NativeTransport>(
    durable: &mut DurableHost<T>,
    cached_models: &Option<Value>,
    cached_discovery: &mut Option<Value>,
    published_discovery: &mut Option<Value>,
    device_name: &str,
) -> Result<()> {
    if let Ok(discovery) = durable.host.discover() {
        let discovery = sanitize_remote_discovery(Some(discovery));
        *cached_discovery = discovery.clone();
        *published_discovery = discovery.clone();
        let ready_payload = remote_ready_payload(
            durable,
            cached_models.as_ref(),
            cached_discovery.as_ref(),
            device_name,
        );
        durable.journal.enqueue_remote(&ready_payload)?;
    }
    Ok(())
}

fn remote_ready_payload<T: NativeTransport>(
    durable: &DurableHost<T>,
    models: Option<&Value>,
    discovery: Option<&Value>,
    device_name: &str,
) -> Value {
    let mut workspaces = durable
        .host
        .workspaces()
        .iter()
        .map(|path| crate::privacy::workspace_id_for_path(&path.to_string_lossy()))
        .collect::<Vec<_>>();
    for key in durable.host.discovered_workspaces().keys() {
        if key.starts_with("ws-") && !workspaces.contains(key) {
            workspaces.push(key.to_owned());
        }
    }
    let sessions = durable
        .host
        .sessions()
        .into_iter()
        .map(|session| {
            json!({
                "hub_session_id": session.hub_session_id,
                "provider_session_id": session.provider_session_id,
                "workspace_path": crate::privacy::workspace_id_for_path(
                    &session.workspace_path.to_string_lossy(),
                ),
                "host_generation": durable.host.generation(),
                "control_connected": session.control_connected,
                "execution_state": session.execution_state,
                "current_turn_id": session.current_turn_id,
            })
        })
        .collect::<Vec<_>>();
    let mut payload = json!({
        "type": "ready",
        "provider": "codex_app_server",
        "host_generation": durable.host.generation(),
        "device_name": device_name,
        "workspaces": workspaces,
        "sessions": sessions,
        "models": models.cloned(),
        "discovery": discovery.cloned(),
        "pending_approvals": durable.host.pending_approvals(),
        "attach_running": false,
    });
    crate::privacy::sanitize_workspace_paths_in_json(&mut payload);
    payload
}

pub fn run_with_local_control(
    binary: PathBuf,
    workspaces: Vec<PathBuf>,
    database: PathBuf,
    identity: DeviceIdentity,
    server: Option<String>,
    control: Arc<RemoteControl>,
    local_channel: Option<LocalHostChannel>,
    device_name: &str,
) -> Result<()> {
    let _cleanup = StopOnExit(control.clone());
    if control.stopped.load(Ordering::Relaxed) {
        return Ok(());
    }
    let (url, bound_server) = match server {
        Some(ref s) if !s.trim().is_empty() && s != "local://agenthub" => {
            let validated = validate_server(s)?;
            (Some(validated.clone()), validated)
        }
        _ => (None, "local://agenthub".to_owned()),
    };
    let mut journal = RemoteJournal::open(database)?;
    journal.bind_identity(identity.device_id(), &bound_server)?;
    let mut effective_workspaces = workspaces.clone();
    if effective_workspaces.is_empty() {
        if let Ok(cwd) = std::env::current_dir() {
            effective_workspaces.push(cwd);
        }
    }
    let codex = CodexAppServer::start_interactive(binary, PROVIDER_RESPONSE_TIMEOUT).ok();
    let zcode =
        crate::collectors::ZCodeAppServer::start_if_available(PROVIDER_RESPONSE_TIMEOUT).ok();
    let claude = crate::collectors::discover_claude_binary();
    if codex.is_none() && zcode.is_none() && !claude.exists() && url.is_some() {
        return Err(anyhow::anyhow!("native_host_start_failed"));
    }
    let native = crate::remote_host::MultiProviderTransport::new(codex, zcode);
    let host = NativeSessionHost::new(native, effective_workspaces)
        .map_err(|error| anyhow::anyhow!(error.code))?;
    let mut durable = DurableHost::new(host, journal)?;
    let mut cached_models = durable.host.models().ok();
    let mut cached_discovery = sanitize_remote_discovery(durable.host.discover().ok());
    // The last discovery actually published to the Gateway. A refresh that
    // finds the same sessions must not enqueue another full snapshot.
    let mut published_discovery = cached_discovery.clone();
    let mut last_discovery_refresh = Instant::now();
    if url.is_some() {
        let ready_payload = remote_ready_payload(
            &durable,
            cached_models.as_ref(),
            cached_discovery.as_ref(),
            device_name,
        );
        durable.journal.enqueue_remote(&ready_payload)?;
    }
    control.ready.store(true, Ordering::Release);
    let command_seq = Arc::new(AtomicI64::new(durable.journal.received_command_seq()?));
    let (events_tx, events) = mpsc::sync_channel(256);
    let (mut send_tx, send_rx) = mpsc::sync_channel(64);
    let host_generation = durable.host.generation().to_owned();
    let remote_generation = Arc::new(AtomicU64::new(u64::from(url.is_some())));
    let worker_stop = control.clone();
    let worker_seq = command_seq.clone();
    let mut current_server = url.clone();
    if let Some(target_url) = url {
        let events_tx_clone = events_tx.clone();
        let worker_remote_generation = remote_generation.clone();
        thread::spawn(move || {
            network_loop(
                NetworkLoopContext {
                    url: target_url,
                    identity,
                    host_generation,
                    command_seq: worker_seq,
                    control: worker_stop,
                    remote_generation: worker_remote_generation,
                    expected_generation: 1,
                },
                events_tx_clone,
                send_rx,
            )
        });
    }
    let mut active = None;
    let mut offered = 0;
    while !control.stopped.load(Ordering::Relaxed) {
        let polled_events = match durable.poll(current_server.is_some()) {
            Ok(events) => events,
            Err(error) => {
                if !durable.host.is_alive() {
                    control.stopped.store(true, Ordering::Relaxed);
                    bail!("native_host_lost");
                }
                bail!(error);
            }
        };
        if !durable.host.is_alive() {
            control.stopped.store(true, Ordering::Relaxed);
            bail!("native_host_lost");
        }
        // Broadcast events to local listeners if registered
        if !polled_events.is_empty()
            && let Some(ref local) = local_channel
            && let Ok(mut listeners) = local.listeners.lock()
        {
            listeners.retain(|_, listener| {
                let mut alive = true;
                for event in &polled_events {
                    if listener.try_send(event.clone()).is_err() {
                        alive = false;
                        break;
                    }
                }
                alive
            });
        }
        // Drain local commands
        if let Some(ref local) = local_channel {
            for _ in 0..32 {
                match local.receiver.try_recv() {
                    Ok(LocalHostCommand::Execute {
                        command,
                        respond_to,
                    }) => {
                        let result = durable
                            .handle_local_command(command)
                            .map_err(|err| err.to_string());
                        if result.is_ok() && current_server.is_some() {
                            if let Err(error) = refresh_discovery_and_publish(
                                &mut durable,
                                &cached_models,
                                &mut cached_discovery,
                                &mut published_discovery,
                                device_name,
                            ) {
                                let _ = respond_to.send(Err(error.to_string()));
                                continue;
                            }
                        }
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::QuerySessions { respond_to }) => {
                        let _ = respond_to.send(durable.host.sessions());
                    }
                    Ok(LocalHostCommand::QuerySnapshot { respond_to }) => {
                        let _ =
                            respond_to.send(durable.snapshot().map_err(|error| error.to_string()));
                    }
                    Ok(LocalHostCommand::QueryEvents {
                        after_cursor,
                        limit,
                        respond_to,
                    }) => {
                        let result = durable
                            .events_after(after_cursor, limit)
                            .map_err(|error| error.to_string());
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::QueryWorkspaces { respond_to }) => {
                        let _ = respond_to.send(durable.host.workspaces().to_vec());
                    }
                    Ok(LocalHostCommand::AuthorizeWorkspace {
                        workspace,
                        respond_to,
                    }) => {
                        let result = durable
                            .host
                            .authorize_workspace(workspace)
                            .map_err(|e| e.code);
                        if result.is_ok() && current_server.is_some() {
                            if let Err(error) = refresh_discovery_and_publish(
                                &mut durable,
                                &cached_models,
                                &mut cached_discovery,
                                &mut published_discovery,
                                device_name,
                            ) {
                                let _ = respond_to.send(Err(error.to_string()));
                                continue;
                            }
                        }
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::QueryGeneration { respond_to }) => {
                        let _ = respond_to.send(durable.host.generation().to_string());
                    }
                    Ok(LocalHostCommand::QueryModels { respond_to }) => {
                        let result = durable.host.models().map_err(|e| e.code);
                        if let Ok(models) = &result {
                            cached_models = Some(models.clone());
                        }
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::Discover { respond_to }) => {
                        let result = durable.host.discover().map_err(|e| e.code);
                        if let Ok(discovery) = &result {
                            let discovery = sanitize_remote_discovery(Some(discovery.clone()));
                            cached_discovery = discovery.clone();
                            published_discovery = discovery;
                        }
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::SetRemoteEnabled {
                        server_url,
                        identity: remote_identity,
                        respond_to,
                    }) => {
                        let res = (|| -> Result<bool> {
                            let validated = match server_url {
                                Some(server_url) => Some(validate_server(&server_url)?),
                                None => None,
                            };
                            if validated.is_some() && current_server == validated {
                                return Ok(true);
                            }
                            let next_generation =
                                remote_generation.fetch_add(1, Ordering::SeqCst) + 1;
                            control.connected.store(false, Ordering::Relaxed);
                            active = None;
                            let Some(validated) = validated else {
                                current_server = None;
                                return Ok(false);
                            };
                            durable.journal.rebind_server_if_local(&validated)?;
                            let ready_payload = remote_ready_payload(
                                &durable,
                                cached_models.as_ref(),
                                cached_discovery.as_ref(),
                                device_name,
                            );
                            durable.journal.enqueue_remote(&ready_payload)?;
                            current_server = Some(validated.clone());
                            let (new_send_tx, new_send_rx) = mpsc::sync_channel(64);
                            send_tx = new_send_tx;
                            let events_tx_clone = events_tx.clone();
                            let worker_stop = control.clone();
                            let worker_seq = command_seq.clone();
                            let worker_remote_generation = remote_generation.clone();
                            let host_generation = durable.host.generation().to_owned();
                            thread::spawn(move || {
                                network_loop(
                                    NetworkLoopContext {
                                        url: validated,
                                        identity: remote_identity,
                                        host_generation,
                                        command_seq: worker_seq,
                                        control: worker_stop,
                                        remote_generation: worker_remote_generation,
                                        expected_generation: next_generation,
                                    },
                                    events_tx_clone,
                                    new_send_rx,
                                )
                            });
                            Ok(true)
                        })();
                        let _ = respond_to.send(res.map_err(|e| e.to_string()));
                    }
                    Ok(LocalHostCommand::QueryStatus { respond_to }) => {
                        let _ = respond_to.send((
                            durable.host.generation().to_string(),
                            durable.host.workspaces().to_vec(),
                            current_server.is_some(),
                            control.connected.load(Ordering::Relaxed),
                        ));
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        // Bound network work each tick so native stdout is drained during replay.
        for event in events.try_iter().take(128) {
            handle_network_event(
                event,
                remote_generation.load(Ordering::SeqCst),
                &mut durable,
                &control,
                &mut active,
                &mut offered,
                &send_tx,
                &command_seq,
            )?;
        }
        // Re-read the provider's session list and republish it. The `ready`
        // frame is the only channel that carries discovered sessions, so a
        // startup discovery that failed — or a session created since the last
        // refresh — otherwise stayed invisible in the web console for the life
        // of the process. The republish is skipped when nothing changed, so
        // the outbox does not grow on every tick.
        if current_server.is_some()
            && last_discovery_refresh.elapsed() >= DISCOVERY_REFRESH_INTERVAL
        {
            last_discovery_refresh = Instant::now();
            if let Ok(discovery) = durable.host.discover() {
                let discovery = sanitize_remote_discovery(Some(discovery));
                if discovery != published_discovery {
                    published_discovery = discovery.clone();
                    cached_discovery = discovery;
                    let ready_payload = remote_ready_payload(
                        &durable,
                        cached_models.as_ref(),
                        cached_discovery.as_ref(),
                        device_name,
                    );
                    durable.journal.enqueue_remote(&ready_payload)?;
                }
            }
        }
        if let Some(epoch) = active {
            for (seq, payload) in durable.journal.pending(offered, 32)? {
                // Persist the send attempt before the network thread can transmit.
                // A crash after send but before its callback must still accept the
                // server's durable ACK on reconnect. Unsent attempts remain replayable.
                durable.journal.mark_sent(seq)?;
                if send_tx
                    .try_send(NetworkSend::Data(epoch, seq, payload))
                    .is_err()
                {
                    break;
                }
                offered = seq;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_network_event<T: NativeTransport>(
    event: NetworkEvent,
    current_remote_generation: u64,
    durable: &mut DurableHost<T>,
    control: &RemoteControl,
    active: &mut Option<u64>,
    offered: &mut i64,
    send_tx: &SyncSender<NetworkSend>,
    command_seq: &AtomicI64,
) -> Result<()> {
    match event {
        NetworkEvent::Connected(epoch, server_seq) if epoch >> 32 == current_remote_generation => {
            let (sent, acked) = durable.journal.watermarks()?;
            if server_seq < acked {
                reject_remote_epoch(
                    control,
                    active,
                    epoch,
                    &format!("server_seq {server_seq} < acked {acked}"),
                );
                return Ok(());
            }
            if server_seq > sent {
                for s in (sent + 1)..=server_seq {
                    let _ = durable.journal.mark_sent(s);
                }
            }
            match durable.journal.acknowledge(server_seq) {
                Ok(()) => {}
                Err(crate::remote_journal::JournalError::InvalidAck) => {
                    reject_remote_epoch(control, active, epoch, "journal ack invalid");
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
            *offered = server_seq;
            *active = Some(epoch);
            control.connected.store(true, Ordering::Relaxed);
            eprintln!("{{\"code\":\"remote_connected\"}}");
        }
        NetworkEvent::Sent(epoch, seq) if *active == Some(epoch) => {
            durable.journal.mark_sent(seq)?
        }
        NetworkEvent::Offline(epoch) if *active == Some(epoch) => {
            *active = None;
            control.connected.store(false, Ordering::Relaxed);
            eprintln!("{{\"code\":\"remote_reconnecting\"}}");
        }
        NetworkEvent::Frame(epoch, frame) if *active == Some(epoch) => {
            match frame["type"].as_str() {
                Some("ack") => {
                    let Some(seq) = frame["seq"].as_i64() else {
                        reject_remote_epoch(control, active, epoch, "ack missing seq");
                        return Ok(());
                    };
                    let (sent, _) = durable.journal.watermarks()?;
                    if seq > sent {
                        for s in (sent + 1)..=seq {
                            let _ = durable.journal.mark_sent(s);
                        }
                    }
                    match durable.journal.acknowledge(seq) {
                        Ok(()) => {}
                        Err(crate::remote_journal::JournalError::InvalidAck) => {
                            reject_remote_epoch(control, active, epoch, "ack seq invalid");
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                Some("command") => {
                    let Some(seq) = frame["seq"].as_i64() else {
                        reject_remote_epoch(control, active, epoch, "command missing seq");
                        return Ok(());
                    };
                    let Ok(command) =
                        serde_json::from_value::<RemoteCommand>(frame["command"].clone())
                    else {
                        reject_remote_epoch(control, active, epoch, "command json invalid");
                        return Ok(());
                    };
                    match durable.handle_inbound_command(seq, command) {
                        Ok(_) => {
                            command_seq
                                .store(durable.journal.received_command_seq()?, Ordering::Relaxed);
                        }
                        Err(crate::remote_journal::JournalError::InvalidAck) => {
                            reject_remote_epoch(control, active, epoch, "command ack invalid");
                            return Ok(());
                        }
                        Err(error) => return Err(error.into()),
                    }
                    // If the bounded queue is full the heartbeat/reconnect handshake
                    // still reports this durable watermark; never block native polling.
                    let _ = send_tx.try_send(NetworkSend::Ack(
                        epoch,
                        durable.journal.received_command_seq()?,
                    ));
                }
                Some("heartbeat_ack") => {}
                Some("error") => {
                    eprintln!("[Runner] Received remote error: {:?}", frame);
                    reject_remote_epoch(control, active, epoch, "received remote error");
                }
                _ => {
                    eprintln!("[Runner] Received unexpected frame: {:?}", frame);
                    reject_remote_epoch(control, active, epoch, "unexpected frame");
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn remote_epoch_is_active(
    control: &RemoteControl,
    remote_generation: &AtomicU64,
    expected_generation: u64,
    epoch: u64,
) -> bool {
    !control.stopped.load(Ordering::Relaxed)
        && remote_generation.load(Ordering::SeqCst) == expected_generation
        && control.rejected_epoch.load(Ordering::SeqCst) != epoch
}

fn reject_remote_epoch(
    control: &RemoteControl,
    active: &mut Option<u64>,
    epoch: u64,
    reason: &str,
) {
    if *active == Some(epoch) {
        *active = None;
    }
    control.connected.store(false, Ordering::Relaxed);
    control.rejected_epoch.store(epoch, Ordering::SeqCst);
    eprintln!(
        "{{\"code\":\"remote_protocol_rejected\", \"reason\":\"{}\"}}",
        reason
    );
}

fn is_loopback_or_private_host(host: Option<&str>) -> bool {
    let Some(host_str) = host else {
        return false;
    };
    if matches!(host_str, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
        return true;
    }
    if !host_str.contains('.') && !host_str.starts_with('[') {
        return true;
    }
    if let Ok(ip) = host_str.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(v4) => {
                let octets = v4.octets();
                octets[0] == 127
                    || octets[0] == 10
                    || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                    || (octets[0] == 192 && octets[1] == 168)
                    || (octets[0] == 100 && (64..=127).contains(&octets[1]))
            }
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        }
    } else {
        false
    }
}

fn validate_server(server: &str) -> Result<String> {
    let mut normalized = server.trim().to_string();
    if let Some(stripped) = normalized.strip_prefix("http://") {
        normalized = format!("ws://{stripped}");
    } else if let Some(stripped) = normalized.strip_prefix("https://") {
        normalized = format!("wss://{stripped}");
    }
    let mut url = url::Url::parse(&normalized)?;
    if !matches!(url.scheme(), "ws" | "wss")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("invalid_remote_url");
    }
    if url.path() == "/" || url.path().is_empty() {
        url.set_path("/v3/remote/ws");
    }
    if url.scheme() == "ws" && !is_loopback_or_private_host(url.host_str()) {
        bail!("remote_tls_required");
    }
    if url.path() != "/v3/remote/ws" {
        bail!("remote_url_path_required");
    }
    Ok(url.to_string())
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

fn set_timeouts(socket: &mut Socket, read: Duration) -> Result<()> {
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => &mut stream.sock,
        _ => bail!("unsupported_remote_tls"),
    };
    stream.set_read_timeout(Some(read))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(())
}

fn send(socket: &mut Socket, value: Value) -> Result<()> {
    socket.send(Message::Text(value.to_string().into()))?;
    Ok(())
}

fn receive(socket: &mut Socket) -> Result<Value> {
    loop {
        match socket.read()? {
            Message::Text(text) => return Ok(serde_json::from_str(text.as_str())?),
            Message::Close(_) => bail!("remote_closed"),
            _ => {}
        }
    }
}

struct NetworkLoopContext {
    url: String,
    identity: DeviceIdentity,
    host_generation: String,
    command_seq: Arc<AtomicI64>,
    control: Arc<RemoteControl>,
    remote_generation: Arc<AtomicU64>,
    expected_generation: u64,
}

fn network_loop(
    context: NetworkLoopContext,
    events: SyncSender<NetworkEvent>,
    outgoing: Receiver<NetworkSend>,
) {
    let NetworkLoopContext {
        url,
        identity,
        host_generation,
        command_seq,
        control,
        remote_generation,
        expected_generation,
    } = context;
    let mut epoch = expected_generation << 32;
    while !control.stopped.load(Ordering::Relaxed)
        && remote_generation.load(Ordering::SeqCst) == expected_generation
    {
        epoch += 1;
        // Drop stale queued frames before announcing a new transport epoch.
        for _ in outgoing.try_iter() {}
        let result = (|| -> Result<()> {
            let (mut socket, _) = tungstenite::connect(&url)?;
            set_timeouts(&mut socket, Duration::from_secs(10))?;
            let challenge = receive(&mut socket)?;
            let nonce = challenge["nonce"]
                .as_str()
                .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| anyhow::anyhow!("invalid_remote_challenge"))?;
            if challenge["version"] != 3 || challenge["type"] != "challenge" {
                bail!("remote_version_mismatch");
            }
            let received = command_seq.load(Ordering::Relaxed);
            let signed = format!(
                "agenthub.remote.v3\n{}\n{}\n{}\n{}",
                identity.device_id(),
                nonce,
                host_generation,
                received
            );
            send(
                &mut socket,
                json!({"type":"hello","runner_id":identity.device_id(),"host_generation":host_generation,
                "received_command_seq":received,"signature":hex::encode(identity.sign(signed.as_bytes()).to_bytes())}),
            )?;
            let ack = receive(&mut socket)?;
            if ack["type"] != "hello_ack" || ack["version"] != 3 {
                eprintln!(
                    "[Runner Handshake] Server returned non-hello_ack: {:?}",
                    ack
                );
                bail!("remote_handshake_rejected");
            }
            events.send(NetworkEvent::Connected(
                epoch,
                ack["received_seq"]
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("invalid_remote_ack"))?,
            ))?;
            set_timeouts(&mut socket, Duration::from_millis(50))?;
            let mut heartbeat = Instant::now();
            while remote_epoch_is_active(&control, &remote_generation, expected_generation, epoch) {
                for value in outgoing.try_iter().take(32) {
                    match value {
                        NetworkSend::Data(e, seq, mut payload) if e == epoch => {
                            crate::privacy::sanitize_workspace_paths_in_json(&mut payload);
                            send(
                                &mut socket,
                                json!({"type":"data","seq":seq,"payload":payload}),
                            )?;
                            events.send(NetworkEvent::Sent(epoch, seq))?;
                        }
                        NetworkSend::Ack(e, seq) if e == epoch => {
                            send(&mut socket, json!({"type":"command_ack","seq":seq}))?
                        }
                        _ => {}
                    }
                }
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        events.send(NetworkEvent::Frame(
                            epoch,
                            serde_json::from_str(text.as_str())?,
                        ))?;
                    }
                    Ok(Message::Close(_)) => break,
                    Err(tungstenite::Error::Io(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(error) => return Err(error.into()),
                    _ => {}
                }
                if heartbeat.elapsed() >= Duration::from_secs(10) {
                    send(
                        &mut socket,
                        json!({"type":"command_ack","seq":command_seq.load(Ordering::Relaxed)}),
                    )?;
                    send(&mut socket, json!({"type":"heartbeat"}))?;
                    heartbeat = Instant::now();
                }
            }
            let _ = socket.close(None);
            Ok(())
        })();
        if remote_generation.load(Ordering::SeqCst) != expected_generation {
            return;
        }
        control.connected.store(false, Ordering::Relaxed);
        if events.send(NetworkEvent::Offline(epoch)).is_err() {
            break;
        }
        if result.is_err() {
            eprintln!("{{\"code\":\"remote_transport_retry\"}}");
        }
        for _ in 0..50 {
            if control.stopped.load(Ordering::Relaxed)
                || remote_generation.load(Ordering::SeqCst) != expected_generation
            {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeTransport;

    /// A transport whose `thread/list` answer the test controls, so a
    /// discovery failure and a later success can both be reproduced.
    struct ListTransport {
        pages: std::sync::Mutex<Vec<Result<Value, crate::remote_host::NativeError>>>,
    }

    impl ListTransport {
        fn new(pages: Vec<Result<Value, crate::remote_host::NativeError>>) -> Self {
            Self {
                pages: std::sync::Mutex::new(pages),
            }
        }
    }

    impl NativeTransport for ListTransport {
        fn request(
            &mut self,
            method: &str,
            _params: Value,
        ) -> Result<Value, crate::remote_host::NativeError> {
            if method != "thread/list" {
                return Ok(json!({"data": []}));
            }
            let mut pages = self.pages.lock().unwrap();
            if pages.is_empty() {
                return Err(crate::remote_host::NativeError {
                    code: "no_more_pages".to_owned(),
                    outcome_unknown: false,
                });
            }
            pages.remove(0)
        }

        fn respond(
            &mut self,
            _id: Value,
            _result: Value,
        ) -> Result<(), crate::remote_host::NativeError> {
            Ok(())
        }

        fn drain(&mut self) -> Result<Vec<Value>, crate::remote_host::NativeError> {
            Ok(Vec::new())
        }
    }

    fn thread_page(id: &str, cwd: &std::path::Path) -> Value {
        json!({"data": [{
            "id": id,
            "cwd": cwd,
            "name": format!("Session {id}"),
            "status": {"type": "idle"},
            "total_tokens": 10
        }]})
    }

    impl NativeTransport for FakeTransport {
        fn request(
            &mut self,
            _method: &str,
            _params: Value,
        ) -> Result<Value, crate::remote_host::NativeError> {
            unreachable!("protocol isolation test must not call the provider")
        }

        fn respond(
            &mut self,
            _id: Value,
            _result: Value,
        ) -> Result<(), crate::remote_host::NativeError> {
            unreachable!("protocol isolation test must not call the provider")
        }

        fn drain(&mut self) -> Result<Vec<Value>, crate::remote_host::NativeError> {
            Ok(Vec::new())
        }
    }

    fn remote_test_host() -> (
        tempfile::TempDir,
        DurableHost<FakeTransport>,
        RemoteControl,
        SyncSender<NetworkSend>,
        Receiver<NetworkSend>,
        AtomicI64,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let host = NativeSessionHost::new(FakeTransport, vec![dir.path().into()]).unwrap();
        let journal = RemoteJournal::open(dir.path().join("remote.db")).unwrap();
        let durable = DurableHost::new(host, journal).unwrap();
        let (send_tx, send_rx) = mpsc::sync_channel(8);
        (
            dir,
            durable,
            RemoteControl::default(),
            send_tx,
            send_rx,
            AtomicI64::new(0),
        )
    }

    fn connect_remote<T: NativeTransport>(
        durable: &mut DurableHost<T>,
        control: &RemoteControl,
        active: &mut Option<u64>,
        offered: &mut i64,
        send_tx: &SyncSender<NetworkSend>,
        command_seq: &AtomicI64,
        epoch: u64,
    ) {
        handle_network_event(
            NetworkEvent::Connected(epoch, 0),
            epoch >> 32,
            durable,
            control,
            active,
            offered,
            send_tx,
            command_seq,
        )
        .unwrap();
        assert_eq!(*active, Some(epoch));
        assert!(control.connected.load(Ordering::Relaxed));
    }

    #[test]
    fn remote_protocol_faults_fence_only_the_transport_epoch() {
        let generation = 7_u64;
        let epoch = generation << 32 | 1;
        for frame in [
            json!({"type":"ack","seq":1}),
            json!({"type":"command","seq":2,"command":{}}),
            json!({"type":"unknown"}),
        ] {
            let (_dir, mut durable, control, send_tx, _send_rx, command_seq) = remote_test_host();
            let host_generation = durable.host.generation().to_owned();
            let mut active = None;
            let mut offered = 0;
            connect_remote(
                &mut durable,
                &control,
                &mut active,
                &mut offered,
                &send_tx,
                &command_seq,
                epoch,
            );

            handle_network_event(
                NetworkEvent::Frame(epoch, frame),
                generation,
                &mut durable,
                &control,
                &mut active,
                &mut offered,
                &send_tx,
                &command_seq,
            )
            .unwrap();

            assert_eq!(active, None);
            assert!(!control.connected.load(Ordering::Relaxed));
            assert!(!control.stopped.load(Ordering::Relaxed));
            assert!(durable.host.is_alive());
            assert_eq!(durable.host.generation(), host_generation);
            assert!(!remote_epoch_is_active(
                &control,
                &AtomicU64::new(generation),
                generation,
                epoch,
            ));
        }
    }

    #[test]
    fn handshake_watermark_mismatch_does_not_stop_the_local_host() {
        let generation = 3_u64;
        let epoch = generation << 32 | 1;
        let (_dir, mut durable, control, send_tx, _send_rx, command_seq) = remote_test_host();
        let host_generation = durable.host.generation().to_owned();
        let mut active = None;
        let mut offered = 0;

        handle_network_event(
            NetworkEvent::Connected(epoch, 1),
            generation,
            &mut durable,
            &control,
            &mut active,
            &mut offered,
            &send_tx,
            &command_seq,
        )
        .unwrap();

        assert_eq!(active, None);
        assert!(!control.stopped.load(Ordering::Relaxed));
        assert!(durable.host.is_alive());
        assert_eq!(durable.host.generation(), host_generation);
        assert_eq!(control.rejected_epoch.load(Ordering::SeqCst), epoch);
    }

    #[test]
    fn embedded_shutdown_prevents_startup_and_clears_connectivity_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("remote.db");
        let stopped = Arc::new(RemoteControl::default());
        stopped.stopped.store(true, Ordering::Relaxed);
        stopped.connected.store(true, Ordering::Relaxed);
        assert!(
            run_with_control(
                PathBuf::from("missing-native"),
                vec![],
                database.clone(),
                DeviceIdentity::generate(),
                "invalid".into(),
                stopped.clone()
            )
            .is_ok()
        );
        assert!(!database.exists());
        assert!(!stopped.connected.load(Ordering::Relaxed));
        let failed = Arc::new(RemoteControl::default());
        assert!(
            run_with_control(
                PathBuf::from("missing-native"),
                vec![],
                database,
                DeviceIdentity::generate(),
                "invalid".into(),
                failed.clone()
            )
            .is_err()
        );
        assert!(failed.stopped.load(Ordering::Relaxed));
        assert!(!failed.connected.load(Ordering::Relaxed));
    }
    #[test]
    fn transport_requires_tls_except_loopback_and_rejects_url_credentials() {
        assert!(validate_server("ws://127.0.0.1:3001/v3/remote/ws").is_ok());
        assert!(validate_server("http://127.0.0.1:3001").is_ok());
        assert!(validate_server("https://example.com").is_ok());
        assert!(validate_server("ws://api:3001/v3/remote/ws").is_ok());
        assert!(validate_server("ws://100.100.100.100:3001/v3/remote/ws").is_ok());
        assert!(validate_server("wss://example.com/v3/remote/ws").is_ok());
        for url in [
            "ws://example.com/v3/remote/ws",
            "wss://user:secret@example.com/v3/remote/ws",
            "wss://example.com/v3/remote/ws?token=x",
        ] {
            assert!(validate_server(url).is_err());
        }
    }

    #[test]
    fn local_channel_roundtrips_events_and_cleans_up_dropped_subscriptions() {
        let (client, channel) = create_local_host_channel();
        assert_eq!(channel.listeners.lock().unwrap().len(), 0);
        let (sub1, rx1) = client.subscribe(16);
        let (sub2, rx2) = client.subscribe(16);
        assert_eq!(channel.listeners.lock().unwrap().len(), 2);

        let event = HostEvent {
            event_id: "evt-test".into(),
            host_generation: "gen-test".into(),
            binding: None,
            hub_session_id: Some("hub-1".into()),
            provider_session_id: Some("thread-1".into()),
            kind: "item/agentMessage/delta".into(),
            raw: json!({"params": {"delta": "hello"}}),
            approval_id: None,
            approval: None,
            question_id: None,
            question: None,
        };

        {
            let listeners = channel.listeners.lock().unwrap();
            for listener in listeners.values() {
                listener.try_send(event.clone()).unwrap();
            }
        }

        assert_eq!(rx1.recv().unwrap().event_id, "evt-test");
        assert_eq!(rx2.recv().unwrap().event_id, "evt-test");

        drop(sub1);
        assert_eq!(channel.listeners.lock().unwrap().len(), 1);
        drop(sub2);
        assert_eq!(channel.listeners.lock().unwrap().len(), 0);
    }

    #[test]
    fn remote_ready_is_current_sanitized_and_remote_only() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().canonicalize().unwrap();
        let host = NativeSessionHost::new(FakeTransport, vec![workspace.clone()]).unwrap();
        let journal = RemoteJournal::open(dir.path().join("ready.db")).unwrap();
        let mut durable = DurableHost::new(host, journal).unwrap();
        durable.host.restore_detached(vec![SessionBinding {
            hub_session_id: "hub-current".into(),
            provider_session_id: "native-current".into(),
            workspace_path: workspace.clone(),
            host_generation: "generation-previous".into(),
            control_connected: false,
            execution_state: crate::remote_host::ExecutionState::Unknown,
            current_turn_id: None,
        }]);
        let models = json!({"data":[{"id":"model-current","displayName":"Current"}]});
        let discovery = sanitize_remote_discovery(Some(json!({
            "candidates":[{
                "provider_session_id":"native-discovered",
                "workspace_path":workspace,
                "title":"Current session"
            }]
        })));

        let payload = remote_ready_payload(&durable, Some(&models), discovery.as_ref(), "书房主机");
        let physical_path = workspace.to_string_lossy();

        assert_eq!(payload["type"], "ready");
        assert_eq!(payload["device_name"], "书房主机");
        assert_eq!(payload["sessions"][0]["hub_session_id"], "hub-current");
        assert_eq!(payload["models"]["data"][0]["id"], "model-current");
        assert_ne!(payload["workspaces"][0], physical_path.as_ref());
        assert_ne!(
            payload["sessions"][0]["workspace_path"],
            physical_path.as_ref()
        );
        assert_ne!(
            payload["discovery"]["candidates"][0]["workspace_path"],
            physical_path.as_ref()
        );
        assert_eq!(durable.journal.enqueue_remote(&payload).unwrap(), 1);
        assert_eq!(durable.journal.snapshot().unwrap().cursor, 0);
        assert_eq!(durable.journal.pending(0, 10).unwrap(), vec![(1, payload)]);
    }

    /// A failed discovery must not blank the sessions the console already
    /// shows, and a later successful discovery must publish its sessions.
    ///
    /// The `ready` frame is the only channel that carries discovered sessions,
    /// so a startup discovery that failed left the web console with an empty
    /// tree for the life of the process.
    #[test]
    fn a_failed_discovery_keeps_the_published_sessions_and_a_later_one_republishes() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().canonicalize().unwrap();
        let transport = ListTransport::new(vec![
            Ok(thread_page("native-one", &workspace)),
            Err(crate::remote_host::NativeError {
                code: "provider_transport_lost".to_owned(),
                outcome_unknown: true,
            }),
            Ok(thread_page("native-two", &workspace)),
        ]);
        let host = NativeSessionHost::new(transport, vec![workspace.clone()]).unwrap();
        let journal = RemoteJournal::open(dir.path().join("discovery.db")).unwrap();
        let mut durable = DurableHost::new(host, journal).unwrap();

        // The first discovery succeeds and is what the console would show.
        let first = sanitize_remote_discovery(durable.host.discover().ok());
        assert_eq!(
            first
                .as_ref()
                .and_then(|value| value["candidates"].as_array())
                .map(|items| items.len()),
            Some(1),
            "the first page is discovered"
        );

        // The next discovery fails, as it did when the provider was too slow
        // for the Runner's response budget.
        let failed = durable.host.discover();
        assert!(
            failed.is_err(),
            "a failed discovery is reported as a failure, not as an empty history"
        );

        // A later discovery succeeds again and finds a different session.
        let third = sanitize_remote_discovery(durable.host.discover().ok());
        let ids = third
            .as_ref()
            .and_then(|value| value["candidates"].as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item["provider_session_id"].as_str())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert_eq!(ids, vec!["native-two".to_owned()]);
    }

    #[test]
    fn local_execute_waits_for_the_queued_receipt() {
        let (client, channel) = create_local_host_channel();
        let command = RemoteCommand {
            client_cmd_id: "command-1".into(),
            hub_session_id: "hub-1".into(),
            host_generation: Some("generation-1".into()),
            command: crate::remote_host::RemoteAction::Interrupt,
        };
        let expected = CommandReceipt {
            client_cmd_id: command.client_cmd_id.clone(),
            hub_session_id: command.hub_session_id.clone(),
            status: "accepted".into(),
            code: None,
            result: None,
        };
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            let LocalHostCommand::Execute { respond_to, .. } = channel.receiver.recv().unwrap()
            else {
                panic!("expected queued execute command");
            };
            respond_to.send(Ok(expected)).unwrap();
        });

        let receipt = client.execute(command).unwrap();

        assert_eq!(receipt.client_cmd_id, "command-1");
        assert_eq!(receipt.status, "accepted");
        worker.join().unwrap();
    }

    #[test]
    fn local_client_times_out_when_the_host_queue_is_saturated() {
        let (client, _channel) = create_local_host_channel();
        for _ in 0..128 {
            let (respond_to, _response) = mpsc::sync_channel(1);
            client
                .sender
                .try_send(LocalHostCommand::QuerySessions { respond_to })
                .unwrap();
        }

        let started = Instant::now();
        let error = client.query_sessions().unwrap_err();

        assert_eq!(error, "host_queue_timeout");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn local_client_handles_dropped_channel_without_panic() {
        let (client, channel) = create_local_host_channel();
        drop(channel);
        assert!(client.query_sessions().is_err());
        assert!(client.query_generation().is_err());
        assert!(client.query_models().is_err());
        assert!(client.discover().is_err());
        assert!(client.query_workspaces().is_err());
        assert!(client.authorize_workspace(PathBuf::from(".")).is_err());
    }

    #[test]
    fn offline_host_enables_remote_and_preserves_registry_across_restart() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("host.sqlite");
        let dev = "device-alpha";
        let session = SessionBinding {
            hub_session_id: "hub-1".into(),
            provider_session_id: "native-1".into(),
            workspace_path: PathBuf::from("D:/test"),
            host_generation: "gen-1".into(),
            control_connected: false,
            execution_state: crate::remote_host::ExecutionState::Idle,
            current_turn_id: None,
        };
        // 1. Initial offline cold-start: bind identity with local://agenthub
        {
            let mut journal = RemoteJournal::open(&db_path).unwrap();
            journal.bind_identity(dev, "local://agenthub").unwrap();
            journal
                .append(&json!({"type":"ready"}), std::slice::from_ref(&session))
                .unwrap();
            assert_eq!(journal.bindings().unwrap().len(), 1);
        }
        // 2. Enable remote dynamically: rebind server to Gateway
        {
            let mut journal = RemoteJournal::open(&db_path).unwrap();
            journal
                .rebind_server_if_local("wss://example.com/v3/remote/ws")
                .unwrap();
            assert_eq!(journal.bindings().unwrap().len(), 1);
        }
        // 3. Simulate restart: opening in offline mode MUST NOT fail with IdentityMismatch
        {
            let mut journal = RemoteJournal::open(&db_path).unwrap();
            journal.bind_identity(dev, "local://agenthub").unwrap();
            assert_eq!(journal.bindings().unwrap().len(), 1);
        }
        // 4. Simulate opening with the Gateway URL: succeeds and preserves registry
        {
            let mut journal = RemoteJournal::open(&db_path).unwrap();
            journal
                .bind_identity(dev, "wss://example.com/v3/remote/ws")
                .unwrap();
            assert_eq!(journal.bindings().unwrap().len(), 1);
        }
        // 5. Tampered device identity on same database MUST be rejected
        {
            let mut journal = RemoteJournal::open(&db_path).unwrap();
            assert!(
                journal
                    .bind_identity("device-other", "local://agenthub")
                    .is_err()
            );
        }
    }
}
