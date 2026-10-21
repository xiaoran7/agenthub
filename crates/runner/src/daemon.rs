use crate::{
    conversation,
    daemon_ipc::{self, DaemonRequestHandler},
    daemon_protocol::{DaemonRequest, DaemonResponse},
    device::DeviceIdentity,
    domain::AgentType,
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
    pub enable_web: bool,
    pub web_port: Option<u16>,
    pub open_browser: bool,
    /// Human-readable name for this computer, shown in the console.
    pub device_name: crate::device_name::DeviceNameSource,
    /// The identity file this Runner was started with.
    pub identity_path: PathBuf,
}

#[derive(Clone)]
struct LocalHandler {
    client: LocalHostClient,
    identity: DeviceIdentity,
    control: Arc<RemoteControl>,
}

impl DaemonRequestHandler for LocalHandler {
    fn handle(&self, request: DaemonRequest) -> DaemonResponse {
        match request {
            // Conversation previews read local provider transcripts directly and
            // do not need the host channel, so they stay available while starting.
            DaemonRequest::GetSessionConversation {
                agent_type,
                session_id,
            } => session_conversation_response(agent_type, &session_id),
            _ if !self.control.ready.load(Ordering::Acquire) => DaemonResponse::Error {
                code: "daemon_starting".into(),
            },
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
    // Warm up local transcript scanning immediately in the background so
    // sessions across Claude, Codex, Grok, etc. are ready concurrently.
    crate::remote_host::warm_up_local_store_scan();
    let ipc_control = ipc_stopped.clone();
    let ipc_handler: Arc<dyn DaemonRequestHandler> = Arc::new(LocalHandler {
        client,
        identity: config.identity.clone(),
        control: control.clone(),
    });
    let ipc_endpoint = endpoint.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let ipc_handler_thread = ipc_handler.clone();
    let ipc_thread = thread::Builder::new()
        .name("agenthubd-ipc".into())
        .spawn(move || {
            daemon_ipc::serve_with_ready(&ipc_endpoint, ipc_handler_thread, ipc_control, ready_tx)
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

    if config.enable_web {
        let web_cfg = crate::web_server::WebServerConfig {
            port: config.web_port.unwrap_or(4200),
            open_browser: config.open_browser,
            identity_path: config.identity_path.clone(),
        };
        let _ = crate::web_server::serve(web_cfg, ipc_handler.clone(), ipc_stopped.clone());
    }

    let result = run_with_local_control(
        config.codex_binary,
        config.workspaces,
        config.database,
        config.identity,
        config.server,
        control.clone(),
        Some(channel),
        &config.device_name,
    );
    control.stopped.store(true, Ordering::Relaxed);
    ipc_stopped.store(true, Ordering::Relaxed);
    crate::tray::cleanup_desktop_window();
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

fn session_conversation_response(agent_type: AgentType, session_id: &str) -> DaemonResponse {
    if conversation::validate_session_id(session_id).is_err() {
        return error_response("invalid_session_id".into());
    }
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return DaemonResponse::SessionConversation(Vec::new());
    };
    DaemonResponse::SessionConversation(conversation::load_session_conversation(
        &home, agent_type, session_id,
    ))
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
