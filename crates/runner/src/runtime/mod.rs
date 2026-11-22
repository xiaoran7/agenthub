pub use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::{Duration, Instant},
};

pub use chrono::{DateTime, Utc};
pub use serde::Serialize;
pub use thiserror::Error;

pub use crate::{
    adapters::{
        antigravity::AntigravityAdapterError,
        claude::{self, ClaudeAdapterError},
        codex::{self, CodexAdapterError, CodexThreadContext},
    },
    collectors::{
        CodexAppServer, CollectorError, DEFAULT_COLLECTOR_TIMEOUT, collect_antigravity_sessions,
        collect_claude_transcripts, collect_codex_local_threads,
    },
    domain::{ActivityStatus, AgentType, ObservationMeta, Presence, SessionObservation},
    outbox::{Outbox, OutboxError},
    privacy::ProjectRedactor,
};

pub mod sources;
pub use sources::*;

pub mod poll;
pub use poll::*;

pub mod runner;
pub use runner::*;

#[cfg(test)]
mod tests;
