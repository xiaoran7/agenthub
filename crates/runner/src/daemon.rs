use crate::{
    daemon_ipc::{self, DaemonRequestHandler},
    daemon_protocol::{DaemonRequest, DaemonResponse},
    device::DeviceIdentity,
    remote_network::{
        LocalHostClient, RemoteControl, create_local_host_channel, run_with_local_control,
    },
};
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering, mpsc},
    thread,
    time::Duration,
};

pub struct DaemonConfig {
    pub codex_binary: PathBuf,
    pub workspaces: Vec<PathBuf>,
    pub database: PathBuf,
    pub identity: DeviceIdentity,
    pub server: Option<String>,
    pub ipc_endpoint: Option<String>,
}

#[derive(Clone)]
struct LocalHandler {
    client: LocalHostClient,
    identity: DeviceIdentity,
    control: Arc<RemoteControl>,
}

impl DaemonRequestHandler for LocalHandler {
    fn handle(&self, request: DaemonRequest) -> DaemonResponse {
        if !self.control.ready.load(Ordering::Acquire) {
            return DaemonResponse::Error {
                code: "daemon_starting".into(),
            };
        }
        match request {
            DaemonRequest::GetSnapshot => self
                .client
                .snapshot()
                .map(DaemonResponse::Snapshot)
                .unwrap_or_else(error_response),
            DaemonRequest::GetEvents {
                after_cursor,
                limit,
            } => self
                .client
                .events_after(after_cursor, limit)
                .map(DaemonResponse::Events)
                .unwrap_or_else(error_response),
            DaemonRequest::Subscribe { .. } => DaemonResponse::Error {
                code: "invalid_subscription_dispatch".into(),
            },
            DaemonRequest::Execute { command } => self
                .client
                .execute(command)
                .map(DaemonResponse::Receipt)
                .unwrap_or_else(error_response),
            DaemonRequest::AuthorizeWorkspace { workspace } => self
                .client
                .authorize_workspace(workspace)
                .map(DaemonResponse::Workspace)
                .unwrap_or_else(error_response),
            DaemonRequest::SetRemoteEnabled { server_url } => self
                .client
                .set_remote_enabled(server_url, self.identity.clone())
                .and_then(|enabled| {
                    self.client
                        .status()
                        .map(|(_, _, _, connected)| (enabled, connected))
                })
                .map(|(enabled, connected)| DaemonResponse::RemoteState { enabled, connected })
                .unwrap_or_else(error_response),
            DaemonRequest::GetStatus => self
                .client
                .status()
                .map(
                    |(host_generation, workspaces, remote_enabled, remote_connected)| {
                        DaemonResponse::Status {
                            host_generation,
                            workspaces,
                            remote_enabled,
                            remote_connected,
                        }
                    },
                )
                .unwrap_or_else(error_response),
            DaemonRequest::GetModels => self
                .client
                .query_models()
                .map(DaemonResponse::Models)
                .unwrap_or_else(error_response),
            DaemonRequest::Discover => self
                .client
                .discover()
                .map(DaemonResponse::Discovery)
                .unwrap_or_else(error_response),
        }
    }
}

pub fn run(config: DaemonConfig) -> Result<()> {
    let endpoint = config
        .ipc_endpoint
        .unwrap_or_else(|| daemon_ipc::default_endpoint(config.identity.device_id()));
    let control = Arc::new(RemoteControl::default());
    let ipc_stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal = control.clone();
    let ipc_signal = ipc_stopped.clone();
    ctrlc::set_handler(move || {
        signal.stopped.store(true, Ordering::Relaxed);
        ipc_signal.store(true, Ordering::Relaxed);
    })?;

    let (client, channel) = create_local_host_channel();
    let ipc_control = ipc_stopped.clone();
    let ipc_handler: Arc<dyn DaemonRequestHandler> = Arc::new(LocalHandler {
        client,
        identity: config.identity.clone(),
        control: control.clone(),
    });
    let ipc_endpoint = endpoint.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let ipc_thread = thread::Builder::new()
        .name("agenthubd-ipc".into())
        .spawn(move || {
            daemon_ipc::serve_with_ready(&ipc_endpoint, ipc_handler, ipc_control, ready_tx)
        })
        .context("ipc_thread_start_failed")?;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {}
        Ok(Err(code)) => {
            ipc_stopped.store(true, Ordering::Relaxed);
            let _ = ipc_thread.join();
            return Err(anyhow::anyhow!(code));
        }
        Err(_) => {
            ipc_stopped.store(true, Ordering::Relaxed);
            return Err(anyhow::anyhow!("ipc_start_timeout"));
        }
    }

    let result = run_with_local_control(
        config.codex_binary,
        config.workspaces,
        config.database,
        config.identity,
        config.server,
        control.clone(),
        Some(channel),
    );
    control.stopped.store(true, Ordering::Relaxed);
    ipc_stopped.store(true, Ordering::Relaxed);
    #[cfg(windows)]
    let _ = daemon_ipc::request(
        &endpoint,
        &crate::daemon_protocol::DaemonRequestEnvelope {
            protocol_version: crate::daemon_protocol::DAEMON_PROTOCOL_VERSION,
            request_id: "daemon-shutdown".into(),
            request: DaemonRequest::GetSnapshot,
        },
    );
    ipc_thread
        .join()
        .map_err(|_| anyhow::anyhow!("ipc_thread_panicked"))??;
    result
}

fn error_response(code: String) -> DaemonResponse {
    DaemonResponse::Error { code }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_ready_status_reports_starting_without_touching_the_host_queue() {
        let (client, _channel) = create_local_host_channel();
        let handler = LocalHandler {
            client,
            identity: DeviceIdentity::generate(),
            control: Arc::new(RemoteControl::default()),
        };

        assert!(matches!(
            handler.handle(DaemonRequest::GetStatus),
            DaemonResponse::Error { code } if code == "daemon_starting"
        ));
    }
}
