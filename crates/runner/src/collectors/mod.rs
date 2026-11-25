use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub mod acp_app_server;
pub use acp_app_server::*;
pub mod antigravity;
pub use antigravity::*;
pub mod capabilities;
pub mod claude;
pub use claude::*;
pub mod claude_app_server;
pub use claude_app_server::*;
pub mod claude_context;
pub(crate) use claude_context::*;
pub mod claude_metadata;
pub(crate) use claude_metadata::*;
pub mod claude_prompt;
pub(crate) use claude_prompt::*;
pub mod claude_subagent;
pub(crate) use claude_subagent::*;
pub mod claude_trash;
pub use claude_trash::*;
pub mod codex;
pub use codex::*;
pub mod codex_context;
pub use codex_context::*;
pub mod codex_enrich;
pub use codex_enrich::*;
pub mod codex_trash;
pub use codex_trash::*;
pub mod codex_app_server;
pub use codex_app_server::*;
pub mod common;
pub use common::*;
pub mod context_usage;
pub use context_usage::*;
pub mod execution;
pub(crate) use crate::lru_cache;
pub(crate) use crate::lru_cache::*;
pub use execution::*;
pub mod process;
pub(crate) use process::*;
pub mod provider_binary;
pub use provider_binary::*;
pub mod stream_parser;
pub(crate) use stream_parser::*;
pub mod workspace;

#[cfg(test)]
mod tests;

const MAX_PROTOCOL_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(15);
const CHILD_WAIT_INTERVAL: Duration = Duration::from_millis(10);
const CLAUDE_RECENT_SESSION_LIMIT: usize = 1_000;
const CLAUDE_ALL_SESSION_LIMIT: usize = 10_000;
const MAX_AGENT_TURN_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const LOCAL_CONTEXT_HEAD_BYTES: u64 = 512 * 1024;
const LOCAL_CONTEXT_TAIL_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SESSION_TITLE_CHARS: usize = 160;
const MAX_CONFIG_VALUE_CHARS: usize = 96;
/// Upper bound on a single `--version` probe. Discovery runs on the daemon
/// startup path, so a wedged binary must cost one short timeout, not a hang.
const BINARY_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("failed to start {program}: {source}")]
    Start {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} exited unsuccessfully with status {status}")]
    Exit { program: String, status: String },
    /// The CLI ran but could not read the requested session, which is what an
    /// older build does when the transcript was written by a newer one.
    #[error("{program} could not read the requested session")]
    SessionUnreadable { program: String },
    #[error("failed to communicate with {program}: {source}")]
    Io {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} did not expose the expected stdio handles")]
    MissingStdio { program: String },
    #[error("invalid JSON from {program}: {source}")]
    InvalidJson {
        program: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{program} returned output that was not UTF-8: {source}")]
    InvalidUtf8 {
        program: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("{program} protocol line exceeded {limit} bytes")]
    ProtocolLineTooLarge { program: String, limit: usize },
    #[error("{program} output exceeded {limit} bytes")]
    OutputTooLarge { program: String, limit: usize },
    #[error("{program} was cancelled")]
    Cancelled { program: String },
    #[error("{program} did not respond within {timeout_ms} ms")]
    Timeout { program: String, timeout_ms: u128 },
    #[error("{program} output reader stopped unexpectedly")]
    ReaderStopped { program: String },
    #[error("{program} closed stdout before responding to request {request_id}")]
    UnexpectedEof { program: String, request_id: u64 },
    #[error("{program} returned protocol error {code}: {message}")]
    Protocol {
        program: String,
        code: String,
        message: String,
    },
    #[error("failed to read {program}: {source}")]
    Database {
        program: String,
        #[source]
        source: rusqlite::Error,
    },
}

impl CollectorError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Start { source, .. } => match source.kind() {
                std::io::ErrorKind::NotFound => "executable_not_found",
                std::io::ErrorKind::PermissionDenied => "permission_denied",
                _ => "process_start_failed",
            },
            Self::Exit { .. } => "process_exit_failed",
            Self::SessionUnreadable { .. } => "session_unreadable",
            Self::Io { .. } => "protocol_io_failed",
            Self::MissingStdio { .. } => "protocol_stdio_missing",
            Self::InvalidJson { .. } => "protocol_invalid_json",
            Self::InvalidUtf8 { .. } => "process_invalid_utf8",
            Self::ProtocolLineTooLarge { .. } => "protocol_line_too_large",
            Self::OutputTooLarge { .. } => "process_output_too_large",
            Self::Cancelled { .. } => "process_cancelled",
            Self::Timeout { .. } => "collector_timeout",
            Self::ReaderStopped { .. } => "collector_reader_stopped",
            Self::UnexpectedEof { .. } => "protocol_unexpected_eof",
            Self::Protocol { .. } => "protocol_error",
            Self::Database { .. } => "local_store_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnUpdate {
    AssistantDelta(String),
    AssistantMessage(String),
    Activity(&'static str),
}

/// Per-turn overrides the desktop panel can attach to a resume or launch.
#[derive(Debug, Clone, Default)]
pub struct TurnOptions<'a> {
    pub model: Option<&'a str>,
    pub permission_mode: Option<&'a str>,
    pub reasoning_effort: Option<&'a str>,
    pub session_exists: Option<bool>,
    pub claude_desktop: bool,
}

/// Outcome of a streamed agent process: final message plus the session id the
/// CLI reported for freshly started conversations.
#[derive(Debug, Clone)]
pub struct AgentStreamOutcome {
    pub response: String,
    pub session_id: Option<String>,
    pub usage: Option<ContextUsage>,
}

/// How full the provider's context window is after a turn.
///
/// `window` stays optional on purpose: a provider that reports consumption but
/// not capacity must still be shown as "used, capacity unknown" rather than
/// being given a guessed denominator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsage {
    pub used: u64,
    pub window: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalSessionContext {
    pub agent_type: crate::domain::AgentType,
    pub agent_session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_used_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_ended: Option<bool>,
}

impl LocalSessionContext {
    pub fn new(agent_type: crate::domain::AgentType, agent_session_id: String) -> Self {
        Self {
            agent_type,
            agent_session_id,
            title: None,
            model: None,
            reasoning_effort: None,
            permission_mode: None,
            approval_policy: None,
            sandbox_mode: None,
            context_window_tokens: None,
            context_used_tokens: None,
            session_total_tokens: None,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            stop_reason: None,
            turn_ended: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaudeTranscriptEntry {
    pub session_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub started_at_ms: Option<i64>,
    pub last_activity_at_ms: Option<i64>,
    /// False when the transcript holds no assistant output at all. A session
    /// the user abandoned before the first reply still persists a transcript and
    /// a `custom-title.json`, so it otherwise looks identical to a real one in
    /// the sidebar while Claude Code's own `/resume` list hides it.
    pub has_output: bool,
    pub model: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    pub context_used_tokens: u64,
    pub context_window_tokens: u64,
    pub request_count: u64,
    /// True when the transcript was written by Claude Desktop rather than the
    /// Coding Assistant CLI. Both write to the same store; each line's
    /// `entrypoint` is the only reliable discriminator.
    pub desktop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeRemoteSessionEntry {
    pub session_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub last_activity_at_ms: i64,
    pub model: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub context_used_tokens: u64,
    #[serde(default)]
    pub context_window_tokens: u64,
    pub request_count: u64,
    /// Written by Claude Desktop, not the Coding Assistant CLI.
    #[serde(default)]
    pub desktop: bool,
    /// The subagents the conversation spawned: their own transcripts beside
    /// its file. A session is listed once, its subagents folded into it, the
    /// way magpie groups a session's files under the conversation that asked
    /// for them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subagents: Vec<ClaudeSubagentEntry>,
}

/// One subagent of a conversation: what its transcript says it was, how much
/// it spent, and which one started it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeSubagentEntry {
    /// The `agentId` its lines carry and its file is named by.
    pub id: String,
    /// What it was for: the `agentType` of its meta file (`Explore`,
    /// `general-purpose`, …), else empty.
    #[serde(default)]
    pub kind: String,
    /// The task it was given, in its meta file's own words.
    #[serde(default)]
    pub description: String,
    /// The subagent that started it, empty for one the conversation did.
    #[serde(default)]
    pub parent: String,
    /// 1 for one the conversation started, 2 for one a subagent did.
    #[serde(default)]
    pub depth: u32,
    pub last_activity_at_ms: i64,
    pub model: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    pub request_count: u64,
}

///
/// The scanners are fingerprint-driven, so a manual refresh needs no cache
/// to be cleared for correctness; clearing it only makes the next pass read
/// every transcript instead of trusting a `(len, mtime)` it has already seen.
pub fn invalidate_session_caches_for_refresh() {
    if let Ok(mut guard) = transcript_metadata_cache().write() {
        guard.clear();
    }
    antigravity::clear_antigravity_transcript_cache();
    antigravity::invalidate_antigravity_daemon_caches();
    antigravity::invalidate_antigravity_models_cache();
    clear_codex_enrichment_cache();
    if let Ok(mut guard) = codex_threads_cache().write() {
        *guard = None;
    }
    forget_store_snapshots();
}
