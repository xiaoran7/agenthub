//! Single outbound v3 connection. Network retries never own or stop the native Host.
use crate::{
    collectors::CodexAppServer,
    device::DeviceIdentity,
    remote_host::{CommandReceipt, HostEvent, NativeSessionHost, RemoteCommand, SessionBinding},
    remote_journal::{DurableHost, RemoteJournal},
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

pub enum LocalHostCommand {
    Execute {
        command: RemoteCommand,
        respond_to: SyncSender<Result<CommandReceipt, String>>,
    },
    QuerySessions {
        respond_to: SyncSender<Vec<SessionBinding>>,
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
    EnableRemote {
        server_url: String,
        identity: DeviceIdentity,
        respond_to: SyncSender<Result<(), String>>,
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
        self.sender
            .send(LocalHostCommand::Execute {
                command,
                respond_to: tx,
            })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(30))
            .map_err(|_| "command_timeout".to_string())?
    }

    pub fn query_sessions(&self) -> Result<Vec<SessionBinding>, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .send(LocalHostCommand::QuerySessions { respond_to: tx })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn query_workspaces(&self) -> Result<Vec<PathBuf>, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .send(LocalHostCommand::QueryWorkspaces { respond_to: tx })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn authorize_workspace(&self, workspace: PathBuf) -> Result<PathBuf, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .send(LocalHostCommand::AuthorizeWorkspace {
                workspace,
                respond_to: tx,
            })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "authorize_timeout".to_string())?
    }

    pub fn query_generation(&self) -> Result<String, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .send(LocalHostCommand::QueryGeneration { respond_to: tx })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn query_models(&self) -> Result<Value, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .send(LocalHostCommand::QueryModels { respond_to: tx })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn discover(&self) -> Result<Value, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .send(LocalHostCommand::Discover { respond_to: tx })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn enable_remote(
        &self,
        server_url: String,
        identity: DeviceIdentity,
    ) -> Result<(), String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .send(LocalHostCommand::EnableRemote {
                server_url,
                identity,
                respond_to: tx,
            })
            .map_err(|_| "host_not_running".to_string())?;
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| "enable_remote_timeout".to_string())?
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
    let models = durable.host.models().ok();
    let discovery = durable.host.discover().ok().map(|mut d| {
        if let Some(candidates) = d.get_mut("candidates").and_then(Value::as_array_mut) {
            for c in candidates {
                if let Some(path_str) = c.get("workspace_path").and_then(Value::as_str) {
                    let sanitized = crate::privacy::workspace_id_for_path(path_str);
                    c["workspace_path"] = json!(sanitized);
                }
            }
        }
        d
    });
    let sanitized_workspaces: Vec<String> = workspaces
        .iter()
        .map(|p| crate::privacy::workspace_id_for_path(&p.to_string_lossy()))
        .collect();
    let sanitized_sessions: Vec<Value> = durable
        .host
        .sessions()
        .into_iter()
        .map(|s| {
            let sanitized_ws =
                crate::privacy::workspace_id_for_path(&s.workspace_path.to_string_lossy());
            json!({
                "hub_session_id": s.hub_session_id,
                "provider_session_id": s.provider_session_id,
                "workspace_path": sanitized_ws,
                "host_generation": s.host_generation,
                "control_connected": s.control_connected,
                "execution_state": s.execution_state,
                "current_turn_id": s.current_turn_id,
            })
        })
        .collect();
    let mut ready_payload = json!({"type":"ready","provider":"codex_app_server","host_generation":durable.host.generation(),
        "workspaces":sanitized_workspaces,"sessions":sanitized_sessions,"models":models,"discovery":discovery,"attach_running":false});
    crate::privacy::sanitize_workspace_paths_in_json(&mut ready_payload);
    durable.journal.append(&ready_payload, &[])?;
    control.ready.store(true, Ordering::Relaxed);
    let command_seq = Arc::new(AtomicI64::new(durable.journal.received_command_seq()?));
    let (events_tx, events) = mpsc::sync_channel(256);
    let (mut send_tx, send_rx) = mpsc::sync_channel(64);
    let generation = durable.host.generation().to_owned();
    let worker_stop = control.clone();
    let worker_seq = command_seq.clone();
    let mut current_server = url.clone();
    if let Some(target_url) = url {
        let events_tx_clone = events_tx.clone();
        thread::spawn(move || {
            network_loop(
                target_url,
                identity,
                generation,
                worker_seq,
                worker_stop,
                events_tx_clone,
                send_rx,
            )
        });
    }
    let mut active = None;
    let mut offered = 0;
    while !control.stopped.load(Ordering::Relaxed) {
        let polled_events = match durable.poll() {
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
                            .handle_command(command)
                            .map(|(receipt, _)| receipt)
                            .map_err(|err| err.to_string());
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::QuerySessions { respond_to }) => {
                        let _ = respond_to.send(durable.host.sessions());
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
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::QueryGeneration { respond_to }) => {
                        let _ = respond_to.send(durable.host.generation().to_string());
                    }
                    Ok(LocalHostCommand::QueryModels { respond_to }) => {
                        let _ = respond_to.send(durable.host.models().map_err(|e| e.code));
                    }
                    Ok(LocalHostCommand::Discover { respond_to }) => {
                        let _ = respond_to.send(durable.host.discover().map_err(|e| e.code));
                    }
                    Ok(LocalHostCommand::EnableRemote {
                        server_url,
                        identity: remote_identity,
                        respond_to,
                    }) => {
                        let res = (|| -> Result<()> {
                            let validated = validate_server(&server_url)?;
                            if current_server.as_ref() == Some(&validated) {
                                return Ok(());
                            }
                            durable.journal.rebind_server_if_local(&validated)?;
                            current_server = Some(validated.clone());
                            let (new_send_tx, new_send_rx) = mpsc::sync_channel(64);
                            send_tx = new_send_tx;
                            let events_tx_clone = events_tx.clone();
                            let worker_stop = control.clone();
                            let worker_seq = command_seq.clone();
                            let host_gen = durable.host.generation().to_owned();
                            thread::spawn(move || {
                                network_loop(
                                    validated,
                                    remote_identity,
                                    host_gen,
                                    worker_seq,
                                    worker_stop,
                                    events_tx_clone,
                                    new_send_rx,
                                )
                            });
                            Ok(())
                        })();
                        let _ = respond_to.send(res.map_err(|e| e.to_string()));
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        // Bound network work each tick so native stdout is drained during replay.
        for event in events.try_iter().take(128) {
            match event {
                NetworkEvent::Connected(epoch, server_seq) => {
                    let (sent, acked) = durable.journal.watermarks()?;
                    if server_seq < acked || server_seq > sent {
                        control.stopped.store(true, Ordering::Relaxed);
                        bail!("remote_journal_mismatch");
                    }
                    durable.journal.acknowledge(server_seq)?;
                    offered = server_seq;
                    active = Some(epoch);
                    control.connected.store(true, Ordering::Relaxed);
                    eprintln!("{{\"code\":\"remote_connected\"}}");
                }
                NetworkEvent::Sent(epoch, seq) if active == Some(epoch) => {
                    durable.journal.mark_sent(seq)?
                }
                NetworkEvent::Offline(epoch) if active == Some(epoch) => {
                    active = None;
                    control.connected.store(false, Ordering::Relaxed);
                    eprintln!("{{\"code\":\"remote_reconnecting\"}}");
                }
                NetworkEvent::Frame(epoch, frame) if active == Some(epoch) => {
                    match frame["type"].as_str() {
                        Some("ack") => durable.journal.acknowledge(
                            frame["seq"]
                                .as_i64()
                                .ok_or_else(|| anyhow::anyhow!("invalid_remote_ack"))?,
                        )?,
                        Some("command") => {
                            let seq = frame["seq"]
                                .as_i64()
                                .ok_or_else(|| anyhow::anyhow!("invalid_remote_command"))?;
                            let received = durable.journal.received_command_seq()?;
                            if seq > received + 1 {
                                bail!("remote_command_gap");
                            }
                            if seq > received {
                                let command: RemoteCommand =
                                    serde_json::from_value(frame["command"].clone())
                                        .map_err(|_| anyhow::anyhow!("invalid_remote_command"))?;
                                durable.handle_command(command)?;
                                durable.journal.confirm_command_seq(seq)?;
                                command_seq.store(seq, Ordering::Relaxed);
                            }
                            // If the bounded queue is full the heartbeat/reconnect handshake
                            // still reports this durable watermark; never block native polling.
                            let _ = send_tx.try_send(NetworkSend::Ack(
                                epoch,
                                durable.journal.received_command_seq()?,
                            ));
                        }
                        Some("error") => {
                            active = None;
                            control.connected.store(false, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                }
                _ => {}
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

fn network_loop(
    url: String,
    identity: DeviceIdentity,
    generation: String,
    command_seq: Arc<AtomicI64>,
    control: Arc<RemoteControl>,
    events: SyncSender<NetworkEvent>,
    outgoing: Receiver<NetworkSend>,
) {
    let mut epoch = 0;
    while !control.stopped.load(Ordering::Relaxed) {
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
                generation,
                received
            );
            send(
                &mut socket,
                json!({"type":"hello","runner_id":identity.device_id(),"host_generation":generation,
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
            while !control.stopped.load(Ordering::Relaxed) {
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
        control.connected.store(false, Ordering::Relaxed);
        if events.send(NetworkEvent::Offline(epoch)).is_err() {
            break;
        }
        if result.is_err() {
            eprintln!("{{\"code\":\"remote_transport_retry\"}}");
        }
        for _ in 0..50 {
            if control.stopped.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
