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
    let device_name = crate::device_name::DeviceNameSource::default();
    handle_network_event(
        NetworkEvent::Connected(epoch, 0, None),
        epoch >> 32,
        durable,
        control,
        active,
        offered,
        send_tx,
        command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(*active, Some(epoch));
    assert!(control.connected.load(Ordering::Relaxed));
}

#[test]
fn remote_protocol_faults_fence_only_the_transport_epoch() {
    let generation = 7_u64;
    let epoch = generation << 32 | 1;
    let device_name = crate::device_name::DeviceNameSource::default();
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
            None,
            None,
            &device_name,
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
    let device_name = crate::device_name::DeviceNameSource::default();
    let (_dir, mut durable, control, send_tx, _send_rx, command_seq) = remote_test_host();
    let host_generation = durable.host.generation().to_owned();
    let mut active = None;
    let mut offered = 0;

    handle_network_event(
        NetworkEvent::Connected(epoch, 1, None),
        generation,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();

    assert_eq!(active, None);
    assert!(!control.stopped.load(Ordering::Relaxed));
    assert!(durable.host.is_alive());
    assert_eq!(durable.host.generation(), host_generation);
    assert_eq!(control.rejected_epoch.load(Ordering::SeqCst), epoch);
}

#[test]
fn server_sequence_rewound_resynchronizes_outbox_stream() {
    let generation = 5_u64;
    let epoch = generation << 32 | 1;
    let device_name = crate::device_name::DeviceNameSource::default();
    let (_dir, mut durable, control, send_tx, _send_rx, command_seq) = remote_test_host();
    let mut active = None;
    let mut offered = 0;

    // Simulate local state having advanced previously (e.g. against older server instance)
    durable
        .journal
        .enqueue_remote(&json!({"type":"test"}))
        .unwrap();
    durable.journal.mark_sent(1).unwrap();
    durable.journal.acknowledge(1).unwrap();
    let (_, acked) = durable.journal.watermarks().unwrap();
    assert_eq!(acked, 1);

    // Server reconnects with server_seq = 0 (server database was reset / lower than acked)
    handle_network_event(
        NetworkEvent::Connected(epoch, 0, None),
        generation,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();

    assert_eq!(active, Some(epoch));
    assert!(control.connected.load(Ordering::Relaxed));
    assert_eq!(offered, 0);

    // Watermarks should be reset and a fresh ready frame queued at seq 1
    let (sent, acked) = durable.journal.watermarks().unwrap();
    assert_eq!(sent, 0);
    assert_eq!(acked, 0);
    let pending = durable.journal.pending(0, 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, 1);
    assert_eq!(pending[0].1["type"], "ready");
}

#[test]
fn a_shorter_gateway_command_journal_rewinds_the_inbound_watermark() {
    let generation = 6_u64;
    let epoch = generation << 32 | 1;
    let device_name = crate::device_name::DeviceNameSource::default();
    let (_dir, mut durable, control, send_tx, _send_rx, command_seq) = remote_test_host();
    let mut active = None;
    let mut offered = 0;

    // This Host has applied commands up to 3 against a Gateway journal that
    // has since been rebuilt and now holds none of them.
    durable
        .journal
        .reset_inbound_command_seq(3)
        .expect("rewind to an applied watermark");
    command_seq.store(3, Ordering::Relaxed);

    handle_network_event(
        NetworkEvent::Connected(epoch, 0, Some(0)),
        generation,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();

    // The event stream was intact, so the transport stays up; only the
    // inbound side moves back to what the Gateway can actually replay.
    assert_eq!(active, Some(epoch));
    assert!(control.connected.load(Ordering::Relaxed));
    assert_eq!(durable.journal.received_command_seq().unwrap(), 0);
    assert_eq!(command_seq.load(Ordering::Relaxed), 0);
}

#[test]
fn an_older_gateway_without_a_command_watermark_keeps_the_local_one() {
    let generation = 4_u64;
    let epoch = generation << 32 | 1;
    let device_name = crate::device_name::DeviceNameSource::default();
    let (_dir, mut durable, control, send_tx, _send_rx, command_seq) = remote_test_host();
    let mut active = None;
    let mut offered = 0;
    durable.journal.reset_inbound_command_seq(2).unwrap();
    command_seq.store(2, Ordering::Relaxed);

    handle_network_event(
        NetworkEvent::Connected(epoch, 0, None),
        generation,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();

    assert_eq!(durable.journal.received_command_seq().unwrap(), 2);
    assert_eq!(command_seq.load(Ordering::Relaxed), 2);
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

    let device_name = crate::device_name::DeviceNameSource::new(Some("书房主机".to_owned()), None);
    let payload = remote_ready_payload(&durable, Some(&models), discovery.as_ref(), &device_name);
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
        let LocalHostCommand::Execute { respond_to, .. } = channel.receiver.recv().unwrap() else {
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

#[test]
fn validate_server_allows_tailnet_and_private_ws_but_rejects_public_ws() {
    for allowed in [
        "ws://portable-pc.tail000001.ts.net",
        "ws://macmini.local:8787",
        "ws://100.64.0.42:8787",
        "ws://169.254.10.20:8787",
        "ws://[fd7a:115c:a1e0::1]:8787",
        "ws://[fe80::1]:8787",
        "ws://[::ffff:100.64.0.99]:8787",
        "ws://[::1]:8787",
        "wss://hub.example.com",
        "wss://[2001:4860:4860::8888]:8787",
    ] {
        assert!(
            validate_server(allowed).is_ok(),
            "expected allowed server URL: {allowed}"
        );
    }

    for rejected in [
        "ws://hub.example.com",
        "ws://8.8.8.8:8787",
        "ws://[2001:4860:4860::8888]:8787",
        "ws://[::ffff:8.8.8.8]:8787",
    ] {
        assert!(
            validate_server(rejected).is_err(),
            "expected TLS-required rejection for public ws URL: {rejected}"
        );
    }
}

#[test]
fn out_of_bounds_connect_and_premature_ack_are_rejected_without_dropping_unsent_rows() {
    let generation = 9_u64;
    let epoch = generation << 32 | 1;
    let device_name = crate::device_name::DeviceNameSource::default();
    let (_dir, mut durable, control, send_tx, _send_rx, command_seq) = remote_test_host();
    let mut active = None;
    let mut offered = 0;

    for idx in 1..=4 {
        durable
            .journal
            .enqueue_remote(&json!({"type": "event", "idx": idx}))
            .unwrap();
    }

    // Extreme out-of-bounds server_seq must be rejected immediately in O(1).
    handle_network_event(
        NetworkEvent::Connected(epoch, i64::MAX, None),
        generation,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(active, None);
    assert_eq!(durable.journal.pending(0, 10).unwrap().len(), 4);

    // Valid connect + coalesced Sent(3) advances sent to 3; premature ack(4) must fail
    // and preserve unsent row 4.
    let epoch2 = generation << 32 | 2;
    connect_remote(
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        epoch2,
    );
    handle_network_event(
        NetworkEvent::Sent(epoch2, 3),
        generation,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(durable.journal.watermarks().unwrap(), (3, 0));

    handle_network_event(
        NetworkEvent::Frame(epoch2, json!({"type": "ack", "seq": 4})),
        generation,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(active, None);
    assert_eq!(durable.journal.watermarks().unwrap(), (3, 0));
    assert_eq!(durable.journal.pending(0, 10).unwrap().len(), 4);
}

#[test]
fn negative_handshake_seqs_and_invalid_sent_seq_reject_epoch_without_crashing() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let host = NativeSessionHost::new(FakeTransport, vec![workspace]).unwrap();
    let journal = RemoteJournal::open(dir.path().join("journal.sqlite")).unwrap();
    let mut durable = DurableHost::new(host, journal).unwrap();
    let control = RemoteControl::default();
    let mut active = None;
    let mut offered = 0;
    let (send_tx, _send_rx) = mpsc::sync_channel(8);
    let command_seq = AtomicI64::new(0);
    let device_name = crate::device_name::DeviceNameSource::default();

    // Negative server_seq in Connected rejects the epoch and never touches the journal.
    handle_network_event(
        NetworkEvent::Connected(1, -5, Some(0)),
        0,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(active, None);
    assert_eq!(control.rejected_epoch.load(Ordering::SeqCst), 1);
    assert_eq!(durable.journal.watermarks().unwrap(), (0, 0));

    // Negative server_command_seq also rejects cleanly.
    handle_network_event(
        NetworkEvent::Connected(2, 0, Some(-1)),
        0,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(active, None);
    assert_eq!(control.rejected_epoch.load(Ordering::SeqCst), 2);

    // Valid connect followed by invalid Sent(seq) rejects epoch instead of returning Err.
    handle_network_event(
        NetworkEvent::Connected(3, 0, Some(0)),
        0,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(active, Some(3));
    handle_network_event(
        NetworkEvent::Sent(3, 999),
        0,
        &mut durable,
        &control,
        &mut active,
        &mut offered,
        &send_tx,
        &command_seq,
        None,
        None,
        &device_name,
    )
    .unwrap();
    assert_eq!(active, None);
    assert_eq!(control.rejected_epoch.load(Ordering::SeqCst), 3);
}

#[test]
fn handshake_receive_enforces_deadline_despite_continuous_ping_frames() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        for _ in 0..8 {
            if ws.send(Message::Ping(vec![1u8, 2, 3].into())).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(60));
        }
    });

    let url = format!("ws://{addr}/v3/remote/ws");
    let mut client = connect_with_timeout(&url, Duration::from_secs(2)).unwrap();
    let stopped = AtomicBool::new(false);
    let err = receive_with_deadline(
        &mut client,
        Instant::now() + Duration::from_millis(250),
        &stopped,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("remote_handshake_timeout"),
        "expected remote_handshake_timeout, got: {err}"
    );
    let _ = server.join();
}
