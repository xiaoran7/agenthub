use super::*;
use std::{collections::BTreeMap, sync::mpsc::sync_channel};

use crate::domain::{Confidence, ObservationSource, ProjectRef, SessionState};

struct FakeSource {
    agent_type: AgentType,
    result: Option<Result<Vec<SessionObservation>, SourceError>>,
}

impl SessionSource for FakeSource {
    fn agent_type(&self) -> AgentType {
        self.agent_type.clone()
    }

    fn collect(
        &mut self,
        _meta: &ObservationMeta,
        _redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError> {
        self.result.take().expect("fake source called once")
    }
}

fn observation(meta: &ObservationMeta, status: ActivityStatus) -> SessionObservation {
    SessionObservation::new(
        meta,
        AgentType::ClaudeCode,
        "session-1",
        ProjectRef {
            display_name: "demo".into(),
            fingerprint: "fingerprint".into(),
        },
        SessionState {
            presence: Presence::Live,
            activity_status: status,
            raw_status: "fixture".into(),
            raw_status_detail: BTreeMap::new(),
            waiting_reason: None,
        },
        ObservationSource::ClaudeAgentsJson,
        Confidence::Authoritative,
    )
}

#[test]
fn active_sessions_use_fast_poll_interval() {
    let attempted_at = Utc::now();
    let meta = ObservationMeta {
        device_id: "device-1".into(),
        observed_at: attempted_at,
    };
    let mut source = FakeSource {
        agent_type: AgentType::ClaudeCode,
        result: Some(Ok(vec![observation(&meta, ActivityStatus::Working)])),
    };
    let mut outbox = Outbox::open_in_memory().unwrap();
    let mut state = PollState::default();

    let report = poll_source_once(
        &mut source,
        "device-1",
        &ProjectRedactor::new(b"secret").unwrap(),
        &mut outbox,
        &mut state,
        PollPolicy::default(),
        attempted_at,
    )
    .unwrap();

    assert!(report.succeeded);
    assert_eq!(report.next_delay_seconds, 2);
    assert_eq!(report.enqueued, 1);
}

#[test]
fn polling_remains_healthy_when_hooks_are_not_installed() {
    let attempted_at = Utc::now();
    let meta = ObservationMeta {
        device_id: "device-without-hooks".into(),
        observed_at: attempted_at,
    };
    let mut source = FakeSource {
        agent_type: AgentType::ClaudeCode,
        result: Some(Ok(vec![observation(&meta, ActivityStatus::Idle)])),
    };
    let mut outbox = Outbox::open_in_memory().unwrap();
    let mut state = PollState::default();

    let report = poll_source_once(
        &mut source,
        "device-without-hooks",
        &ProjectRedactor::new(b"secret").unwrap(),
        &mut outbox,
        &mut state,
        PollPolicy::default(),
        attempted_at,
    )
    .unwrap();

    assert!(report.succeeded);
    assert_eq!(report.enqueued, 1);
    assert_eq!(outbox.stats().unwrap().pending_events, 1);
    assert_eq!(
        outbox
            .adapter_health(AgentType::ClaudeCode)
            .unwrap()
            .unwrap()
            .status,
        "healthy"
    );
}

#[test]
fn newly_enqueued_observations_wake_background_sync() {
    let meta = ObservationMeta::now("device-1");
    let source = FakeSource {
        agent_type: AgentType::Codex,
        result: Some(Ok(vec![observation(&meta, ActivityStatus::Working)])),
    };
    let (sync_trigger, receiver) = sync_channel(1);
    let mut runtime = RunnerRuntime::new(
        "device-1",
        ProjectRedactor::new(b"secret").unwrap(),
        Outbox::open_in_memory().unwrap(),
        PollPolicy::default(),
        vec![Box::new(source)],
    )
    .with_sync_trigger(sync_trigger);

    runtime
        .run(Arc::new(AtomicBool::new(false)), Some(1))
        .unwrap();

    receiver
        .try_recv()
        .expect("a newly persisted observation must wake the sync worker");
}

#[test]
fn failures_back_off_and_store_only_safe_error_codes() {
    let attempted_at = Utc::now();
    let policy = PollPolicy::default();
    let mut state = PollState::default();
    let mut outbox = Outbox::open_in_memory().unwrap();

    for expected_delay in [2, 4, 8] {
        let mut source = FakeSource {
            agent_type: AgentType::Codex,
            result: Some(Err(SourceError::Collector(CollectorError::Start {
                program: "secret-path".into(),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "private operating system message",
                ),
            }))),
        };
        let report = poll_source_once(
            &mut source,
            "device-1",
            &ProjectRedactor::new(b"secret").unwrap(),
            &mut outbox,
            &mut state,
            policy,
            attempted_at,
        )
        .unwrap();
        assert_eq!(report.next_delay_seconds, expected_delay);
        assert_eq!(report.error_code.as_deref(), Some("permission_denied"));
    }

    let serialized =
        serde_json::to_string(&outbox.adapter_health(AgentType::Codex).unwrap().unwrap()).unwrap();
    assert!(!serialized.contains("secret-path"));
    assert!(!serialized.contains("private operating system message"));
}

#[test]
fn codex_passive_messages_override_polled_status_without_leaking_content() {
    let response: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/codex-thread-list.json")).unwrap();
    let result = response.get("result").unwrap();
    let approval =
        serde_json::from_str(include_str!("../../fixtures/codex-approval.json")).unwrap();
    let meta = ObservationMeta::now("device-1");
    let mut context = CodexThreadContext::default();

    let observations = normalize_codex_collection(
        result,
        vec![approval],
        &meta,
        &ProjectRedactor::new(b"secret").unwrap(),
        &mut context,
        false,
    )
    .unwrap();

    let latest = observations
        .iter()
        .rev()
        .find(|observation| observation.agent_session_id == "codex-active")
        .unwrap();
    assert_eq!(
        latest.state.activity_status,
        ActivityStatus::WaitingPermission
    );
    let serialized = serde_json::to_string(&observations).unwrap();
    assert!(!serialized.contains("TOP_SECRET_MARKER"));
    assert!(!serialized.contains("PRIVATE_APPROVAL_REASON_MARKER"));
    assert!(!serialized.contains("PRIVATE_CODEX_PREVIEW_MARKER"));
}
