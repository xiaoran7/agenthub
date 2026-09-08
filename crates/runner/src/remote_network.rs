//! Single outbound v3 connection. Network retries never own or stop the native Host.
use crate::{
    collectors::CodexAppServer,
    device::DeviceIdentity,
    remote_host::{NativeSessionHost, RemoteCommand},
    remote_journal::{DurableHost, RemoteJournal},
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{
    net::TcpStream,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

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
}

struct StopOnExit(Arc<RemoteControl>);
impl Drop for StopOnExit {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Relaxed);
        self.0.connected.store(false, Ordering::Relaxed);
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
    let _cleanup = StopOnExit(control.clone());
    if control.stopped.load(Ordering::Relaxed) {
        return Ok(());
    }
    let url = validate_server(&server)?;
    let mut journal = RemoteJournal::open(database)?;
    journal.bind_identity(identity.device_id(), url.as_str())?;
    let native = CodexAppServer::start_interactive(binary, Duration::from_secs(20))
        .map_err(|_| anyhow::anyhow!("native_host_start_failed"))?;
    let host = NativeSessionHost::new(native, workspaces.clone())
        .map_err(|error| anyhow::anyhow!(error.code))?;
    let mut durable = DurableHost::new(host, journal)?;
    let models = durable.host.models().ok();
    let discovery = durable.host.discover().ok();
    durable.journal.append(&json!({"type":"ready","provider":"codex_app_server","host_generation":durable.host.generation(),
        "workspaces":workspaces,"sessions":durable.host.sessions(),"models":models,"discovery":discovery,"attach_running":false}),&[])?;
    let command_seq = Arc::new(AtomicI64::new(durable.journal.received_command_seq()?));
    let (events_tx, events) = mpsc::sync_channel(256);
    let (send_tx, send_rx) = mpsc::sync_channel(64);
    let generation = durable.host.generation().to_owned();
    let worker_stop = control.clone();
    let worker_seq = command_seq.clone();
    thread::spawn(move || {
        network_loop(
            url,
            identity,
            generation,
            worker_seq,
            worker_stop,
            events_tx,
            send_rx,
        )
    });
    let mut active = None;
    let mut offered = 0;
    while !control.stopped.load(Ordering::Relaxed) {
        durable.poll()?;
        if !durable.host.is_alive() {
            control.stopped.store(true, Ordering::Relaxed);
            bail!("native_host_lost");
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
                        NetworkSend::Data(e, seq, payload) if e == epoch => {
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
}
