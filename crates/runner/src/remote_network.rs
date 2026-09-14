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

fn remote_ready_payload<T: NativeTransport>(
    durable: &DurableHost<T>,
    models: Option<&Value>,
    discovery: Option<&Value>,
) -> Value {
    let mut workspaces = durable
        .host
        .workspaces()
        .iter()
        .map(|path| crate::privacy::workspace_id_for_path(&path.to_string_lossy()))
        .collect::<Vec<_>>();
    for (key, _) in durable.host.discovered_workspaces() {
        if key.starts_with("ws-") && !workspaces.contains(key) {
            workspaces.push(key.clone());
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
                "host_generation": session.host_generation,
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
    let native = CodexAppServer::start_interactive(binary, Duration::from_secs(20))
        .map_err(|_| anyhow::anyhow!("native_host_start_failed"))?;
    let host = NativeSessionHost::new(native, workspaces.clone())
        .map_err(|error| anyhow::anyhow!(error.code))?;
    let mut durable = DurableHost::new(host, journal)?;
    let mut cached_models = durable.host.models().ok();
    let mut cached_discovery = sanitize_remote_discovery(durable.host.discover().ok());
    if url.is_some() {
        let ready_payload =
            remote_ready_payload(&durable, cached_models.as_ref(), cached_discovery.as_ref());
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
                            let ready_payload = remote_ready_payload(
                                &durable,
                                cached_models.as_ref(),
                                cached_discovery.as_ref(),
                            );
                            if let Err(error) = durable.journal.enqueue_remote(&ready_payload) {
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
                            let ready_payload = remote_ready_payload(
                                &durable,
                                cached_models.as_ref(),
                                cached_discovery.as_ref(),
                            );
                            if let Err(error) = durable.journal.enqueue_remote(&ready_payload) {
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
                            cached_discovery = sanitize_remote_discovery(Some(discovery.clone()));
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
            if server_seq < acked || server_seq > sent {
                reject_remote_epoch(control, active, epoch);
                return Ok(());
            }
            match durable.journal.acknowledge(server_seq) {
                Ok(()) => {}
                Err(crate::remote_journal::JournalError::InvalidAck) => {
                    reject_remote_epoch(control, active, epoch);
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
                        reject_remote_epoch(control, active, epoch);
                        return Ok(());
                    };
                    match durable.journal.acknowledge(seq) {
                        Ok(()) => {}
                        Err(crate::remote_journal::JournalError::InvalidAck) => {
                            reject_remote_epoch(control, active, epoch);
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                Some("command") => {
                    let Some(seq) = frame["seq"].as_i64() else {
                        reject_remote_epoch(control, active, epoch);
                        return Ok(());
                    };
                    let Ok(command) =
                        serde_json::from_value::<RemoteCommand>(frame["command"].clone())
                    else {
                        reject_remote_epoch(control, active, epoch);
                        return Ok(());
                    };
                    match durable.handle_inbound_command(seq, command) {
                        Ok(_) => {
                            command_seq
                                .store(durable.journal.received_command_seq()?, Ordering::Relaxed);
                        }
                        Err(crate::remote_journal::JournalError::InvalidAck) => {
                            reject_remote_epoch(control, active, epoch);
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
                Some("error") => reject_remote_epoch(control, active, epoch),
                _ => reject_remote_epoch(control, active, epoch),
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

fn reject_remote_epoch(control: &RemoteControl, active: &mut Option<u64>, epoch: u64) {
    if *active == Some(epoch) {
        *active = None;
    }
    control.connected.store(false, Ordering::Relaxed);
    control.rejected_epoch.store(epoch, Ordering::SeqCst);
    eprintln!("{{\"code\":\"remote_protocol_rejected\"}}");
}

fn validate_server(server: &str) -> Result<String> {
    let url = url::Url::parse(server)?;
    if !matches!(url.scheme(), "ws" | "wss")
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("invalid_remote_url");
    }
    if url.scheme() == "ws" && !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
    {
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

        let payload = remote_ready_payload(&durable, Some(&models), discovery.as_ref());
        let physical_path = workspace.to_string_lossy();

        assert_eq!(payload["type"], "ready");
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
