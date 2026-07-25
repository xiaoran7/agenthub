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
