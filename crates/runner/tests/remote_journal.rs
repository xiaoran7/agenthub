use agent_pulse_runner::{remote_host::*, remote_journal::*};
use serde_json::json;

fn command() -> RemoteCommand {
    RemoteCommand {
        client_cmd_id: "cmd-one".into(),
        hub_session_id: "hub-one".into(),
        host_generation: Some("generation-one".into()),
        command: RemoteAction::SendPrompt {
            text: "do not repeat this action".into(),
        },
    }
}

#[test]
fn journal_cannot_be_reused_with_another_device_or_gateway() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.db");
    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        journal
            .bind_identity("device-a", "wss://example.test/v3/remote/ws")
            .unwrap();
    }
    let mut journal = RemoteJournal::open(&path).unwrap();
    journal
        .bind_identity("device-a", "wss://example.test/v3/remote/ws")
        .unwrap();
    assert!(
        journal
            .bind_identity("device-b", "wss://example.test/v3/remote/ws")
            .is_err()
    );
    assert!(
        journal
            .bind_identity("device-a", "wss://other.test/v3/remote/ws")
            .is_err()
    );
}

#[test]
fn crashed_intent_is_indeterminate_and_never_becomes_a_new_command() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        assert!(matches!(
            journal.begin_command(&command()).unwrap(),
            BeginCommand::New
        ));
    }
    let mut journal = RemoteJournal::open(&path).unwrap();
    let BeginCommand::Existing(receipt) = journal.begin_command(&command()).unwrap() else {
        panic!("would execute again")
    };
    assert_eq!(receipt.status, "indeterminate");
}

#[test]
fn receipt_and_native_mapping_survive_restart_but_live_control_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    let receipt = CommandReceipt {
        client_cmd_id: "cmd-one".into(),
        hub_session_id: "hub-one".into(),
        status: "accepted".into(),
        code: None,
        result: Some(json!({"turn":{"id":"native-turn"}})),
    };
    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        journal.begin_command(&command()).unwrap();
        let binding = SessionBinding {
            hub_session_id: "hub-one".into(),
            provider_session_id: "native-real".into(),
            workspace_path: dir.path().into(),
            host_generation: "old".into(),
            control_connected: true,
            execution_state: ExecutionState::Running,
            current_turn_id: Some("native-turn".into()),
        };
        journal.finish_command(&receipt, &[binding]).unwrap();
    }
    let mut journal = RemoteJournal::open(&path).unwrap();
    let BeginCommand::Existing(stored) = journal.begin_command(&command()).unwrap() else {
        panic!("would repeat")
    };
    assert_eq!(stored, receipt);
    let bindings = journal.bindings().unwrap();
    assert_eq!(bindings[0].provider_session_id, "native-real");
    assert!(!bindings[0].control_connected);
    assert_eq!(bindings[0].execution_state, ExecutionState::Unknown);
    assert_eq!(journal.pending(0, 10).unwrap().len(), 1);
}

#[test]
fn durable_command_id_conflict_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = RemoteJournal::open(dir.path().join("remote.db")).unwrap();
    journal.begin_command(&command()).unwrap();
    let mut changed = command();
    changed.hub_session_id = "another".into();
    let BeginCommand::Existing(receipt) = journal.begin_command(&changed).unwrap() else {
        panic!("conflict accepted")
    };
    assert_eq!(receipt.code.as_deref(), Some("command_id_conflict"));
}

#[test]
fn only_contiguous_sent_events_can_be_acknowledged_and_unacked_events_survive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        assert_eq!(journal.append(&json!({"event":"first"}), &[]).unwrap(), 1);
        assert_eq!(journal.append(&json!({"event":"second"}), &[]).unwrap(), 2);
        assert!(journal.mark_sent(2).is_err());
        assert!(journal.acknowledge(1).is_err());
        journal.mark_sent(1).unwrap();
        journal.acknowledge(1).unwrap();
        assert!(journal.acknowledge(2).is_err());
    }
    let mut journal = RemoteJournal::open(&path).unwrap();
    assert_eq!(journal.watermarks().unwrap(), (1, 1));
    assert_eq!(
        journal.pending(0, 10).unwrap(),
        vec![(2, json!({"event":"second"}))]
    );
    journal.mark_sent(2).unwrap();
    journal.acknowledge(2).unwrap();
    assert!(journal.pending(0, 10).unwrap().is_empty());
    assert_eq!(journal.append(&json!({"event":"third"}), &[]).unwrap(), 3);
}

#[test]
fn two_daemons_cannot_open_the_same_journal_for_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    let journal = RemoteJournal::open(&path).unwrap();
    assert!(RemoteJournal::open(&path).is_err());
    drop(journal);
    assert!(RemoteJournal::open(&path).is_ok());
}

#[test]
fn received_command_watermark_rejects_gaps_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        assert!(journal.confirm_command_seq(2).is_err());
        journal.confirm_command_seq(1).unwrap();
    }
    let journal = RemoteJournal::open(&path).unwrap();
    assert_eq!(journal.received_command_seq().unwrap(), 1);
}
