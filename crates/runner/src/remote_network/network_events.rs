use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_network_event<T: NativeTransport>(
    event: NetworkEvent,
    current_remote_generation: u64,
    durable: &mut DurableHost<T>,
    control: &RemoteControl,
    active: &mut Option<u64>,
    offered: &mut i64,
    send_tx: &SyncSender<NetworkSend>,
    command_seq: &AtomicI64,
    cached_models: Option<&Value>,
    cached_discovery: Option<&Value>,
    device_name: &crate::device_name::DeviceNameSource,
) -> Result<()> {
    handle_network_event_inner(
        event,
        current_remote_generation,
        durable,
        control,
        active,
        offered,
        send_tx,
        command_seq,
        cached_models,
        cached_discovery,
        device_name,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_network_event_inner<T: NativeTransport>(
    event: NetworkEvent,
    current_remote_generation: u64,
    durable: &mut DurableHost<T>,
    control: &RemoteControl,
    active: &mut Option<u64>,
    offered: &mut i64,
    send_tx: &SyncSender<NetworkSend>,
    command_seq: &AtomicI64,
    cached_models: Option<&Value>,
    cached_discovery: Option<&Value>,
    device_name: &crate::device_name::DeviceNameSource,
    plugin_tx: Option<&mpsc::Sender<PluginCommandTaskResult>>,
) -> Result<()> {
    match event {
        NetworkEvent::Connected(epoch, server_seq, server_command_seq)
            if epoch >> 32 == current_remote_generation =>
        {
            if server_seq < 0 || server_command_seq.is_some_and(|seq| seq < 0) {
                reject_remote_epoch(control, active, epoch, "negative handshake seq");
                return Ok(());
            }
            let (sent, acked) = durable.journal.watermarks()?;
            if server_seq < acked {
                eprintln!(
                    "[Runner] Server sequence rewound: server_seq {server_seq} < acked {acked}. Resetting outbox stream to re-sync."
                );
                match durable.journal.reset_watermarks_and_outbox(server_seq) {
                    Ok(()) => {}
                    Err(crate::remote_journal::JournalError::InvalidAck) => {
                        reject_remote_epoch(control, active, epoch, "invalid server seq reset");
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                }
                // An older Gateway does not report its command watermark, and
                // guessing zero would discard replay protection for commands it
                // still holds; the journal is then left as it is.
                if let Some(server_command_seq) = server_command_seq {
                    match durable
                        .journal
                        .reset_inbound_command_seq(server_command_seq)
                    {
                        Ok(()) => {
                            command_seq.store(server_command_seq, Ordering::Release);
                        }
                        Err(crate::remote_journal::JournalError::InvalidAck) => {
                            reject_remote_epoch(
                                control,
                                active,
                                epoch,
                                "invalid server command seq reset",
                            );
                            return Ok(());
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                let ready_payload =
                    remote_ready_payload(durable, cached_models, cached_discovery, device_name);
                durable.journal.enqueue_remote(&ready_payload)?;
            } else {
                // The Gateway's journal can be shorter than this Host's applied
                // watermark even when the event stream is intact: the command
                // journal and the event log are separate tables, and only the
                // former is truncated when stale commands are superseded. Pull
                // the inbound watermark back so replayed commands at those
                // sequences are applied instead of rejected as already-seen.
                if let Some(server_command_seq) = server_command_seq
                    && let Ok(applied) = durable.journal.received_command_seq()
                    && applied > server_command_seq
                {
                    eprintln!(
                        "[Runner] Gateway command journal rewound: applied {applied} > server {server_command_seq}. Resetting inbound sequence."
                    );
                    durable
                        .journal
                        .reset_inbound_command_seq(server_command_seq)?;
                    command_seq.store(server_command_seq, Ordering::Release);
                }
                if server_seq > sent {
                    match durable.journal.reconcile_sent_through(server_seq) {
                        Ok(()) => {}
                        Err(crate::remote_journal::JournalError::InvalidAck) => {
                            reject_remote_epoch(
                                control,
                                active,
                                epoch,
                                "journal reconcile invalid",
                            );
                            return Ok(());
                        }
                        Err(error) => return Err(error.into()),
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
            }
            *offered = server_seq;
            *active = Some(epoch);
            control.connected.store(true, Ordering::Release);
            let (plugins, reg_url) = crate::plugins::list_local_plugins_view_cached();
            let plugin_frame = build_plugin_state_frame(None, &reg_url, &plugins, true, None);
            let _ = send_tx.try_send(NetworkSend::Control(epoch, plugin_frame));
            eprintln!("{{\"code\":\"remote_connected\"}}");
        }
        NetworkEvent::Sent(epoch, seq) if *active == Some(epoch) => {
            match durable.journal.mark_sent_through(seq) {
                Ok(()) => {}
                Err(crate::remote_journal::JournalError::InvalidAck) => {
                    reject_remote_epoch(control, active, epoch, "journal sent seq invalid");
                }
                Err(error) => return Err(error.into()),
            }
        }
        NetworkEvent::ResetInboundCommandSeq(epoch) if epoch >> 32 == current_remote_generation => {
            durable.journal.reset_inbound_command_seq(0)?;
            command_seq.store(0, Ordering::Release);
        }
        NetworkEvent::Offline(epoch) if *active == Some(epoch) => {
            *active = None;
            control.connected.store(false, Ordering::Release);
            eprintln!("{{\"code\":\"remote_reconnecting\"}}");
        }
        NetworkEvent::Frame(epoch, frame) if *active == Some(epoch) => {
            match frame["type"].as_str() {
                Some("ack") => {
                    let Some(seq) = frame["seq"].as_i64() else {
                        reject_remote_epoch(control, active, epoch, "ack missing seq");
                        return Ok(());
                    };
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
                                .store(durable.journal.received_command_seq()?, Ordering::Release);
                            let _ = durable.poll(true);
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
                Some("plugins_sync") => {}
                Some("plugin_command") => {
                    if let Some(tx) = plugin_tx {
                        spawn_plugin_command_task(epoch, &frame, send_tx, tx);
                    } else {
                        let (fallback_tx, _) = mpsc::channel();
                        spawn_plugin_command_task(epoch, &frame, send_tx, &fallback_tx);
                    }
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

pub(crate) fn remote_epoch_is_active(
    control: &RemoteControl,
    remote_generation: &AtomicU64,
    expected_generation: u64,
    epoch: u64,
) -> bool {
    !control.stopped.load(Ordering::Acquire)
        && remote_generation.load(Ordering::SeqCst) == expected_generation
        && control.rejected_epoch.load(Ordering::SeqCst) != epoch
}

pub(crate) fn reject_remote_epoch(
    control: &RemoteControl,
    active: &mut Option<u64>,
    epoch: u64,
    reason: &str,
) {
    if *active == Some(epoch) {
        *active = None;
    }
    control.connected.store(false, Ordering::Release);
    control.rejected_epoch.store(epoch, Ordering::SeqCst);
    eprintln!(
        "{{\"code\":\"remote_protocol_rejected\", \"reason\":\"{}\"}}",
        reason
    );
}
