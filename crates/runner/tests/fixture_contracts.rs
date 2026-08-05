use std::{fs, path::PathBuf};

use agent_pulse_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
    },
    device::DeviceIdentity,
    domain::{
        ActivityStatus, Confidence, ObservationMeta, Presence, SessionObservation, WaitingReason,
    },
    outbox::Outbox,
    privacy::ProjectRedactor,
    sync::prepare_sync_batch,
};
use chrono::{TimeZone, Utc};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read_to_string(path).unwrap()
}

fn meta() -> ObservationMeta {
    ObservationMeta {
        device_id: "fixture-device".into(),
        observed_at: Utc.timestamp_opt(1_784_941_800, 0).unwrap(),
    }
}

fn redactor() -> ProjectRedactor {
    ProjectRedactor::new(b"fixture-device-key").unwrap()
}

fn assert_no_sensitive_payload(observation: &SessionObservation) {
    let serialized = serde_json::to_string(observation).unwrap();
    for forbidden in [
        "TOP_SECRET_MARKER",
        "PRIVATE_PROMPT_MARKER",
        "PRIVATE_CODEX_PREVIEW_MARKER",
        "PRIVATE_CODEX_TITLE_MARKER",
        "PRIVATE_CLAUDE_NAME_MARKER",
        "PRIVATE_APPROVAL_REASON_MARKER",
        "PRIVATE_ASSISTANT_MESSAGE_MARKER",
        "Users\\\\alice",
        "Users\\alice",
        "FUTURE_PRIVATE_",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized observation leaked {forbidden}"
        );
    }
}

#[test]
fn future_agent_payloads_keep_known_states_and_degrade_unknown_states() {
    let claude =
        claude::normalize_agents_json(&fixture("claude-agents-future.json"), &meta(), &redactor())
            .unwrap();
    assert_eq!(claude.len(), 2);
    assert_eq!(
        claude[0].state.activity_status,
        ActivityStatus::Working,
        "new fields must not break a known status"
    );
    assert_eq!(claude[1].state.activity_status, ActivityStatus::Unknown);
    assert_eq!(claude[1].confidence, Confidence::Inferred);

    let mut context = CodexThreadContext::default();
    let codex = codex::normalize_thread_list_result(
        &fixture("codex-thread-list-future.json"),
        &meta(),
        &redactor(),
        &mut context,
    )
    .unwrap();
    assert_eq!(codex.len(), 2);
    assert_eq!(codex[0].state.activity_status, ActivityStatus::Idle);
    assert_eq!(codex[1].state.activity_status, ActivityStatus::Unknown);
    assert_eq!(codex[1].confidence, Confidence::Inferred);

    claude
        .iter()
        .chain(codex.iter())
        .for_each(assert_no_sensitive_payload);
}

#[test]
fn unknown_hooks_degrade_and_malformed_hooks_are_rejected() {
    let claude_input = r#"{
      "session_id": "claude-new-hook",
      "cwd": "C:\\Users\\alice\\source\\future-hook",
      "hook_event_name": "FutureApprovalWorkflow",
      "prompt": "FUTURE_PRIVATE_CLAUDE_PROMPT"
    }"#;
    let claude = claude::normalize_hook(claude_input, &meta(), &redactor()).unwrap();
    assert_eq!(claude.state.activity_status, ActivityStatus::Unknown);
    assert_eq!(claude.confidence, Confidence::Inferred);
    assert_no_sensitive_payload(&claude);

    let codex_input = r#"{
      "session_id": "codex-new-hook",
      "cwd": "C:\\Users\\alice\\source\\future-hook",
      "hook_event_name": "FutureApprovalWorkflow",
      "command": "FUTURE_PRIVATE_CODEX_COMMAND"
    }"#;
    let codex = codex::normalize_hook(codex_input, &meta(), &redactor()).unwrap();
    assert_eq!(codex.state.activity_status, ActivityStatus::Unknown);
    assert_eq!(codex.confidence, Confidence::Inferred);
    assert_no_sensitive_payload(&codex);

    let session_end = claude::normalize_hook(
        r#"{"session_id":"ended","hook_event_name":"SessionEnd"}"#,
        &meta(),
        &redactor(),
    )
    .unwrap();
    assert_eq!(session_end.state.presence, Presence::Persisted);
    assert_eq!(session_end.confidence, Confidence::Observed);

    assert!(claude::normalize_hook(r#"{"hook_event_name":"Stop"}"#, &meta(), &redactor()).is_err());
    assert!(codex::normalize_hook(r#"{"hook_event_name":"Stop"}"#, &meta(), &redactor()).is_err());
}

#[test]
fn private_fixture_content_never_reaches_a_signed_batch() {
    let identity = DeviceIdentity::generate();
    let identity_meta = ObservationMeta {
        device_id: identity.device_id().to_owned(),
        observed_at: meta().observed_at,
    };
    let mut observations =
        claude::normalize_agents_json(&fixture("claude-agents.json"), &identity_meta, &redactor())
            .unwrap();
    let mut context = CodexThreadContext::default();
    observations.extend(
        codex::normalize_thread_list_result(
            &fixture("codex-thread-list.json"),
            &identity_meta,
            &redactor(),
            &mut context,
        )
        .unwrap(),
    );

    let mut outbox = Outbox::open_in_memory().unwrap();
    for observation in &observations {
        outbox.enqueue(observation).unwrap();
    }
    let batch = prepare_sync_batch(&outbox, &identity, 100)
        .unwrap()
        .unwrap();
    batch.verify().unwrap();
    let serialized = serde_json::to_string(&batch).unwrap();
    for forbidden in [
        "TOP_SECRET_MARKER",
        "PRIVATE_PROMPT_MARKER",
        "PRIVATE_CODEX_PREVIEW_MARKER",
        "PRIVATE_CODEX_TITLE_MARKER",
        "PRIVATE_CLAUDE_NAME_MARKER",
        "Users\\\\alice",
        r#""prompt":"#,
        r#""transcript":"#,
        r#""command":"#,
        r#""tool_input":"#,
    ] {
        assert!(
            !serialized.contains(forbidden),
            "signed batch leaked {forbidden}"
        );
    }
}

#[test]
fn claude_agents_fixture_maps_all_primary_states() {
    let observations =
        claude::normalize_agents_json(&fixture("claude-agents.json"), &meta(), &redactor())
            .unwrap();

    // The fixture contains five entries; the one without a session id is
    // skipped instead of failing the whole collection.
    assert_eq!(observations.len(), 4);
    assert!(observations.iter().any(|item| {
        item.agent_session_id == "claude-working"
            && item.state.activity_status == ActivityStatus::Working
    }));
    assert!(observations.iter().any(|item| {
        item.agent_session_id == "claude-permission"
            && item.state.activity_status == ActivityStatus::WaitingPermission
            && item.state.waiting_reason == Some(WaitingReason::Permission)
    }));
    assert!(observations.iter().any(|item| {
        item.agent_session_id == "claude-input"
            && item.state.activity_status == ActivityStatus::WaitingInput
    }));
    assert!(observations.iter().any(|item| {
        item.agent_session_id == "claude-done"
            && item.state.activity_status == ActivityStatus::Completed
            && item.state.presence == Presence::Persisted
    }));
    observations.iter().for_each(assert_no_sensitive_payload);
}

#[test]
fn hook_and_approval_fixtures_drop_unapproved_content() {
    let claude = claude::normalize_hook(
        &fixture("claude-hook-permission.json"),
        &meta(),
        &redactor(),
    )
    .unwrap();
    let mut context = CodexThreadContext::default();
    let codex = codex::normalize_app_server_message(
        &fixture("codex-approval.json"),
        &meta(),
        &redactor(),
        &mut context,
    )
    .unwrap()
    .unwrap();
    let codex_hook =
        codex::normalize_hook(&fixture("codex-hook-stop.json"), &meta(), &redactor()).unwrap();

    assert_eq!(
        claude.state.activity_status,
        ActivityStatus::WaitingPermission
    );
    assert_eq!(
        codex.state.activity_status,
        ActivityStatus::WaitingPermission
    );
    assert_eq!(codex_hook.state.activity_status, ActivityStatus::Completed);
    assert_no_sensitive_payload(&claude);
    assert_no_sensitive_payload(&codex);
    assert_no_sensitive_payload(&codex_hook);
}

#[test]
fn codex_thread_list_does_not_upload_preview() {
    let mut context = CodexThreadContext::default();
    let observations = codex::normalize_thread_list_result(
        &fixture("codex-thread-list.json"),
        &meta(),
        &redactor(),
        &mut context,
    )
    .unwrap();

    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].state.presence, Presence::Live);
    assert_eq!(
        observations[0].state.activity_status,
        ActivityStatus::Working
    );
    assert_eq!(observations[1].state.presence, Presence::Unknown);
    observations.iter().for_each(assert_no_sensitive_payload);
}

#[test]
fn normalized_fixtures_round_trip_through_outbox() {
    let mut outbox = Outbox::open_in_memory().unwrap();
    let observations =
        claude::normalize_agents_json(&fixture("claude-agents.json"), &meta(), &redactor())
            .unwrap();

    for observation in &observations {
        outbox.enqueue(observation).unwrap();
    }

    let pending = outbox.pending("fixture-device", 100).unwrap();
    assert_eq!(pending.len(), observations.len());
    for record in pending {
        assert_no_sensitive_payload(&record.observation);
    }
}
