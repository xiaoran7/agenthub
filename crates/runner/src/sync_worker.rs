use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::Serialize;
use thiserror::Error;

use crate::{
    device::DeviceIdentity,
    gateway::{GatewayClient, GatewayError, PushSummary, push_outbox},
    outbox::{Outbox, OutboxError},
};

const STOP_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
pub struct SyncPolicy {
    pub interval: Duration,
    pub failure_initial: Duration,
    pub failure_max: Duration,
}

impl Default for SyncPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5),
            failure_initial: Duration::from_secs(2),
            failure_max: Duration::from_secs(60),
        }
    }
}

impl SyncPolicy {
    fn failure_delay(self, consecutive_failures: u32) -> Duration {
        let exponent = consecutive_failures.saturating_sub(1).min(31);
        self.failure_initial
            .saturating_mul(1_u32 << exponent)
            .min(self.failure_max)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub component: &'static str,
    pub succeeded: bool,
    pub batches_sent: usize,
    pub acknowledged: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_events: Option<u64>,
    pub consecutive_failures: u32,
    pub next_delay_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Error)]
pub enum SyncWorkerJoinError {
    #[error(transparent)]
    Outbox(#[from] OutboxError),
    #[error("background sync thread panicked")]
    Panicked,
}

impl SyncWorkerJoinError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Outbox(error) => error.code(),
            Self::Panicked => "sync_thread_panicked",
        }
    }
}

pub struct SyncWorker {
    trigger: SyncSender<()>,
    handle: JoinHandle<Result<(), OutboxError>>,
}

impl SyncWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        database_path: PathBuf,
        client: GatewayClient,
        identity: DeviceIdentity,
        policy: SyncPolicy,
        limit: usize,
        max_batches: usize,
        stop: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let (trigger, receiver) = sync_channel(1);
        let handle = thread::Builder::new()
            .name("agent-pulse-sync".into())
            .spawn(move || {
                let mut outbox = Outbox::open(database_path)?;
                run_sync_loop(
                    &receiver,
                    &stop,
                    policy,
                    || {
                        push_outbox(&client, &mut outbox, &identity, limit, max_batches).map_err(
                            |error| SyncAttemptFailure {
                                error,
                                pending_events: outbox
                                    .stats()
                                    .ok()
                                    .map(|stats| stats.pending_events),
                            },
                        )
                    },
                    |report| {
                        eprintln!(
                            "{}",
                            serde_json::to_string(report)
                                .expect("SyncReport is always JSON serializable")
                        );
                    },
                );
                Ok(())
            })?;
        Ok(Self { trigger, handle })
    }

    pub fn trigger_sender(&self) -> SyncSender<()> {
        self.trigger.clone()
    }

    pub fn join(self) -> Result<(), SyncWorkerJoinError> {
        self.handle
            .join()
            .map_err(|_| SyncWorkerJoinError::Panicked)??;
        Ok(())
    }
}

struct SyncAttemptFailure {
    error: GatewayError,
    pending_events: Option<u64>,
}

fn run_sync_loop(
    receiver: &Receiver<()>,
    stop: &Arc<AtomicBool>,
    policy: SyncPolicy,
    mut push: impl FnMut() -> Result<PushSummary, SyncAttemptFailure>,
    mut report: impl FnMut(&SyncReport),
) {
    let mut consecutive_failures = 0_u32;
    while !stop.load(Ordering::Relaxed) {
        let (next_delay, backoff) = match push() {
            Ok(summary) if summary.batches_sent == 0 && summary.pending_events > 0 => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let delay = policy.failure_delay(consecutive_failures);
                report(&SyncReport {
                    component: "background_sync",
                    succeeded: false,
                    batches_sent: 0,
                    acknowledged: 0,
                    pending_events: Some(summary.pending_events),
                    consecutive_failures,
                    next_delay_seconds: delay.as_secs(),
                    error_code: Some("sync_pending_device_mismatch".into()),
                });
                (delay, true)
            }
            Ok(summary) => {
                let was_recovering = consecutive_failures > 0;
                consecutive_failures = 0;
                if summary.batches_sent > 0 || summary.pending_events > 0 || was_recovering {
                    report(&SyncReport {
                        component: "background_sync",
                        succeeded: true,
                        batches_sent: summary.batches_sent,
                        acknowledged: summary.acknowledged,
                        pending_events: Some(summary.pending_events),
                        consecutive_failures: 0,
                        next_delay_seconds: policy.interval.as_secs(),
                        error_code: None,
                    });
                }
                (policy.interval, false)
            }
            Err(failure) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let delay = policy.failure_delay(consecutive_failures);
                report(&SyncReport {
                    component: "background_sync",
                    succeeded: false,
                    batches_sent: 0,
                    acknowledged: 0,
                    pending_events: failure.pending_events,
                    consecutive_failures,
                    next_delay_seconds: delay.as_secs(),
                    error_code: Some(failure.error.code().to_owned()),
                });
                (delay, true)
            }
        };
        wait_for_next_attempt(receiver, stop, next_delay, backoff);
    }
}

fn wait_for_next_attempt(
    receiver: &Receiver<()>,
    stop: &Arc<AtomicBool>,
    delay: Duration,
    backoff: bool,
) {
    let deadline = Instant::now() + delay;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        match receiver.recv_timeout(remaining.min(STOP_POLL_INTERVAL)) {
            Ok(()) if !backoff => return,
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::AtomicBool,
        time::Duration,
    };

    use crate::domain::{
        ActivityStatus, AgentType, Confidence, ObservationMeta, ObservationSource, Presence,
        ProjectRef, SessionObservation, SessionState,
    };

    use super::*;

    #[test]
    fn failed_sync_backs_off_and_recovers_without_a_manual_retry() {
        let stop = Arc::new(AtomicBool::new(false));
        let (_trigger, receiver) = sync_channel(1);
        let policy = SyncPolicy {
            interval: Duration::from_millis(1),
            failure_initial: Duration::from_millis(1),
            failure_max: Duration::from_millis(4),
        };
        let mut attempts = 0;
        let mut reports = Vec::new();

        run_sync_loop(
            &receiver,
            &stop,
            policy,
            || {
                attempts += 1;
                if attempts == 1 {
                    Err(SyncAttemptFailure {
                        error: GatewayError::Unreachable,
                        pending_events: Some(2),
                    })
                } else {
                    stop.store(true, Ordering::Relaxed);
                    Ok(PushSummary {
                        batches_sent: 1,
                        acknowledged: 2,
                        pending_events: 0,
                    })
                }
            },
            |report| reports.push(report.clone()),
        );

        assert_eq!(attempts, 2);
        assert_eq!(reports.len(), 2);
        assert!(!reports[0].succeeded);
        assert_eq!(reports[0].consecutive_failures, 1);
        assert_eq!(reports[0].pending_events, Some(2));
        assert_eq!(
            reports[0].error_code.as_deref(),
            Some("gateway_unreachable")
        );
        assert!(reports[1].succeeded);
        assert_eq!(reports[1].acknowledged, 2);
        assert_eq!(reports[1].consecutive_failures, 0);
    }

    #[test]
    fn events_for_another_device_are_reported_and_backed_off() {
        let stop = Arc::new(AtomicBool::new(false));
        let (_trigger, receiver) = sync_channel(1);
        let mut attempts = 0;
        let mut reports = Vec::new();

        run_sync_loop(
            &receiver,
            &stop,
            SyncPolicy {
                interval: Duration::from_millis(1),
                failure_initial: Duration::from_millis(1),
                failure_max: Duration::from_millis(4),
            },
            || {
                attempts += 1;
                if attempts == 1 {
                    Ok(PushSummary {
                        batches_sent: 0,
                        acknowledged: 0,
                        pending_events: 3,
                    })
                } else {
                    stop.store(true, Ordering::Relaxed);
                    Ok(PushSummary {
                        batches_sent: 1,
                        acknowledged: 1,
                        pending_events: 0,
                    })
                }
            },
            |report| reports.push(report.clone()),
        );

        assert_eq!(reports.len(), 2);
        assert!(!reports[0].succeeded);
        assert_eq!(reports[0].pending_events, Some(3));
        assert_eq!(
            reports[0].error_code.as_deref(),
            Some("sync_pending_device_mismatch")
        );
        assert!(reports[1].succeeded);
    }

    #[test]
    fn worker_drains_persisted_outbox_on_startup() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("runner.db");
        let identity = DeviceIdentity::generate();
        Outbox::open(&database_path)
            .unwrap()
            .enqueue(&observation(&identity))
            .unwrap();
        let expected_device_id = identity.device_id().to_owned();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request_json(&mut stream);
            assert_eq!(request["device_id"], expected_device_id);
            let response = serde_json::to_vec(&serde_json::json!({
                "protocol_version": request["protocol_version"],
                "batch_id": request["batch_id"],
                "device_id": request["device_id"],
                "accepted_through": request["last_sequence"]
            }))
            .unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            )
            .unwrap();
            stream.write_all(&response).unwrap();
        });
        let stop = Arc::new(AtomicBool::new(false));
        let client =
            GatewayClient::new(&format!("http://{address}"), Duration::from_secs(2)).unwrap();
        let worker = SyncWorker::start(
            database_path.clone(),
            client,
            identity,
            SyncPolicy {
                interval: Duration::from_millis(10),
                ..SyncPolicy::default()
            },
            100,
            1,
            Arc::clone(&stop),
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        while Outbox::open(&database_path)
            .unwrap()
            .stats()
            .unwrap()
            .pending_events
            != 0
        {
            assert!(Instant::now() < deadline, "background sync did not drain");
            thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn transport_failure_keeps_disk_outbox_and_retries_after_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("runner.db");
        let identity = DeviceIdentity::generate();
        let mut outbox = Some(Outbox::open(&database_path).unwrap());
        outbox
            .as_mut()
            .unwrap()
            .enqueue(&observation(&identity))
            .unwrap();

        let unavailable = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = unavailable.local_addr().unwrap();
        drop(unavailable);
        let client =
            GatewayClient::new(&format!("http://{address}"), Duration::from_millis(250)).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let (_trigger, receiver) = sync_channel(1);
        let mut attempts = 0;
        let mut server = None;
        let mut reports = Vec::new();

        run_sync_loop(
            &receiver,
            &stop,
            SyncPolicy {
                interval: Duration::from_millis(1),
                failure_initial: Duration::from_millis(1),
                failure_max: Duration::from_millis(4),
            },
            || {
                attempts += 1;
                let result = push_outbox(&client, outbox.as_mut().unwrap(), &identity, 100, 1);
                if attempts == 1 {
                    assert!(matches!(
                        result,
                        Err(GatewayError::Unreachable | GatewayError::Timeout)
                    ));
                    assert_eq!(outbox.as_ref().unwrap().stats().unwrap().pending_events, 1);
                    drop(outbox.take());
                    outbox = Some(Outbox::open(&database_path).unwrap());

                    let listener = TcpListener::bind(address).unwrap();
                    let expected_device_id = identity.device_id().to_owned();
                    server = Some(thread::spawn(move || {
                        let (mut stream, _) = listener.accept().unwrap();
                        let request = read_request_json(&mut stream);
                        assert_eq!(request["device_id"], expected_device_id);
                        let response = serde_json::to_vec(&serde_json::json!({
                            "protocol_version": request["protocol_version"],
                            "batch_id": request["batch_id"],
                            "device_id": request["device_id"],
                            "accepted_through": request["last_sequence"]
                        }))
                        .unwrap();
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            response.len()
                        )
                        .unwrap();
                        stream.write_all(&response).unwrap();
                    }));
                } else if result.is_ok() {
                    stop.store(true, Ordering::Relaxed);
                }
                result.map_err(|error| SyncAttemptFailure {
                    error,
                    pending_events: outbox
                        .as_ref()
                        .and_then(|outbox| outbox.stats().ok())
                        .map(|stats| stats.pending_events),
                })
            },
            |report| reports.push(report.clone()),
        );

        server.unwrap().join().unwrap();
        drop(outbox);
        assert_eq!(attempts, 2);
        assert_eq!(reports.len(), 2);
        assert!(!reports[0].succeeded);
        assert!(reports[1].succeeded);
        assert_eq!(reports[1].acknowledged, 1);
        assert_eq!(
            Outbox::open(&database_path)
                .unwrap()
                .stats()
                .unwrap()
                .pending_events,
            0
        );
    }

    fn observation(identity: &DeviceIdentity) -> SessionObservation {
        SessionObservation::new(
            &ObservationMeta::now(identity.device_id()),
            AgentType::Codex,
            "background-session",
            ProjectRef {
                display_name: "safe-project".into(),
                fingerprint: "a".repeat(64),
            },
            SessionState {
                presence: Presence::Live,
                activity_status: ActivityStatus::Working,
                raw_status: "active".into(),
                raw_status_detail: BTreeMap::new(),
                waiting_reason: None,
            },
            ObservationSource::CodexAppServer,
            Confidence::Authoritative,
        )
    }

    fn read_request_json(stream: &mut impl Read) -> serde_json::Value {
        let mut request = Vec::new();
        let body_start = loop {
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..body_start]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while request.len() < body_start + content_length {
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        serde_json::from_slice(&request[body_start..body_start + content_length]).unwrap()
    }
}
