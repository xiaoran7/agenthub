pub use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub use anyhow::Context;
pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value, json};

pub use crate::{
    daemon_ipc::DaemonRequestHandler, daemon_protocol::DaemonRequest, domain::AgentType,
    remote_host::RemoteCommand,
};

pub mod types;
pub use types::*;

pub mod server;
pub use server::*;

pub(crate) mod api_router;
#[allow(unused_imports)]
pub(crate) use api_router::*;

pub(crate) mod plugin_routes;
pub(crate) use plugin_routes::*;

pub(crate) mod dashboard;
pub(crate) use dashboard::*;

pub(crate) mod dashboard_summary;
pub(crate) use dashboard_summary::*;

pub(crate) mod device_info;
pub(crate) use device_info::*;

pub(crate) mod stream;
pub(crate) use stream::*;

pub(crate) mod http_io;
pub(crate) use http_io::*;

pub(crate) mod assets;
pub(crate) use assets::*;

#[cfg(test)]
mod tests;
