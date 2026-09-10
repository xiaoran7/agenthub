use agent_pulse_runner::{
    daemon_ipc,
    daemon_protocol::{
        DAEMON_PROTOCOL_VERSION, DaemonRequest, DaemonRequestEnvelope, DaemonResponse,
        DaemonResponseEnvelope,
    },
    device::DeviceIdentity,
    privacy::workspace_id_for_path,
    remote_host::{
        CodexApprovalPolicy, CodexSandbox, CommandReceipt, ConnectMode, RemoteAction,
        RemoteCommand, SessionBinding,
    },
    remote_journal::{EventsAfter, JournalEvent, RemoteJournal},
};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_daemon(
    identity_path: &std::path::Path,
    codex_binary: &std::path::Path,
    workspace: &std::path::Path,
    database: &std::path::Path,
    endpoint: &str,
) -> ChildGuard {
    ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_agent-pulse-runner"))
            .arg("--identity")
            .arg(identity_path)
            .arg("daemon")
            .arg("--codex-binary")
            .arg(codex_binary)
            .arg("--workspace")
            .arg(workspace)
            .arg("--db")
            .arg(database)
            .arg("--ipc-endpoint")
            .arg(endpoint)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    )
}

fn call(endpoint: &str, request: DaemonRequest) -> DaemonResponse {
    let request_id = uuid::Uuid::new_v4().to_string();
    let response = daemon_ipc::request(
        endpoint,
        &DaemonRequestEnvelope {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            request_id: request_id.clone(),
            request,
        },
    )
    .unwrap();
    assert_eq!(response.protocol_version, DAEMON_PROTOCOL_VERSION);
    assert_eq!(response.request_id, request_id);
    response.response
}

fn wait_until_ready(child: &mut Child, endpoint: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            panic!("agenthubd stopped during startup with {status}");
        }
        if let Ok(DaemonResponseEnvelope {
            response: DaemonResponse::Status {
                host_generation, ..
            },
            ..
        }) = daemon_ipc::request(
            endpoint,
            &DaemonRequestEnvelope {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                request_id: "startup-status".into(),
                request: DaemonRequest::GetStatus,
            },
        ) {
            return host_generation;
        }
        assert!(Instant::now() < deadline, "agenthubd startup timed out");
        thread::sleep(Duration::from_millis(100));
    }
}

fn execute(endpoint: &str, command: RemoteCommand) -> CommandReceipt {
    match call(endpoint, DaemonRequest::Execute { command }) {
        DaemonResponse::Receipt(receipt) => receipt,
        response => panic!("unexpected daemon response: {response:?}"),
    }
}

fn events_after(endpoint: &str, cursor: i64) -> (Vec<JournalEvent>, i64) {
    match call(
        endpoint,
        DaemonRequest::GetEvents {
            after_cursor: cursor,
            limit: 256,
        },
    ) {
        DaemonResponse::Events(EventsAfter::Events { events, cursor }) => (events, cursor),
        response => panic!("unexpected event response: {response:?}"),
    }
}

fn wait_for_turn(
    child: &mut Child,
    endpoint: &str,
    mut cursor: i64,
    provider_session_id: &str,
) -> i64 {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut recent_events = Vec::new();
    loop {
        assert_eq!(
            child.try_wait().unwrap(),
            None,
            "agenthubd stopped during turn"
        );
        let (events, next_cursor) = events_after(endpoint, cursor);
        cursor = next_cursor;
        for entry in events {
            let payload_type = entry.payload["type"].as_str().unwrap_or("unknown");
            let summary = match payload_type {
                "event" if entry.payload["event"]["kind"] == "error" => {
                    let params = &entry.payload["event"]["raw"]["params"];
                    let error = &params["error"];
                    let info = &error["codexErrorInfo"];
                    let info_kind = info
                        .as_object()
                        .and_then(|object| object.keys().next())
                        .map(String::as_str)
                        .unwrap_or("unknown");
                    let http_status = info
                        .as_object()
                        .and_then(|object| object.values().next())
                        .and_then(|value| value["httpStatusCode"].as_i64())
                        .map_or_else(|| "-".to_owned(), |status| status.to_string());
                    format!(
                        "{}:error:{}:{}:{}:{}",
                        entry.cursor,
                        info_kind,
                        error["code"].as_str().unwrap_or("-"),
                        http_status,
                        params["willRetry"].as_bool().unwrap_or(false)
                    )
                }
                "event" => format!(
                    "{}:{}:{}",
                    entry.cursor,
                    entry.payload["event"]["kind"].as_str().unwrap_or("unknown"),
                    entry.payload["event"]["raw"]["params"]["turn"]["status"]
                        .as_str()
                        .unwrap_or("-")
                ),
                "receipt" => format!(
                    "{}:receipt:{}:{}",
                    entry.cursor,
                    entry.payload["receipt"]["status"]
                        .as_str()
                        .unwrap_or("unknown"),
                    entry.payload["receipt"]["code"].as_str().unwrap_or("-")
                ),
                "host_lost" => format!(
                    "{}:host_lost:{}",
                    entry.cursor,
                    entry.payload["code"].as_str().unwrap_or("unknown")
                ),
                _ => format!("{}:{payload_type}", entry.cursor),
            };
            recent_events.push(summary);
            if recent_events.len() > 32 {
                recent_events.remove(0);
            }
            if entry.payload["type"] == "event"
                && entry.payload["event"]["kind"] == "turn/completed"
                && entry.payload["event"]["provider_session_id"] == provider_session_id
            {
                let status = entry.payload["event"]["raw"]["params"]["turn"]["status"]
                    .as_str()
                    .unwrap_or("unknown");
                assert_eq!(
                    status, "completed",
                    "Codex turn failed; recent events: {recent_events:?}"
                );
                return cursor;
            }
        }
        assert!(
            Instant::now() < deadline,
            "Codex turn timed out; recent events: {recent_events:?}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "requires AGENTHUB_TEST_CODEX_BINARY pointing to a working native Codex"]
fn daemon_ipc_resumes_same_native_session_after_restart_and_three_turns() {
    let codex_binary = PathBuf::from(
        std::env::var_os("AGENTHUB_TEST_CODEX_BINARY").expect("native Codex binary required"),
    );
    assert!(codex_binary.is_file(), "native Codex binary does not exist");
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let identity = DeviceIdentity::generate();
    let identity_path = dir.path().join("identity.json");
    identity.save_new(&identity_path).unwrap();
    let database = dir.path().join("agenthubd.sqlite");
    let endpoint = daemon_ipc::default_endpoint(&format!(
        "native-e2e-{}-{}",
        identity.device_id(),
        uuid::Uuid::new_v4()
    ));
    let mut child = spawn_daemon(
        &identity_path,
        &codex_binary,
        &workspace,
        &database,
        &endpoint,
    );

    let generation = wait_until_ready(&mut child.0, &endpoint);
    let hub_session_id = "native-daemon-e2e";
    let connect = RemoteCommand {
        client_cmd_id: "native-daemon-create".into(),
        hub_session_id: hub_session_id.into(),
        host_generation: Some(generation.clone()),
        command: RemoteAction::Connect {
            mode: ConnectMode::Create,
            workspace_path: workspace_id_for_path(&workspace.to_string_lossy()).into(),
            provider_session_id: None,
            model: None,
            sandbox: CodexSandbox::ReadOnly,
            approval_policy: CodexApprovalPolicy::Untrusted,
        },
    };
    let receipt = execute(&endpoint, connect);
    assert_eq!(receipt.status, "accepted");
    let binding: SessionBinding =
        serde_json::from_value(receipt.result.unwrap()["binding"].clone()).unwrap();
    assert_eq!(binding.host_generation, generation);
    assert!(!binding.provider_session_id.is_empty());

    let first = RemoteCommand {
        client_cmd_id: "native-daemon-turn-1".into(),
        hub_session_id: hub_session_id.into(),
        host_generation: Some(generation.clone()),
        command: RemoteAction::SendPrompt {
            text: "Do not use tools. Reply exactly AGENTHUB_DAEMON_OK.".into(),
        },
    };
    let first_receipt = execute(&endpoint, first.clone());
    assert_eq!(first_receipt.status, "accepted");
    assert_eq!(execute(&endpoint, first), first_receipt);
    let mut cursor = wait_for_turn(&mut child.0, &endpoint, 0, &binding.provider_session_id);

    let second = RemoteCommand {
        client_cmd_id: "native-daemon-turn-2".into(),
        hub_session_id: hub_session_id.into(),
        host_generation: Some(generation),
        command: RemoteAction::SendPrompt {
            text: "Without tools, repeat the exact marker from the previous turn.".into(),
        },
    };
    assert_eq!(execute(&endpoint, second).status, "accepted");
    cursor = wait_for_turn(
        &mut child.0,
        &endpoint,
        cursor,
        &binding.provider_session_id,
    );

    let snapshot = match call(&endpoint, DaemonRequest::GetSnapshot) {
        DaemonResponse::Snapshot(snapshot) => snapshot,
        response => panic!("unexpected snapshot response: {response:?}"),
    };
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(
        snapshot.sessions[0].provider_session_id,
        binding.provider_session_id
    );
    assert!(snapshot.cursor >= cursor);

    thread::sleep(Duration::from_millis(250));
    assert_eq!(
        child.0.try_wait().unwrap(),
        None,
        "daemon stopped after IPC clients disconnected"
    );
    child.0.kill().unwrap();
    child.0.wait().unwrap();

    let mut restarted = spawn_daemon(
        &identity_path,
        &codex_binary,
        &workspace,
        &database,
        &endpoint,
    );
    let restarted_generation = wait_until_ready(&mut restarted.0, &endpoint);
    assert_ne!(restarted_generation, binding.host_generation);
    let detached = match call(&endpoint, DaemonRequest::GetSnapshot) {
        DaemonResponse::Snapshot(snapshot) => snapshot.sessions,
        response => panic!("unexpected restarted snapshot response: {response:?}"),
    };
    assert_eq!(detached.len(), 1);
    assert_eq!(detached[0].provider_session_id, binding.provider_session_id);
    assert!(!detached[0].control_connected);

    let resume = RemoteCommand {
        client_cmd_id: "native-daemon-resume".into(),
        hub_session_id: hub_session_id.into(),
        host_generation: Some(restarted_generation.clone()),
        command: RemoteAction::Connect {
            mode: ConnectMode::Resume,
            workspace_path: workspace_id_for_path(&workspace.to_string_lossy()).into(),
            provider_session_id: Some(binding.provider_session_id.clone()),
            model: None,
            sandbox: CodexSandbox::ReadOnly,
            approval_policy: CodexApprovalPolicy::Untrusted,
        },
    };
    let resumed = execute(&endpoint, resume);
    assert_eq!(resumed.status, "accepted");
    let resumed_binding: SessionBinding =
        serde_json::from_value(resumed.result.unwrap()["binding"].clone()).unwrap();
    assert_eq!(
        resumed_binding.provider_session_id,
        binding.provider_session_id
    );
    assert_eq!(resumed_binding.host_generation, restarted_generation);
    assert!(resumed_binding.control_connected);

    let third = RemoteCommand {
        client_cmd_id: "native-daemon-turn-3".into(),
        hub_session_id: hub_session_id.into(),
        host_generation: Some(restarted_generation),
        command: RemoteAction::SendPrompt {
            text: "Without tools, reply exactly AGENTHUB_RESTART_OK.".into(),
        },
    };
    assert_eq!(execute(&endpoint, third).status, "accepted");
    cursor = wait_for_turn(
        &mut restarted.0,
        &endpoint,
        cursor,
        &binding.provider_session_id,
    );
    restarted.0.kill().unwrap();
    restarted.0.wait().unwrap();

    let journal = RemoteJournal::open(&database).unwrap();
    let persisted = journal.snapshot().unwrap();
    assert_eq!(persisted.sessions.len(), 1);
    assert_eq!(
        persisted.sessions[0].provider_session_id,
        binding.provider_session_id
    );
    assert!(persisted.cursor >= cursor);
}
