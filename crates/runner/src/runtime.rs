use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;

use crate::{
    adapters::{
        claude::{self, ClaudeAdapterError},
        codex::{self, CodexAdapterError, CodexThreadContext},
    },
    collectors::{CodexAppServer, CollectorError, collect_claude_agents},
    domain::{ActivityStatus, AgentType, ObservationMeta, Presence, SessionObservation},
    outbox::{Outbox, OutboxError},
    privacy::ProjectRedactor,
};

const LOOP_SLEEP_CAP: Duration = Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum SourceError {
    #[error(transparent)]
    Collector(#[from] CollectorError),
    #[error(transparent)]
    ClaudeAdapter(#[from] ClaudeAdapterError),
    #[error(transparent)]
    CodexAdapter(#[from] CodexAdapterError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl SourceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Collector(error) => error.code(),
            Self::ClaudeAdapter(_) => "claude_payload_invalid",
            Self::CodexAdapter(_) => "codex_payload_invalid",
            Self::Json(_) => "internal_json_failed",
        }
    }
}

pub trait SessionSource {
    fn agent_type(&self) -> AgentType;

    fn collect(
        &mut self,
        meta: &ObservationMeta,
        redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError>;
}

pub struct ClaudeSessionSource {
    binary: PathBuf,
    include_completed: bool,
}

impl ClaudeSessionSource {
    pub fn new(binary: impl AsRef<Path>, include_completed: bool) -> Self {
        Self {
            binary: binary.as_ref().to_owned(),
            include_completed,
        }
    }
}

impl SessionSource for ClaudeSessionSource {
    fn agent_type(&self) -> AgentType {
        AgentType::ClaudeCode
    }

    fn collect(
        &mut self,
        meta: &ObservationMeta,
        redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError> {
        let payload = collect_claude_agents(&self.binary, self.include_completed)?;
        Ok(claude::normalize_agents_json(&payload, meta, redactor)?)
    }
}

pub struct CodexSessionSource {
    binary: PathBuf,
    limit: u32,
    server: Option<CodexAppServer>,
    context: CodexThreadContext,
}

impl CodexSessionSource {
    pub fn new(binary: impl AsRef<Path>, limit: u32) -> Self {
        Self {
            binary: binary.as_ref().to_owned(),
            limit,
            server: None,
            context: CodexThreadContext::default(),
        }
    }

    fn collect_result(&mut self) -> Result<serde_json::Value, CollectorError> {
        if self.server.is_none() {
            self.server = Some(CodexAppServer::start(&self.binary)?);
        }
        let result = self
            .server
            .as_mut()
            .expect("Codex app-server was initialized")
            .list_threads(self.limit);
        if result.is_err() {
            self.server = None;
        }
        result
    }
}

impl SessionSource for CodexSessionSource {
    fn agent_type(&self) -> AgentType {
        AgentType::Codex
    }

    fn collect(
        &mut self,
        meta: &ObservationMeta,
        redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError> {
        let payload = serde_json::to_string(&self.collect_result()?)?;
        Ok(codex::normalize_thread_list_result(
            &payload,
            meta,
            redactor,
            &mut self.context,
        )?)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PollPolicy {
    pub active_interval: Duration,
    pub idle_interval: Duration,
    pub failure_initial: Duration,
    pub failure_max: Duration,
}

impl Default for PollPolicy {
    fn default() -> Self {
        Self {
            active_interval: Duration::from_secs(2),
            idle_interval: Duration::from_secs(10),
            failure_initial: Duration::from_secs(2),
            failure_max: Duration::from_secs(60),
        }
    }
}

impl PollPolicy {
    fn success_delay(self, observations: &[SessionObservation]) -> Duration {
        if observations.iter().any(is_active) {
            self.active_interval
        } else {
            self.idle_interval
        }
    }

    fn failure_delay(self, consecutive_failures: u32) -> Duration {
        let exponent = consecutive_failures.saturating_sub(1).min(31);
        self.failure_initial
            .saturating_mul(1_u32 << exponent)
            .min(self.failure_max)
    }
}

fn is_active(observation: &SessionObservation) -> bool {
    observation.state.presence == Presence::Live
        && matches!(
            observation.state.activity_status,
            ActivityStatus::Working
                | ActivityStatus::WaitingInput
                | ActivityStatus::WaitingPermission
        )
}

#[derive(Debug, Default)]
pub struct PollState {
    consecutive_failures: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PollReport {
    pub agent_type: AgentType,
    pub succeeded: bool,
    pub observations: usize,
    pub enqueued: usize,
    pub consecutive_failures: u32,
    pub next_attempt_at: DateTime<Utc>,
    pub next_delay_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

pub fn poll_source_once(
    source: &mut dyn SessionSource,
    device_id: &str,
    redactor: &ProjectRedactor,
    outbox: &mut Outbox,
    state: &mut PollState,
    policy: PollPolicy,
    attempted_at: DateTime<Utc>,
) -> Result<PollReport, OutboxError> {
    let meta = ObservationMeta {
        device_id: device_id.to_owned(),
        observed_at: attempted_at,
    };
    match source.collect(&meta, redactor) {
        Ok(observations) => {
            let delay = policy.success_delay(&observations);
            let next_attempt_at = add_std_duration(attempted_at, delay);
            let mut enqueued = 0;
            for observation in &observations {
                if outbox.enqueue_if_changed(observation)?.is_some() {
                    enqueued += 1;
                }
            }
            state.consecutive_failures = 0;
            outbox.record_adapter_health(
                source.agent_type(),
                attempted_at,
                true,
                0,
                next_attempt_at,
                None,
            )?;
            Ok(PollReport {
                agent_type: source.agent_type(),
                succeeded: true,
                observations: observations.len(),
                enqueued,
                consecutive_failures: 0,
                next_attempt_at,
                next_delay_seconds: delay.as_secs(),
                error_code: None,
            })
        }
        Err(error) => {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            let delay = policy.failure_delay(state.consecutive_failures);
            let next_attempt_at = add_std_duration(attempted_at, delay);
            let error_code = error.code();
            outbox.record_adapter_health(
                source.agent_type(),
                attempted_at,
                false,
                state.consecutive_failures,
                next_attempt_at,
                Some(error_code),
            )?;
            Ok(PollReport {
                agent_type: source.agent_type(),
                succeeded: false,
                observations: 0,
                enqueued: 0,
                consecutive_failures: state.consecutive_failures,
                next_attempt_at,
                next_delay_seconds: delay.as_secs(),
                error_code: Some(error_code.to_owned()),
            })
        }
    }
}

fn add_std_duration(timestamp: DateTime<Utc>, duration: Duration) -> DateTime<Utc> {
    chrono::Duration::from_std(duration)
        .map(|duration| timestamp + duration)
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

struct ManagedSource {
    source: Box<dyn SessionSource>,
    state: PollState,
    next_attempt: Instant,
}

pub struct RunnerRuntime {
    device_id: String,
    redactor: ProjectRedactor,
    outbox: Outbox,
    policy: PollPolicy,
    sources: Vec<ManagedSource>,
}

impl RunnerRuntime {
    pub fn new(
        device_id: impl Into<String>,
        redactor: ProjectRedactor,
        outbox: Outbox,
        policy: PollPolicy,
        sources: Vec<Box<dyn SessionSource>>,
    ) -> Self {
        let now = Instant::now();
        Self {
            device_id: device_id.into(),
            redactor,
            outbox,
            policy,
            sources: sources
                .into_iter()
                .map(|source| ManagedSource {
                    source,
                    state: PollState::default(),
                    next_attempt: now,
                })
                .collect(),
        }
    }

    pub fn run(
        &mut self,
        stop: Arc<AtomicBool>,
        max_attempts: Option<u64>,
    ) -> Result<(), OutboxError> {
        let mut attempts = 0_u64;
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            let mut attempted = false;
            for managed in &mut self.sources {
                if now < managed.next_attempt {
                    continue;
                }
                let report = poll_source_once(
                    managed.source.as_mut(),
                    &self.device_id,
                    &self.redactor,
                    &mut self.outbox,
                    &mut managed.state,
                    self.policy,
                    Utc::now(),
                )?;
                managed.next_attempt =
                    Instant::now() + Duration::from_secs(report.next_delay_seconds);
                eprintln!(
                    "{}",
                    serde_json::to_string(&report).expect("PollReport is always JSON serializable")
                );
                attempts = attempts.saturating_add(1);
                attempted = true;
                if max_attempts.is_some_and(|maximum| attempts >= maximum) {
                    return Ok(());
                }
            }

            if !attempted {
                let sleep_for = self
                    .sources
                    .iter()
                    .map(|managed| {
                        managed
                            .next_attempt
                            .saturating_duration_since(Instant::now())
                    })
                    .min()
                    .unwrap_or(LOOP_SLEEP_CAP)
                    .min(LOOP_SLEEP_CAP);
                thread::sleep(sleep_for);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::{Confidence, ObservationSource, ProjectRef, SessionState};

    use super::*;

    struct FakeSource {
        agent_type: AgentType,
        result: Option<Result<Vec<SessionObservation>, SourceError>>,
    }

    impl SessionSource for FakeSource {
        fn agent_type(&self) -> AgentType {
            self.agent_type
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
            serde_json::to_string(&outbox.adapter_health(AgentType::Codex).unwrap().unwrap())
                .unwrap();
        assert!(!serialized.contains("secret-path"));
        assert!(!serialized.contains("private operating system message"));
    }
}
