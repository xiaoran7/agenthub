use super::*;

pub(crate) enum NetworkEvent {
    /// Transport epoch, the Gateway's received-event watermark, and the highest
    /// command its journal still holds (`None` when the Gateway does not say).
    Connected(u64, i64, Option<i64>),
    Sent(u64, i64),
    ResetInboundCommandSeq(u64),
    Frame(u64, Value),
    Offline(u64),
}
pub(crate) enum NetworkSend {
    Data(u64, i64, Value),
    Ack(u64, i64),
    Control(u64, Value),
}

pub(crate) struct NetworkLoopContext {
    pub(crate) url: String,
    pub(crate) identity: DeviceIdentity,
    pub(crate) host_generation: String,
    pub(crate) command_seq: Arc<AtomicI64>,
    pub(crate) control: Arc<RemoteControl>,
    pub(crate) remote_generation: Arc<AtomicU64>,
    pub(crate) expected_generation: u64,
}

pub(crate) fn network_loop(
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
    let mut attempt: u32 = 0;
    while !control.stopped.load(Ordering::Acquire)
        && remote_generation.load(Ordering::SeqCst) == expected_generation
    {
        epoch += 1;
        // Drop stale queued frames before announcing a new transport epoch.
        for _ in outgoing.try_iter() {}
        let result = (|| -> Result<Duration> {
            let mut socket = connect_with_timeout(&url, Duration::from_secs(10))?;
            let handshake_deadline = Instant::now() + Duration::from_secs(10);
            let challenge =
                receive_with_deadline(&mut socket, handshake_deadline, &control.stopped)?;
            let nonce = challenge["nonce"]
                .as_str()
                .filter(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| anyhow::anyhow!("invalid_remote_challenge"))?;
            if challenge["version"] != 3 || challenge["type"] != "challenge" {
                bail!("remote_version_mismatch");
            }
            let received = command_seq.load(Ordering::Acquire);
            let signed = format!(
                "agenthub.remote.v3\n{}\n{}\n{}\n{}",
                identity.device_id(),
                nonce,
                host_generation,
                received
            );
            send(
                &mut socket,
                json!({
                    "type": "hello",
                    "runner_id": identity.device_id(),
                    "host_generation": host_generation,
                    "received_command_seq": received,
                    "signature": hex::encode(identity.sign(signed.as_bytes()).to_bytes()),
                    "capabilities": ["plugins", "plugins_v2", "codex_session_control"],
                }),
            )?;
            let ack = receive_with_deadline(&mut socket, handshake_deadline, &control.stopped)?;
            if ack["type"] != "hello_ack" || ack["version"] != 3 {
                eprintln!(
                    "[Runner Handshake] Server returned non-hello_ack: {:?}",
                    ack
                );
                if ack["type"] == "error" && ack["code"] == "command_journal_mismatch" {
                    eprintln!(
                        "[Runner Handshake] Server reported command_journal_mismatch, resetting received_command_seq to 0"
                    );
                    command_seq.store(0, Ordering::Release);
                    let _ = events.send(NetworkEvent::ResetInboundCommandSeq(epoch));
                }
                bail!("remote_handshake_rejected");
            }
            // `command_seq` is the Gateway's own high-water mark. An older
            // Gateway does not send it, and then the local watermark stands.
            let received_seq = ack["received_seq"]
                .as_i64()
                .filter(|&seq| seq >= 0)
                .ok_or_else(|| anyhow::anyhow!("invalid_remote_ack"))?;
            let server_command_seq = match ack.get("command_seq") {
                None | Some(Value::Null) => None,
                Some(value) => Some(
                    value
                        .as_i64()
                        .filter(|&seq| seq >= 0)
                        .ok_or_else(|| anyhow::anyhow!("invalid_remote_command_seq"))?,
                ),
            };
            if let Some(server_command_seq) = server_command_seq
                && command_seq.load(Ordering::Acquire) > server_command_seq
            {
                eprintln!(
                    "[Runner Handshake] Gateway command journal is shorter than the applied watermark ({} > {server_command_seq}); rewinding inbound sequence",
                    command_seq.load(Ordering::Acquire)
                );
            }
            events.send(NetworkEvent::Connected(
                epoch,
                received_seq,
                server_command_seq,
            ))?;
            const INBOUND_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(35);
            let mut read_timeout = AdaptiveReadTimeout::new(Instant::now());
            set_timeouts(&mut socket, read_timeout.current())?;
            let connected_at = Instant::now();
            let mut heartbeat = Instant::now();
            let mut last_inbound_at = Instant::now();
            while remote_epoch_is_active(&control, &remote_generation, expected_generation, epoch) {
                let outbound_batch = drain_prioritized(&outgoing, epoch, OUTBOUND_DRAIN_LIMIT);
                let mut had_io = !outbound_batch.is_empty();
                for value in outbound_batch {
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
                        NetworkSend::Control(e, mut payload) if e == epoch => {
                            crate::privacy::sanitize_workspace_paths_in_json(&mut payload);
                            send(&mut socket, payload)?;
                        }
                        _ => {}
                    }
                }
                match socket.read() {
                    Ok(Message::Text(text)) => {
                        had_io = true;
                        last_inbound_at = Instant::now();
                        if let Ok(frame) = serde_json::from_str(text.as_str())
                            && events.send(NetworkEvent::Frame(epoch, frame)).is_err()
                        {
                            // The consumer is gone, so there is nothing left
                            // to deliver frames to.
                            break;
                        }
                    }
                    Ok(Message::Pong(_)) => {
                        last_inbound_at = Instant::now();
                    }
                    // tungstenite queues the Pong reply itself; answering here
                    // would replace that reply with a second one.
                    Ok(Message::Ping(_)) => {}
                    Ok(Message::Close(_)) => break,
                    Err(tungstenite::Error::Io(error))
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(error) => return Err(error.into()),
                    _ => {}
                }
                let now = Instant::now();
                let next_timeout = if had_io {
                    read_timeout.note_activity(now)
                } else {
                    read_timeout.maybe_relax(now)
                };
                if let Some(duration) = next_timeout {
                    set_timeouts(&mut socket, duration)?;
                }
                if last_inbound_at.elapsed() > INBOUND_HEARTBEAT_TIMEOUT {
                    eprintln!("{{\"code\":\"remote_heartbeat_timeout\"}}");
                    bail!("remote_heartbeat_timeout");
                }
                if heartbeat.elapsed() >= Duration::from_secs(10) {
                    send(
                        &mut socket,
                        json!({"type":"command_ack","seq":command_seq.load(Ordering::Acquire)}),
                    )?;
                    send(&mut socket, json!({"type":"heartbeat"}))?;
                    heartbeat = Instant::now();
                }
            }
            let _ = socket.close(None);
            Ok(connected_at.elapsed())
        })();
        if remote_generation.load(Ordering::SeqCst) != expected_generation {
            return;
        }
        control.connected.store(false, Ordering::Release);
        if events.send(NetworkEvent::Offline(epoch)).is_err() {
            break;
        }
        let stayed_healthy = matches!(result, Ok(elapsed) if elapsed >= Duration::from_secs(10));
        if stayed_healthy {
            attempt = 0;
        } else {
            attempt = attempt.saturating_add(1);
        }
        if result.is_err() {
            eprintln!("{{\"code\":\"remote_transport_retry\",\"attempt\":{attempt}}}");
        }
        let delay = reconnect_delay(attempt);
        let deadline = Instant::now() + delay;
        while Instant::now() < deadline {
            if control.stopped.load(Ordering::Acquire)
                || remote_generation.load(Ordering::SeqCst) != expected_generation
            {
                return;
            }
            let step = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50));
            if step.is_zero() {
                break;
            }
            thread::sleep(step);
        }
    }
}

fn reconnect_delay(attempt: u32) -> Duration {
    const STEPS_MS: &[u64] = &[250, 500, 1000, 2000, 5000];
    let idx = attempt.saturating_sub(1) as usize;
    let base_ms = STEPS_MS.get(idx).copied().unwrap_or(5000);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos()))
        .unwrap_or(0);
    let jitter_ms = nanos % (base_ms / 4 + 1);
    Duration::from_millis(base_ms + jitter_ms)
}

/// Collapses adjacent monotonic `Sent` or valid `ack` frames on the same epoch so
/// a streaming burst commits one watermark/delete transaction per tick instead of N.
pub(crate) fn coalesce_network_events(events: Vec<NetworkEvent>) -> Vec<NetworkEvent> {
    let mut out: Vec<NetworkEvent> = Vec::with_capacity(events.len());
    for event in events {
        if let Some(last) = out.last_mut() {
            match (&*last, &event) {
                (NetworkEvent::Sent(e1, s1), NetworkEvent::Sent(e2, s2))
                    if e1 == e2 && s2 >= s1 =>
                {
                    *last = event;
                    continue;
                }
                (NetworkEvent::Frame(e1, f1), NetworkEvent::Frame(e2, f2))
                    if e1 == e2
                        && f1.get("type").and_then(Value::as_str) == Some("ack")
                        && f2.get("type").and_then(Value::as_str) == Some("ack") =>
                {
                    if let (Some(s1), Some(s2)) = (
                        f1.get("seq").and_then(Value::as_i64),
                        f2.get("seq").and_then(Value::as_i64),
                    ) && s1 >= 0
                        && s2 >= s1
                    {
                        *last = event;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(event);
    }
    out
}
