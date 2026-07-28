use std::{collections::BTreeMap, fs, process::Command};

use agent_platform_runner::{
    device::DeviceIdentity,
    domain::{
        ActivityStatus, AgentType, Confidence, ObservationMeta, ObservationSource, Presence,
        ProjectRef, SessionObservation, SessionState,
    },
    outbox::Outbox,
    sync::{SYNC_PROTOCOL_VERSION, SyncAck, SyncBatch},
};

fn runner() -> Command {
    Command::new(env!("CARGO_BIN_EXE_agent-platform-runner"))
}

#[test]
fn device_and_sync_commands_complete_a_signed_acknowledgement_cycle() {
    let directory = tempfile::tempdir().unwrap();
    let identity_path = directory.path().join("identity.json");
    let database_path = directory.path().join("runner.db");
    let batch_path = directory.path().join("batch.json");
    let acknowledgement_path = directory.path().join("ack.json");

    let init = runner()
        .args(["device", "init", "--identity"])
        .arg(&identity_path)
        .output()
        .unwrap();
    assert!(init.status.success(), "{:?}", init.stderr);
    let identity = DeviceIdentity::load(&identity_path).unwrap();

    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("claude-agents.json");
    let normalized = runner()
        .arg("--identity")
        .arg(&identity_path)
        .args(["normalize", "claude-agents", "--input"])
        .arg(fixture)
        .output()
        .unwrap();
    assert!(normalized.status.success(), "{:?}", normalized.stderr);
    let normalized: Vec<SessionObservation> = serde_json::from_slice(&normalized.stdout).unwrap();
    assert!(
        normalized
            .iter()
            .all(|observation| observation.device_id == identity.device_id())
    );

    let observation = SessionObservation::new(
        &ObservationMeta::now(identity.device_id()),
        AgentType::ClaudeCode,
        "session-1",
        ProjectRef {
            display_name: "safe-project".into(),
            fingerprint: "safe-fingerprint".into(),
        },
        SessionState {
            presence: Presence::Live,
            activity_status: ActivityStatus::Working,
            raw_status: "working".into(),
            raw_status_detail: BTreeMap::new(),
            waiting_reason: None,
        },
        ObservationSource::ClaudeAgentsJson,
        Confidence::Authoritative,
    );
    Outbox::open(&database_path)
        .unwrap()
        .enqueue(&observation)
        .unwrap();

    let prepared = runner()
        .args(["sync", "prepare", "--identity"])
        .arg(&identity_path)
        .arg("--db")
        .arg(&database_path)
        .output()
        .unwrap();
    assert!(prepared.status.success(), "{:?}", prepared.stderr);
    let batch: SyncBatch = serde_json::from_slice(&prepared.stdout).unwrap();
    batch.verify().unwrap();
    assert!(!String::from_utf8_lossy(&prepared.stdout).contains("private_key"));
    fs::write(&batch_path, &prepared.stdout).unwrap();

    let verified = runner()
        .args(["sync", "verify", "--batch"])
        .arg(&batch_path)
        .output()
        .unwrap();
    assert!(verified.status.success(), "{:?}", verified.stderr);

    let acknowledgement = SyncAck {
        protocol_version: SYNC_PROTOCOL_VERSION,
        batch_id: batch.batch_id,
        device_id: batch.device_id.clone(),
        accepted_through: batch.last_sequence,
    };
    fs::write(
        &acknowledgement_path,
        serde_json::to_vec(&acknowledgement).unwrap(),
    )
    .unwrap();
    let acknowledged = runner()
        .args(["sync", "ack", "--db"])
        .arg(&database_path)
        .arg("--batch")
        .arg(&batch_path)
        .arg("--acknowledgement")
        .arg(&acknowledgement_path)
        .output()
        .unwrap();
    assert!(acknowledged.status.success(), "{:?}", acknowledged.stderr);

    assert_eq!(
        Outbox::open(database_path)
            .unwrap()
            .stats()
            .unwrap()
            .pending_events,
        0
    );
}

#[test]
fn background_sync_rejects_an_unidentified_runner() {
    let directory = tempfile::tempdir().unwrap();
    let database_path = directory.path().join("runner.db");
    let output = runner()
        .env("AGENT_PLATFORM_FINGERPRINT_KEY", "test-only-key")
        .args(["run", "claude", "--db"])
        .arg(&database_path)
        .args([
            "--sync-server",
            "http://127.0.0.1:3001",
            "--max-attempts",
            "1",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("background sync requires --identity"),
        "{:?}",
        output.stderr
    );
}

#[test]
fn gateway_ca_file_failures_use_stable_errors_without_leaking_contents() {
    let directory = tempfile::tempdir().unwrap();
    let identity_path = directory.path().join("identity.json");
    let invalid_ca_path = directory.path().join("invalid-ca.pem");
    let missing_ca_path = directory.path().join("missing-ca.pem");
    fs::write(&invalid_ca_path, "private-marker-not-a-certificate").unwrap();
    let init = runner()
        .args(["device", "init", "--identity"])
        .arg(&identity_path)
        .output()
        .unwrap();
    assert!(init.status.success(), "{:?}", init.stderr);

    let invalid = runner()
        .args(["device", "bind", "--identity"])
        .arg(&identity_path)
        .args([
            "--server",
            "https://gateway.example",
            "--binding-code",
            "test-binding-code",
            "--gateway-ca-cert",
        ])
        .arg(&invalid_ca_path)
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    let invalid_error = String::from_utf8_lossy(&invalid.stderr);
    assert!(invalid_error.contains("gateway_invalid_ca_certificate"));
    assert!(!invalid_error.contains("private-marker"));
    assert!(!invalid_error.contains(&invalid_ca_path.display().to_string()));

    let missing = runner()
        .args(["sync", "push", "--identity"])
        .arg(&identity_path)
        .arg("--db")
        .arg(directory.path().join("runner.db"))
        .args(["--server", "https://gateway.example", "--gateway-ca-cert"])
        .arg(&missing_ca_path)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let missing_error = String::from_utf8_lossy(&missing.stderr);
    assert!(missing_error.contains("gateway_ca_cert_unreadable"));
    assert!(!missing_error.contains(&missing_ca_path.display().to_string()));
}
