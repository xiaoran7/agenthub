//! Per-request usage ledger.
//!
//! The console's usage page needs more than session totals: it plots a trend,
//! a yearly heatmap and a ranked breakdown, and it lists individual requests.
//! Those all need one record per model call, with its time, agent and model.
//!
//! Coding Assistant's transcripts are the one store on this machine that
//! records every call with a timestamp, a model and its usage, so they are the
//! ledger's backbone. A reply is written as several assistant lines (one per
//! content block), each repeating the message's usage; the last line of a
//! message carries the final figures, so records are keyed by `message.id`
//! plus `requestId` and the latest write wins — the same rule magpie's
//! `sessions.calls` applies.
//!
//! Claude Desktop is not Coding Assistant: both write to the same
//! `~/.claude/projects` tree, and the only reliable discriminator is each
//! line's `entrypoint` (`claude-desktop`, `claude-desktop-3p`). Those calls
//! are attributed to `claude_desktop`; everything else is `claude_code`.
//!
//! ZCode and Codex are folded in from their own stores, so the page's total
//! is the whole machine's usage rather than Coding Assistant's alone. ZCode's
//! `model_usage` table is one row per call, complete with prompt, cache,
//! completion and both timestamps; its older sessions, which predate that
//! table, are read from their assistant messages instead. Codex writes no
//! per-call rows at all — only a running total per session — so each rollout
//! contributes one record holding its final `token_count`.

pub use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, RwLock},
    time::{Duration, Instant, UNIX_EPOCH},
};

pub use chrono::DateTime;
pub use serde::{Deserialize, Serialize};

pub use crate::collectors::CollectorError;
pub(crate) use crate::collectors::home_dir;

pub mod types;
pub use types::*;

pub mod cache;
pub use cache::*;

pub mod collector_codex;
pub(crate) use collector_codex::*;

pub mod collector_extra;
pub(crate) use collector_extra::*;

pub mod collector_magpie;
pub(crate) use collector_magpie::*;

pub mod collector_claude;
pub(crate) use collector_claude::*;

pub(crate) mod index_db;
pub(crate) use index_db::*;

pub mod digest;
pub use digest::*;

#[cfg(test)]
mod tests;
