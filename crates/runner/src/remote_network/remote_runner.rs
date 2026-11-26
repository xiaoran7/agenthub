use super::*;

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

/// The shortest gap between two discovery republishes that a scan change can
/// ask for. The snapshot is the whole session list, so a store that grows on
/// every tick must not turn the stream into a broadcast.
const DISCOVERY_CHANGE_FLOOR: Duration = Duration::from_secs(5);

/// How often the Runner checks whether the device label changed.
///
/// The label travels in the same `ready` frame as the sessions, so a rename
/// is only published on a republish. Waiting for the full discovery interval
/// would leave the console showing the old name for up to a minute after the
/// user renamed this computer.
const DEVICE_NAME_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct RemoteControl {
    pub stopped: AtomicBool,
    pub connected: AtomicBool,
    pub ready: AtomicBool,
    pub(crate) rejected_epoch: AtomicU64,
}

struct StopOnExit(Arc<RemoteControl>);
impl Drop for StopOnExit {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Release);
        self.0.connected.store(false, Ordering::Release);
        self.0.ready.store(false, Ordering::Release);
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
    ctrlc::set_handler(move || signal.stopped.store(true, Ordering::Release))?;
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
        &crate::device_name::DeviceNameSource::default(),
    )
}

pub(crate) fn sanitize_remote_discovery(mut discovery: Option<Value>) -> Option<Value> {
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

/// Builds the `ready` frame the Gateway stores as the authoritative session set.
///
/// The other half of this contract is `packages/contracts/src/remote.ts`
/// (`parseRemoteReady`), which enforces the same allowlist on the Gateway
/// side. Any change here has to land there too.
pub(crate) fn remote_ready_payload<T: NativeTransport>(
    durable: &DurableHost<T>,
    models: Option<&Value>,
    discovery: Option<&Value>,
    device_name: &crate::device_name::DeviceNameSource,
) -> Value {
    let mut workspace_labels = std::collections::BTreeMap::<String, String>::new();
    let mut workspaces = Vec::new();
    for path in durable.host.workspaces() {
        let raw = path.to_string_lossy();
        let ws_id = crate::privacy::workspace_id_for_path(&raw);
        let display = crate::privacy::workspace_display_path(&raw);
        if !display.is_empty() {
            workspace_labels.insert(ws_id.clone(), display);
        }
        if !workspaces.contains(&ws_id) {
            workspaces.push(ws_id);
        }
    }
    for (key, path) in durable.host.discovered_workspaces() {
        if key.starts_with("ws-") {
            let display = crate::privacy::workspace_display_path(&path.to_string_lossy());
            if !display.is_empty() {
                workspace_labels.entry(key.to_owned()).or_insert(display);
            }
            if !workspaces.contains(key) {
                workspaces.push(key.to_owned());
            }
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
        "device_name": device_name.resolve(),
        "workspaces": workspaces,
        "workspace_labels": workspace_labels,
        "sessions": sessions,
        "models": models.cloned(),
        "discovery": discovery.cloned(),
        "plugins": crate::plugins::ready_plugins_payload(),
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
    device_name: &crate::device_name::DeviceNameSource,
) -> Result<()> {
    let _cleanup = StopOnExit(control.clone());
    if control.stopped.load(Ordering::Acquire) {
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
    // Start the interactive Codex child and initial catalog/session discovery
    // asynchronously so the remote WebSocket connects within ~1 s of launch
    // instead of blocking up to ~10-30 s on cold provider/disk probes.
    let native = crate::remote_host::MultiProviderTransport::with_async_codex(
        binary.clone(),
        PROVIDER_RESPONSE_TIMEOUT,
    );
    let host = NativeSessionHost::new(native, effective_workspaces)
        .map_err(|error| anyhow::anyhow!(error.code))?;
    let mut durable = DurableHost::new(host, journal)?;
    let mut catalog = CatalogWorker::new(Some(binary), durable.host.workspaces().to_vec());
    let mut cached_models: Option<Value> = None;
    let mut cached_discovery: Option<Value> = None;
    // The last discovery actually published to the Gateway. Starting at `None`
    // guarantees the first background `CatalogWorker` result immediately
    // publishes the full models + discovery + workspace_labels snapshot.
    let mut published_discovery: Option<[u8; 32]> = None;
    let mut published_structural: Option<[u8; 32]> = None;
    let mut published_models: Option<[u8; 32]> = None;
    let mut last_ready_publish = Instant::now();
    // The label last published in the `ready` frame, so a rename is detected
    // even when the session list is unchanged.
    let mut published_device_name = device_name.resolve();
    let mut last_discovery_refresh = Instant::now();
    // The scan version this loop last republished for. The scan pass bumps it
    // when a store changed, which is what turns the 60 s floor below into
    // "as soon as there is something new".
    let mut published_scan_version = crate::live::version();
    let mut last_device_name_refresh = Instant::now();
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
    let (events_tx, events) = mpsc::sync_channel(1024);
    let (mut send_tx, send_rx) = mpsc::sync_channel(64);
    let host_generation = durable.host.generation().to_owned();
    let remote_generation = Arc::new(AtomicU64::new(u64::from(url.is_some())));
    let worker_stop = control.clone();
    let worker_seq = command_seq.clone();
    let mut current_server = url.clone();
    let mut refresh_requested = url.is_some();
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
    let mut idle_ticks: u32 = 0;
    let mut pending_net: Vec<NetworkEvent> = Vec::new();
    let (plugin_tx, plugin_rx) = mpsc::channel::<PluginCommandTaskResult>();
    let mut published_plugin_gen = crate::plugins::state_generation();
    while !control.stopped.load(Ordering::Acquire) {
        let polled_events = match durable.poll(current_server.is_some()) {
            Ok(events) => events,
            Err(error) => {
                if !durable.host.is_alive() {
                    control.stopped.store(true, Ordering::Release);
                    bail!("native_host_lost");
                }
                bail!(error);
            }
        };
        if !durable.host.is_alive() {
            control.stopped.store(true, Ordering::Release);
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
                        let changes_catalog = matches!(
                            &command.command,
                            crate::remote_host::RemoteAction::Connect { .. }
                                | crate::remote_host::RemoteAction::SetArchived { .. }
                        );
                        let result = durable
                            .handle_local_command(command)
                            .map_err(|err| err.to_string());
                        if changes_catalog
                            && result
                                .as_ref()
                                .is_ok_and(|receipt| receipt.status == "accepted")
                            && current_server.is_some()
                        {
                            let ready = remote_ready_payload(
                                &durable,
                                cached_models.as_ref(),
                                cached_discovery.as_ref(),
                                device_name,
                            );
                            let _ = durable.journal.enqueue_remote(&ready);
                            refresh_requested = true;
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
                            let ready = remote_ready_payload(
                                &durable,
                                cached_models.as_ref(),
                                cached_discovery.as_ref(),
                                device_name,
                            );
                            if let Err(error) = durable.journal.enqueue_remote(&ready) {
                                let _ = respond_to.send(Err(error.to_string()));
                                continue;
                            }
                            refresh_requested = true;
                        }
                        let _ = respond_to.send(result);
                    }
                    Ok(LocalHostCommand::QueryGeneration { respond_to }) => {
                        let _ = respond_to.send(durable.host.generation().to_string());
                    }
                    Ok(LocalHostCommand::QueryModels { respond_to }) => {
                        catalog.query(CatalogQuery::Models, respond_to);
                    }
                    Ok(LocalHostCommand::Discover { respond_to }) => {
                        catalog.query(CatalogQuery::Discovery, respond_to);
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
                            let next_generation =
                                remote_generation.fetch_add(1, Ordering::SeqCst) + 1;
                            control.connected.store(false, Ordering::Release);
                            active = None;
                            offered = 0;
                            let Some(validated) = validated else {
                                current_server = None;
                                let _ = durable.journal.unbind_server();
                                return Ok(false);
                            };
                            durable.journal.rebind_device_and_server(
                                remote_identity.device_id(),
                                &validated,
                            )?;
                            command_seq.store(0, Ordering::Release);
                            refresh_requested = true;
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
                            control.connected.load(Ordering::Acquire),
                        ));
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        // Bound network work each tick and coalesce monotonic ACKs before hitting SQLite.
        let mut raw_net = std::mem::take(&mut pending_net);
        let remaining = 128usize.saturating_sub(raw_net.len());
        raw_net.extend(events.try_iter().take(remaining));
        let net_batch = coalesce_network_events(raw_net);
        let had_net = !net_batch.is_empty();
        for event in net_batch {
            handle_network_event_inner(
                event,
                remote_generation.load(Ordering::SeqCst),
                &mut durable,
                &control,
                &mut active,
                &mut offered,
                &send_tx,
                &command_seq,
                cached_models.as_ref(),
                cached_discovery.as_ref(),
                device_name,
                Some(&plugin_tx),
            )?;
        }
        let mut sent_plugin_any = false;
        while let Ok(res) = plugin_rx.try_recv() {
            if active == Some(res.epoch) {
                let _ = send_tx.try_send(NetworkSend::Control(res.epoch, res.frame));
                sent_plugin_any = true;
            }
        }
        let current_plugin_gen = crate::plugins::state_generation();
        if current_plugin_gen != published_plugin_gen {
            published_plugin_gen = current_plugin_gen;
            refresh_requested = true;
            if current_server.is_some() {
                let ready_payload = remote_ready_payload(
                    &durable,
                    cached_models.as_ref(),
                    cached_discovery.as_ref(),
                    device_name,
                );
                durable.journal.enqueue_remote(&ready_payload)?;
            }
            if let Some(epoch) = active {
                let (plugins, reg_url) = crate::plugins::list_local_plugins_view_cached();
                let frame = build_plugin_state_frame(None, &reg_url, &plugins, true, None);
                let _ = send_tx.try_send(NetworkSend::Control(epoch, frame));
                sent_plugin_any = true;
            }
        }
        // Re-read the provider's session list and republish it. The `ready`
        // frame is the only channel that carries discovered sessions, so a
        // startup discovery that failed — or a session created since the last
        // refresh — otherwise stayed invisible in the web console for the life
        // of the process. The republish is skipped when nothing changed, so
        // the outbox does not grow on every tick.
        // A scan pass that found a new session asks for the republish right
        // away, instead of leaving it to the 60 s floor. A store that changes
        // on every pass (a live transcript) is still bounded by the shorter
        // discovery floor below, so a busy session cannot push a full
        // snapshot to the Gateway once per tick.
        let scan_advanced = crate::live::version() != published_scan_version;
        let due = if scan_advanced {
            last_discovery_refresh.elapsed() >= DISCOVERY_CHANGE_FLOOR
        } else {
            last_discovery_refresh.elapsed() >= DISCOVERY_REFRESH_INTERVAL
        };
        if current_server.is_some() && (due || refresh_requested) && catalog.request() {
            refresh_requested = false;
            last_discovery_refresh = Instant::now();
            published_scan_version = crate::live::version();
        }
        if let Some(snapshot) = catalog.take_result() {
            if let Ok(discovery) = &snapshot.discovery {
                discovery.apply(&mut durable.host);
            }
            catalog.reply(&snapshot);
            if let Ok(models) = snapshot.models {
                cached_models = Some(models);
            }
            if let Ok(discovery) = snapshot.discovery {
                cached_discovery = sanitize_remote_discovery(Some(discovery.raw));
            }
            if current_server.is_some() {
                let decision = evaluate_ready_publish(
                    cached_models.as_ref(),
                    cached_discovery.as_ref(),
                    &published_models,
                    &published_discovery,
                    &published_structural,
                    last_ready_publish,
                );
                if decision.publish {
                    published_models = decision.next_models_hash;
                    published_discovery = decision.next_discovery_hash;
                    published_structural = decision.next_structural_hash;
                    last_ready_publish = Instant::now();
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
        // The label travels in the same `ready` frame as the sessions, so a
        // rename is only published on a republish. It is checked on its own
        // shorter interval, so the console does not keep showing the old name
        // for a minute after this computer was renamed.
        if current_server.is_some()
            && last_device_name_refresh.elapsed() >= DEVICE_NAME_REFRESH_INTERVAL
        {
            last_device_name_refresh = Instant::now();
            let current_name = device_name.resolve();
            if current_name != published_device_name {
                published_device_name = current_name;
                let ready_payload = remote_ready_payload(
                    &durable,
                    cached_models.as_ref(),
                    cached_discovery.as_ref(),
                    device_name,
                );
                durable.journal.enqueue_remote(&ready_payload)?;
            }
        }
        let mut sent_any = false;
        if let Some(epoch) = active {
            let pending = durable.journal.pending(offered, OUTBOUND_DRAIN_LIMIT)?;
            for (seq, payload) in pending {
                if send_tx
                    .try_send(NetworkSend::Data(epoch, seq, payload))
                    .is_err()
                {
                    break;
                }
                offered = seq;
                sent_any = true;
            }
        }
        let pause = if !polled_events.is_empty() || had_net || sent_any || sent_plugin_any {
            idle_ticks = 0;
            Duration::from_millis(2)
        } else {
            idle_ticks = idle_ticks.saturating_add(1);
            if idle_ticks < 8 {
                Duration::from_millis(12)
            } else {
                Duration::from_millis(25)
            }
        };
        if let Ok(first) = events.recv_timeout(pause) {
            pending_net.push(first);
        }
    }
    Ok(())
}
