use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::Serialize;
use thiserror::Error;

use crate::{
    auth::{AUTH_SYNC_PROTOCOL_VERSION, AuthManager, AuthProvider, AuthSyncBatch},
    device::DeviceIdentity,
    gateway::{GatewayClient, GatewayError},
};

const STOP_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthReport {
    pub component: &'static str,
    pub succeeded: bool,
    pub snapshots_sent: usize,
    pub consecutive_failures: u32,
    pub next_delay_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Debug, Error)]
pub enum AuthWorkerJoinError {
    #[error("background auth thread panicked")]
    Panicked,
}

pub struct AuthWorker {
    trigger: SyncSender<()>,
    handle: JoinHandle<()>,
}

impl AuthWorker {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        client: GatewayClient,
        identity: DeviceIdentity,
        cache_path: Option<std::path::PathBuf>,
        claude_binary: std::path::PathBuf,
        codex_binary: std::path::PathBuf,
        interval: Duration,
        timeout: Duration,
        stop: Arc<AtomicBool>,
    ) -> std::io::Result<Self> {
        let (trigger, receiver) = sync_channel(1);
        let handle = thread::Builder::new()
            .name("agent-pulse-auth".into())
            .spawn(move || {
                let mut manager = match AuthManager::from_identity(
                    Some(&identity),
                    cache_path,
                    true,
                    claude_binary,
                    codex_binary,
                    timeout,
                ) {
                    Ok(manager) => manager,
                    Err(error) => {
                        report(AuthReport {
                            component: "background_auth",
                            succeeded: false,
                            snapshots_sent: 0,
                            consecutive_failures: 1,
                            next_delay_seconds: interval.as_secs(),
                            error_code: Some(error.code().to_owned()),
                        });
                        return;
                    }
                };
                run_auth_loop(&receiver, &stop, interval, &client, &identity, &mut manager);
            })?;
        Ok(Self { trigger, handle })
    }

    pub fn trigger_sender(&self) -> SyncSender<()> {
        self.trigger.clone()
    }

    pub fn join(self) -> Result<(), AuthWorkerJoinError> {
        self.handle
            .join()
            .map_err(|_| AuthWorkerJoinError::Panicked)
    }
}

fn run_auth_loop(
    receiver: &std::sync::mpsc::Receiver<()>,
    stop: &Arc<AtomicBool>,
    interval: Duration,
    client: &GatewayClient,
    identity: &DeviceIdentity,
    manager: &mut AuthManager,
) {
    let mut consecutive_failures = 0_u32;
    while !stop.load(Ordering::Relaxed) {
        let attempt = collect_and_upload(client, identity, manager);
        let delay = match attempt {
            Ok(sent) => {
                let recovering = consecutive_failures > 0;
                consecutive_failures = 0;
                if sent > 0 || recovering {
                    report(AuthReport {
                        component: "background_auth",
                        succeeded: true,
                        snapshots_sent: sent,
                        consecutive_failures: 0,
                        next_delay_seconds: interval.as_secs(),
                        error_code: None,
                    });
                }
                interval
            }
            Err(error) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let delay = interval
                    .saturating_mul(1_u32 << consecutive_failures.saturating_sub(1).min(4))
                    .min(Duration::from_secs(300));
                report(AuthReport {
                    component: "background_auth",
                    succeeded: false,
                    snapshots_sent: 0,
                    consecutive_failures,
                    next_delay_seconds: delay.as_secs(),
                    error_code: Some(error.code().to_owned()),
                });
                delay
            }
        };
        wait_for_next_attempt(receiver, stop, delay);
    }
}

fn collect_and_upload(
    client: &GatewayClient,
    identity: &DeviceIdentity,
    manager: &mut AuthManager,
) -> Result<usize, AuthWorkerError> {
    let mut snapshots = Vec::with_capacity(2);
    for provider in [AuthProvider::ClaudeCode, AuthProvider::Codex] {
        snapshots.push(manager.status(provider)?);
    }
    if snapshots.is_empty() {
        return Ok(0);
    }
    let batch = AuthSyncBatch::new(identity, snapshots)?;
    if batch.protocol_version != AUTH_SYNC_PROTOCOL_VERSION {
        return Err(AuthWorkerError::InvalidProtocol);
    }
    Ok(client.upload_auth(&batch)?.accepted)
}

#[derive(Debug, Error)]
enum AuthWorkerError {
    #[error(transparent)]
    Auth(#[from] crate::auth::AuthError),
    #[error(transparent)]
    Gateway(#[from] GatewayError),
    #[error("invalid auth protocol version")]
    InvalidProtocol,
}

impl AuthWorkerError {
    fn code(&self) -> &'static str {
        match self {
            Self::Auth(error) => error.code(),
            Self::Gateway(error) => error.code(),
            Self::InvalidProtocol => "auth_protocol_invalid",
        }
    }
}

fn wait_for_next_attempt(
    receiver: &std::sync::mpsc::Receiver<()>,
    stop: &Arc<AtomicBool>,
    delay: Duration,
) {
    let deadline = std::time::Instant::now() + delay;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return;
        }
        match receiver.recv_timeout(remaining.min(STOP_POLL_INTERVAL)) {
            Ok(()) => return,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn report(report: AuthReport) {
    eprintln!(
        "{}",
        serde_json::to_string(&report).expect("AuthReport is always JSON serializable")
    );
}
