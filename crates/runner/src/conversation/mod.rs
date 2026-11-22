//! Bounded read-only session conversation loading.
//!
//! This is the single transcript reader in the codebase: `agenthubd` owns it
//! and Desktop consumes it over local IPC. Discovery metadata stays in
//! `collectors`; this module only serves conversation previews for sessions
//! that can be resumed or are already managed.

pub use crate::domain::AgentType;
pub use serde_json::{Value, json};
pub use std::collections::HashMap;
pub use std::sync::{OnceLock, RwLock};
pub use std::time::{Duration, Instant, SystemTime};
pub use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub mod antigravity;
pub mod codex;

pub mod index;
pub use index::*;

pub mod loader;
pub use loader::*;

pub mod scanner;
pub use scanner::*;

pub mod parser;
pub use parser::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../conversation_regression.rs"]
mod regression;
