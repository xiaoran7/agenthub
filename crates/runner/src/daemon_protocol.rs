use crate::{
    remote_host::{CommandReceipt, RemoteCommand},
    remote_journal::{EventsAfter, JournalSnapshot},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub const DAEMON_PROTOCOL_VERSION: u16 = 2;
pub const MAX_DAEMON_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonRequestEnvelope {
    pub protocol_version: u16,
    pub request_id: String,
    #[serde(flatten)]
    pub request: DaemonRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum DaemonRequest {
    GetSnapshot,
    GetEvents { after_cursor: i64, limit: usize },
    Subscribe { after_cursor: i64 },
    Execute { command: RemoteCommand },
    AuthorizeWorkspace { workspace: PathBuf },
    SetRemoteEnabled { server_url: Option<String> },
    GetStatus,
    GetModels,
    Discover,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonResponseEnvelope {
    pub protocol_version: u16,
    pub request_id: String,
    #[serde(flatten)]
    pub response: DaemonResponse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "result", rename_all = "snake_case")]
pub enum DaemonResponse {
    Snapshot(JournalSnapshot),
    Events(EventsAfter),
    Receipt(CommandReceipt),
    Workspace(PathBuf),
    RemoteState {
        enabled: bool,
        connected: bool,
    },
    Status {
        host_generation: String,
        workspaces: Vec<PathBuf>,
        remote_enabled: bool,
        remote_connected: bool,
    },
    Models(Value),
    Discovery(Value),
    Error {
        code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonEventEnvelope {
    pub protocol_version: u16,
    pub request_id: String,
    pub cursor: i64,
    pub event: Value,
}

impl DaemonResponseEnvelope {
    pub fn success(request_id: impl Into<String>, response: DaemonResponse) -> Self {
        Self {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            request_id: request_id.into(),
            response,
        }
    }

    pub fn error(request_id: impl Into<String>, code: impl Into<String>) -> Self {
        Self::success(request_id, DaemonResponse::Error { code: code.into() })
    }
}

impl DaemonEventEnvelope {
    pub fn new(request_id: impl Into<String>, cursor: i64, event: Value) -> Self {
        Self {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            request_id: request_id.into(),
            cursor,
            event,
        }
    }
}

pub fn validate_request(request: &DaemonRequestEnvelope) -> Result<(), &'static str> {
    if request.protocol_version != DAEMON_PROTOCOL_VERSION {
        return Err("unsupported_protocol_version");
    }
    if request.request_id.is_empty() || request.request_id.len() > 128 {
        return Err("invalid_request_id");
    }
    Ok(())
}
