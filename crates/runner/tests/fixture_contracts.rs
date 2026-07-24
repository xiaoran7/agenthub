use std::{fs, path::PathBuf};

use agent_platform_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
    },
    domain::{ActivityStatus, ObservationMeta, Presence, SessionObservation, WaitingReason},
    outbox::Outbox,
    privacy::ProjectRedactor,
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
        "PRIVATE_APPROVAL_REASON_MARKER",
        "PRIVATE_ASSISTANT_MESSAGE_MARKER",
        "Users\\\\alice",
        "Users\\alice",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "serialized observation leaked {forbidden}"
        );
    }
}

#[test]
fn claude_agents_fixture_maps_all_primary_states() {
    let observations =
        claude::normalize_agents_json(&fixture("claude-agents.json"), &meta(), &redactor())
            .unwrap();

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
