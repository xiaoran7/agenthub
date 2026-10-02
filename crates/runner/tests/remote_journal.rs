use agent_pulse_runner::{remote_host::*, remote_journal::*};
use serde_json::{Value, json};

fn command() -> RemoteCommand {
    RemoteCommand {
        client_cmd_id: "cmd-one".into(),
        hub_session_id: "hub-one".into(),
        host_generation: Some("generation-one".into()),
        command: RemoteAction::SendPrompt {
            text: "do not repeat this action".into(),
            effort: None,
            model: None,
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
fn local_journal_binds_once_and_never_moves_gateway_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = RemoteJournal::open(dir.path().join("identity.db")).unwrap();
    journal
        .bind_identity("device-a", "local://agenthub")
        .unwrap();
    journal
        .rebind_server_if_local("wss://example.test/v3/remote/ws")
        .unwrap();
    journal
        .rebind_server_if_local("wss://example.test/v3/remote/ws")
        .unwrap();
    assert!(
        journal
            .rebind_server_if_local("wss://other.test/v3/remote/ws")
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
fn local_receipt_and_native_mapping_survive_restart_without_remote_upload() {
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
        journal.finish_local_command(&receipt, &[binding]).unwrap();
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
    assert!(journal.pending(0, 10).unwrap().is_empty());
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
fn local_only_events_preserve_registry_without_consuming_remote_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    let binding = SessionBinding {
        hub_session_id: "hub-one".into(),
        provider_session_id: "native-one".into(),
        workspace_path: dir.path().into(),
        host_generation: "generation-one".into(),
        control_connected: true,
        execution_state: ExecutionState::Running,
        current_turn_id: Some("turn-one".into()),
    };
    let mut journal = RemoteJournal::open(&path).unwrap();

    assert_eq!(
        journal
            .append(&json!({"type":"authorized-before-disable"}), &[])
            .unwrap(),
        1
    );
    journal.mark_sent(1).unwrap();
    journal
        .append_local(
            &json!({"type":"event","marker":"disabled-content"}),
            std::slice::from_ref(&binding),
        )
        .unwrap();
    journal
        .append_local(&json!({"type":"event","marker":"disabled-content-2"}), &[])
        .unwrap();

    assert_eq!(journal.watermarks().unwrap(), (1, 0));
    assert_eq!(
        journal.pending(0, 10).unwrap(),
        vec![(1, json!({"type":"authorized-before-disable"}))]
    );
    assert_eq!(
        journal.bindings().unwrap()[0].provider_session_id,
        "native-one"
    );
    assert_eq!(
        journal
            .enqueue_remote(&json!({"type":"ready","marker":"current"}))
            .unwrap(),
        2
    );
    assert_eq!(journal.watermarks().unwrap(), (1, 0));
    assert!(
        journal
            .pending(0, 10)
            .unwrap()
            .iter()
            .all(|(_, payload)| payload["marker"] != "disabled-content"
                && payload["marker"] != "disabled-content-2")
    );

    journal.acknowledge(1).unwrap();
    journal.mark_sent(2).unwrap();
    journal.acknowledge(2).unwrap();
    assert!(journal.pending(0, 10).unwrap().is_empty());
    let EventsAfter::Events { events, cursor } = journal.events_after(0, 10).unwrap() else {
        panic!("disabled local events must remain replayable");
    };
    assert_eq!(cursor, 3);
    assert!(
        events
            .iter()
            .any(|event| event.payload["marker"] == "disabled-content")
    );
}

#[test]
fn local_command_receipt_survives_restart_without_remote_upload() {
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
        assert!(matches!(
            journal.begin_command(&command()).unwrap(),
            BeginCommand::New
        ));
        assert_eq!(journal.finish_local_command(&receipt, &[]).unwrap(), 1);
        assert!(journal.pending(0, 10).unwrap().is_empty());
    }

    let mut journal = RemoteJournal::open(&path).unwrap();
    let BeginCommand::Existing(stored) = journal.begin_command(&command()).unwrap() else {
        panic!("disabled command would be repeated")
    };
    assert_eq!(stored, receipt);
    assert!(journal.pending(0, 10).unwrap().is_empty());
    let EventsAfter::Events { events, .. } = journal.events_after(0, 10).unwrap() else {
        panic!("local receipt must be replayable")
    };
    assert_eq!(events[0].payload["type"], "receipt");
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
fn inbound_sequence_is_payload_bound_across_crash_and_atomic_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    let indeterminate = CommandReceipt {
        client_cmd_id: "cmd-one".into(),
        hub_session_id: "hub-one".into(),
        status: "indeterminate".into(),
        code: Some("previous_execution_unconfirmed".into()),
        result: None,
    };

    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        assert!(matches!(
            journal.begin_inbound_command(1, &command()).unwrap(),
            BeginInboundCommand::New
        ));
        assert_eq!(journal.received_command_seq().unwrap(), 0);
    }

    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        let BeginInboundCommand::CompleteExisting(receipt) =
            journal.begin_inbound_command(1, &command()).unwrap()
        else {
            panic!("crashed inbound intent must become indeterminate")
        };
        assert_eq!(receipt, indeterminate);
        let mut changed = command();
        changed.client_cmd_id = "substituted".into();
        assert!(journal.begin_inbound_command(1, &changed).is_err());
        journal.finish_inbound_command(1, &receipt, &[]).unwrap();
        assert_eq!(journal.received_command_seq().unwrap(), 1);
    }

    let mut journal = RemoteJournal::open(&path).unwrap();
    let BeginInboundCommand::AlreadyCompleted(receipt) =
        journal.begin_inbound_command(1, &command()).unwrap()
    else {
        panic!("completed inbound command must replay without execution")
    };
    assert_eq!(receipt, indeterminate);
    let mut changed = command();
    changed.hub_session_id = "different-payload".into();
    assert!(journal.begin_inbound_command(1, &changed).is_err());
    assert!(journal.begin_inbound_command(3, &command()).is_err());
    assert_eq!(journal.received_command_seq().unwrap(), 1);
}

#[test]
fn inbound_receipt_and_received_watermark_commit_together() {
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
        assert!(matches!(
            journal.begin_inbound_command(1, &command()).unwrap(),
            BeginInboundCommand::New
        ));
        journal.finish_inbound_command(1, &receipt, &[]).unwrap();
    }

    let mut journal = RemoteJournal::open(&path).unwrap();
    assert_eq!(journal.received_command_seq().unwrap(), 1);
    let BeginInboundCommand::AlreadyCompleted(stored) =
        journal.begin_inbound_command(1, &command()).unwrap()
    else {
        panic!("committed receipt must be replayable")
    };
    assert_eq!(stored, receipt);
    let pending = journal.pending(0, 10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].1["type"], "receipt");
}

#[test]
fn gateway_ack_does_not_delete_local_events_and_cursor_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote.db");
    {
        let mut journal = RemoteJournal::open(&path).unwrap();
        journal.append(&json!({"event":"first"}), &[]).unwrap();
        journal.append(&json!({"event":"second"}), &[]).unwrap();
        journal.mark_sent(1).unwrap();
        journal.mark_sent(2).unwrap();
        journal.acknowledge(2).unwrap();
        assert!(journal.pending(0, 10).unwrap().is_empty());
        let EventsAfter::Events { events, cursor } = journal.events_after(0, 10).unwrap() else {
            panic!("local replay unexpectedly required a snapshot")
        };
        assert_eq!(cursor, 2);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].payload, json!({"event":"first"}));
    }
    let journal = RemoteJournal::open(&path).unwrap();
    assert_eq!(journal.snapshot().unwrap().cursor, 2);
    let EventsAfter::Events { events, cursor } = journal.events_after(1, 10).unwrap() else {
        panic!("persisted cursor unexpectedly expired")
    };
    assert_eq!(cursor, 2);
    assert_eq!(events[0].payload, json!({"event":"second"}));
}

#[test]
fn bounded_local_log_requires_snapshot_for_expired_or_future_cursors() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal =
        RemoteJournal::open_with_event_retention(dir.path().join("remote.db"), 3).unwrap();
    for index in 1..=5 {
        journal.append(&json!({"event":index}), &[]).unwrap();
    }

    let EventsAfter::SnapshotRequired { snapshot } = journal.events_after(1, 10).unwrap() else {
        panic!("expired cursor must require a snapshot")
    };
    assert_eq!(snapshot.cursor, 5);
    assert!(matches!(
        journal.events_after(6, 10).unwrap(),
        EventsAfter::SnapshotRequired { .. }
    ));

    let EventsAfter::Events { events, cursor } = journal.events_after(2, 10).unwrap() else {
        panic!("cursor immediately before retention window must remain replayable")
    };
    assert_eq!(cursor, 5);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].cursor, 3);
    assert_eq!(events[2].payload, json!({"event":5}));
}

#[test]
fn batched_appends_commit_every_event_and_trim_only_once_at_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal =
        RemoteJournal::open_with_event_retention(dir.path().join("remote.db"), 3).unwrap();

    let payloads: Vec<Value> = (1..=5).map(|index| json!({"event": index})).collect();
    let batch: Vec<BatchedAppend<'_>> = payloads
        .iter()
        .map(|payload| BatchedAppend {
            local: payload,
            remote: Some(payload),
            bindings: &[],
        })
        .collect();
    let cursor = journal.append_batch(&batch).unwrap();
    assert_eq!(cursor, 5);

    // Trimming runs once against the final cursor, so the batch keeps the same
    // newest-N window that sequential appends would have produced.
    let EventsAfter::Events { events, cursor } = journal.events_after(2, 10).unwrap() else {
        panic!("retention window must still be replayable")
    };
    assert_eq!(cursor, 5);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].payload, json!({"event": 3}));
    assert_eq!(events[2].payload, json!({"event": 5}));

    // Every batched event with a remote projection reached the outbox.
    assert_eq!(journal.pending(0, 10).unwrap().len(), 5);
}

#[test]
fn a_batch_of_local_only_events_skips_the_remote_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = RemoteJournal::open(dir.path().join("remote.db")).unwrap();

    let payloads: Vec<Value> = (1..=3).map(|index| json!({"event": index})).collect();
    let batch: Vec<BatchedAppend<'_>> = payloads
        .iter()
        .map(|payload| BatchedAppend {
            local: payload,
            remote: None,
            bindings: &[],
        })
        .collect();
    assert_eq!(journal.append_batch(&batch).unwrap(), 3);

    assert!(journal.pending(0, 10).unwrap().is_empty());
    let EventsAfter::Events { events, .. } = journal.events_after(0, 10).unwrap() else {
        panic!("local events must still be replayable")
    };
    assert_eq!(events.len(), 3);
}

#[test]
fn an_empty_batch_is_a_no_op_that_leaves_the_cursor_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = RemoteJournal::open(dir.path().join("remote.db")).unwrap();
    assert_eq!(journal.append_batch(&[]).unwrap(), 0);
}

#[test]
fn event_pages_are_bounded_and_advance_the_returned_cursor() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = RemoteJournal::open(dir.path().join("remote.db")).unwrap();
    for index in 1..=4 {
        journal.append(&json!({"event":index}), &[]).unwrap();
    }
    let EventsAfter::Events { events, cursor } = journal.events_after(0, 2).unwrap() else {
        panic!("fresh cursor unexpectedly required a snapshot")
    };
    assert_eq!(events.len(), 2);
    assert_eq!(cursor, 2);
}
