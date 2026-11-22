use super::*;

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

    pub(crate) fn failure_delay(self, consecutive_failures: u32) -> Duration {
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
    pub(crate) consecutive_failures: u32,
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
    #[serde(skip)]
    pub next_delay: Duration,
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
                next_delay: delay,
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
                next_delay: delay,
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
