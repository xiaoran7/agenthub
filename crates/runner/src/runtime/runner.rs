use super::*;

const LOOP_SLEEP_CAP: Duration = Duration::from_millis(250);

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
    sync_trigger: Option<SyncSender<()>>,
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
            sync_trigger: None,
        }
    }

    pub fn with_sync_trigger(mut self, sync_trigger: SyncSender<()>) -> Self {
        self.sync_trigger = Some(sync_trigger);
        self
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
                match poll_source_once(
                    managed.source.as_mut(),
                    &self.device_id,
                    &self.redactor,
                    &mut self.outbox,
                    &mut managed.state,
                    self.policy,
                    Utc::now(),
                ) {
                    Ok(report) => {
                        managed.next_attempt = Instant::now() + report.next_delay;
                        if report.enqueued > 0
                            && let Some(sync_trigger) = &self.sync_trigger
                        {
                            let _ = sync_trigger.try_send(());
                        }
                        eprintln!(
                            "{}",
                            serde_json::to_string(&report)
                                .expect("PollReport is always JSON serializable")
                        );
                    }
                    Err(error) => {
                        // A transient local database failure (for example
                        // SQLITE_BUSY while a hook process writes) must not
                        // kill the monitoring daemon; back off and retry.
                        managed.state.consecutive_failures =
                            managed.state.consecutive_failures.saturating_add(1);
                        let delay = self
                            .policy
                            .failure_delay(managed.state.consecutive_failures);
                        managed.next_attempt = Instant::now() + delay;
                        eprintln!(
                            "{}",
                            serde_json::json!({
                                "agent_type": managed.source.agent_type(),
                                "succeeded": false,
                                "consecutive_failures": managed.state.consecutive_failures,
                                "next_delay_seconds": delay.as_secs(),
                                "error_code": error.code(),
                            })
                        );
                    }
                }
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
