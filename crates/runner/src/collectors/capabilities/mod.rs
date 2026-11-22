//! Capability & Extension ecosystem collector (Skills, MCP Servers, Lifecycle Hooks)
//!
//! Inspired by upstream magpie's unified Library architecture:
//! Discovers and manages skills, Model Context Protocol (MCP) servers, and
//! lifecycle execution hooks across official runtimes (Claude Code, OpenAI Codex,
//! and AgentHub workspaces).

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::UNIX_EPOCH,
};

pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value, json};

pub mod types;
pub use types::*;

pub mod antigravity;
pub mod discovery;
pub use discovery::*;

pub mod parsers;
pub use parsers::*;

#[cfg(test)]
mod tests;
