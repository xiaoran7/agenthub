use agent_pulse_runner::{
    daemon_protocol::*,
    remote_host::{
        CommandReceipt, ExecutionState, PendingApprovalSnapshot, RemoteAction, RemoteCommand,
        SessionBinding,
    },
    remote_journal::{EventsAfter, JournalSnapshot},
};
use serde_json::json;
use std::path::PathBuf;

fn command() -> RemoteCommand {
    RemoteCommand {
        client_cmd_id: "cmd-1".into(),
        hub_session_id: "hub-1".into(),
        host_generation: Some("generation-1".into()),
        command: RemoteAction::SendPrompt {
            text: "continue".into(),
            effort: None,
        },
    }
}

#[test]
fn request_envelope_roundtrips_remote_command_without_another_command_model() {
    let envelope = DaemonRequestEnvelope {
        protocol_version: DAEMON_PROTOCOL_VERSION,
        request_id: "request-1".into(),
        request: DaemonRequest::Execute { command: command() },
    };
    let encoded = serde_json::to_value(&envelope).unwrap();
    assert_eq!(encoded["protocol_version"], 2);
    assert_eq!(encoded["request_id"], "request-1");
    assert_eq!(encoded["method"], "execute");
    assert_eq!(
        encoded["params"]["command"]["command"]["action"],
        "send_prompt"
    );
    assert_eq!(
        serde_json::from_value::<DaemonRequestEnvelope>(encoded).unwrap(),
        envelope
    );
}

#[test]
fn response_and_event_envelopes_preserve_request_identity_and_cursor() {
    let binding = SessionBinding {
        hub_session_id: "hub-1".into(),
        provider_session_id: "native-1".into(),
        workspace_path: PathBuf::from("workspace-id"),
        host_generation: "generation-1".into(),
        control_connected: true,
        execution_state: ExecutionState::Idle,
        current_turn_id: None,
    };
    let response = DaemonResponseEnvelope::success(
        "request-2",
        DaemonResponse::Snapshot(JournalSnapshot {
            sessions: vec![binding.clone()],
            pending_approvals: vec![PendingApprovalSnapshot {
                approval_id: "approval-1".into(),
                hub_session_id: "hub-1".into(),
                host_generation: "generation-1".into(),
                turn_id: "turn-1".into(),
                can_approve: true,
                can_decline: true,
                tool_name: Some("Bash".into()),
                command_preview: Some("cargo test".into()),
                path: None,
                description: None,
                risk_level: Some("high".into()),
            }],
            pending_questions: Vec::new(),
            cursor: 9,
        }),
    );
    let encoded = serde_json::to_value(&response).unwrap();
    assert_eq!(encoded["protocol_version"], DAEMON_PROTOCOL_VERSION);
    assert_eq!(encoded["request_id"], "request-2");
    assert_eq!(encoded["type"], "snapshot");
    assert_eq!(encoded["result"]["sessions"][0]["hub_session_id"], "hub-1");
    assert_eq!(
        encoded["result"]["pending_approvals"][0]["approval_id"],
        "approval-1"
    );
    assert!(
        encoded["result"]["pending_approvals"][0]
            .get("request_id")
            .is_none()
    );

    let event = DaemonEventEnvelope::new("request-2", 10, json!({"type": "event"}));
    assert_eq!(event.protocol_version, DAEMON_PROTOCOL_VERSION);
    assert_eq!(event.request_id, "request-2");
    assert_eq!(event.cursor, 10);

    let receipt = CommandReceipt {
        client_cmd_id: "cmd-1".into(),
        hub_session_id: binding.hub_session_id,
        status: "accepted".into(),
        code: None,
        result: None,
    };
    let receipt_response =
        DaemonResponseEnvelope::success("request-3", DaemonResponse::Receipt(receipt.clone()));
    let decoded: DaemonResponseEnvelope =
        serde_json::from_slice(&serde_json::to_vec(&receipt_response).unwrap()).unwrap();
    assert_eq!(decoded, receipt_response);
}

#[test]
fn control_surface_roundtrips_remote_toggle_status_and_archive() {
    let disable = DaemonRequestEnvelope {
        protocol_version: DAEMON_PROTOCOL_VERSION,
        request_id: "request-disable".into(),
        request: DaemonRequest::SetRemoteEnabled { server_url: None },
    };
    let encoded = serde_json::to_value(&disable).unwrap();
    assert_eq!(encoded["method"], "set_remote_enabled");
    assert!(encoded["params"]["server_url"].is_null());
    assert_eq!(
        serde_json::from_value::<DaemonRequestEnvelope>(encoded).unwrap(),
        disable
    );

    let status = DaemonResponseEnvelope::success(
        "request-status",
        DaemonResponse::Status {
            host_generation: "generation-2".into(),
            workspaces: vec![PathBuf::from("workspace-id")],
            remote_enabled: false,
            remote_connected: false,
        },
    );
    let decoded: DaemonResponseEnvelope =
        serde_json::from_slice(&serde_json::to_vec(&status).unwrap()).unwrap();
    assert_eq!(decoded, status);

    let mut archive_command = command();
    archive_command.command = RemoteAction::SetArchived { archived: true };
    assert_eq!(
        serde_json::to_value(archive_command).unwrap()["command"]["action"],
        "set_archived"
    );
}

#[test]
fn unsupported_versions_invalid_ids_and_unknown_fields_are_rejected() {
    let mut request = DaemonRequestEnvelope {
        protocol_version: DAEMON_PROTOCOL_VERSION + 1,
        request_id: "request-1".into(),
        request: DaemonRequest::GetEvents {
            after_cursor: 0,
            limit: 10,
        },
    };
    assert_eq!(
        validate_request(&request),
        Err("unsupported_protocol_version")
    );
    request.protocol_version = 1;
    assert_eq!(
        validate_request(&request),
        Err("unsupported_protocol_version")
    );
    request.protocol_version = DAEMON_PROTOCOL_VERSION;
    request.request_id.clear();
    assert_eq!(validate_request(&request), Err("invalid_request_id"));

    let invalid = json!({
        "protocol_version": DAEMON_PROTOCOL_VERSION,
        "request_id": "request-1",
        "method": "get_snapshot",
        "unexpected": true
    });
    assert!(serde_json::from_value::<DaemonRequestEnvelope>(invalid).is_err());
}

#[test]
fn snapshot_required_is_an_explicit_versioned_response() {
    let response = DaemonResponseEnvelope::success(
        "request-4",
        DaemonResponse::Events(EventsAfter::SnapshotRequired {
            snapshot: JournalSnapshot {
                sessions: vec![],
                pending_approvals: vec![],
                pending_questions: vec![],
                cursor: 42,
            },
        }),
    );
    let encoded = serde_json::to_value(response).unwrap();
    assert_eq!(encoded["type"], "events");
    assert_eq!(encoded["result"]["status"], "snapshot_required");
    assert_eq!(encoded["result"]["snapshot"]["cursor"], 42);
}
