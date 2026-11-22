//! Automatic discovery and adaptation for third-party Claude Code gateways (e.g. CC Switch, Claude-3p).
//!
//! When a user runs Claude Code via a third-party gateway or router such as CC Switch,
//! the runtime credentials and proxy endpoints are not committed to standard user profiles or
//! global system environment variables. Instead, they are dynamically managed under
//! platform app data roots (`Claude-3p`, `Claude`, `.cc-switch`).
//!
//! Without these environment variables, `claude.exe` defaults to the official Anthropic API
//! and fails with `OAuth session expired`. Furthermore, third-party routers often map specific
//! model IDs (e.g. `claude-fable-5` -> `gemini-3.8-flash-high`); running without an explicit
//! model can cause the router to reject unknown or unmapped models with a 502/400 error.
//!
//! This module automatically detects active gateway environments, extracts valid base URLs and
//! API tokens, maps transcript/provider model IDs to configured router models, and injects
//! them when launching agent CLI subprocesses.

pub use std::{
    collections::HashMap,
    env, fs,
    hash::{Hash, Hasher},
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::Command,
    sync::RwLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub use serde::{Deserialize, Serialize};
pub use serde_json::Value;

pub mod types;
pub use types::*;

pub mod cc_switch;
pub use cc_switch::*;

pub(crate) mod route_parsers;
#[allow(unused_imports)]
pub(crate) use route_parsers::*;

pub mod route_summary;
pub use route_summary::*;

pub mod network;
pub use network::*;

pub mod discovery;
pub use discovery::*;

pub mod config_library;
pub use config_library::*;

#[cfg(test)]
mod route_summary_tests;

#[cfg(test)]
mod tests;
