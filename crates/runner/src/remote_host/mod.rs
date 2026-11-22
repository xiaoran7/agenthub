//! Native session control core. This is local IPC, not the legacy v2 wire format.
//! A Host owns one long-lived app-server and never resumes arbitrary live processes.

pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value, json};
pub use sha2::{Digest, Sha256};
pub use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub use crate::collectors::{
    AcpAppServer, AntigravityProcessSession, ClaudeProcessSession, CodexAppServer,
};
pub use crate::domain::AgentType;

pub mod local_store_scanner;
pub use local_store_scanner::*;
pub mod multi_drain;
pub mod multi_drain_antigravity;
#[cfg(test)]
mod multi_drain_antigravity_tests;
pub mod multi_drain_claude;
pub mod multi_request;
pub mod multi_request_acp;
pub mod multi_request_antigravity;
pub mod multi_request_claude;
pub mod multi_request_plugin;
pub mod multi_transport;
pub use multi_transport::*;
pub mod session_host;
pub use session_host::*;
mod session_control;
pub mod session_host_exec;
pub mod session_host_poll;
pub mod transport_adapter;
pub mod types;
pub use types::*;
