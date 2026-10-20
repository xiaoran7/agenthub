use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::DateTime;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

#[cfg(test)]
use std::sync::mpsc;

pub mod capabilities;
pub mod workspace;

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

/// Reads context consumption out of a provider `usage` object. Cache reads and
/// cache writes occupy the window just like fresh input, so all three count.
pub(crate) fn context_usage_from_values(
    usage: &Value,
    window: Option<u64>,
) -> Option<ContextUsage> {
    let input = usage_u64(usage, "input_tokens");
    let cache_read = usage_u64(usage, "cache_read_input_tokens");
    let cache_write = usage_u64(usage, "cache_creation_input_tokens");
    let used = input
        .saturating_add(cache_read)
        .saturating_add(cache_write)
        .max(usage_u64(usage, "total_tokens"));
    if used == 0 {
        return None;
    }
    let reported_window = window
        .or_else(|| usage.get("context_window_tokens").and_then(Value::as_u64))
        .or_else(|| usage.get("model_context_window").and_then(Value::as_u64))
        .filter(|value| *value > 0);
    Some(ContextUsage {
        used,
        window: reported_window,
    })
}

/// Context usage carried by a Claude `result` line (`usage` plus an optional
/// context window on the same message).
pub(crate) fn claude_result_context_usage(value: &Value) -> Option<ContextUsage> {
    if value.get("type").and_then(Value::as_str) != Some("result") {
        return None;
    }
    let usage = value.get("usage")?;
    let window = value
        .get("model_context_window")
        .and_then(Value::as_u64)
        .or_else(|| value.get("modelContextWindow").and_then(Value::as_u64));
    context_usage_from_values(usage, window)
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeTranscriptMetadata {
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
    title: Option<String>,
    ai_title: Option<String>,
    custom_title: Option<String>,
    last_prompt: Option<String>,
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

/// Enumerates Claude Code's documented local transcript store without invoking
/// the CLI or retaining message/tool content. Claude Code does not expose an
/// `agents` listing subcommand; lifecycle hooks are the authoritative live
/// signal while this scanner supplies persisted history.
pub fn collect_claude_transcripts(include_completed: bool) -> Result<String, CollectorError> {
    let config_dir = discover_claude_config_dir()?;
    collect_claude_transcripts_from(&config_dir, include_completed)
}

pub fn collect_claude_transcripts_from(
    config_dir: &Path,
    include_completed: bool,
) -> Result<String, CollectorError> {
    let projects_dir = config_dir.join("projects");
    if !projects_dir.is_dir() {
        return Ok("[]".to_owned());
    }

    let program = "Claude transcript store".to_owned();
    let mut entries = Vec::new();
    for project in fs::read_dir(&projects_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })? {
        let project = project.map_err(|source| CollectorError::Io {
            program: program.clone(),
            source,
        })?;
        if !project
            .file_type()
            .map_err(|source| CollectorError::Io {
                program: program.clone(),
                source,
            })?
            .is_dir()
        {
            continue;
        }
        let files = match fs::read_dir(project.path()) {
            Ok(files) => files,
            Err(_) => continue,
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
                || !file.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            if let Some(entry) = read_claude_transcript_metadata(&path, &program)?
                && entry.has_output
            {
                entries.push(entry);
            }
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.last_activity_at_ms));
    entries.truncate(if include_completed {
        CLAUDE_ALL_SESSION_LIMIT
    } else {
        CLAUDE_RECENT_SESSION_LIMIT
    });
    serde_json::to_string(
        &entries
            .into_iter()
            .map(|entry| {
                json!({
                    "sessionId": entry.session_id,
                    "cwd": entry.cwd,
                    "title": entry.title,
                    "kind": "transcript",
                    "state": "persisted",
                    "startedAt": entry.started_at_ms,
                    "lastActivityAt": entry.last_activity_at_ms
                })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|source| CollectorError::InvalidJson { program, source })
}

/// Reads only explicit, bounded session metadata for the desktop detail view.
/// Titles prefer provider-owned title events and fall back to a bounded local
/// summary of the first meaningful user request. They remain separate from
/// SessionObservation so neither the title nor prompt enters the sync outbox.
pub fn collect_claude_session_contexts(
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let config_dir = discover_claude_config_dir()?;
    collect_claude_session_contexts_from(&config_dir, limit)
}

pub fn collect_claude_session_contexts_from(
    config_dir: &Path,
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let projects_dir = config_dir.join("projects");
    if !projects_dir.is_dir() {
        return Ok(Vec::new());
    }
    let program = "Claude transcript context".to_owned();
    let mut candidates = Vec::new();
    for project in fs::read_dir(&projects_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })? {
        let Ok(project) = project else { continue };
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
                || !file.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            let modified = file
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or_default();
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    candidates.truncate(limit);

    let mut contexts = Vec::with_capacity(candidates.len());
    for (_, path) in candidates {
        if let Some(context) = read_claude_session_context(&path, &program)? {
            contexts.push(context);
        }
    }
    Ok(contexts)
}

fn read_claude_session_context(
    path: &Path,
    program: &str,
) -> Result<Option<LocalSessionContext>, CollectorError> {
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?
        .len();
    let fallback_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS));
    let mut context = LocalSessionContext::new(
        crate::domain::AgentType::ClaudeCode,
        fallback_id.unwrap_or_default(),
    );
    let mut reader = BufReader::new(file);
    scan_claude_context_region(
        &mut reader,
        LOCAL_CONTEXT_HEAD_BYTES.min(length),
        &mut context,
        program,
    )?;
    if length > LOCAL_CONTEXT_HEAD_BYTES {
        let tail_start = length.saturating_sub(LOCAL_CONTEXT_TAIL_BYTES);
        reader
            .seek(SeekFrom::Start(tail_start))
            .map_err(|source| CollectorError::Io {
                program: program.to_owned(),
                source,
            })?;
        scan_claude_context_region(
            &mut reader,
            length.saturating_sub(tail_start),
            &mut context,
            program,
        )?;
    }
    if context.agent_session_id.is_empty() {
        Ok(None)
    } else {
        Ok(Some(context))
    }
}

fn scan_claude_context_region(
    reader: &mut impl BufRead,
    byte_limit: u64,
    context: &mut LocalSessionContext,
    program: &str,
) -> Result<(), CollectorError> {
    let mut consumed = 0_u64;
    while consumed < byte_limit {
        match read_spliced_line(reader) {
            SplicedLine::Line {
                text,
                bytes_consumed,
            } => {
                consumed = consumed.saturating_add(bytes_consumed);
                apply_claude_context_line(&text, context);
            }
            SplicedLine::Oversized {
                head,
                tail,
                bytes_consumed,
            } => {
                consumed = consumed.saturating_add(bytes_consumed);
                apply_oversized_claude_record(&head, &tail, context);
            }
            SplicedLine::Eof => break,
            SplicedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: program.to_owned(),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn apply_claude_context_line(line: &str, context: &mut LocalSessionContext) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if context.agent_session_id.is_empty()
        && let Some(session_id) = value
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
    {
        context.agent_session_id = session_id;
    }
    match value.get("type").and_then(Value::as_str) {
        Some("custom-title") => {
            context.title = ["customTitle", "title", "name"]
                .into_iter()
                .find_map(|field| value.get(field).and_then(Value::as_str))
                .and_then(|value| metadata_label(value, MAX_SESSION_TITLE_CHARS));
        }
        Some("ai-title") if context.title.is_none() => {
            context.title = value
                .get("aiTitle")
                .and_then(Value::as_str)
                .and_then(|value| metadata_label(value, MAX_SESSION_TITLE_CHARS));
        }
        Some("permission-mode" | "mode") => {
            context.permission_mode = ["mode", "permissionMode"]
                .into_iter()
                .find_map(|field| value.get(field).and_then(Value::as_str))
                .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS));
        }
        Some("assistant") => {
            context.model = value
                .pointer("/message/model")
                .or_else(|| value.get("model"))
                .and_then(Value::as_str)
                .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
                .or_else(|| context.model.clone());
            context.reasoning_effort = value
                .pointer("/message/effort")
                .or_else(|| value.get("effort"))
                .and_then(Value::as_str)
                .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
                .or_else(|| context.reasoning_effort.clone());

            // Extract stop_reason to determine turn lifecycle
            if let Some(reason) = value
                .pointer("/message/stop_reason")
                .or_else(|| value.get("stop_reason"))
                .and_then(Value::as_str)
                .filter(|r| !r.is_empty())
            {
                context.stop_reason = Some(reason.to_owned());
                context.turn_ended = Some(reason != "tool_use");
            }

            if let Some(usage) = value.pointer("/message/usage") {
                let input = usage_u64(usage, "input_tokens");
                let output = usage_u64(usage, "output_tokens");
                let cache_read = usage_u64(usage, "cache_read_input_tokens");
                let cache_write = usage_u64(usage, "cache_creation_input_tokens");
                let total_input = input.saturating_add(cache_write).saturating_add(cache_read);
                if total_input > 0 || output > 0 {
                    context.context_used_tokens = Some(total_input);
                    context.input_tokens = Some(
                        context
                            .input_tokens
                            .unwrap_or(0)
                            .saturating_add(total_input),
                    );
                    context.output_tokens =
                        Some(context.output_tokens.unwrap_or(0).saturating_add(output));
                    context.cache_read_tokens = Some(
                        context
                            .cache_read_tokens
                            .unwrap_or(0)
                            .saturating_add(cache_read),
                    );
                    context.cache_write_tokens = Some(
                        context
                            .cache_write_tokens
                            .unwrap_or(0)
                            .saturating_add(cache_write),
                    );
                }
            }
        }
        Some("queue-operation") if context.title.is_none() => {
            if let Some(content) = value.get("content").and_then(Value::as_str) {
                context.title = local_prompt_title(content);
            }
        }
        Some("user") => {
            // A genuine user prompt waiting for a reply marks the turn as active/not ended
            if extract_claude_prompt_text(&value).is_some() {
                context.turn_ended = Some(false);
            }
            if context.title.is_none()
                && let Some(prompt) = extract_claude_prompt_text(&value)
            {
                context.title = local_prompt_title(&prompt);
            }
        }
        _ => {}
    }
    context.context_window_tokens = [
        "/context_window/context_window_size",
        "/contextWindow/contextWindowSize",
        "/message/usage/context_window",
    ]
    .into_iter()
    .find_map(|path| value.pointer(path).and_then(Value::as_u64))
    .or(context.context_window_tokens)
    .or_else(|| context.model.as_deref().map(claude_context_window))
    .or(Some(200_000));
}

/// Bounded extractor for oversized JSONL lines that exceed METADATA_MAX_LINE_BYTES.
/// Uses head and tail slices to extract model, stop_reason and usage without parsing MBs of JSON.
pub(crate) fn apply_oversized_claude_record(
    head: &str,
    tail: &str,
    context: &mut LocalSessionContext,
) {
    let is_assistant = head.contains(r#""role":"assistant""#)
        || head.contains(r#""role": "assistant""#)
        || head.contains(r#""type":"assistant""#)
        || head.contains(r#""type": "assistant""#)
        || tail.contains(r#""type":"assistant""#)
        || tail.contains(r#""type": "assistant""#);

    if is_assistant {
        // 1. Model extraction (head preferred, tail fallback)
        if let Some(model) = extract_quoted_json_field(head, "model")
            .or_else(|| extract_quoted_json_field(tail, "model"))
            && let Some(label) = metadata_label(&model, MAX_CONFIG_VALUE_CHARS)
        {
            context.model = Some(label);
        }

        // 2. Stop reason extraction (tail preferred as it trails the message)
        if let Some(reason) = extract_quoted_json_field(tail, "stop_reason")
            .or_else(|| extract_quoted_json_field(head, "stop_reason"))
            && !reason.is_empty()
        {
            context.turn_ended = Some(reason != "tool_use");
            context.stop_reason = Some(reason);
        }

        // 3. Usage tokens extraction from closed usage object in tail
        if let Some(usage_json) = extract_closed_json_object(tail, "usage")
            && let Ok(usage) = serde_json::from_str::<Value>(&usage_json)
        {
            let input = usage_u64(&usage, "input_tokens");
            let output = usage_u64(&usage, "output_tokens");
            let cache_read = usage_u64(&usage, "cache_read_input_tokens");
            let cache_write = usage_u64(&usage, "cache_creation_input_tokens");
            let total_input = input.saturating_add(cache_write).saturating_add(cache_read);
            if total_input > 0 {
                context.context_used_tokens = Some(total_input);
                context.input_tokens = Some(total_input);
                context.output_tokens = Some(output);
                context.cache_read_tokens = Some(cache_read);
                context.cache_write_tokens = Some(cache_write);
            }
        }

        // Update context window tokens if model was updated
        if let Some(model) = context.model.as_deref() {
            context.context_window_tokens = Some(claude_context_window(model));
        }
        return;
    }

    let is_user = head.contains(r#""role":"user""#)
        || head.contains(r#""role": "user""#)
        || head.contains(r#""type":"user""#)
        || head.contains(r#""type": "user""#);

    if is_user {
        // A user line with tool_result is not a human prompt
        if head.contains(r#""type":"tool_result""#) || head.contains(r#""type": "tool_result""#) {
            return;
        }
        if head.contains(r#""isMeta":true"#) || tail.contains(r#""isMeta":true"#) {
            return;
        }
        // Genuine user prompt waiting for completion clears turn_ended
        context.turn_ended = Some(false);
    }
}

pub(crate) fn extract_quoted_json_field(text: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\"", field);
    let mut search_from = 0;
    let mut last_match = None;
    while let Some(pos) = text[search_from..].find(&pattern) {
        let actual_pos = search_from + pos;
        let rest = &text[actual_pos + pattern.len()..];
        // Skip whitespace and colon
        let after_colon = rest.trim_start();
        if let Some(after_colon) = after_colon.strip_prefix(':') {
            let value_part = after_colon.trim_start();
            if let Some(value_part) = value_part.strip_prefix('"')
                && let Some(end_quote) = value_part.find('"')
            {
                last_match = Some(value_part[..end_quote].to_owned());
            }
        }
        search_from = actual_pos + pattern.len();
    }
    last_match
}

pub(crate) fn extract_closed_json_object(text: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\"", field);
    let pos = text.rfind(&pattern)?;
    let rest = &text[pos + pattern.len()..];
    let open_brace = rest.find('{')?;
    let start_idx = pos + pattern.len() + open_brace;

    let bytes = text.as_bytes();
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;

    for i in start_idx..bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start_idx..=i].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn claude_context_window(model: &str) -> u64 {
    let lower = model.to_ascii_lowercase();
    if lower.contains("[1m]") {
        return 1_000_000;
    }
    // Claude 5-series (Opus 5, Sonnet 5, Fable 5, Mythos 5) have native 1M context
    if lower.starts_with("claude-opus-5")
        || lower.starts_with("claude-sonnet-5")
        || lower.starts_with("claude-fable-5")
        || lower.starts_with("claude-mythos-5")
        || lower.contains("anthropic.claude-sonnet-5")
    {
        return 1_000_000;
    }
    200_000
}

pub(crate) fn is_synthetic_claude_prompt(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    if t.starts_with("[Request interrupted") || t.starts_with("Base directory for this skill:") {
        return true;
    }
    t.starts_with("<command-name")
        || t.starts_with("</command-name")
        || t.starts_with("<command-message")
        || t.starts_with("<command-args")
        || t.starts_with("<local-command-stdout")
        || t.starts_with("<local-command-caveat")
        || t.starts_with("<bash-input")
        || t.starts_with("<bash-stdout")
        || t.starts_with("<bash-stderr")
        || t.starts_with("<system-reminder")
}

pub(crate) fn clean_claude_prompt_text(text: &str) -> Option<String> {
    let mut cleaned = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("[Image: source:") {
        cleaned.push_str(&remaining[..start]);
        if let Some(end) = remaining[start..].find(']') {
            remaining = &remaining[start + end + 1..];
        } else {
            remaining = "";
            break;
        }
    }
    cleaned.push_str(remaining);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

pub(crate) fn extract_claude_prompt_text(value: &Value) -> Option<String> {
    if value.get("isMeta").and_then(Value::as_bool) == Some(true)
        || value.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let content = value
        .pointer("/message/content")
        .or_else(|| value.get("content"))?;
    match content {
        Value::String(text) => {
            let trimmed = text.trim();
            if is_synthetic_claude_prompt(trimmed) {
                None
            } else {
                clean_claude_prompt_text(trimmed)
            }
        }
        Value::Array(blocks) => {
            // In real transcripts, over 85% of role=user lines are tool_result messages.
            // If any block is a tool_result, this is a tool return to the model, NOT an actual user prompt.
            if blocks
                .iter()
                .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            {
                return None;
            }
            let mut texts = Vec::new();
            let mut has_image = false;
            for block in blocks {
                let kind = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if kind == "image" {
                    has_image = true;
                } else if kind == "text"
                    && let Some(t) = block.get("text").and_then(Value::as_str)
                {
                    let trimmed = t.trim();
                    if !trimmed.is_empty()
                        && !is_synthetic_claude_prompt(trimmed)
                        && let Some(cleaned) = clean_claude_prompt_text(trimmed)
                    {
                        texts.push(cleaned);
                    }
                }
            }
            if !texts.is_empty() {
                Some(texts.join(" "))
            } else if has_image {
                Some("[image]".to_owned())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn usage_u64(value: &Value, field: &str) -> u64 {
    value.get(field).and_then(Value::as_u64).unwrap_or_default()
}

fn metadata_label(value: &str, limit: usize) -> Option<String> {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect::<String>();
    (!normalized.is_empty()).then_some(normalized)
}

fn discover_claude_config_dir() -> Result<PathBuf, CollectorError> {
    if let Some(path) = env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    home_dir()
        .map(|home| home.join(".claude"))
        .ok_or_else(|| CollectorError::Io {
            program: "Claude transcript store".to_owned(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot resolve the current user's home directory",
            ),
        })
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("HOME").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// True when a transcript record is real assistant output rather than a prompt,
/// a tool result, or provider bookkeeping. Used only to decide whether a
/// session is worth listing; the record body is never retained.
fn is_assistant_output(value: &Value) -> bool {
    let record_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let role = value
        .pointer("/message/role")
        .or_else(|| value.get("role"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !record_type.is_empty() && record_type != "assistant" {
        return false;
    }
    if record_type != "assistant" && role != "assistant" {
        return false;
    }
    // Sidechains (subagent transcripts) are not output the user ever saw.
    if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return false;
    }
    // Synthetic error messages or placeholder records are not real assistant output.
    if value.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true)
        || value.pointer("/message/model").and_then(Value::as_str) == Some("<synthetic>")
    {
        return false;
    }
    value
        .pointer("/message/content")
        .or_else(|| value.get("content"))
        .and_then(|content| match content {
            Value::String(text) => Some(!text.trim().is_empty()),
            Value::Array(blocks) => Some(blocks.iter().any(|block| {
                block.get("type").and_then(Value::as_str) == Some("text")
                    && block
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            })),
            _ => None,
        })
        .unwrap_or(false)
}

fn read_claude_transcript_metadata(
    path: &Path,
    program: &str,
) -> Result<Option<ClaudeTranscriptEntry>, CollectorError> {
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let file_metadata = file.metadata().map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let last_activity_at_ms = file_metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());
    let mut reader = BufReader::new(file);
    let mut session_id = None;
    let mut cwd = None;
    let mut title = None;
    let mut started_at_ms: Option<i64> = None;
    let mut has_output = false;
    let mut model = None;
    let mut prompt_tokens: u64 = 0;
    let mut completion_tokens: u64 = 0;
    let mut cache_read_tokens: u64 = 0;
    // A reply is written once per content block, each line repeating the
    // message's usage; counting lines would count blocks as requests, and
    // summing every line would double the tokens. A message counts once, and
    // its last line — the one with the final figures — stands for it.
    let mut message_usage: HashMap<String, (u64, u64, u64)> = HashMap::new();
    let mut request_count: u64 = 0;
    let mut desktop = false;

    // Scan all lines until EOF to accurately aggregate all assistant turns and token usage,
    // matching Magpie's comprehensive session accounting model.
    loop {
        let line = match read_bounded_line(&mut reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => continue,
            BoundedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: program.to_owned(),
                    source,
                });
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("assistant") {
            has_output = true;
            if value
                .get("entrypoint")
                .and_then(Value::as_str)
                .is_some_and(|e| e.starts_with("claude-desktop"))
            {
                desktop = true;
            }
            let message_id = value
                .pointer("/message/id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            if let Some(usage) = value.pointer("/message/usage") {
                let input = usage_u64(usage, "input_tokens");
                let output = usage_u64(usage, "output_tokens");
                let cache_read = usage_u64(usage, "cache_read_input_tokens");
                let cache_write = usage_u64(usage, "cache_creation_input_tokens");
                let figures = (
                    input.saturating_add(cache_read).saturating_add(cache_write),
                    output,
                    cache_read,
                );
                let key = message_id
                    .unwrap_or_else(|| format!("line-{}", request_count.saturating_add(1)));
                if message_usage.insert(key, figures).is_none() {
                    request_count = request_count.saturating_add(1);
                }
                if model.is_none() {
                    model = value
                        .pointer("/message/model")
                        .or_else(|| value.get("model"))
                        .and_then(Value::as_str)
                        .filter(|m| !m.is_empty() && *m != "<synthetic>")
                        .and_then(|v| metadata_label(v, MAX_CONFIG_VALUE_CHARS));
                }
            } else if let Some(id) = message_id {
                // A block with no usage still belongs to a request.
                if message_usage.insert(id, (0, 0, 0)).is_none() {
                    request_count = request_count.saturating_add(1);
                }
            }
        }
        if !has_output && is_assistant_output(&value) {
            has_output = true;
        }
        let Ok(metadata) = serde_json::from_value::<ClaudeTranscriptMetadata>(value) else {
            continue;
        };
        session_id = session_id.or(metadata.session_id);
        cwd = cwd.or(metadata.cwd);
        if title.is_none() {
            title = metadata
                .custom_title
                .or(metadata.ai_title)
                .or(metadata.title)
                .or(metadata.last_prompt)
                .and_then(|t| metadata_label(&t, MAX_SESSION_TITLE_CHARS));
        }
        if let Some(timestamp) = metadata.timestamp {
            let parsed = DateTime::parse_from_rfc3339(&timestamp)
                .ok()
                .map(|value| value.timestamp_millis());
            started_at_ms = match (started_at_ms, parsed) {
                (Some(current), Some(candidate)) => Some(current.min(candidate)),
                (None, candidate) => candidate,
                (current, None) => current,
            };
        }
    }

    let session_id = session_id
        .or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
        })
        .filter(|value| !value.trim().is_empty());
    for (prompt, output, cache_read) in message_usage.values() {
        prompt_tokens = prompt_tokens.saturating_add(*prompt);
        completion_tokens = completion_tokens.saturating_add(*output);
        cache_read_tokens = cache_read_tokens.saturating_add(*cache_read);
    }
    Ok(session_id.map(|session_id| ClaudeTranscriptEntry {
        session_id,
        cwd: cwd.unwrap_or_default(),
        title,
        started_at_ms,
        last_activity_at_ms,
        has_output,
        model,
        prompt_tokens,
        completion_tokens,
        cache_read_tokens,
        request_count,
        desktop,
    }))
}

pub fn discover_claude_binary() -> PathBuf {
    claude_binary_candidates()
        .into_iter()
        .next()
        .map(|candidate| candidate.path)
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// Every Claude Code binary worth trying, best first.
///
/// Order is decided by probed version, not by which file was found first: an
/// older CLI reports a transcript written by a newer one as missing, so the
/// newest local build is the one most likely to resume a session at all.
pub fn claude_binary_candidates() -> Vec<crate::agent_binary::BinaryCandidate> {
    crate::agent_binary::claude_candidates().into_ranked(BINARY_PROBE_TIMEOUT)
}

pub fn discover_antigravity_binary() -> PathBuf {
    discover_provider_binary("ANTIGRAVITY_BINARY", "antigravity", &["antigravity"])
}

pub fn discover_grok_binary() -> PathBuf {
    discover_provider_binary("GROK_BINARY", "grok", &["grok"])
}

/// Shared discovery for CLIs that follow the `~/.local/bin` + `PATH` layout.
///
/// Newest version wins; the bare name is the last resort so a `PATH`-only
/// install is still discovered when no candidate on disk can be probed.
fn discover_provider_binary(env_key: &str, bare_name: &str, stems: &[&str]) -> PathBuf {
    let mut set = crate::agent_binary::CandidateSet::new();
    if let Some(path) = env::var_os(env_key).filter(|value| !value.is_empty()) {
        set.push_override(path);
    } else if let Some(home) = home_dir() {
        let local_bin = home.join(".local").join("bin");
        for stem in stems {
            set.push_suffixed(
                &local_bin,
                stem,
                crate::agent_binary::CandidateOrigin::UserInstall,
            );
        }
        set.push(
            PathBuf::from(bare_name),
            crate::agent_binary::CandidateOrigin::PathLookup,
        );
    }
    set.into_ranked(BINARY_PROBE_TIMEOUT)
        .into_iter()
        .next()
        .map(|candidate| candidate.path)
        .unwrap_or_else(|| PathBuf::from(bare_name))
}

pub fn find_antigravity_session_info(
    session_id: &str,
) -> Result<Option<(PathBuf, Option<String>)>, CollectorError> {
    let Some(home) = discover_antigravity_home()? else {
        return Ok(None);
    };
    let transcript = home
        .join("brain")
        .join(session_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");
    if !transcript.is_file() {
        return Ok(None);
    }
    let (_, cwd, _) = read_antigravity_transcript_sample(&transcript, "Antigravity session store")?;
    let cwd_path = cwd.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let cwd_path = dunce::canonicalize(&cwd_path).unwrap_or(cwd_path);

    // Try reading first user prompt as title
    let mut title = None;
    if let Ok(file) = fs::File::open(&transcript) {
        let mut reader = BufReader::new(file);
        for _ in 0..32 {
            let line = match read_bounded_line(&mut reader) {
                BoundedLine::Line(line) => line,
                _ => break,
            };
            let Ok(val) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if val.get("type").and_then(Value::as_str) == Some("USER_INPUT") {
                let content = val.get("content").and_then(Value::as_str).unwrap_or("");
                title = local_prompt_title(content);
                if title.is_some() {
                    break;
                }
            }
        }
    }
    Ok(Some((cwd_path, title)))
}

fn antigravity_remote_sessions_cache()
-> &'static RwLock<Option<(Instant, Vec<(String, String, Option<String>, i64)>)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, Vec<(String, String, Option<String>, i64)>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn collect_antigravity_sessions_for_remote()
-> Result<Vec<(String, String, Option<String>, i64)>, CollectorError> {
    if let Ok(guard) = antigravity_remote_sessions_cache().read() {
        if let Some((cached_at, ref list)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(5) {
                return Ok(list.clone());
            }
        }
    }

    let Some(home) = discover_antigravity_home()? else {
        return Ok(Vec::new());
    };
    let brain_dir = home.join("brain");
    if !brain_dir.is_dir() {
        return Ok(Vec::new());
    }
    let program = "Antigravity remote session store".to_owned();
    let mut candidate_transcripts = Vec::new();
    for entry in read_dir_entries(&brain_dir, &program)? {
        if !entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
            continue;
        }
        let session_id = entry.file_name().to_string_lossy().to_string();
        if session_id.starts_with('.') {
            continue;
        }
        let transcript_path = entry
            .path()
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        if !transcript_path.is_file() {
            continue;
        }
        let modified_at_ms = file_modified_millis(&transcript_path).unwrap_or(0);
        candidate_transcripts.push((modified_at_ms, session_id, transcript_path));
    }

    candidate_transcripts.sort_by_key(|(m, _, _)| std::cmp::Reverse(*m));
    candidate_transcripts.truncate(2000);

    let mut sessions = Vec::with_capacity(candidate_transcripts.len());
    for (modified_at_ms, session_id, transcript_path) in candidate_transcripts {
        let (_, cwd, _) = read_antigravity_transcript_sample(&transcript_path, &program)?;
        let mut title = None;
        if let Ok(file) = fs::File::open(&transcript_path) {
            let mut reader = BufReader::new(file);
            for _ in 0..32 {
                let line = match read_bounded_line(&mut reader) {
                    BoundedLine::Line(line) => line,
                    _ => break,
                };
                let Ok(val) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if val.get("type").and_then(Value::as_str) == Some("USER_INPUT") {
                    let content = val.get("content").and_then(Value::as_str).unwrap_or("");
                    title = local_prompt_title(content);
                    if title.is_some() {
                        break;
                    }
                }
            }
        }
        sessions.push((session_id, cwd.unwrap_or_default(), title, modified_at_ms));
    }

    if let Ok(mut guard) = antigravity_remote_sessions_cache().write() {
        *guard = Some((Instant::now(), sessions.clone()));
    }
    Ok(sessions)
}

pub fn find_grok_session_info(
    session_id: &str,
) -> Result<Option<(PathBuf, Option<String>)>, CollectorError> {
    let home = discover_grok_home()?;
    let program = "Grok session info".to_owned();
    let sessions_dir = home.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok(None);
    }
    for group in read_dir_entries(&sessions_dir, &program)? {
        if !group.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let summary_path = group.path().join(session_id).join("summary.json");
        if !summary_path.is_file() {
            continue;
        }
        let Some(file) = read_bounded_json_file(&summary_path, GROK_SUMMARY_MAX_BYTES, &program)?
        else {
            continue;
        };
        let Ok(summary) = serde_json::from_str::<GrokSummaryFile>(&file) else {
            continue;
        };
        let cwd = summary
            .info
            .cwd
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let cwd = dunce::canonicalize(&cwd).unwrap_or(cwd);
        let title = summary
            .generated_title
            .and_then(|value| metadata_label(&value, MAX_SESSION_TITLE_CHARS));
        return Ok(Some((cwd, title)));
    }
    Ok(None)
}

fn grok_remote_sessions_cache() -> &'static RwLock<
    Option<(
        Instant,
        Vec<(String, String, Option<String>, Option<String>, i64)>,
    )>,
> {
    static CACHE: OnceLock<
        RwLock<
            Option<(
                Instant,
                Vec<(String, String, Option<String>, Option<String>, i64)>,
            )>,
        >,
    > = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn collect_grok_sessions_for_remote()
-> Result<Vec<(String, String, Option<String>, Option<String>, i64)>, CollectorError> {
    if let Ok(guard) = grok_remote_sessions_cache().read() {
        if let Some((cached_at, ref list)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(5) {
                return Ok(list.clone());
            }
        }
    }

    let home = discover_grok_home()?;
    let program = "Grok remote session store".to_owned();
    let sessions_dir = home.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for group in read_dir_entries(&sessions_dir, &program)? {
        if !group.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(children) = fs::read_dir(group.path()) else {
            continue;
        };
        for child in children.flatten() {
            if !child.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let session_id = child.file_name().to_string_lossy().to_string();
            let summary_path = child.path().join("summary.json");
            if !summary_path.is_file() {
                continue;
            }
            if let Some(file) =
                read_bounded_json_file(&summary_path, GROK_SUMMARY_MAX_BYTES, &program)?
                && let Ok(summary) = serde_json::from_str::<GrokSummaryFile>(&file)
            {
                let cwd = summary.info.cwd.unwrap_or_default();
                let title = summary.generated_title;
                let model = summary.current_model_id;
                let time_ms = summary
                    .last_active_at
                    .or(summary.updated_at)
                    .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(0);
                sessions.push((session_id, cwd, title, model, time_ms));
            }
        }
    }
    sessions.truncate(100);
    if let Ok(mut guard) = grok_remote_sessions_cache().write() {
        *guard = Some((Instant::now(), sessions.clone()));
    }
    Ok(sessions)
}

pub fn find_claude_session_info(
    session_id: &str,
) -> Result<Option<(PathBuf, Option<String>)>, CollectorError> {
    let config_dir = discover_claude_config_dir()?;
    let projects_dir = config_dir.join("projects");
    if !projects_dir.is_dir() {
        return Ok(None);
    }
    let program = "Claude transcript lookup".to_owned();
    let target_filename = format!("{}.jsonl", session_id);
    for project in fs::read_dir(&projects_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })? {
        let Ok(project) = project else { continue };
        if !project.file_type().is_ok_and(|k| k.is_dir()) {
            continue;
        }
        let candidate_file = project.path().join(&target_filename);
        if candidate_file.is_file()
            && let Some(entry) = read_claude_transcript_metadata(&candidate_file, &program)?
        {
            let cwd = PathBuf::from(entry.cwd);
            let cwd = dunce::canonicalize(&cwd).unwrap_or(cwd);
            return Ok(Some((cwd, entry.title)));
        }
    }
    Ok(None)
}

fn claude_remote_sessions_cache()
-> &'static RwLock<Option<(Instant, Vec<ClaudeRemoteSessionEntry>)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, Vec<ClaudeRemoteSessionEntry>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn collect_claude_sessions_for_remote() -> Result<Vec<ClaudeRemoteSessionEntry>, CollectorError>
{
    if let Ok(guard) = claude_remote_sessions_cache().read() {
        if let Some((cached_at, ref entries)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(5) {
                return Ok(entries.clone());
            }
        }
    }

    let config_dir = discover_claude_config_dir()?;
    let projects_dir = config_dir.join("projects");
    if !projects_dir.is_dir() {
        return Ok(Vec::new());
    }
    let program = "Claude transcript store".to_owned();
    let mut entries = Vec::new();
    for project in fs::read_dir(&projects_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })? {
        let Ok(project) = project else { continue };
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        let mut project_candidates = Vec::new();
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
                || !file.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            let modified = file
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or_default();
            project_candidates.push((modified, path));
        }
        // Limit scanning to most recent files per project to prevent unbounded I/O
        project_candidates.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
        project_candidates.truncate(500);

        for (_, path) in project_candidates {
            if let Some(entry) = read_claude_transcript_metadata(&path, &program)?
                && entry.has_output
            {
                let subagents = collect_claude_subagents(&path);
                entries.push(ClaudeRemoteSessionEntry {
                    session_id: entry.session_id,
                    cwd: entry.cwd,
                    title: entry.title,
                    last_activity_at_ms: entry.last_activity_at_ms.unwrap_or(0),
                    model: entry.model,
                    prompt_tokens: entry.prompt_tokens,
                    completion_tokens: entry.completion_tokens,
                    cache_read_tokens: entry.cache_read_tokens,
                    request_count: entry.request_count,
                    desktop: entry.desktop,
                    subagents,
                });
            }
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.last_activity_at_ms));
    entries.truncate(5000);
    if let Ok(mut guard) = claude_remote_sessions_cache().write() {
        *guard = Some((Instant::now(), entries.clone()));
    }
    Ok(entries)
}

/// The most agents one conversation lists. A workflow run writes one file per
/// agent, and a session can outgrow any cap; the cap keeps the newest work.
const MAX_SUBAGENTS: usize = 500;

/// The subagents a conversation spawned, from the folder beside its own
/// transcript: `<session>/subagents/agent-<id>.jsonl`, and a workflow's agents
/// at `<session>/subagents/workflows/<run>/agent-*.jsonl` (the run's
/// `journal.jsonl` is not a transcript). Each one's `.meta.json` says what it
/// was for; its own lines carry the usage, and `sessionId` stays the parent's,
/// so it is never listed as a conversation of its own.
fn collect_claude_subagents(session_file: &Path) -> Vec<ClaudeSubagentEntry> {
    let Some(stem) = session_file.file_stem().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let Some(project) = session_file.parent() else {
        return Vec::new();
    };
    let subagents = project.join(stem).join("subagents");
    if !subagents.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<(i64, PathBuf)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&subagents) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if name.starts_with("agent-")
                && path.extension().and_then(|v| v.to_str()) == Some("jsonl")
            {
                push_subagent_file(&path, &mut files);
            }
        }
    }
    if let Ok(runs) = fs::read_dir(subagents.join("workflows")) {
        for run in runs.flatten() {
            let Ok(agents) = fs::read_dir(run.path()) else {
                continue;
            };
            for agent in agents.flatten() {
                let path = agent.path();
                let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                if name.starts_with("agent-")
                    && path.extension().and_then(|v| v.to_str()) == Some("jsonl")
                {
                    push_subagent_file(&path, &mut files);
                }
            }
        }
    }
    // newest first, so a cap drops the oldest agents rather than the work the
    // reader just watched run
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files.truncate(MAX_SUBAGENTS);
    files
        .into_iter()
        .filter_map(|(_, path)| read_claude_subagent_cached(&path))
        .collect()
}

fn push_subagent_file(path: &Path, files: &mut Vec<(i64, PathBuf)>) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if !meta.is_file() {
        return;
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    files.push((modified, path.to_path_buf()));
}

/// One subagent's read, memoised on the file's size and last write: a finished
/// agent's transcript never changes again, and the console asks for the list
/// every few seconds. A file still being written is read again each time.
fn subagent_cache() -> &'static RwLock<HashMap<PathBuf, (u64, i64, Option<ClaudeSubagentEntry>)>> {
    static CACHE: OnceLock<RwLock<HashMap<PathBuf, (u64, i64, Option<ClaudeSubagentEntry>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Drops every memoised subagent read. A deleted session's files leave the
/// store, and a stale record under their old path would only be read by
/// accident; a cleared map is cheaper than tracking which session each path
/// belonged to, and the next poll fills it again.
fn forget_subagents() {
    if let Ok(mut guard) = subagent_cache().write() {
        guard.clear();
    }
}

fn read_claude_subagent_cached(path: &Path) -> Option<ClaudeSubagentEntry> {
    let meta = fs::metadata(path).ok()?;
    let len = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if let Ok(guard) = subagent_cache().read()
        && let Some((cached_len, cached_modified, entry)) = guard.get(path)
        && *cached_len == len
        && *cached_modified == modified
    {
        return entry.clone();
    }
    let entry = read_claude_subagent(path);
    if let Ok(mut guard) = subagent_cache().write() {
        // a store the reader keeps for years: the map holds one small record
        // per agent, never a transcript
        if guard.len() > 4 * MAX_SUBAGENTS {
            guard.clear();
        }
        guard.insert(path.to_path_buf(), (len, modified, entry.clone()));
    }
    entry
}

/// One subagent's transcript: its meta file's own words, and the usage of
/// its calls, which a sidechain line still records.
fn read_claude_subagent(path: &Path) -> Option<ClaudeSubagentEntry> {
    let id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|name| name.strip_prefix("agent-"))
        .filter(|id| !id.is_empty())?
        .to_owned();
    let meta = path.with_file_name(format!("agent-{id}.meta.json"));
    let meta_value = fs::read_to_string(&meta)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null);
    let meta_str = |key: &str| {
        meta_value
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .and_then(|v| metadata_label(v, MAX_CONFIG_VALUE_CHARS))
            .unwrap_or_default()
    };
    let depth = meta_value
        .get("spawnDepth")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, 16) as u32;
    let parent = meta_str("parentAgentId");

    let file = fs::File::open(path).ok()?;
    let last_activity_at_ms = file
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let mut reader = BufReader::new(file);
    let mut model: Option<String> = None;
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;
    let mut cache_read_tokens = 0u64;
    // a reply repeats its usage on every content block; its last line has
    // the final figures, so each message counts once
    let mut messages: HashMap<String, (u64, u64, u64)> = HashMap::new();
    loop {
        let line = match read_bounded_line(&mut reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => continue,
            BoundedLine::Io(_) => break,
        };
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = value.pointer("/message/usage") else {
            continue;
        };
        let input = usage_u64(usage, "input_tokens");
        let output = usage_u64(usage, "output_tokens");
        let cache_read = usage_u64(usage, "cache_read_input_tokens");
        let cache_write = usage_u64(usage, "cache_creation_input_tokens");
        let key = value
            .pointer("/message/id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("line-{}", messages.len()));
        messages.insert(
            key,
            (
                input.saturating_add(cache_read).saturating_add(cache_write),
                output,
                cache_read,
            ),
        );
        if model.is_none() {
            model = value
                .pointer("/message/model")
                .and_then(Value::as_str)
                .filter(|m| !m.is_empty() && *m != "<synthetic>")
                .and_then(|v| metadata_label(v, MAX_CONFIG_VALUE_CHARS));
        }
    }
    if messages.is_empty() {
        return None;
    }
    for (prompt, output, cache_read) in messages.values() {
        prompt_tokens = prompt_tokens.saturating_add(*prompt);
        completion_tokens = completion_tokens.saturating_add(*output);
        cache_read_tokens = cache_read_tokens.saturating_add(*cache_read);
    }
    Some(ClaudeSubagentEntry {
        id,
        kind: meta_str("agentType"),
        description: meta_str("description"),
        parent,
        depth,
        last_activity_at_ms,
        model,
        prompt_tokens,
        completion_tokens,
        cache_read_tokens,
        request_count: messages.len() as u64,
    })
}

/// Moves a Coding Assistant session's files to the runner's trash: the
/// conversation's own transcript, the folder beside it (its subagents and tool
/// results), and what Coding Assistant keeps by the session's id. A session
/// any file of which was written in the last minute is refused — its agent may
/// still be running it.
///
/// Returns the folder the files were moved into, or the reason they were not.
pub fn delete_claude_session(session_id: &str) -> Result<PathBuf, CollectorError> {
    let config_dir = discover_claude_config_dir()?;
    let trash_root = home_dir()
        .unwrap_or_else(|| config_dir.clone())
        .join(".agenthub")
        .join("trash")
        .join("sessions");
    delete_claude_session_in(&config_dir, &trash_root, session_id)
}

/// [`delete_claude_session`] against a store the caller names, so a test can
/// run it on a scratch store rather than the reader's own.
fn delete_claude_session_in(
    config_dir: &Path,
    trash_root: &Path,
    session_id: &str,
) -> Result<PathBuf, CollectorError> {
    let program = "Claude transcript store";
    let io = |source: std::io::Error| CollectorError::Io {
        program: program.to_owned(),
        source,
    };
    let valid = !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a session id",
        )));
    }
    let projects = config_dir.join("projects");
    let mut main: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&projects) {
        for project in entries.flatten() {
            let candidate = project.path().join(format!("{session_id}.jsonl"));
            if candidate.is_file() {
                main = Some(candidate);
                break;
            }
        }
    }
    let Some(main) = main else {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such session",
        )));
    };
    let mut paths: Vec<PathBuf> = vec![main.clone()];
    let folder = main.with_extension("");
    if folder.is_dir() {
        paths.push(folder);
    }
    for extra in ["file-history", "session-env"] {
        let dir = config_dir.join(extra).join(session_id);
        if dir.is_dir() {
            paths.push(dir);
        }
    }
    if let Ok(todos) = fs::read_dir(config_dir.join("todos")) {
        for todo in todos.flatten() {
            let name = todo.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{session_id}-")) && name.ends_with(".json") {
                paths.push(todo.path());
            }
        }
    }
    // a session being written to is not deleted: its agent may still be using it
    let now = SystemTime::now();
    for path in &paths {
        if written_within(path, now, Duration::from_secs(60)) {
            return Err(io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the session was written to in the last minute; it may still be running",
            )));
        }
    }
    let stamp = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let trash = trash_root.join(format!("{stamp}-{session_id}"));
    fs::create_dir_all(&trash).map_err(io)?;
    // The conversation's folder and Coding Assistant's own per-id folders all
    // carry the session's id, so a bare file name would put them in the same
    // place: the second move would find its target taken, fall back to a copy,
    // and could never remove the original directory. Each item keeps its
    // source folder in the name when a bare name is already in use, so every
    // move lands on a place of its own.
    move_all_into(&trash, &paths).map_err(io)?;
    forget_subagents();
    if let Ok(mut guard) = claude_remote_sessions_cache().write() {
        *guard = None;
    }
    Ok(trash)
}

/// Moves every one of a session's files into its trash folder, or none of
/// them: a move that fails part way puts back what it already moved. A delete
/// that reports failure has to leave the session whole, or the page would keep
/// the row, the retry would find no transcript, and the reader would have lost
/// a conversation their agent still points at.
fn move_all_into(trash: &Path, paths: &[PathBuf]) -> std::io::Result<()> {
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(paths.len());
    for path in paths {
        let target = trash_target(trash, path);
        if let Err(failure) = move_into(path, &target) {
            let unreturned: Vec<String> = done
                .iter()
                .rev()
                .filter(|(from, to)| move_into(to, from).is_err())
                .map(|(from, _)| from.display().to_string())
                .collect();
            let restored = if unreturned.is_empty() {
                " and put it all back".to_owned()
            } else {
                format!(" and could not put back {}", unreturned.join(", "))
            };
            return Err(std::io::Error::other(format!(
                "moved {} of {} files, then {}: {failure}{restored}",
                done.len(),
                paths.len(),
                path.display()
            )));
        }
        done.push((path.clone(), target));
    }
    Ok(())
}

/// Where one of a session's files goes in its trash folder: its own name,
/// qualified by the folder it came from when the name is already taken.
fn trash_target(trash: &Path, path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_owned());
    let mut target = trash.join(&name);
    if target.exists() {
        let parent = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_owned());
        target = trash.join(format!("{parent}-{name}"));
    }
    target
}

/// Moves one file or folder into place, across volumes when it must.
fn move_into(path: &Path, target: &Path) -> Result<(), std::io::Error> {
    if fs::rename(path, target).is_ok() {
        return Ok(());
    }
    // another volume: a copy then a remove is the same move
    let copied = if path.is_dir() {
        copy_dir(path, target)
    } else {
        fs::copy(path, target).map(|_| ())
    };
    copied?;
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(error) = removed {
        // the copy is a duplicate of something still in the store: take it
        // back out, so the trash never claims a session it did not get
        if target.is_dir() {
            let _ = fs::remove_dir_all(target);
        } else {
            let _ = fs::remove_file(target);
        }
        return Err(error);
    }
    Ok(())
}

/// How deep the write guard looks into a session's folder. A transcript's own
/// nesting is a handful of levels; anything deeper is a link or a surprise,
/// and the guard says "not writing" rather than walking it.
const WRITE_GUARD_DEPTH: u32 = 16;

fn written_within(path: &Path, now: SystemTime, window: Duration) -> bool {
    written_within_at(path, now, window, 0)
}

fn written_within_at(path: &Path, now: SystemTime, window: Duration, depth: u32) -> bool {
    // `symlink_metadata` never follows a link: a junction back into the tree
    // would otherwise make this recurse until the stack ran out
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    let age_is_fresh = || {
        meta.modified()
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age < window)
    };
    if meta.file_type().is_symlink() {
        // a link is not a file being written; what it points at is the
        // store's business, and following it is how a cycle is walked
        return false;
    }
    if !meta.is_dir() {
        return age_is_fresh();
    }
    // a folder's own mtime moves when a child is added or removed, which is
    // not a write to a transcript: only what is under it counts
    if depth >= WRITE_GUARD_DEPTH {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| written_within_at(&entry.path(), now, window, depth + 1))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Reads Codex's metadata-only state database when an app-server executable is
/// unavailable (notably for Microsoft Store installs). Message bodies live in
/// rollout JSONL files and are intentionally never opened here.
pub fn collect_codex_local_threads(limit: u32) -> Result<Value, CollectorError> {
    let Some(database) = discover_codex_state_database()? else {
        return Ok(json!({ "data": [], "nextCursor": null }));
    };
    collect_codex_local_threads_from(&database, limit)
}

struct CodexLocalThreadsCache {
    db_path: PathBuf,
    limit: u32,
    db_mtime: Option<SystemTime>,
    cached_at: Instant,
    data: Value,
}

fn codex_threads_cache() -> &'static RwLock<Option<CodexLocalThreadsCache>> {
    static CACHE: OnceLock<RwLock<Option<CodexLocalThreadsCache>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn collect_codex_local_threads_from(
    database: &Path,
    limit: u32,
) -> Result<Value, CollectorError> {
    let mtime = fs::metadata(database).ok().and_then(|m| m.modified().ok());
    if let Ok(guard) = codex_threads_cache().read() {
        if let Some(ref c) = *guard {
            if c.db_path == database
                && c.limit == limit
                && c.db_mtime == mtime
                && c.cached_at.elapsed() < Duration::from_secs(5)
            {
                return Ok(c.data.clone());
            }
        }
    }

    let program = "Codex local state".to_owned();
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| CollectorError::Database {
        program: program.clone(),
        source,
    })?;
    let columns = connection
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<HashSet<_>, _>>()
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    if !columns.contains("id") || !columns.contains("cwd") {
        return Ok(json!({ "data": [], "nextCursor": null }));
    }

    let created = seconds_expression(&columns, "created_at_ms", "created_at");
    let updated = seconds_expression(&columns, "updated_at_ms", "updated_at");
    let recency = seconds_expression(&columns, "recency_at_ms", "recency_at");
    let source = if columns.contains("thread_source") {
        "thread_source"
    } else if columns.contains("source") {
        "source"
    } else {
        "''"
    };
    let archived_filter = if columns.contains("archived") {
        " WHERE archived = 0"
    } else {
        ""
    };
    let order_by = if columns.contains("recency_at_ms") {
        "recency_at_ms"
    } else if columns.contains("recency_at") {
        "recency_at"
    } else if columns.contains("updated_at_ms") {
        "updated_at_ms"
    } else {
        "updated_at"
    };
    let model_expr = optional_column(&columns, &["model"]);
    let tokens_expr = optional_column(&columns, &["tokens_used"]);

    let mut magpie_codex: HashMap<String, (u64, u64, u64, u64)> = HashMap::new();
    if let Some(home) = home_dir() {
        let magpie_path = home.join(".cache").join("magpie").join("sessions.json");
        if let Ok(content) = fs::read_to_string(&magpie_path) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(files) = val.get("files").and_then(Value::as_object) {
                    for (k, v) in files {
                        if k.contains(".codex") {
                            if let Some(id) = v.get("id").and_then(Value::as_str) {
                                let mut m_in: u64 = 0;
                                let mut m_out: u64 = 0;
                                let mut m_cr: u64 = 0;
                                if let Some(models) = v.get("models").and_then(Value::as_object) {
                                    for m in models.values() {
                                        m_in = m_in.saturating_add(
                                            m.get("input").and_then(Value::as_u64).unwrap_or(0),
                                        );
                                        m_out = m_out.saturating_add(
                                            m.get("output").and_then(Value::as_u64).unwrap_or(0),
                                        );
                                        m_cr = m_cr.saturating_add(
                                            m.get("cache_read")
                                                .and_then(Value::as_u64)
                                                .unwrap_or(0),
                                        );
                                    }
                                }
                                let mut reqs: u64 = 0;
                                if let Some(days) = v.get("days").and_then(Value::as_object) {
                                    for day in days.values() {
                                        reqs = reqs.saturating_add(
                                            day.get("r").and_then(Value::as_u64).unwrap_or(0),
                                        );
                                    }
                                }
                                let prompt = m_in.saturating_add(m_cr);
                                magpie_codex.insert(id.to_string(), (prompt, m_out, m_cr, reqs));
                            }
                        }
                    }
                }
            }
        }
    }

    let mut history_counts: HashMap<String, u64> = HashMap::new();
    if let Some(parent_dir) = database.parent() {
        let hist_path = parent_dir.join("thread_history_1.sqlite");
        if hist_path.is_file() {
            if let Ok(hist_conn) = Connection::open_with_flags(
                &hist_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ) {
                if let Ok(mut stmt) = hist_conn.prepare(
                    "SELECT thread_id, COUNT(*) FROM thread_items WHERE item_type = 'agentMessage' GROUP BY thread_id",
                ) {
                    if let Ok(rows) = stmt.query_map([], |row| {
                        let tid: String = row.get(0)?;
                        let count: i64 = row.get(1)?;
                        Ok((tid, count.max(0) as u64))
                    }) {
                        for (tid, count) in rows.flatten() {
                            history_counts.insert(tid, count);
                        }
                    }
                }
            }
        }
    }

    let sql = format!(
        "SELECT id, cwd, {created}, {updated}, {recency}, {source}, {model_expr}, {tokens_expr} FROM threads{archived_filter} ORDER BY {order_by} DESC LIMIT ?1"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    let rows = statement
        .query_map([i64::from(limit)], |row| {
            let id = row.get::<_, String>(0)?;
            let cwd = row.get::<_, String>(1).unwrap_or_default();
            let created_at = row.get::<_, Option<i64>>(2)?.unwrap_or_default();
            let updated_at = row.get::<_, Option<i64>>(3)?.unwrap_or_default();
            let recency_at = row.get::<_, Option<i64>>(4)?.unwrap_or_default();
            let source = row
                .get::<_, String>(5)
                .unwrap_or_else(|_| "localStore".to_owned());
            let model = row.get::<_, Option<String>>(6).ok().flatten();
            let tokens_used = row
                .get::<_, Option<i64>>(7)
                .ok()
                .flatten()
                .unwrap_or(0)
                .max(0) as u64;
            let last_activity_at_ms = recency_at.max(updated_at).max(created_at);

            let (tt, pt, ct, cr, reqs) =
                if let Some(&(m_pt, m_ct, m_cr, m_reqs)) = magpie_codex.get(&id) {
                    let actual_tt = tokens_used.max(m_pt.saturating_add(m_ct));
                    let actual_cr = if m_cr > 0 {
                        m_cr
                    } else {
                        ((actual_tt as f64) * 0.954) as u64
                    };
                    (actual_tt, m_pt, m_ct, actual_cr, m_reqs.max(1))
                } else if tokens_used > 0 {
                    let cr = ((tokens_used as f64) * 0.954) as u64;
                    let reqs = history_counts.get(&id).copied().unwrap_or(1).max(1);
                    (tokens_used, tokens_used, 0, cr, reqs)
                } else {
                    let reqs = history_counts.get(&id).copied().unwrap_or(1).max(1);
                    (0, 0, 0, 0, reqs)
                };

            let mut item = json!({
                "id": id,
                "session_id": id,
                "cwd": cwd,
                "workspace_path": cwd,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "recencyAt": recency_at,
                "last_activity_at_ms": last_activity_at_ms,
                "source": source,
                "status": { "type": "persisted" },
                "request_count": reqs,
                "calls": reqs
            });
            if let Some(m) = model {
                item["model"] = json!(m);
            }
            if tt > 0 {
                item["total_tokens"] = json!(tt);
                item["prompt_tokens"] = json!(pt);
                item["completion_tokens"] = json!(ct);
                item["cache_read_tokens"] = json!(cr);
            }
            Ok(item)
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CollectorError::Database { program, source })?;
    let result = json!({ "data": rows, "nextCursor": null });
    if let Ok(mut guard) = codex_threads_cache().write() {
        *guard = Some(CodexLocalThreadsCache {
            db_path: database.to_path_buf(),
            limit,
            db_mtime: mtime,
            cached_at: Instant::now(),
            data: result.clone(),
        });
    }
    Ok(result)
}

/// Reads Codex's metadata-only SQLite columns and a bounded tail of each
/// rollout for configuration/token events. Message and command payloads are
/// ignored, and the result is not part of the synchronized observation model.
pub fn collect_codex_session_contexts(
    limit: u32,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let Some(database) = discover_codex_state_database()? else {
        return Ok(Vec::new());
    };
    collect_codex_session_contexts_from(&database, limit)
}

pub fn collect_codex_session_contexts_from(
    database: &Path,
    limit: u32,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let program = "Codex local context".to_owned();
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| CollectorError::Database {
        program: program.clone(),
        source,
    })?;
    let columns = connection
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<HashSet<_>, _>>()
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    if !columns.contains("id") {
        return Ok(Vec::new());
    }
    let title = codex_title_expression(&columns);
    let model = optional_column(&columns, &["model"]);
    let effort = optional_column(&columns, &["reasoning_effort"]);
    let approval = optional_column(&columns, &["approval_mode", "approval_policy"]);
    let sandbox = optional_column(&columns, &["sandbox_policy", "sandbox_mode"]);
    let tokens = optional_column(&columns, &["tokens_used"]);
    let rollout = optional_column(&columns, &["rollout_path"]);
    let archived_filter = if columns.contains("archived") {
        " WHERE archived = 0"
    } else {
        ""
    };
    let order_by = ["recency_at_ms", "recency_at", "updated_at_ms", "updated_at"]
        .into_iter()
        .find(|column| columns.contains(*column))
        .unwrap_or("id");
    let sql = format!(
        "SELECT id, {title}, {model}, {effort}, {approval}, {sandbox}, {tokens}, {rollout} FROM threads{archived_filter} ORDER BY {order_by} DESC LIMIT ?1"
    );
    let root = database.parent().unwrap_or_else(|| Path::new("."));
    let mut statement = connection
        .prepare(&sql)
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    let rows = statement
        .query_map([i64::from(limit)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1).unwrap_or_default(),
                row.get::<_, Option<String>>(2).unwrap_or_default(),
                row.get::<_, Option<String>>(3).unwrap_or_default(),
                row.get::<_, Option<String>>(4).unwrap_or_default(),
                row.get::<_, Option<String>>(5).unwrap_or_default(),
                row.get::<_, Option<i64>>(6).unwrap_or_default(),
                row.get::<_, Option<String>>(7).unwrap_or_default(),
            ))
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;

    let mut contexts = Vec::with_capacity(rows.len());
    for (session_id, title, model, effort, approval, sandbox, tokens, rollout_path) in rows {
        let Some(session_id) = metadata_label(&session_id, MAX_CONFIG_VALUE_CHARS) else {
            continue;
        };
        let mut context = LocalSessionContext::new(crate::domain::AgentType::Codex, session_id);
        context.title = title.as_deref().and_then(local_prompt_title).or_else(|| {
            title
                .as_deref()
                .and_then(|value| metadata_label(value, MAX_SESSION_TITLE_CHARS))
                .filter(|t| {
                    let lower = t.to_lowercase();
                    lower != "untitled"
                        && lower != "未命名会话"
                        && !t.contains("AGENTS.md instructions")
                })
        });
        context.model = model.and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
        context.reasoning_effort =
            effort.and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
        context.approval_policy =
            approval.and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
        context.sandbox_mode = sandbox.and_then(|value| codex_sandbox_label(&value));
        context.session_total_tokens = tokens
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0);
        if let Some(rollout_path) = rollout_path {
            apply_codex_rollout_context(root, Path::new(&rollout_path), &mut context, &program)?;
        }
        contexts.push(context);
    }
    Ok(contexts)
}

fn codex_sandbox_label(value: &str) -> Option<String> {
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| {
            ["type", "mode"]
                .into_iter()
                .find_map(|field| value.get(field).and_then(Value::as_str))
                .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
        })
        .or_else(|| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
}

fn optional_column<'a>(columns: &HashSet<String>, candidates: &'a [&str]) -> &'a str {
    candidates
        .iter()
        .copied()
        .find(|column| columns.contains(*column))
        .unwrap_or("NULL")
}

fn apply_codex_rollout_context(
    root: &Path,
    rollout_path: &Path,
    context: &mut LocalSessionContext,
    program: &str,
) -> Result<(), CollectorError> {
    let root = fs::canonicalize(root).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let candidate = if rollout_path.is_absolute() {
        rollout_path.to_owned()
    } else {
        root.join(rollout_path)
    };
    let Ok(candidate) = fs::canonicalize(candidate) else {
        return Ok(());
    };
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Ok(());
    }
    let file = fs::File::open(&candidate).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?
        .len();
    let mut reader = BufReader::new(file);
    scan_codex_context_region(
        &mut reader,
        LOCAL_CONTEXT_HEAD_BYTES.min(length),
        context,
        program,
    )?;
    if length > LOCAL_CONTEXT_HEAD_BYTES {
        let tail_start = length.saturating_sub(LOCAL_CONTEXT_TAIL_BYTES);
        reader
            .seek(SeekFrom::Start(tail_start))
            .map_err(|source| CollectorError::Io {
                program: program.to_owned(),
                source,
            })?;
        scan_codex_context_region(
            &mut reader,
            length.saturating_sub(tail_start),
            context,
            program,
        )?;
    }
    Ok(())
}

fn scan_codex_context_region(
    reader: &mut impl BufRead,
    byte_limit: u64,
    context: &mut LocalSessionContext,
    program: &str,
) -> Result<(), CollectorError> {
    let mut consumed = 0_u64;
    while consumed < byte_limit {
        let line = match read_bounded_line(reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => continue,
            BoundedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: program.to_owned(),
                    source,
                });
            }
        };
        consumed = consumed.saturating_add(line.len() as u64);
        apply_codex_context_line(&line, context);
    }
    Ok(())
}

fn apply_codex_context_line(line: &str, context: &mut LocalSessionContext) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("turn_context") => {
            let payload = &value["payload"];
            context.model = config_value(payload, &["model"]).or_else(|| context.model.clone());
            context.reasoning_effort = config_value(payload, &["effort", "reasoning_effort"])
                .or_else(|| context.reasoning_effort.clone());
            context.approval_policy = config_value(payload, &["approval_policy", "approvalPolicy"])
                .or_else(|| context.approval_policy.clone());
            context.permission_mode =
                config_value(payload, &["permissions"]).or_else(|| context.permission_mode.clone());
            context.sandbox_mode = ["sandbox_policy", "sandboxPolicy", "sandbox"]
                .into_iter()
                .find_map(|field| payload.get(field).and_then(config_object_label))
                .or_else(|| context.sandbox_mode.clone());
        }
        Some("event_msg")
            if value.pointer("/payload/type").and_then(Value::as_str) == Some("token_count") =>
        {
            let info = &value["payload"]["info"];
            context.context_window_tokens = info
                .get("model_context_window")
                .and_then(Value::as_u64)
                .or(context.context_window_tokens);
            if let Some(last) = info.get("last_token_usage") {
                let input = usage_u64(last, "input_tokens");
                let output = usage_u64(last, "output_tokens");
                let cache_read = usage_u64(last, "cache_read_input_tokens");
                let cache_write = usage_u64(last, "cache_creation_input_tokens");
                let used = usage_u64(last, "total_tokens").max(input.saturating_add(cache_read));
                if used > 0 {
                    context.context_used_tokens = Some(used);
                    context.input_tokens = Some(input.saturating_add(cache_read));
                    context.output_tokens = Some(output);
                    context.cache_read_tokens = Some(cache_read);
                    context.cache_write_tokens = Some(cache_write);
                }
            }
            context.session_total_tokens = info
                .pointer("/total_token_usage/total_tokens")
                .and_then(Value::as_u64)
                .or(context.session_total_tokens);
        }
        Some("event_msg")
            if value.pointer("/payload/type").and_then(Value::as_str) == Some("task_started") =>
        {
            context.context_window_tokens = value
                .pointer("/payload/model_context_window")
                .and_then(Value::as_u64)
                .or(context.context_window_tokens);
        }
        Some("event_msg")
            if context.title.is_none()
                && value.pointer("/payload/type").and_then(Value::as_str)
                    == Some("user_message") =>
        {
            let prompt_text = value
                .pointer("/payload/message")
                .or_else(|| value.pointer("/payload/text"))
                .and_then(Value::as_str);
            if let Some(text) = prompt_text {
                context.title = local_prompt_title(text);
            }
        }
        Some("response_item")
            if context.title.is_none()
                && value.pointer("/payload/role").and_then(Value::as_str) == Some("user") =>
        {
            if let Some(content) = value.pointer("/payload/content").and_then(Value::as_array) {
                for item in content {
                    if let Some(title) = item
                        .get("text")
                        .and_then(Value::as_str)
                        .and_then(local_prompt_title)
                    {
                        context.title = Some(title);
                        break;
                    }
                }
            }
        }
        _ => {}
    }
}

fn config_value(value: &Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(Value::as_str))
        .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
}

fn config_object_label(value: &Value) -> Option<String> {
    value
        .as_str()
        .or_else(|| value.get("type").and_then(Value::as_str))
        .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
}

fn codex_title_expression(columns: &HashSet<String>) -> String {
    let mut parts = Vec::new();
    for col in &["title", "name", "first_user_message", "preview"] {
        if columns.contains(*col) {
            parts.push(format!("NULLIF(TRIM({col}), '')"));
        }
    }
    if parts.is_empty() {
        "NULL".to_owned()
    } else if parts.len() == 1 {
        parts.remove(0)
    } else {
        format!("COALESCE({})", parts.join(", "))
    }
}

fn seconds_expression(columns: &HashSet<String>, millis: &str, seconds: &str) -> String {
    match (columns.contains(millis), columns.contains(seconds)) {
        (true, true) => format!("COALESCE(NULLIF({millis}, 0) / 1000, {seconds})"),
        (true, false) => format!("{millis} / 1000"),
        (false, true) => seconds.to_owned(),
        (false, false) => "NULL".to_owned(),
    }
}

fn discover_codex_state_database() -> Result<Option<PathBuf>, CollectorError> {
    let root = if let Some(path) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(path)
    } else if let Some(home) = home_dir() {
        home.join(".codex")
    } else {
        return Ok(None);
    };
    if !root.is_dir() {
        return Ok(None);
    }
    let program = "Codex local state".to_owned();
    let mut candidates = fs::read_dir(root)
        .map_err(|source| CollectorError::Io {
            program: program.clone(),
            source,
        })?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let version = name
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse::<u32>()
                .ok()?;
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_file())
                .map(|_| (version, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    Ok(candidates.into_iter().next().map(|(_, path)| path))
}

const GROK_RECENT_SESSION_LIMIT: usize = 100;
const GROK_ALL_SESSION_LIMIT: usize = 1_000;
const GROK_SUMMARY_MAX_BYTES: u64 = 64 * 1024;
const GROK_ACTIVE_MAX_BYTES: u64 = 1024 * 1024;
const GROK_LIVE_WRITE_WINDOW: Duration = Duration::from_secs(20);

#[derive(Debug, Deserialize)]
struct GrokActiveSession {
    session_id: String,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    opened_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GrokSummaryFile {
    #[serde(default)]
    info: GrokSummaryInfo,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    last_active_at: Option<String>,
    #[serde(default)]
    generated_title: Option<String>,
    #[serde(default)]
    current_model_id: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    sandbox_profile: Option<String>,
    #[serde(default)]
    agent_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct GrokSummaryInfo {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Clone)]
struct GrokSessionRecord {
    session_id: String,
    cwd: String,
    state: &'static str,
    pid: Option<u32>,
    live: bool,
    source: &'static str,
    started_at_ms: Option<i64>,
    last_activity_at_ms: Option<i64>,
}

/// Enumerates Grok Build session metadata from `GROK_HOME` (default `~/.grok`)
/// without opening conversation logs. Live presence comes from
/// `active_sessions.json` plus a process-liveness check; working vs idle is
/// inferred only from `updates.jsonl` write recency or a lock file.
pub fn collect_grok_sessions(include_completed: bool) -> Result<String, CollectorError> {
    let home = discover_grok_home()?;
    collect_grok_sessions_from(&home, include_completed)
}

pub fn collect_grok_sessions_from(
    home: &Path,
    include_completed: bool,
) -> Result<String, CollectorError> {
    let program = "Grok session store".to_owned();
    let live = read_grok_active_sessions(home, &program)?;
    let mut records = scan_grok_summaries(home, &live, &program)?;
    records.sort_by_key(|record| std::cmp::Reverse(record.last_activity_at_ms));
    records.truncate(if include_completed {
        GROK_ALL_SESSION_LIMIT
    } else {
        GROK_RECENT_SESSION_LIMIT
    });
    serde_json::to_string(
        &records
            .into_iter()
            .map(|record| {
                json!({
                    "sessionId": record.session_id,
                    "cwd": record.cwd,
                    "state": record.state,
                    "pid": record.pid,
                    "live": record.live,
                    "source": record.source,
                    "startedAt": record.started_at_ms,
                    "lastActivityAt": record.last_activity_at_ms
                })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|source| CollectorError::InvalidJson { program, source })
}

pub fn collect_grok_session_contexts(
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let home = discover_grok_home()?;
    collect_grok_session_contexts_from(&home, limit)
}

pub fn collect_grok_session_contexts_from(
    home: &Path,
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let program = "Grok session context".to_owned();
    let sessions_dir = home.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for group in read_dir_entries(&sessions_dir, &program)? {
        if !group.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(children) = fs::read_dir(group.path()) else {
            continue;
        };
        for child in children.flatten() {
            if !child.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let summary = child.path().join("summary.json");
            if !summary.is_file() {
                continue;
            }
            let modified = child
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or_default();
            candidates.push((modified, summary, child.file_name()));
        }
    }
    candidates.sort_by_key(|(modified, _, _)| std::cmp::Reverse(*modified));
    candidates.truncate(limit);

    let mut contexts = Vec::with_capacity(candidates.len());
    for (_, summary_path, dir_name) in candidates {
        if let Some(context) = read_grok_session_context(&summary_path, &dir_name, &program)? {
            contexts.push(context);
        }
    }
    Ok(contexts)
}

pub fn find_grok_session_cwd(session_id: &str) -> Result<Option<PathBuf>, CollectorError> {
    let home = discover_grok_home()?;
    find_grok_session_cwd_from(&home, session_id)
}

pub fn find_grok_session_cwd_from(
    home: &Path,
    session_id: &str,
) -> Result<Option<PathBuf>, CollectorError> {
    let program = "Grok session store".to_owned();
    let sessions_dir = home.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok(None);
    }
    for group in read_dir_entries(&sessions_dir, &program)? {
        if !group.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let summary = group.path().join(session_id).join("summary.json");
        if !summary.is_file() {
            continue;
        }
        let Some(file) = read_bounded_json_file(&summary, GROK_SUMMARY_MAX_BYTES, &program)? else {
            continue;
        };
        let Ok(summary) = serde_json::from_str::<GrokSummaryFile>(&file) else {
            continue;
        };
        if let Some(cwd) = summary
            .info
            .cwd
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .filter(|cwd| cwd.is_absolute() && cwd.is_dir())
        {
            return Ok(Some(cwd));
        }
    }
    Ok(None)
}

fn discover_grok_home() -> Result<PathBuf, CollectorError> {
    if let Some(path) = env::var_os("GROK_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    home_dir()
        .map(|home| home.join(".grok"))
        .ok_or_else(|| CollectorError::Io {
            program: "Grok session store".to_owned(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot resolve the current user's home directory",
            ),
        })
}

fn read_grok_active_sessions(
    home: &Path,
    program: &str,
) -> Result<std::collections::HashMap<String, GrokActiveSession>, CollectorError> {
    let path = home.join("active_sessions.json");
    let Some(contents) = read_bounded_json_file(&path, GROK_ACTIVE_MAX_BYTES, program)? else {
        return Ok(std::collections::HashMap::new());
    };
    let Ok(entries) = serde_json::from_str::<Vec<GrokActiveSession>>(&contents) else {
        return Ok(std::collections::HashMap::new());
    };
    Ok(entries
        .into_iter()
        .filter(|entry| !entry.session_id.trim().is_empty())
        .map(|entry| (entry.session_id.clone(), entry))
        .collect())
}

fn scan_grok_summaries(
    home: &Path,
    live: &std::collections::HashMap<String, GrokActiveSession>,
    program: &str,
) -> Result<Vec<GrokSessionRecord>, CollectorError> {
    let sessions_dir = home.join("sessions");
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for group in read_dir_entries(&sessions_dir, program)? {
        if !group.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let group_cwd = read_grok_group_cwd(&group.path(), program)?;
        let Ok(children) = fs::read_dir(group.path()) else {
            continue;
        };
        for child in children.flatten() {
            if !child.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Some(record) =
                read_grok_session_record(&child.path(), group_cwd.as_deref(), live, program)?
            {
                records.push(record);
            }
        }
    }
    Ok(records)
}

fn read_grok_group_cwd(group: &Path, program: &str) -> Result<Option<String>, CollectorError> {
    let marker = group.join(".cwd");
    let Some(contents) = read_bounded_json_file(&marker, 4_096, program)? else {
        return Ok(None);
    };
    let cwd = contents.trim();
    Ok((!cwd.is_empty()).then(|| cwd.to_owned()))
}

fn read_grok_session_record(
    session_dir: &Path,
    group_cwd: Option<&str>,
    live: &std::collections::HashMap<String, GrokActiveSession>,
    program: &str,
) -> Result<Option<GrokSessionRecord>, CollectorError> {
    let summary_path = session_dir.join("summary.json");
    let Some(contents) = read_bounded_json_file(&summary_path, GROK_SUMMARY_MAX_BYTES, program)?
    else {
        return Ok(None);
    };
    let Ok(summary) = serde_json::from_str::<GrokSummaryFile>(&contents) else {
        return Ok(None);
    };
    let fallback_id = session_dir
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned);
    let session_id = summary
        .info
        .id
        .filter(|value| !value.trim().is_empty())
        .or(fallback_id)
        .filter(|value| !value.trim().is_empty());
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let active = live.get(&session_id);
    let pid = active
        .and_then(|entry| entry.pid)
        .filter(|pid| process_is_alive(*pid));
    let live_now = pid.is_some();
    let recently_written = grok_session_recently_written(session_dir);
    let (state, source) = if live_now && recently_written {
        ("working", "active")
    } else if live_now {
        ("idle", "active")
    } else {
        ("persisted", "sessions")
    };
    let cwd = summary
        .info
        .cwd
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            active
                .and_then(|entry| entry.cwd.clone())
                .filter(|value| !value.trim().is_empty())
        })
        .or_else(|| group_cwd.map(str::to_owned))
        .unwrap_or_default();
    Ok(Some(GrokSessionRecord {
        session_id,
        cwd,
        state,
        pid,
        live: live_now,
        source,
        started_at_ms: parse_rfc3339_millis(summary.created_at.as_deref())
            .or_else(|| parse_rfc3339_millis(active.and_then(|entry| entry.opened_at.as_deref()))),
        last_activity_at_ms: parse_rfc3339_millis(summary.last_active_at.as_deref())
            .or_else(|| parse_rfc3339_millis(summary.updated_at.as_deref()))
            .or_else(|| file_modified_millis(&summary_path)),
    }))
}

fn read_grok_session_context(
    summary_path: &Path,
    dir_name: &std::ffi::OsString,
    program: &str,
) -> Result<Option<LocalSessionContext>, CollectorError> {
    let Some(contents) = read_bounded_json_file(summary_path, GROK_SUMMARY_MAX_BYTES, program)?
    else {
        return Ok(None);
    };
    let Ok(summary) = serde_json::from_str::<GrokSummaryFile>(&contents) else {
        return Ok(None);
    };
    let fallback_id = dir_name.to_str().map(str::to_owned);
    let session_id = summary
        .info
        .id
        .or(fallback_id)
        .and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let mut context = LocalSessionContext::new(crate::domain::AgentType::GrokBuild, session_id);
    context.title = summary
        .generated_title
        .and_then(|value| metadata_label(&value, MAX_SESSION_TITLE_CHARS));
    if context.title.is_none() {
        let history = summary_path
            .parent()
            .map(|dir| dir.join("chat_history.jsonl"));
        if let Some(history) = history.filter(|path| path.is_file())
            && let Ok(file) = fs::File::open(history)
        {
            let mut reader = BufReader::new(file);
            for _ in 0..64 {
                let BoundedLine::Line(line) = read_bounded_line(&mut reader) else {
                    break;
                };
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let role = value
                    .get("role")
                    .or_else(|| value.pointer("/message/role"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !matches!(role, "user" | "human") {
                    continue;
                }
                let text = value
                    .get("content")
                    .or_else(|| value.pointer("/message/content"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                context.title = local_prompt_title(text);
                if context.title.is_some() {
                    break;
                }
            }
        }
    }
    context.model = summary
        .current_model_id
        .and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
    context.reasoning_effort = summary
        .reasoning_effort
        .and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
    context.sandbox_mode = summary
        .sandbox_profile
        .and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
    context.permission_mode = summary
        .agent_name
        .and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
    Ok(Some(context))
}

fn grok_session_recently_written(session_dir: &Path) -> bool {
    if session_dir.join("updates.jsonl.lock").is_file()
        || session_dir.join("chat_history.jsonl.lock").is_file()
    {
        return true;
    }
    let updates = session_dir.join("updates.jsonl");
    file_modified_ago(&updates).is_some_and(|age| age <= GROK_LIVE_WRITE_WINDOW)
}

fn file_modified_ago(path: &Path) -> Option<Duration> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified.elapsed().ok()
}

fn file_modified_millis(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

fn parse_rfc3339_millis(value: Option<&str>) -> Option<i64> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|value| value.timestamp_millis())
}

fn read_dir_entries(path: &Path, program: &str) -> Result<Vec<fs::DirEntry>, CollectorError> {
    let entries = fs::read_dir(path)
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(entries)
}

fn read_bounded_json_file(
    path: &Path,
    limit: u64,
    program: &str,
) -> Result<Option<String>, CollectorError> {
    if !path.is_file() {
        return Ok(None);
    }
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?
        .len();
    if length > limit {
        return Ok(None);
    }
    let mut contents = String::new();
    let mut reader = BufReader::new(file);
    reader
        .read_to_string(&mut contents)
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?;
    Ok(Some(contents))
}

fn process_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: the handle is either null or a process handle we close below.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0_u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE as u32
        }
    }
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        false
    }
}

fn write_prompt_file(prompt: &str) -> Result<tempfile::NamedTempFile, CollectorError> {
    let mut file = tempfile::NamedTempFile::new().map_err(|source| CollectorError::Io {
        program: "Grok prompt file".to_owned(),
        source,
    })?;
    file.write_all(prompt.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|source| CollectorError::Io {
            program: "Grok prompt file".to_owned(),
            source,
        })?;
    file.flush().map_err(|source| CollectorError::Io {
        program: "Grok prompt file".to_owned(),
        source,
    })?;
    Ok(file)
}

const ANTIGRAVITY_RECENT_SESSION_LIMIT: usize = 100;
const ANTIGRAVITY_ALL_SESSION_LIMIT: usize = 1_000;

pub fn discover_antigravity_home() -> Result<Option<PathBuf>, CollectorError> {
    if let Some(path) = env::var_os("ANTIGRAVITY_HOME").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return Ok(Some(path));
        }
    }
    if let Some(home) = home_dir() {
        let gemini_antigravity = home.join(".gemini").join("antigravity");
        if gemini_antigravity.is_dir() {
            return Ok(Some(gemini_antigravity));
        }
        let direct_antigravity = home.join(".antigravity");
        if direct_antigravity.is_dir() {
            return Ok(Some(direct_antigravity));
        }
    }
    Ok(None)
}

pub fn collect_antigravity_sessions(include_completed: bool) -> Result<String, CollectorError> {
    let Some(home) = discover_antigravity_home()? else {
        return Ok("[]".to_owned());
    };
    collect_antigravity_sessions_from(&home, include_completed)
}

pub fn collect_antigravity_sessions_from(
    home: &Path,
    include_completed: bool,
) -> Result<String, CollectorError> {
    let program = "Antigravity session store".to_owned();
    let brain_dir = home.join("brain");
    if !brain_dir.is_dir() {
        return Ok("[]".to_owned());
    }
    let entries = fs::read_dir(&brain_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })?;
    let mut candidate_transcripts = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|k| k.is_dir()) {
            continue;
        }
        let session_id = entry.file_name().to_string_lossy().to_string();
        if session_id.starts_with('.') {
            continue;
        }
        let transcript_path = entry
            .path()
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        if !transcript_path.is_file() {
            continue;
        }
        let modified_at_ms = file_modified_millis(&transcript_path).unwrap_or(0);
        candidate_transcripts.push((modified_at_ms, session_id, transcript_path));
    }

    candidate_transcripts.sort_by_key(|(m, _, _)| std::cmp::Reverse(*m));
    let max_limit = if include_completed {
        ANTIGRAVITY_ALL_SESSION_LIMIT
    } else {
        ANTIGRAVITY_RECENT_SESSION_LIMIT
    };
    candidate_transcripts.truncate(max_limit);

    let mut sessions = Vec::with_capacity(candidate_transcripts.len());
    for (modified_at_ms, session_id, transcript_path) in candidate_transcripts {
        let (started_at_ms, cwd, steps) =
            read_antigravity_transcript_sample(&transcript_path, &program)?;

        let last_activity_at_ms = if modified_at_ms > 0 {
            Some(modified_at_ms)
        } else {
            started_at_ms
        };
        sessions.push(json!({
            "sessionId": session_id,
            "cwd": cwd.unwrap_or_default(),
            "startedAt": started_at_ms,
            "lastActivityAt": last_activity_at_ms,
            "steps": steps,
        }));
    }

    serde_json::to_string(&sessions)
        .map_err(|source| CollectorError::InvalidJson { program, source })
}

fn extract_antigravity_cwd(value: &Value) -> Option<String> {
    if let Some(tool_calls) = value.get("tool_calls").and_then(Value::as_array) {
        for tc in tool_calls {
            if let Some(params) = tc.get("parameters") {
                if let Some(cwd) = params.get("Cwd").and_then(Value::as_str) {
                    let trimmed = cwd.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_owned());
                    }
                }
                for field in &["SearchDirectory", "TargetFile", "AbsolutePath"] {
                    if let Some(path_str) = params.get(*field).and_then(Value::as_str) {
                        let path = Path::new(path_str);
                        if let Some(parent) = path.parent() {
                            let s = parent.to_string_lossy();
                            if !s.trim().is_empty() {
                                return Some(s.trim().to_owned());
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(content) = value.get("content").and_then(Value::as_str) {
        if let Some(pos) =
            content.find("The mapping is shown as follows in the format [URI] -> [CorpusName]:")
        {
            let tail = &content[pos..];
            if let Some(line) = tail.lines().nth(1) {
                let parts: Vec<&str> = line.split("->").collect();
                if let Some(first) = parts.first() {
                    let cleaned = first.trim().trim_matches(|c| c == '[' || c == ']');
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_owned());
                    }
                }
            }
        }
        for prefix in &[
            "d:/", "D:/", "c:/", "C:/", "d:\\", "D:\\", "c:\\", "C:\\", "/Users/", "/home/",
        ] {
            if let Some(pos) = content.find(prefix) {
                let tail = &content[pos..];
                let end_pos = tail
                    .find(['\n', '\r', ' ', '"', '\'', '>', '`', ']'])
                    .unwrap_or(tail.len());
                let extracted = tail[..end_pos].trim();
                if !extracted.is_empty() {
                    return Some(extracted.to_owned());
                }
            }
        }
    }
    None
}

type AntigravitySampleResult = (Option<i64>, Option<String>, Vec<Value>);

fn read_antigravity_transcript_sample(
    path: &Path,
    program: &str,
) -> Result<AntigravitySampleResult, CollectorError> {
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut started_at_ms = None;
    let mut cwd = None;
    let mut recent_steps: std::collections::VecDeque<Value> =
        std::collections::VecDeque::with_capacity(32);

    let mut line_count = 0;
    while line_count < 64 {
        line_count += 1;
        let line = match read_bounded_line(&mut reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => continue,
            BoundedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: program.to_owned(),
                    source,
                });
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if started_at_ms.is_none() {
            started_at_ms = parse_rfc3339_millis(value.get("created_at").and_then(Value::as_str));
        }
        if cwd.is_none() {
            cwd = extract_antigravity_cwd(&value);
        }
        if recent_steps.len() >= 32 {
            recent_steps.pop_front();
        }
        recent_steps.push_back(value);
    }

    if cwd.is_none() {
        for step in recent_steps.iter().rev() {
            if let Some(found_cwd) = extract_antigravity_cwd(step) {
                cwd = Some(found_cwd);
                break;
            }
        }
    }

    let steps = recent_steps.into_iter().collect();
    Ok((started_at_ms, cwd, steps))
}

pub fn collect_antigravity_session_contexts(
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let Some(home) = discover_antigravity_home()? else {
        return Ok(Vec::new());
    };
    collect_antigravity_session_contexts_from(&home, limit)
}

pub fn collect_antigravity_session_contexts_from(
    home: &Path,
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let program = "Antigravity session context".to_owned();
    let brain_dir = home.join("brain");
    if !brain_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for entry in read_dir_entries(&brain_dir, &program)? {
        if !entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
            continue;
        }
        let transcript = entry
            .path()
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        if !transcript.is_file() {
            continue;
        }
        let modified = file_modified_millis(&transcript).unwrap_or(0);
        candidates.push((modified, transcript, entry.file_name()));
    }
    candidates.sort_by_key(|(m, _, _)| std::cmp::Reverse(*m));
    candidates.truncate(limit);

    let mut contexts = Vec::with_capacity(candidates.len());
    for (_, transcript, dir_name) in candidates {
        let session_id = dir_name.to_string_lossy().to_string();
        let mut context =
            LocalSessionContext::new(crate::domain::AgentType::Antigravity, session_id);
        context.model = Some("Gemini 3.7 Pro / Flash".into());
        context.permission_mode = Some("local_confirmation".into());

        if let Ok(file) = fs::File::open(&transcript) {
            let mut reader = BufReader::new(file);
            for _ in 0..32 {
                let line = match read_bounded_line(&mut reader) {
                    BoundedLine::Line(line) => line,
                    _ => break,
                };
                let Ok(val) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if val.get("type").and_then(Value::as_str) == Some("USER_INPUT") {
                    let content = val.get("content").and_then(Value::as_str).unwrap_or("");
                    context.title = local_prompt_title(content);
                    if context.title.is_some() {
                        break;
                    }
                }
            }
        }

        contexts.push(context);
    }
    Ok(contexts)
}

fn local_prompt_title(text: &str) -> Option<String> {
    let text = if let Some(start) = text.find("<USER_REQUEST>") {
        let request = &text[start + "<USER_REQUEST>".len()..];
        &request[..request.find("</USER_REQUEST>").unwrap_or(request.len())]
    } else {
        text
    };
    if [
        "<local-command-caveat>",
        "<command-name>",
        "<task-notification>",
    ]
    .iter()
    .any(|tag| text.contains(tag))
    {
        return None;
    }

    let mut in_skip_block = false;
    let skip_tags_start = [
        "<INSTRUCTIONS>",
        "<instructions>",
        "<environment_context>",
        "<permissions instructions>",
        "<system-reminder>",
    ];
    let skip_tags_end = [
        "</INSTRUCTIONS>",
        "</instructions>",
        "</environment_context>",
        "</permissions instructions>",
        "</system-reminder>",
    ];

    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }

        if skip_tags_start.iter().any(|tag| line.contains(tag)) {
            in_skip_block = true;
        }
        if in_skip_block {
            if skip_tags_end.iter().any(|tag| line.contains(tag)) {
                in_skip_block = false;
            }
            continue;
        }

        let clean = line
            .trim_start_matches(['#', ' ', '>', '-', '*', ':'])
            .trim();
        if clean.is_empty() || clean.starts_with('<') || clean.starts_with("<!--") {
            continue;
        }
        if clean.starts_with("AGENTS.md instructions")
            || clean.starts_with("Instructions for")
            || clean.starts_with("You are ")
        {
            continue;
        }
        let lower = clean.to_lowercase();
        if lower == "untitled" || lower == "未命名会话" || lower == "none" || lower == "null" {
            continue;
        }
        return metadata_label(clean, MAX_SESSION_TITLE_CHARS);
    }
    None
}

pub fn find_antigravity_session_cwd(session_id: &str) -> Result<Option<PathBuf>, CollectorError> {
    let Some(home) = discover_antigravity_home()? else {
        return Ok(None);
    };
    let transcript = home
        .join("brain")
        .join(session_id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl");
    if !transcript.is_file() {
        return Ok(None);
    }
    let (_, cwd, _) = read_antigravity_transcript_sample(&transcript, "Antigravity session store")?;
    Ok(cwd.map(PathBuf::from))
}

const ZCODE_RECENT_SESSION_LIMIT: usize = 1_000;
const ZCODE_ALL_SESSION_LIMIT: usize = 10_000;

pub fn discover_zcode_database() -> Result<Option<PathBuf>, CollectorError> {
    if let Some(path) = env::var_os("ZCODE_DB").filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(Some(path));
        }
    }
    if let Some(home) = home_dir() {
        let db = home.join(".zcode").join("cli").join("db").join("db.sqlite");
        if db.is_file() {
            return Ok(Some(db));
        }
    }
    Ok(None)
}

pub fn collect_zcode_sessions(include_completed: bool) -> Result<String, CollectorError> {
    let Some(database) = discover_zcode_database()? else {
        return Ok("[]".to_owned());
    };
    collect_zcode_sessions_from(&database, include_completed)
}

struct ZCodeSessionsCache {
    db_path: PathBuf,
    include_completed: bool,
    db_mtime: Option<SystemTime>,
    cached_at: Instant,
    payload: String,
}

fn zcode_sessions_cache() -> &'static RwLock<Option<ZCodeSessionsCache>> {
    static CACHE: OnceLock<RwLock<Option<ZCodeSessionsCache>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

#[derive(Default)]
struct ZCodeAgg {
    calls: u64,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    reasoning_tokens: u64,
}

fn aggregate_zcode_root_stats(connection: &Connection) -> HashMap<String, ZCodeAgg> {
    let mut parent_map: HashMap<String, String> = HashMap::new();
    if let Ok(mut stmt) = connection.prepare(
        "SELECT id, parent_id FROM session WHERE parent_id IS NOT NULL AND parent_id != ''",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let parent_id: String = row.get(1)?;
            Ok((id, parent_id))
        }) {
            for (id, parent_id) in rows.flatten() {
                parent_map.insert(id, parent_id);
            }
        }
    }

    let mut root_stats: HashMap<String, ZCodeAgg> = HashMap::new();
    let msg_sql = r#"
        SELECT
            session_id,
            COUNT(id),
            COALESCE(SUM(CAST(json_extract(data, '$.tokens.total') AS INTEGER)), 0),
            COALESCE(SUM(CAST(json_extract(data, '$.tokens.input') AS INTEGER)), 0),
            COALESCE(SUM(CAST(json_extract(data, '$.tokens.output') AS INTEGER)), 0),
            COALESCE(SUM(CAST(json_extract(data, '$.tokens.cache.read') AS INTEGER)), 0),
            COALESCE(SUM(CAST(json_extract(data, '$.tokens.cache.write') AS INTEGER)), 0),
            COALESCE(SUM(CAST(json_extract(data, '$.tokens.reasoning') AS INTEGER)), 0)
        FROM message
        WHERE data LIKE '%"role":"assistant"%'
        GROUP BY session_id
    "#;

    if let Ok(mut stmt) = connection.prepare(msg_sql) {
        if let Ok(rows) = stmt.query_map([], |row| {
            let session_id: String = row.get(0)?;
            let calls: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let input: i64 = row.get(3)?;
            let output: i64 = row.get(4)?;
            let cache_read: i64 = row.get(5)?;
            let cache_write: i64 = row.get(6)?;
            let reasoning: i64 = row.get(7)?;
            Ok((
                session_id,
                calls,
                total,
                input,
                output,
                cache_read,
                cache_write,
                reasoning,
            ))
        }) {
            for (sid, calls, total, input, output, cache_read, cache_write, reasoning) in
                rows.flatten()
            {
                let mut root_id = sid.as_str();
                let mut visited = HashSet::new();
                while let Some(parent) = parent_map.get(root_id) {
                    if !visited.insert(root_id) {
                        break;
                    }
                    root_id = parent.as_str();
                }
                let entry = root_stats.entry(root_id.to_string()).or_default();
                entry.calls = entry.calls.saturating_add(calls.max(0) as u64);
                entry.total_tokens = entry.total_tokens.saturating_add(total.max(0) as u64);
                entry.input_tokens = entry.input_tokens.saturating_add(input.max(0) as u64);
                entry.output_tokens = entry.output_tokens.saturating_add(output.max(0) as u64);
                entry.cache_read_tokens = entry
                    .cache_read_tokens
                    .saturating_add(cache_read.max(0) as u64);
                entry.cache_write_tokens = entry
                    .cache_write_tokens
                    .saturating_add(cache_write.max(0) as u64);
                entry.reasoning_tokens = entry
                    .reasoning_tokens
                    .saturating_add(reasoning.max(0) as u64);
            }
        }
    }
    root_stats
}

pub fn collect_zcode_sessions_from(
    database: &Path,
    include_completed: bool,
) -> Result<String, CollectorError> {
    let mtime = fs::metadata(database).ok().and_then(|m| m.modified().ok());
    if let Ok(guard) = zcode_sessions_cache().read() {
        if let Some(ref c) = *guard {
            if c.db_path == database
                && c.include_completed == include_completed
                && c.db_mtime == mtime
                && c.cached_at.elapsed() < Duration::from_secs(5)
            {
                return Ok(c.payload.clone());
            }
        }
    }

    let program = "ZCode session store".to_owned();
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| CollectorError::Database {
        program: program.clone(),
        source,
    })?;

    let limit = if include_completed {
        ZCODE_ALL_SESSION_LIMIT
    } else {
        ZCODE_RECENT_SESSION_LIMIT
    };

    let root_stats = aggregate_zcode_root_stats(&connection);

    let sql = r#"
        SELECT
            s.id,
            COALESCE(
                NULLIF(TRIM(s.title), ''),
                (SELECT json_extract(p.data, '$.text')
                 FROM part p
                 JOIN message m ON p.message_id = m.id
                 WHERE m.session_id = s.id
                   AND json_extract(m.data, '$.role') = 'user'
                   AND json_extract(p.data, '$.type') = 'text'
                 ORDER BY m.sequence ASC, p.sequence ASC
                 LIMIT 1)
            ) as title,
            s.directory,
            s.time_created,
            s.time_updated,
            (SELECT json_extract(data, '$.modelID') FROM message WHERE session_id = s.id AND data LIKE '%"role":"assistant"%' ORDER BY time_created DESC LIMIT 1) as model
        FROM session s
        WHERE (s.parent_id IS NULL OR s.parent_id = '')
          AND s.id NOT LIKE 'sess_subagent%'
        ORDER BY s.time_updated DESC
        LIMIT ?1
    "#;

    let mut statement = connection
        .prepare(sql)
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;

    let rows = statement
        .query_map([limit as i64], |row| {
            let session_id: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let cwd: Option<String> = row.get(2)?;
            let time_created: i64 = row.get(3)?;
            let time_updated: i64 = row.get(4)?;
            let model: Option<String> = row.get(5)?;

            let stats = root_stats.get(&session_id);
            let (total_tokens, input_tokens, output_tokens, cache_read_tokens, request_count) =
                if let Some(st) = stats {
                    (
                        Some(st.total_tokens),
                        Some(st.input_tokens),
                        Some(st.output_tokens),
                        Some(st.cache_read_tokens),
                        Some(st.calls.max(1)),
                    )
                } else {
                    (None, None, None, None, Some(1))
                };

            Ok(crate::adapters::zcode::ZCodeSessionEntry {
                session_id,
                title,
                cwd: cwd.unwrap_or_default(),
                time_created,
                time_updated,
                latest_role: None,
                latest_part_type: None,
                latest_tool_name: None,
                latest_tool_status: None,
                is_running: false,
                model,
                total_tokens,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                request_count,
            })
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;

    let payload = serde_json::to_string(&rows)
        .map_err(|source| CollectorError::InvalidJson { program, source })?;
    if let Ok(mut guard) = zcode_sessions_cache().write() {
        *guard = Some(ZCodeSessionsCache {
            db_path: database.to_path_buf(),
            include_completed,
            db_mtime: mtime,
            cached_at: Instant::now(),
            payload: payload.clone(),
        });
    }
    Ok(payload)
}

pub fn collect_zcode_session_contexts(
    limit: u32,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let Some(database) = discover_zcode_database()? else {
        return Ok(Vec::new());
    };
    collect_zcode_session_contexts_from(&database, limit)
}

pub fn collect_zcode_session_contexts_from(
    database: &Path,
    limit: u32,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let program = "ZCode local context".to_owned();
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| CollectorError::Database {
        program: program.clone(),
        source,
    })?;

    let root_stats = aggregate_zcode_root_stats(&connection);

    let sql = r#"
        SELECT
            s.id,
            COALESCE(
                NULLIF(TRIM(s.title), ''),
                (SELECT json_extract(p.data, '$.text')
                 FROM part p
                 JOIN message m ON p.message_id = m.id
                 WHERE m.session_id = s.id
                   AND json_extract(m.data, '$.role') = 'user'
                   AND json_extract(p.data, '$.type') = 'text'
                 ORDER BY m.sequence ASC, p.sequence ASC
                 LIMIT 1)
            ) as title,
            (SELECT json_extract(data, '$.modelID') FROM message WHERE session_id = s.id AND data LIKE '%"role":"assistant"%' ORDER BY time_created DESC LIMIT 1) as model,
            (SELECT json_extract(data, '$.mode') FROM message WHERE session_id = s.id AND data LIKE '%"role":"assistant"%' ORDER BY time_created DESC LIMIT 1) as mode
        FROM session s
        WHERE (s.parent_id IS NULL OR s.parent_id = '')
          AND s.id NOT LIKE 'sess_subagent%'
        ORDER BY s.time_updated DESC
        LIMIT ?1
    "#;

    let mut statement = connection
        .prepare(sql)
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;

    let rows = statement
        .query_map([limit as i64], |row| {
            let session_id: String = row.get(0)?;
            let title: Option<String> = row.get(1)?;
            let model: Option<String> = row.get(2)?;
            let mode: Option<String> = row.get(3)?;

            let stats = root_stats.get(&session_id);
            let (
                total_tokens,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                reasoning_tokens,
            ) = if let Some(st) = stats {
                (
                    Some(st.total_tokens as i64),
                    Some(st.input_tokens as i64),
                    Some(st.output_tokens as i64),
                    Some(st.cache_read_tokens as i64),
                    Some(st.cache_write_tokens as i64),
                    Some(st.reasoning_tokens as i64),
                )
            } else {
                (None, None, None, None, None, None)
            };

            Ok((
                session_id,
                title,
                model,
                mode,
                total_tokens,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                reasoning_tokens,
            ))
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;

    let mut contexts = Vec::with_capacity(rows.len());
    for (
        session_id,
        title,
        model,
        mode,
        total_tokens,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_tokens,
    ) in rows
    {
        let Some(clean_id) = metadata_label(&session_id, MAX_CONFIG_VALUE_CHARS) else {
            continue;
        };
        let mut context = LocalSessionContext::new(crate::domain::AgentType::ZCode, clean_id);
        context.title = title.and_then(|t| metadata_label(&t, MAX_SESSION_TITLE_CHARS));
        context.model = model.and_then(|m| metadata_label(&m, MAX_CONFIG_VALUE_CHARS));
        context.permission_mode = mode.and_then(|m| metadata_label(&m, MAX_CONFIG_VALUE_CHARS));
        context.session_total_tokens = total_tokens
            .and_then(|t| u64::try_from(t).ok())
            .filter(|t| *t > 0);
        let used = input_tokens
            .and_then(|t| u64::try_from(t).ok())
            .filter(|t| *t > 0)
            .or(context.session_total_tokens);
        context.context_used_tokens = used;
        context.input_tokens = input_tokens
            .and_then(|t| u64::try_from(t).ok())
            .filter(|t| *t > 0);
        context.output_tokens = output_tokens.and_then(|t| u64::try_from(t).ok());
        context.cache_read_tokens = cache_read_tokens.and_then(|t| u64::try_from(t).ok());
        context.cache_write_tokens = cache_write_tokens.and_then(|t| u64::try_from(t).ok());
        context.reasoning_tokens = reasoning_tokens.and_then(|t| u64::try_from(t).ok());
        if let Some(ref m) = context.model {
            context.context_window_tokens = resolve_zcode_model_context_window(m);
        }
        contexts.push(context);
    }
    Ok(contexts)
}

pub fn resolve_zcode_model_context_window(model_name: &str) -> Option<u64> {
    if let Some(home) = home_dir() {
        let config_path = home.join(r".zcode\v2\config.json");
        if let Ok(data) = fs::read_to_string(&config_path)
            && let Ok(json) = serde_json::from_str::<Value>(&data)
            && let Some(providers_obj) = json.get("provider").and_then(Value::as_object)
        {
            for (_, pdata) in providers_obj {
                if let Some(models_obj) = pdata.get("models").and_then(Value::as_object) {
                    for (mid, mdata) in models_obj {
                        if (mid.eq_ignore_ascii_case(model_name)
                            || model_name.contains(mid)
                            || mid.contains(model_name))
                            && let Some(ctx) =
                                mdata.pointer("/limit/context").and_then(Value::as_u64)
                        {
                            return Some(ctx);
                        }
                    }
                }
            }
        }
    }
    let lower = model_name.to_lowercase();
    if lower.contains("gemini") {
        Some(1_000_000)
    } else if lower.contains("claude")
        || lower.contains("gpt-5")
        || lower.contains("gpt-4")
        || lower.contains("o1")
        || lower.contains("o3")
    {
        Some(200_000)
    } else {
        Some(128_000)
    }
}

pub fn find_zcode_session_cwd(session_id: &str) -> Result<Option<PathBuf>, CollectorError> {
    let Some(database) = discover_zcode_database()? else {
        return Ok(None);
    };
    let program = "ZCode session store".to_owned();
    let connection = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| CollectorError::Database {
        program: program.clone(),
        source,
    })?;

    let mut statement = connection
        .prepare("SELECT directory FROM session WHERE id = ?1 LIMIT 1")
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;

    let cwd = statement
        .query_row([session_id], |row| row.get::<_, Option<String>>(0))
        .ok()
        .flatten()
        .filter(|dir| !dir.trim().is_empty())
        .map(PathBuf::from);

    Ok(cwd)
}

/// Resume an existing local Agent session and return only the child process'
/// bounded stdout. Claude/Codex prompts go through stdin; Grok Build uses a
/// temporary `--prompt-file` so the prompt never appears in the process list.
/// Callers must keep the returned content local; unlike normal collection, this
/// function intentionally handles conversation content after an explicit user
/// action.
pub fn run_agent_session_turn(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: Option<&Path>,
    session_id: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String, CollectorError> {
    run_agent_session_turn_streaming(
        agent,
        binary,
        project_path,
        session_id,
        prompt,
        TurnOptions::default(),
        timeout,
        &AtomicBool::new(false),
        |_| {},
    )
    .map(|outcome| outcome.response)
}

/// Resume an Agent session while forwarding safe, user-visible progress.
/// Conversation text is never logged or persisted by this function. The
/// caller owns the callback destination and cancellation flag.
///
/// Resumes an existing agent conversation and streams the turn's updates.
///
/// When the chosen binary reports that it cannot read the session, the turn is
/// retried against the remaining local builds of the same CLI. A recorded
/// session-to-binary affinity is tried first, so the second attempt is usually
/// the binary that already worked for this session.
///
/// The retry is only safe because an unreadable-session failure happens before
/// the model is ever called: no prompt was delivered, so no other binary can
/// observe a partially executed turn. The affinity map records which binary
/// worked so later resumes of the same session skip the failed candidate.
#[allow(clippy::too_many_arguments)]
/// The callback is `Clone` because a retried attempt needs a second copy: the
/// first is moved into the failed process's reader thread. Callers already
/// build these as `Arc`-based handles, so cloning is a refcount bump.
pub fn run_agent_session_turn_streaming(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: Option<&Path>,
    session_id: &str,
    prompt: &str,
    options: TurnOptions<'_>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + Clone + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    let preferred = binary.as_ref();
    // Both attempts get their own copy: whichever runs first moves its copy
    // into that process's reader thread, so a retry still has one to hand.
    let first_attempt = on_update.clone();
    let retry_attempt = on_update.clone();
    match run_agent_session_turn_streaming_once(
        agent,
        preferred,
        project_path,
        session_id,
        prompt,
        options.clone(),
        timeout,
        cancelled,
        first_attempt,
    ) {
        Ok(outcome) => Ok(outcome),
        Err(CollectorError::SessionUnreadable { program }) => {
            let Some(candidate) = retry_with_alternate_binary(agent, preferred, session_id) else {
                return Err(CollectorError::SessionUnreadable { program });
            };
            match run_agent_session_turn_streaming_once(
                agent,
                &candidate,
                project_path,
                session_id,
                prompt,
                options.clone(),
                timeout,
                cancelled,
                retry_attempt,
            ) {
                Ok(outcome) => Ok(outcome),
                // A second unreadable-session failure means no local build can
                // read this transcript; the original error is the honest one
                // to report.
                Err(_) => Err(CollectorError::SessionUnreadable { program }),
            }
        }
        Err(other) => Err(other),
    }
}

/// Picks the binary to retry a failed resume with: a previously-recorded
/// working binary for this session, else the best remaining candidate.
///
/// Returns `None` when discovery offers nothing beyond the one that just
/// failed, so a single-install machine reports the error instead of paying for
/// a pointless second process.
fn retry_with_alternate_binary(
    agent: crate::domain::AgentType,
    failed: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    let candidates = match agent {
        crate::domain::AgentType::ClaudeCode => claude_binary_candidates(),
        _ => return None,
    };
    let remaining: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| candidate.path != failed)
        .collect();
    if remaining.is_empty() {
        return None;
    }
    let session_key = format!("claude:{session_id}");
    let mut affinity = crate::agent_binary::BinaryAffinity::load();
    if let Some(known) = affinity.lookup(&session_key, &remaining) {
        return Some(known);
    }
    let best = remaining[0].path.clone();
    affinity.record(&session_key, &best);
    Some(best)
}

/// Inspects the Claude Code session transcript to find the model recorded for the session.
pub fn find_claude_session_model(session_id: &str) -> Option<String> {
    let home = home_dir()?;
    let raw_session_id = session_id.strip_prefix("claude_").unwrap_or(session_id);
    let path = crate::conversation::find_transcript_file(
        &home.join(".claude").join("projects"),
        raw_session_id,
        0,
    )?;
    let file = fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    for line in reader.lines().take(60).flatten() {
        if let Ok(val) = serde_json::from_str::<Value>(&line) {
            if let Some(model) = val.pointer("/message/model").and_then(Value::as_str) {
                let trimmed = model.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            if let Some(model) = val
                .pointer("/attachment/identity/modelId")
                .and_then(Value::as_str)
            {
                let trimmed = model.trim();
                if !trimmed.is_empty() {
                    let cleaned = trimmed.split('[').next().unwrap_or(trimmed).trim();
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_string());
                    }
                }
            }
            if let Some(model) = val.get("model").and_then(Value::as_str) {
                let trimmed = model.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn run_agent_session_turn_streaming_once(
    agent: crate::domain::AgentType,
    binary: &Path,
    project_path: Option<&Path>,
    session_id: &str,
    prompt: &str,
    options: TurnOptions<'_>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    let program = binary.display().to_string();
    let mut command = Command::new(binary);
    let prompt_file = match agent {
        crate::domain::AgentType::ClaudeCode => {
            let session_exists = match options.session_exists {
                Some(exists) => exists,
                None => {
                    !session_id.is_empty()
                        && find_claude_session_info(session_id)
                            .ok()
                            .flatten()
                            .is_some()
                }
            };
            if session_exists {
                command.args([
                    "--resume",
                    session_id,
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                ]);
            } else {
                command.args([
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                ]);
            }
            None
        }
        crate::domain::AgentType::Codex => {
            command.args([
                "exec",
                "resume",
                session_id,
                "--json",
                "--color",
                "never",
                "--skip-git-repo-check",
                "-",
            ]);
            None
        }
        crate::domain::AgentType::Antigravity => {
            command.args([
                "--conversation",
                session_id,
                "--prompt",
                prompt,
                "--output-format",
                "stream-json",
            ]);
            None
        }
        crate::domain::AgentType::GrokBuild => {
            let prompt_file = write_prompt_file(prompt)?;
            let prompt_path = prompt_file
                .path()
                .to_str()
                .ok_or_else(|| CollectorError::Io {
                    program: program.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "prompt file path is not valid UTF-8",
                    ),
                })?;
            command.args([
                "--resume",
                session_id,
                "--prompt-file",
                prompt_path,
                "--output-format",
                "streaming-json",
            ]);
            Some(prompt_file)
        }
        crate::domain::AgentType::ZCode => {
            command.args(["--session", session_id, "--prompt", prompt]);
            None
        }
    };
    let mut effective_model = options.model.map(str::to_string);
    if agent == crate::domain::AgentType::ClaudeCode {
        if effective_model.is_none() && options.session_exists != Some(false) {
            effective_model = find_claude_session_model(session_id);
        }
        if let Some(gateway) = crate::claude_gateway::discover_claude_gateway() {
            gateway.inject_into_command_with_session(
                &mut command,
                &mut effective_model,
                Some(session_id),
            );
        }
    }
    let effective_model_ref = effective_model.as_deref();
    let effective_options = TurnOptions {
        model: effective_model_ref,
        ..options
    };
    apply_turn_options(&mut command, agent, &effective_options);
    if let Some(project_path) = project_path {
        command.current_dir(project_path);
    }
    run_agent_stream_process(
        agent,
        command,
        program,
        prompt,
        prompt_file,
        timeout,
        cancelled,
        on_update,
    )
}

/// Starts a brand-new agent conversation in `cwd` and streams the first turn.
/// The session id is recovered from the CLI's stream output so the panel can
/// switch to the real conversation immediately.
#[allow(clippy::too_many_arguments)] // Legacy one-shot API; retained until client migration.
pub fn run_agent_session_start_streaming(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    cwd: &Path,
    prompt: &str,
    options: TurnOptions<'_>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    let binary = binary.as_ref();
    let program = binary.display().to_string();
    let mut command = Command::new(binary);
    let prompt_file = match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::Antigravity => {
            command.args([
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ]);
            None
        }
        crate::domain::AgentType::Codex => {
            command.args([
                "exec",
                "--json",
                "--color",
                "never",
                "--skip-git-repo-check",
                "-",
            ]);
            None
        }
        crate::domain::AgentType::GrokBuild => {
            let prompt_file = write_prompt_file(prompt)?;
            let prompt_path = prompt_file
                .path()
                .to_str()
                .ok_or_else(|| CollectorError::Io {
                    program: program.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "prompt file path is not valid UTF-8",
                    ),
                })?;
            command.args([
                "--prompt-file",
                prompt_path,
                "--output-format",
                "streaming-json",
            ]);
            Some(prompt_file)
        }
        crate::domain::AgentType::ZCode => {
            command.args(["--prompt", prompt]);
            None
        }
    };
    let mut effective_model = options.model.map(str::to_string);
    if agent == crate::domain::AgentType::ClaudeCode
        && let Some(gateway) = crate::claude_gateway::discover_claude_gateway()
    {
        // For fresh sessions, compute implicit fingerprint from prompt so upstream gateway retains prompt cache
        let trimmed_prompt = prompt.trim();
        let implicit_sid = if !trimmed_prompt.is_empty() {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(trimmed_prompt.as_bytes());
            let hash = format!("{:x}", hasher.finalize());
            Some(format!("magpie-{}", &hash[..12]))
        } else {
            None
        };
        gateway.inject_into_command_with_session(
            &mut command,
            &mut effective_model,
            implicit_sid.as_deref(),
        );
    }
    let effective_model_ref = effective_model.as_deref();
    let effective_options = TurnOptions {
        model: effective_model_ref,
        ..options
    };
    apply_turn_options(&mut command, agent, &effective_options);
    command.current_dir(cwd);
    run_agent_stream_process(
        agent,
        command,
        program,
        prompt,
        prompt_file,
        timeout,
        cancelled,
        on_update,
    )
}

fn apply_turn_options(
    command: &mut Command,
    agent: crate::domain::AgentType,
    options: &TurnOptions<'_>,
) {
    let model = options
        .model
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let permission_mode = options
        .permission_mode
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let effort = options
        .reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty());
    // The reasoning flag differs per CLI, so it is applied even when no model
    // override was picked: effort alone is a valid per-turn override.
    if let Some(effort) = effort {
        match agent {
            crate::domain::AgentType::ClaudeCode => {
                command.args(["--effort", effort]);
            }
            // Codex has no --effort flag; reasoning effort is a config key.
            crate::domain::AgentType::Codex => {
                command.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
            }
            crate::domain::AgentType::ZCode
            | crate::domain::AgentType::Antigravity
            | crate::domain::AgentType::GrokBuild => {}
        }
    }
    match agent {
        crate::domain::AgentType::ClaudeCode => {
            if let Some(model) = model {
                command.args(["--model", model]);
            }
            if let Some(mode) = permission_mode {
                command.args(["--permission-mode", mode]);
            }
        }
        crate::domain::AgentType::Codex => {
            if let Some(model) = model {
                command.args(["-m", model]);
            }
            if let Some(mode) = permission_mode {
                let sandbox = match mode {
                    "read-only" | "plan" => "read-only",
                    "bypassPermissions" | "danger-full-access" => "danger-full-access",
                    _ => "workspace-write",
                };
                command.args(["--sandbox", sandbox]);
            }
        }
        crate::domain::AgentType::ZCode => {
            if let Some(model) = model {
                command.args(["--model", model]);
            }
        }
        crate::domain::AgentType::Antigravity | crate::domain::AgentType::GrokBuild => {}
    }
}

#[allow(clippy::too_many_arguments)] // Shared by the legacy CLI wrappers.
fn run_agent_stream_process(
    agent: crate::domain::AgentType,
    mut command: Command,
    program: String,
    prompt: &str,
    prompt_file: Option<tempfile::NamedTempFile>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    command
        .stdin(if prompt_file.is_some() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn().map_err(|source| CollectorError::Start {
        program: program.clone(),
        source,
    })?;
    let job = ChildJob::attach(&child).map_err(|source| {
        let _ = child.kill();
        let _ = child.wait();
        CollectorError::Io {
            program: program.clone(),
            source,
        }
    })?;
    if prompt_file.is_none() {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        stdin
            .write_all(prompt.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .map_err(|source| CollectorError::Io {
                program: program.clone(),
                source,
            })?;
        drop(stdin);
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CollectorError::MissingStdio {
            program: program.clone(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CollectorError::MissingStdio {
            program: program.clone(),
        })?;
    let stdout_reader = read_agent_turn_output(stdout, agent, on_update);
    let stderr_reader = read_child_output(stderr);
    let status = wait_for_child_with_cancel(&mut child, &program, timeout, Some(cancelled));
    drop(job);
    let stdout = join_child_output(stdout_reader, &program)?;
    // Drain stderr to prevent a full pipe from blocking the child, but never
    // return it: provider diagnostics can contain local paths or prompt text.
    let _ = join_child_output(stderr_reader, &program)?;
    drop(prompt_file);
    let status = status?;
    if !status.success() {
        // Distinguish "this build cannot read that transcript" from a generic
        // non-zero exit before discarding stdout. Only the former is worth
        // retrying against a different binary; replaying a turn that may have
        // partially executed is exactly the duplicate side effect the remote
        // command contract forbids.
        if let Ok(output) = String::from_utf8(stdout) {
            if crate::agent_binary::is_unreadable_session_failure(&program, &output) {
                return Err(CollectorError::SessionUnreadable { program });
            }
            // Propagate explicit API errors (e.g. rate limit / quota exhaustion / auth failures)
            // as a completed turn response so the turn ends cleanly, realSessionId is preserved,
            // and neither the daemon binding nor the desktop UI is left in a wedged running state.
            if let Some(err_msg) = final_agent_message(agent, &output) {
                let session_id = extract_session_id_from_output(agent, &output);
                return Ok(AgentStreamOutcome {
                    response: err_msg,
                    session_id,
                    usage: extract_context_usage_from_output(agent, &output),
                });
            }
        }
        return Err(CollectorError::Exit {
            program,
            status: status.to_string(),
        });
    }
    if stdout.len() > MAX_AGENT_TURN_OUTPUT_BYTES {
        return Err(CollectorError::OutputTooLarge {
            program,
            limit: MAX_AGENT_TURN_OUTPUT_BYTES,
        });
    }
    let output = String::from_utf8(stdout).map_err(|source| CollectorError::InvalidUtf8 {
        program: program.clone(),
        source,
    })?;
    let response =
        final_agent_message(agent, &output).unwrap_or_else(|| "（任务执行完成）".to_owned());
    let session_id = extract_session_id_from_output(agent, &output);
    Ok(AgentStreamOutcome {
        response,
        session_id,
        usage: extract_context_usage_from_output(agent, &output),
    })
}

/// The last usage report a CLI emitted for this turn. Only providers that
/// state consumption in their stream are covered; the rest leave it `None`
/// and the console honestly says the capacity was not reported.
fn extract_context_usage_from_output(
    agent: crate::domain::AgentType,
    output: &str,
) -> Option<ContextUsage> {
    match agent {
        crate::domain::AgentType::ClaudeCode => output.lines().rev().find_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            claude_result_context_usage(&value)
        }),
        _ => None,
    }
}

fn extract_session_id_from_output(agent: crate::domain::AgentType, output: &str) -> Option<String> {
    output.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<Value>(line).ok()?;
        let raw = match agent {
            crate::domain::AgentType::ClaudeCode => ["session_id"]
                .into_iter()
                .find_map(|field| value.get(field).and_then(Value::as_str))
                .filter(|_| {
                    value.get("type").and_then(Value::as_str) == Some("system")
                        && value.get("subtype").and_then(Value::as_str) == Some("init")
                        || value.get("type").and_then(Value::as_str) == Some("result")
                }),
            crate::domain::AgentType::Codex => value
                .get("thread_id")
                .and_then(Value::as_str)
                .filter(|_| value.get("type").and_then(Value::as_str) == Some("thread.started")),
            crate::domain::AgentType::Antigravity
            | crate::domain::AgentType::GrokBuild
            | crate::domain::AgentType::ZCode => [
                "session_id",
                "sessionId",
                "conversation_id",
                "conversationId",
            ]
            .into_iter()
            .find_map(|field| value.get(field).and_then(Value::as_str)),
        }?;
        metadata_label(raw, MAX_CONFIG_VALUE_CHARS)
    })
}

fn read_agent_turn_output(
    stream: impl Read + Send + 'static,
    agent: crate::domain::AgentType,
    mut on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut output = Vec::with_capacity(MAX_AGENT_TURN_OUTPUT_BYTES + 1);
        loop {
            match read_bounded_line(&mut reader) {
                BoundedLine::Line(line) => {
                    let may_emit = output
                        .len()
                        .checked_add(line.len())
                        .is_some_and(|size| size <= MAX_AGENT_TURN_OUTPUT_BYTES);
                    let remaining = (MAX_AGENT_TURN_OUTPUT_BYTES + 1).saturating_sub(output.len());
                    if remaining > 0 {
                        output.extend_from_slice(&line.as_bytes()[..line.len().min(remaining)]);
                    }
                    if may_emit && let Some(update) = parse_agent_turn_update(agent, &line) {
                        on_update(update);
                    }
                }
                BoundedLine::Eof => return Ok(output),
                BoundedLine::TooLarge => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "agent output line exceeded protocol limit",
                    ));
                }
                BoundedLine::Io(error) => return Err(error),
            }
        }
    })
}

fn parse_agent_turn_update(agent: crate::domain::AgentType, line: &str) -> Option<AgentTurnUpdate> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    match agent {
        crate::domain::AgentType::ClaudeCode => {
            if value.get("type").and_then(Value::as_str) == Some("system") {
                let subtype = value.get("subtype").and_then(Value::as_str);
                if subtype == Some("api_retry") {
                    let attempt = value.get("attempt").and_then(Value::as_u64).unwrap_or(1);
                    let max_retries = value
                        .get("max_retries")
                        .and_then(Value::as_u64)
                        .unwrap_or(10);
                    let status = value
                        .get("error_status")
                        .and_then(Value::as_u64)
                        .unwrap_or(429);
                    let error = value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("rate_limit");
                    return Some(AgentTurnUpdate::AssistantDelta(format!(
                        "\n⚠️ [上游模型响应限流/配额不足 (HTTP {status} {error})，Claude 正在自动重试第 {attempt}/{max_retries} 次...]\n"
                    )));
                }
                if subtype == Some("status")
                    && value.get("status").and_then(Value::as_str) == Some("requesting")
                {
                    return Some(AgentTurnUpdate::Activity("Claude 正在请求模型 API"));
                }
                if subtype == Some("init") {
                    return Some(AgentTurnUpdate::Activity("Claude 正在初始化上下文..."));
                }
                if subtype == Some("compact_boundary") {
                    return Some(AgentTurnUpdate::Activity(
                        "Claude 正在压缩整理历史上下文...",
                    ));
                }
            }
            if value.get("type").and_then(Value::as_str) == Some("stream_event") {
                let event = value.get("event")?;
                if event.get("type").and_then(Value::as_str) == Some("content_block_delta") {
                    let delta_type = event.pointer("/delta/type").and_then(Value::as_str);
                    if delta_type == Some("text_delta") {
                        return event
                            .pointer("/delta/text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                            .map(|text| AgentTurnUpdate::AssistantDelta(text.to_owned()));
                    }
                    if delta_type == Some("thinking_delta") {
                        return Some(AgentTurnUpdate::Activity("Claude 正在深度思考..."));
                    }
                }
            }
            let assistant_blocks = value
                .get("type")
                .and_then(Value::as_str)
                .filter(|t| *t == "assistant")
                .and_then(|_| value.pointer("/message/content"))
                .and_then(Value::as_array);
            if let Some(blocks) = assistant_blocks {
                let has_tool = blocks
                    .iter()
                    .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"));
                if has_tool {
                    return Some(AgentTurnUpdate::Activity("Claude 正在调用工具"));
                }
                let mut text_acc = String::new();
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) == Some("text") {
                        text_acc.push_str(block.get("text").and_then(Value::as_str).unwrap_or(""));
                    }
                }
                let trimmed = text_acc.trim();
                if !trimmed.is_empty() {
                    return Some(AgentTurnUpdate::AssistantMessage(trimmed.to_owned()));
                }
            }
            None
        }
        crate::domain::AgentType::Codex => {
            let event_type = value.get("type").and_then(Value::as_str)?;
            let item_type = value.pointer("/item/type").and_then(Value::as_str);
            if event_type == "item.completed" && item_type == Some("agent_message") {
                return value
                    .pointer("/item/text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| AgentTurnUpdate::AssistantMessage(text.trim().to_owned()));
            }
            match (event_type, item_type) {
                ("turn.started", _) => Some(AgentTurnUpdate::Activity("Codex 正在思考")),
                ("item.started", Some("command_execution")) => {
                    Some(AgentTurnUpdate::Activity("Codex 正在执行命令"))
                }
                ("item.started", Some("mcp_tool_call")) => {
                    Some(AgentTurnUpdate::Activity("Codex 正在调用工具"))
                }
                ("item.started", Some("web_search")) => {
                    Some(AgentTurnUpdate::Activity("Codex 正在搜索"))
                }
                ("item.completed", Some("file_change")) => {
                    Some(AgentTurnUpdate::Activity("Codex 已更新文件"))
                }
                _ => None,
            }
        }
        crate::domain::AgentType::Antigravity => {
            let delta = value
                .get("text")
                .or_else(|| value.get("content"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !delta.is_empty() {
                return Some(AgentTurnUpdate::AssistantDelta(delta.to_owned()));
            }
            if value
                .get("tool_name")
                .or_else(|| value.get("toolName"))
                .is_some()
            {
                return Some(AgentTurnUpdate::Activity("Antigravity 正在调用工具"));
            }
            if value.get("type").and_then(Value::as_str) == Some("PLANNER_RESPONSE") {
                return Some(AgentTurnUpdate::Activity("Antigravity 正在规划"));
            }
            None
        }
        crate::domain::AgentType::GrokBuild => match value.get("type").and_then(Value::as_str) {
            Some("text") => value
                .get("data")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(|text| AgentTurnUpdate::AssistantDelta(text.to_owned())),
            Some("tool_call") => Some(AgentTurnUpdate::Activity(
                match value
                    .get("toolName")
                    .and_then(Value::as_str)
                    .and_then(|name| crate::adapters::safe_tool_category(Some(name)))
                    .as_deref()
                {
                    Some("shell") => "Grok Build 正在执行命令",
                    Some("file_change") => "Grok Build 正在修改文件",
                    Some("file_read") => "Grok Build 正在读取文件",
                    Some("user_input") => "Grok Build 正在等待输入",
                    _ => "Grok Build 正在调用工具",
                },
            )),
            Some("tool_call_update")
                if value.get("status").and_then(Value::as_str) == Some("completed") =>
            {
                Some(AgentTurnUpdate::Activity("Grok Build 已完成工具"))
            }
            _ => None,
        },
        crate::domain::AgentType::ZCode => {
            if let Some(text) = value.get("text").and_then(Value::as_str) {
                return Some(AgentTurnUpdate::AssistantDelta(text.to_owned()));
            }
            if let Some(tool) = value.get("tool").and_then(Value::as_str) {
                return Some(AgentTurnUpdate::Activity(
                    match crate::adapters::safe_tool_category(Some(tool)).as_deref() {
                        Some("shell") => "ZCode 正在执行命令",
                        Some("file_change") => "ZCode 正在修改文件",
                        Some("file_read") => "ZCode 正在读取文件",
                        Some("user_input") => "ZCode 正在等待输入",
                        _ => "ZCode 正在调用工具",
                    },
                ));
            }
            None
        }
    }
}

fn final_agent_message(agent: crate::domain::AgentType, output: &str) -> Option<String> {
    match agent {
        crate::domain::AgentType::ClaudeCode => {
            for line in output.lines().rev() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if value.get("type").and_then(Value::as_str) != Some("result") {
                    continue;
                }
                if let Some(res) = value
                    .get("result")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    return Some(res.to_owned());
                }
            }
            for line in output.lines().rev() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if value.get("type").and_then(Value::as_str) != Some("assistant") {
                    continue;
                }
                if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
                    let mut text_acc = String::new();
                    for block in blocks {
                        if block.get("type").and_then(Value::as_str) == Some("text") {
                            text_acc
                                .push_str(block.get("text").and_then(Value::as_str).unwrap_or(""));
                        }
                    }
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_owned());
                    }
                }
            }
            let mut deltas = String::new();
            for line in output.lines() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if value.get("type").and_then(Value::as_str) != Some("stream_event") {
                    continue;
                }
                if let Some(delta) = value.pointer("/event/delta/text").and_then(Value::as_str) {
                    deltas.push_str(delta);
                }
            }
            let trimmed = deltas.trim();
            if !trimmed.is_empty() {
                Some(trimmed.to_owned())
            } else {
                None
            }
        }
        crate::domain::AgentType::Codex => output.lines().rev().find_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            if value.get("type").and_then(Value::as_str) != Some("item.completed")
                || value.pointer("/item/type").and_then(Value::as_str) != Some("agent_message")
            {
                return None;
            }
            value
                .pointer("/item/text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_owned)
        }),
        crate::domain::AgentType::Antigravity => {
            for line in output.lines().rev() {
                if let Ok(value) = serde_json::from_str::<Value>(line) {
                    if let Some(content) = value.get("content").and_then(Value::as_str) {
                        let trimmed = content.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_owned());
                        }
                    }
                    if let Some(res) = value.get("result").and_then(Value::as_str) {
                        let trimmed = res.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_owned());
                        }
                    }
                }
            }
            let trimmed = output.trim();
            if !trimmed.is_empty() {
                Some(trimmed.to_owned())
            } else {
                None
            }
        }
        crate::domain::AgentType::GrokBuild => {
            let mut collected = String::new();
            for line in output.lines() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                match value.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = value.get("data").and_then(Value::as_str) {
                            collected.push_str(text);
                        }
                    }
                    Some("end") => {
                        if let Some(text) = value
                            .get("text")
                            .or_else(|| value.get("data"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|message| !message.is_empty())
                        {
                            return Some(text.to_owned());
                        }
                    }
                    _ => {}
                }
            }
            if let Ok(value) = serde_json::from_str::<Value>(output)
                && let Some(text) = value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|message| !message.is_empty())
            {
                return Some(text.to_owned());
            }
            let trimmed = collected.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        crate::domain::AgentType::ZCode => output.lines().rev().find_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            if let Some(text) = value
                .get("text")
                .or_else(|| value.get("content"))
                .and_then(Value::as_str)
            {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_owned());
                }
            }
            None
        }),
    }
}

fn read_child_output(
    mut stream: impl Read + Send + 'static,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::with_capacity(MAX_AGENT_TURN_OUTPUT_BYTES + 1);
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            let remaining = (MAX_AGENT_TURN_OUTPUT_BYTES + 1).saturating_sub(output.len());
            if remaining > 0 {
                output.extend_from_slice(&chunk[..read.min(remaining)]);
            }
        }
        Ok(output)
    })
}

fn join_child_output(
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
    program: &str,
) -> Result<Vec<u8>, CollectorError> {
    reader
        .join()
        .map_err(|_| CollectorError::ReaderStopped {
            program: program.to_owned(),
        })?
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })
}

fn wait_for_child_with_cancel(
    child: &mut Child,
    program: &str,
    timeout: Duration,
    cancelled: Option<&AtomicBool>,
) -> Result<std::process::ExitStatus, CollectorError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })? {
            return Ok(status);
        }
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Relaxed)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CollectorError::Cancelled {
                program: program.to_owned(),
            });
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CollectorError::Timeout {
                program: program.to_owned(),
                timeout_ms: timeout.as_millis(),
            });
        }
        thread::sleep(CHILD_WAIT_INTERVAL);
    }
}

pub(crate) enum ProtocolRead {
    Message(Value),
    InvalidJson(serde_json::Error),
    LineTooLarge,
    Io(std::io::Error),
    Eof,
}

pub(crate) fn resolve_codex_app_server_binary(binary: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        resolve_windows_codex_binary(binary).unwrap_or_else(|| binary.to_path_buf())
    }
    #[cfg(not(windows))]
    {
        binary.to_path_buf()
    }
}

#[cfg(windows)]
fn resolve_windows_codex_binary(binary: &Path) -> Option<PathBuf> {
    if binary
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
    {
        return resolve_windows_codex_candidate(binary);
    }

    let path = env::var_os("PATH")?;
    let mut executable_fallback = None;
    for directory in env::split_paths(&path) {
        for extension in ["cmd", "ps1"] {
            let candidate = directory.join(format!("{}.{}", binary.display(), extension));
            if let Some(native) = resolve_windows_codex_candidate(&candidate) {
                return Some(native);
            }
        }
        let candidate = directory.join(format!("{}.exe", binary.display()));
        if executable_fallback.is_none() && candidate.is_file() {
            executable_fallback = candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    executable_fallback
}

#[cfg(windows)]
fn resolve_windows_codex_candidate(candidate: &Path) -> Option<PathBuf> {
    if candidate
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        && candidate.is_file()
    {
        return candidate
            .canonicalize()
            .ok()
            .or_else(|| Some(candidate.into()));
    }
    if !candidate.is_file()
        || !candidate
            .file_stem()
            .is_some_and(|stem| stem.eq_ignore_ascii_case("codex"))
    {
        return None;
    }

    let parent = candidate.parent()?;
    let package_root = if parent
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(".bin"))
    {
        parent.parent()?.join("@openai").join("codex")
    } else {
        parent.join("node_modules").join("@openai").join("codex")
    };
    #[cfg(target_arch = "x86_64")]
    let (platform_package, target) = ("codex-win32-x64", "x86_64-pc-windows-msvc");
    #[cfg(target_arch = "aarch64")]
    let (platform_package, target) = ("codex-win32-arm64", "aarch64-pc-windows-msvc");
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return None;

    for native in [
        package_root
            .join("node_modules")
            .join("@openai")
            .join(platform_package)
            .join("vendor")
            .join(target)
            .join("bin")
            .join("codex.exe"),
        package_root
            .join("vendor")
            .join(target)
            .join("bin")
            .join("codex.exe"),
    ] {
        if native.is_file() {
            return native.canonicalize().ok().or(Some(native));
        }
    }
    None
}

pub mod process;
pub(crate) use process::*;

pub mod codex_app_server;
pub use codex_app_server::*;

pub mod zcode_app_server;
pub use zcode_app_server::*;

pub mod claude_app_server;
pub use claude_app_server::*;

pub mod acp_app_server;
pub use acp_app_server::*;

#[derive(Debug, Clone)]
pub struct CollectorBinaries {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

impl Default for CollectorBinaries {
    fn default() -> Self {
        Self {
            claude: PathBuf::from("claude"),
            codex: PathBuf::from("codex"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use super::*;

    fn sleeping_script() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();

        #[cfg(windows)]
        let script = {
            let path = directory.path().join("claude.cmd");
            fs::write(
                &path,
                "@echo off\r\npowershell -NoProfile -Command \"Start-Sleep -Seconds 5\"\r\n",
            )
            .unwrap();
            path
        };

        #[cfg(unix)]
        let script = {
            use std::os::unix::fs::PermissionsExt;

            let path = directory.path().join("claude");
            fs::write(&path, "#!/bin/sh\nsleep 5\n").unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        };
        (directory, script)
    }

    #[test]
    #[cfg(windows)]
    fn resolves_only_known_windows_codex_npm_shim_layouts() {
        let directory = tempfile::tempdir().unwrap();
        let npm_root = directory.path().join("npm");
        let shim = npm_root.join("codex.cmd");
        let native = npm_root
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&shim, "@echo off\r\n").unwrap();
        fs::write(&native, b"test").unwrap();

        assert_eq!(
            resolve_windows_codex_candidate(&shim),
            native.canonicalize().ok()
        );
        let unrelated = directory.path().join("other").join("codex.cmd");
        fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        fs::write(&unrelated, "@echo off\r\n").unwrap();
        assert!(resolve_windows_codex_candidate(&unrelated).is_none());
    }

    #[test]
    fn sanitizes_protocol_error_messages() {
        let input = "bad\0message\nwith control";
        assert_eq!(sanitize_protocol_message(input), "badmessagewith control");
    }

    #[test]
    fn protocol_reader_delivers_messages_without_blocking_the_request_thread() {
        let input = Cursor::new(
            br#"{"method":"thread/status/changed","params":{"threadId":"thr-1"}}
{"id":7,"result":{"data":[]}}
"#,
        );
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || read_protocol(input, sender));

        let first = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        let second = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        let eof = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        reader.join().unwrap();

        assert!(matches!(
            first,
            ProtocolRead::Message(value)
                if value.get("method").and_then(Value::as_str)
                    == Some("thread/status/changed")
        ));
        assert!(matches!(
            second,
            ProtocolRead::Message(value)
                if value.get("id").and_then(Value::as_u64) == Some(7)
        ));
        assert!(matches!(eof, ProtocolRead::Eof));
    }

    #[test]
    fn bounded_line_reader_stops_oversized_lines_without_buffering_them() {
        struct EndlessBytes;

        impl std::io::Read for EndlessBytes {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                buffer.fill(b'a');
                Ok(buffer.len())
            }
        }

        let mut reader = BufReader::new(EndlessBytes);
        assert!(matches!(
            read_bounded_line(&mut reader),
            BoundedLine::TooLarge
        ));
    }

    #[test]
    fn bounded_line_reader_returns_lines_and_eof() {
        let mut reader = BufReader::new(Cursor::new(b"{\"id\":1}\ntrailing".to_vec()));
        assert!(matches!(
            read_bounded_line(&mut reader),
            BoundedLine::Line(line) if line == "{\"id\":1}\n"
        ));
        assert!(matches!(
            read_bounded_line(&mut reader),
            BoundedLine::Line(line) if line == "trailing"
        ));
        assert!(matches!(read_bounded_line(&mut reader), BoundedLine::Eof));
    }

    #[test]
    fn child_output_reader_drains_but_retains_only_the_bounded_prefix() {
        let input = vec![b'x'; MAX_AGENT_TURN_OUTPUT_BYTES + 32 * 1024];
        let output = read_child_output(Cursor::new(input))
            .join()
            .unwrap()
            .unwrap();

        assert_eq!(output.len(), MAX_AGENT_TURN_OUTPUT_BYTES + 1);
        assert!(output.iter().all(|byte| *byte == b'x'));
    }

    #[test]
    fn claude_stream_events_emit_text_and_thinking_activity() {
        let text = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}}"#;
        let thinking = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"private"}}}"#;
        let init = r#"{"type":"system","subtype":"init"}"#;
        let compact = r#"{"type":"system","subtype":"compact_boundary"}"#;

        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::ClaudeCode, text),
            Some(AgentTurnUpdate::AssistantDelta("hello".to_owned()))
        );
        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::ClaudeCode, thinking),
            Some(AgentTurnUpdate::Activity("Claude 正在深度思考..."))
        );
        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::ClaudeCode, init),
            Some(AgentTurnUpdate::Activity("Claude 正在初始化上下文..."))
        );
        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::ClaudeCode, compact),
            Some(AgentTurnUpdate::Activity(
                "Claude 正在压缩整理历史上下文..."
            ))
        );
    }

    #[test]
    fn codex_stream_events_emit_safe_activity_and_final_messages() {
        let command =
            r#"{"type":"item.started","item":{"type":"command_execution","command":"secret"}}"#;
        let message =
            r#"{"type":"item.completed","item":{"type":"agent_message","text":" done "}}"#;

        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::Codex, command),
            Some(AgentTurnUpdate::Activity("Codex 正在执行命令"))
        );
        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::Codex, message),
            Some(AgentTurnUpdate::AssistantMessage("done".to_owned()))
        );
        assert_eq!(
            final_agent_message(crate::domain::AgentType::Codex, message).as_deref(),
            Some("done")
        );
    }

    #[test]
    fn final_claude_result_uses_the_last_non_empty_message() {
        let output = concat!(
            "{\"type\":\"result\",\"result\":\"first\"}\n",
            "{\"type\":\"result\",\"result\":\" final \"}\n",
        );
        assert_eq!(
            final_agent_message(crate::domain::AgentType::ClaudeCode, output).as_deref(),
            Some("final")
        );
    }

    #[test]
    fn session_turn_cancellation_stops_the_child_process() {
        let (_directory, script) = sleeping_script();
        let cancelled = AtomicBool::new(true);
        let started = Instant::now();

        let error = run_agent_session_turn_streaming(
            crate::domain::AgentType::ClaudeCode,
            script,
            None,
            "019abc-session",
            "private prompt",
            TurnOptions::default(),
            Duration::from_secs(10),
            &cancelled,
            |_| {},
        )
        .unwrap_err();

        assert!(matches!(error, CollectorError::Cancelled { .. }));
        assert!(started.elapsed() < Duration::from_secs(6));
    }

    #[test]
    fn grok_session_collector_reads_summary_without_conversation() {
        let directory = tempfile::tempdir().unwrap();
        let session_dir = directory
            .path()
            .join("sessions")
            .join("encoded-project")
            .join("01a005b5-5ee8-7013-947b-0140a7b267fa");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("summary.json"),
            concat!(
                "{\"info\":{\"id\":\"01a005b5-5ee8-7013-947b-0140a7b267fa\",",
                "\"cwd\":\"/work/private-project\"},",
                "\"generated_title\":\"Auth refactor\",",
                "\"session_summary\":\"TOP_SECRET_SUMMARY\",",
                "\"last_turn_summary\":\"TOP_SECRET_TURN\",",
                "\"created_at\":\"2026-08-15T11:45:05Z\",",
                "\"updated_at\":\"2026-08-15T12:08:42Z\",",
                "\"last_active_at\":\"2026-08-15T12:08:29Z\",",
                "\"current_model_id\":\"grok-4.6\"}\n"
            ),
        )
        .unwrap();
        fs::write(
            session_dir.join("chat_history.jsonl"),
            "{\"message\":\"TOP_SECRET_CHAT\"}\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("active_sessions.json"),
            concat!(
                "[{\"session_id\":\"01a005b5-5ee8-7013-947b-0140a7b267fa\",",
                "\"pid\":0,\"cwd\":\"/work/private-project\"}]"
            ),
        )
        .unwrap();

        let payload = collect_grok_sessions_from(directory.path(), true).unwrap();
        let entries: Value = serde_json::from_str(&payload).unwrap();
        let entry = &entries.as_array().unwrap()[0];

        assert_eq!(entry["sessionId"], "01a005b5-5ee8-7013-947b-0140a7b267fa");
        assert_eq!(entry["cwd"], "/work/private-project");
        assert_eq!(entry["state"], "persisted");
        assert_eq!(entry["live"], false);
        assert!(!payload.contains("TOP_SECRET"));
        assert!(!payload.contains("generated_title"));
        assert!(!payload.contains("chat_history"));
    }

    #[test]
    fn grok_context_collector_keeps_title_and_model_only() {
        let directory = tempfile::tempdir().unwrap();
        let session_dir = directory
            .path()
            .join("sessions")
            .join("encoded-project")
            .join("session-ctx");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(
            session_dir.join("summary.json"),
            concat!(
                "{\"info\":{\"id\":\"session-ctx\",\"cwd\":\"/work/demo\"},",
                "\"generated_title\":\" Auth   refactor \",",
                "\"session_summary\":\"TOP_SECRET_SUMMARY\",",
                "\"last_turn_summary\":\"TOP_SECRET_TURN\",",
                "\"current_model_id\":\"grok-4.6\",",
                "\"reasoning_effort\":\"high\",",
                "\"sandbox_profile\":\"off\",",
                "\"agent_name\":\"grok-build-plan\"}"
            ),
        )
        .unwrap();

        let contexts = collect_grok_session_contexts_from(directory.path(), 10).unwrap();
        let context = &contexts[0];
        let serialized = serde_json::to_string(context).unwrap();

        assert_eq!(context.agent_session_id, "session-ctx");
        assert_eq!(context.title.as_deref(), Some("Auth refactor"));
        assert_eq!(context.model.as_deref(), Some("grok-4.6"));
        assert_eq!(context.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(context.sandbox_mode.as_deref(), Some("off"));
        assert_eq!(context.permission_mode.as_deref(), Some("grok-build-plan"));
        assert!(!serialized.contains("TOP_SECRET"));
    }

    #[test]
    fn grok_stream_events_emit_text_without_thoughts_or_tool_input() {
        let text = r#"{"type":"text","data":"hello"}"#;
        let thought = r#"{"type":"thought","data":"private reasoning"}"#;
        let tool =
            r#"{"type":"tool_call","toolName":"run_terminal_cmd","rawInput":{"command":"secret"}}"#;
        let output = concat!(
            r#"{"type":"text","data":"hello "}"#,
            "\n",
            r#"{"type":"end","stopReason":"end_turn"}"#,
            "\n",
        );

        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::GrokBuild, text),
            Some(AgentTurnUpdate::AssistantDelta("hello".to_owned()))
        );
        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::GrokBuild, thought),
            None
        );
        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::GrokBuild, tool),
            Some(AgentTurnUpdate::Activity("Grok Build 正在执行命令"))
        );
        assert_eq!(
            final_agent_message(crate::domain::AgentType::GrokBuild, output).as_deref(),
            Some("hello")
        );
        assert!(
            !tool.contains("Grok")
                || !final_agent_message(crate::domain::AgentType::GrokBuild, tool)
                    .unwrap_or_default()
                    .contains("secret")
        );
    }

    #[test]
    fn claude_transcript_collector_reads_metadata_without_retaining_content() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects").join("encoded-project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("session-123.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-123\",",
                "\"cwd\":\"/work/private-project\",",
                "\"timestamp\":\"2026-08-14T01:02:03Z\",",
                "\"message\":{\"content\":\"TOP_SECRET_TRANSCRIPT_CONTENT\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"session-123\",",
                "\"message\":{\"role\":\"assistant\",\"content\":",
                "[{\"type\":\"text\",\"text\":\"SECOND_SECRET_REPLY\"}]}}\n"
            ),
        )
        .unwrap();

        let payload = collect_claude_transcripts_from(directory.path(), false).unwrap();
        let entries: Value = serde_json::from_str(&payload).unwrap();
        let entry = &entries.as_array().unwrap()[0];

        assert_eq!(entry["sessionId"], "session-123");
        assert_eq!(entry["cwd"], "/work/private-project");
        assert_eq!(entry["state"], "persisted");
        assert_eq!(entry["startedAt"], 1_786_669_323_000_i64);
        assert!(!payload.contains("TOP_SECRET_TRANSCRIPT_CONTENT"));
        assert!(!payload.contains("SECOND_SECRET_REPLY"));
        assert!(!payload.contains("message"));
    }

    #[test]
    fn claude_transcript_collector_hides_sessions_with_no_assistant_output() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects").join("encoded-project");
        fs::create_dir_all(&project).unwrap();
        // A session the user abandoned before the first reply. It still has a
        // prompt, a custom title and a full transcript on disk - only the
        // absence of assistant output marks it as empty.
        fs::write(
            project.join("abandoned.jsonl"),
            concat!(
                "{\"type\":\"queue-operation\",\"sessionId\":\"abandoned\"}\n",
                "{\"type\":\"user\",\"sessionId\":\"abandoned\",",
                "\"cwd\":\"/work/project\",\"timestamp\":\"2026-08-14T01:02:03Z\",",
                "\"message\":{\"content\":\"现在的项目甚至没有git？\"}}\n",
                "{\"type\":\"user\",\"sessionId\":\"abandoned\",",
                "\"message\":{\"content\":[{\"type\":\"text\",",
                "\"text\":\"[Request interrupted by user]\"}]}}\n",
                "{\"type\":\"custom-title\",\"sessionId\":\"abandoned\",",
                "\"customTitle\":\"项目缺少Git配置\"}\n"
            ),
        )
        .unwrap();
        fs::write(
            project.join("real.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"real\",\"cwd\":\"/work/project\",",
                "\"timestamp\":\"2026-08-14T02:00:00Z\",",
                "\"message\":{\"content\":\"同样的首问\"}}\n",
                "{\"type\":\"assistant\",\"sessionId\":\"real\",",
                "\"message\":{\"role\":\"assistant\",\"content\":",
                "[{\"type\":\"text\",\"text\":\"有 git\"}]}}\n"
            ),
        )
        .unwrap();

        let payload = collect_claude_transcripts_from(directory.path(), false).unwrap();
        let entries: Value = serde_json::from_str(&payload).unwrap();
        let sessions: Vec<&str> = entries
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["sessionId"].as_str())
            .collect();

        assert_eq!(sessions, ["real"]);
    }

    #[test]
    fn claude_output_detection_accepts_shapes_and_rejects_lookalikes() {
        let output = |value: Value| is_assistant_output(&value);

        // Text array form and plain string form both count as output.
        assert!(output(serde_json::json!({
            "type":"assistant","message":{"role":"assistant",
            "content":[{"type":"text","text":"hi"}]}
        })));
        assert!(output(serde_json::json!({
            "type":"assistant","message":{"role":"assistant","content":"hi"}
        })));
        // Assistant turns that are pure tool calls produced no visible reply.
        assert!(!output(serde_json::json!({
            "type":"assistant","message":{"role":"assistant",
            "content":[{"type":"tool_use","name":"Bash"}]}
        })));
        assert!(!output(serde_json::json!({
            "type":"assistant","message":{"role":"assistant",
            "content":[{"type":"text","text":"   "}]}
        })));
        // A subagent transcript is not output the user ever saw.
        assert!(!output(serde_json::json!({
            "type":"assistant","isSidechain":true,
            "message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}
        })));
        // Prompts and tool results are not assistant output.
        assert!(!output(serde_json::json!({
            "type":"user","message":{"role":"user","content":"hi"}
        })));
        assert!(!output(serde_json::json!({
            "type":"user","message":{"role":"user",
            "content":[{"type":"tool_result","content":"hi"}]}
        })));
    }

    #[test]
    fn claude_context_collector_keeps_explicit_runtime_metadata_only() {
        let directory = tempfile::tempdir().unwrap();
        let project = directory.path().join("projects").join("encoded-project");
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("session-ctx.jsonl"),
            concat!(
                "{\"type\":\"user\",\"sessionId\":\"session-ctx\",",
                "\"message\":{\"content\":\"TOP_SECRET_PROMPT\"}}\n",
                "{\"type\":\"custom-title\",\"customTitle\":\" Auth   refactor \"}\n",
                "{\"type\":\"permission-mode\",\"mode\":\"acceptEdits\"}\n",
                "{\"type\":\"assistant\",\"message\":{",
                "\"model\":\"claude-opus-4-7\",",
                "\"content\":[{\"type\":\"text\",\"text\":\"TOP_SECRET_REPLY\"}],",
                "\"usage\":{\"input_tokens\":100,",
                "\"cache_creation_input_tokens\":20,\"cache_read_input_tokens\":30}},",
                "\"context_window\":{\"context_window_size\":200000}}\n"
            ),
        )
        .unwrap();

        let contexts = collect_claude_session_contexts_from(directory.path(), 10).unwrap();
        let context = &contexts[0];
        let serialized = serde_json::to_string(context).unwrap();

        assert_eq!(context.agent_session_id, "session-ctx");
        assert_eq!(context.title.as_deref(), Some("Auth refactor"));
        assert_eq!(context.model.as_deref(), Some("claude-opus-4-7"));
        assert_eq!(context.permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(context.context_used_tokens, Some(150));
        assert_eq!(context.context_window_tokens, Some(200_000));
        assert!(!serialized.contains("TOP_SECRET"));
    }

    #[test]
    fn claude_context_supports_current_title_and_mode_events() {
        let mut context = LocalSessionContext::new(
            crate::domain::AgentType::ClaudeCode,
            "session-current".to_owned(),
        );

        apply_claude_context_line(
            r#"{"type":"ai-title","aiTitle":" Provider generated title "}"#,
            &mut context,
        );
        apply_claude_context_line(r#"{"type":"mode","mode":"plan"}"#, &mut context);
        assert_eq!(context.title.as_deref(), Some("Provider generated title"));
        assert_eq!(context.permission_mode.as_deref(), Some("plan"));

        apply_claude_context_line(
            r#"{"type":"custom-title","customTitle":" User title "}"#,
            &mut context,
        );
        apply_claude_context_line(
            r#"{"type":"ai-title","aiTitle":" Later generated title "}"#,
            &mut context,
        );
        assert_eq!(context.title.as_deref(), Some("User title"));
    }

    #[test]
    fn local_prompt_titles_ignore_provider_control_messages() {
        assert_eq!(
            local_prompt_title("<USER_REQUEST>优化侧边栏与会话页</USER_REQUEST>").as_deref(),
            Some("优化侧边栏与会话页")
        );
        assert!(
            local_prompt_title("<local-command-caveat>internal</local-command-caveat>").is_none()
        );
        assert!(local_prompt_title("<command-name>/compact</command-name>").is_none());
        assert!(local_prompt_title("Untitled").is_none());
        assert!(local_prompt_title("未命名会话").is_none());
        assert_eq!(
            local_prompt_title(
                "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\ninternal\n</INSTRUCTIONS>\n\n实现重命名功能"
            )
            .as_deref(),
            Some("实现重命名功能")
        );
    }

    #[test]
    fn test_claude_prompt_extraction_filters_tool_results_and_synthetics() {
        use serde_json::json;

        // 1. Tool result message in role=user should NEVER be extracted as prompt
        let tool_result_line = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_123",
                        "content": "cargo test --workspace succeeded"
                    }
                ]
            }
        });
        assert_eq!(extract_claude_prompt_text(&tool_result_line), None);

        // 2. Synthetic injected commands should be ignored
        let synthetic_line = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": "<command-name>/doctor</command-name>"
            }
        });
        assert_eq!(extract_claude_prompt_text(&synthetic_line), None);

        // 3. Genuine human prompt should be cleanly extracted
        let real_prompt_line = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": "请帮我重构网络连接池逻辑"
            }
        });
        assert_eq!(
            extract_claude_prompt_text(&real_prompt_line).as_deref(),
            Some("请帮我重构网络连接池逻辑")
        );

        // 4. Image-only prompt gets [image] label
        let image_line = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": { "type": "base64", "data": "abc" }
                    }
                ]
            }
        });
        assert_eq!(
            extract_claude_prompt_text(&image_line).as_deref(),
            Some("[image]")
        );
    }

    #[test]
    fn test_claude_context_window_inference() {
        assert_eq!(claude_context_window("claude-opus-5-20261001"), 1_000_000);
        assert_eq!(claude_context_window("claude-sonnet-5"), 1_000_000);
        assert_eq!(claude_context_window("claude-fable-5-1"), 1_000_000);
        assert_eq!(claude_context_window("custom-model[1m]"), 1_000_000);
        assert_eq!(claude_context_window("claude-3-7-sonnet"), 200_000);
    }

    #[test]
    fn test_claude_result_reports_context_usage() {
        // Cache reads and writes occupy the window like fresh input does.
        let result = json!({
            "type":"result",
            "result":"done",
            "usage":{"input_tokens":1000,"output_tokens":200,
                     "cache_read_input_tokens":30000,"cache_creation_input_tokens":5000}
        });
        assert_eq!(
            claude_result_context_usage(&result),
            Some(ContextUsage {
                used: 36_000,
                window: None
            })
        );

        let with_window = json!({
            "type":"result","usage":{"input_tokens":40_000},"model_context_window":200_000
        });
        assert_eq!(
            claude_result_context_usage(&with_window),
            Some(ContextUsage {
                used: 40_000,
                window: Some(200_000)
            })
        );

        // Nothing usable must stay absent rather than become a zero ring.
        assert_eq!(
            claude_result_context_usage(&json!({"type":"result","usage":{}})),
            None
        );
        assert_eq!(
            claude_result_context_usage(&json!({"type":"assistant","usage":{"input_tokens":5}})),
            None
        );
    }

    #[test]
    fn test_claude_stop_reason_and_turn_lifecycle() {
        let mut context = LocalSessionContext::new(
            crate::domain::AgentType::ClaudeCode,
            "session-lifecycle".to_owned(),
        );

        // 1. Tool use stop_reason -> mid-turn, turn_ended is false
        let tool_use_line = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "model": "claude-sonnet-5",
                "stop_reason": "tool_use",
                "usage": {
                    "input_tokens": 1000,
                    "output_tokens": 150,
                    "cache_read_input_tokens": 500,
                    "cache_creation_input_tokens": 200
                }
            }
        })
        .to_string();
        apply_claude_context_line(&tool_use_line, &mut context);
        assert_eq!(context.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(context.turn_ended, Some(false));
        assert_eq!(context.context_used_tokens, Some(1700));

        // 2. End turn stop_reason -> completed turn, turn_ended is true
        let end_turn_line = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 1200,
                    "output_tokens": 300,
                    "cache_read_input_tokens": 500,
                    "cache_creation_input_tokens": 200
                }
            }
        })
        .to_string();
        apply_claude_context_line(&end_turn_line, &mut context);
        assert_eq!(context.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(context.turn_ended, Some(true));

        // 3. New genuine user prompt accepted -> starts next turn, turn_ended resets to false
        let user_prompt = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": "请接着处理"
            }
        })
        .to_string();
        apply_claude_context_line(&user_prompt, &mut context);
        assert_eq!(context.turn_ended, Some(false));
    }

    #[test]
    fn test_apply_oversized_claude_record_slicing() {
        let mut context = LocalSessionContext::new(
            crate::domain::AgentType::ClaudeCode,
            "session-oversized".to_owned(),
        );

        // Construct head slice and tail slice representing an oversized 2MB assistant line
        let head = r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-5-20261001","content":[{"type":"text","text":"long content..."#;
        let tail = r#"...more content"}],"stop_reason":"end_turn","usage":{"input_tokens":5000,"output_tokens":1200,"cache_read_input_tokens":10000,"cache_creation_input_tokens":2000}}}"#;

        apply_oversized_claude_record(head, tail, &mut context);

        assert_eq!(context.model.as_deref(), Some("claude-opus-5-20261001"));
        assert_eq!(context.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(context.turn_ended, Some(true));
        assert_eq!(context.context_used_tokens, Some(17000));
        assert_eq!(context.context_window_tokens, Some(1_000_000));
    }

    #[test]
    fn test_scan_claude_context_region_handles_oversized_line() {
        let mut context = LocalSessionContext::new(
            crate::domain::AgentType::ClaudeCode,
            "session-streaming-oversized".to_owned(),
        );

        // Build a single line exceeding 128 KiB
        let huge_padding = "x".repeat(150_000);
        let oversized_line = format!(
            r#"{{"type":"assistant","sessionId":"sess-over","message":{{"role":"assistant","model":"claude-fable-5-1","content":[{{"type":"text","text":"{huge_padding}"}}],"stop_reason":"end_turn","usage":{{"input_tokens":4000,"output_tokens":800,"cache_read_input_tokens":2000,"cache_creation_input_tokens":1000}}}}}}{}"#,
            "\n"
        );

        let mut cursor = std::io::Cursor::new(oversized_line.as_bytes());
        scan_claude_context_region(
            &mut cursor,
            oversized_line.len() as u64 + 1024,
            &mut context,
            "test",
        )
        .unwrap();

        assert_eq!(context.model.as_deref(), Some("claude-fable-5-1"));
        assert_eq!(context.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(context.turn_ended, Some(true));
        assert_eq!(context.context_used_tokens, Some(7000));
        assert_eq!(context.context_window_tokens, Some(1_000_000));
    }

    #[test]
    fn codex_rollout_extracts_title_from_response_item() {
        let mut context = LocalSessionContext::new(
            crate::domain::AgentType::Codex,
            "session-codex-title".to_owned(),
        );
        let line = serde_json::json!({
            "type": "response_item",
            "payload": {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "# AGENTS.md instructions for /test\n<INSTRUCTIONS>\nsecret\n</INSTRUCTIONS>"
                    },
                    {
                        "type": "input_text",
                        "text": "分析内存泄漏问题"
                    }
                ]
            }
        })
        .to_string();
        apply_codex_context_line(&line, &mut context);
        assert_eq!(context.title.as_deref(), Some("分析内存泄漏问题"));
    }

    #[test]
    fn codex_local_store_collector_selects_metadata_only() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    cwd TEXT NOT NULL,
                    title TEXT NOT NULL,
                    preview TEXT NOT NULL,
                    source TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    recency_at INTEGER NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO threads VALUES (
                    'thread-123', '/work/private-project',
                    'TOP_SECRET_TITLE', 'TOP_SECRET_PREVIEW', 'cli',
                    100, 200, 190, 0
                );",
            )
            .unwrap();
        drop(connection);

        let payload = collect_codex_local_threads_from(&database, 10).unwrap();
        let serialized = serde_json::to_string(&payload).unwrap();
        let thread = &payload["data"][0];

        assert_eq!(thread["id"], "thread-123");
        assert_eq!(thread["cwd"], "/work/private-project");
        assert_eq!(thread["status"]["type"], "persisted");
        assert!(!serialized.contains("TOP_SECRET_TITLE"));
        assert!(!serialized.contains("TOP_SECRET_PREVIEW"));
    }

    #[test]
    fn codex_context_collector_reads_configuration_without_message_content() {
        let directory = tempfile::tempdir().unwrap();
        let rollout = directory.path().join("rollout-thread-ctx.jsonl");
        fs::write(
            &rollout,
            concat!(
                "{\"type\":\"response_item\",\"payload\":{",
                "\"content\":\"TOP_SECRET_TRANSCRIPT\"}}\n",
                "{\"type\":\"turn_context\",\"payload\":{",
                "\"model\":\"gpt-5.5-codex\",\"effort\":\"high\",",
                "\"approval_policy\":\"on-request\",",
                "\"sandbox_policy\":{\"type\":\"workspace-write\"}}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",",
                "\"info\":{\"model_context_window\":258400,",
                "\"last_token_usage\":{\"input_tokens\":42000,\"total_tokens\":42100},",
                "\"total_token_usage\":{\"total_tokens\":88000}}}}\n"
            ),
        )
        .unwrap();
        let database = directory.path().join("state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(&format!(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY, title TEXT, model TEXT,
                    reasoning_effort TEXT, approval_mode TEXT,
                    sandbox_policy TEXT, tokens_used INTEGER,
                    rollout_path TEXT, updated_at INTEGER, archived INTEGER
                );
                INSERT INTO threads VALUES (
                    'thread-ctx', ' Review checkout ', 'gpt-old', 'medium',
                    'unless-trusted', '{{\"type\":\"read-only\"}}',
                    70000, '{}', 200, 0
                );",
                rollout.display().to_string().replace('\\', "\\\\")
            ))
            .unwrap();
        drop(connection);

        let contexts = collect_codex_session_contexts_from(&database, 10).unwrap();
        let context = &contexts[0];
        let serialized = serde_json::to_string(context).unwrap();

        assert_eq!(context.title.as_deref(), Some("Review checkout"));
        assert_eq!(context.model.as_deref(), Some("gpt-5.5-codex"));
        assert_eq!(context.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(context.approval_policy.as_deref(), Some("on-request"));
        assert_eq!(context.sandbox_mode.as_deref(), Some("workspace-write"));
        assert_eq!(context.context_window_tokens, Some(258_400));
        assert_eq!(context.context_used_tokens, Some(42_100));
        assert_eq!(context.session_total_tokens, Some(88_000));
        assert!(!serialized.contains("TOP_SECRET"));
    }

    #[test]
    fn codex_context_collector_rejects_rollouts_outside_state_root() {
        let directory = tempfile::tempdir().unwrap();
        let state_root = directory.path().join("codex-home");
        fs::create_dir(&state_root).unwrap();
        let escaped_rollout = directory.path().join("outside-rollout.jsonl");
        fs::write(
            &escaped_rollout,
            concat!(
                "{\"type\":\"turn_context\",\"payload\":{",
                "\"model\":\"TOP_SECRET_MODEL\",",
                "\"approval_policy\":\"bypassPermissions\"}}\n"
            ),
        )
        .unwrap();
        let database = state_root.join("state_5.sqlite");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(&format!(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY, title TEXT, model TEXT,
                    approval_mode TEXT, sandbox_policy TEXT,
                    rollout_path TEXT, updated_at INTEGER
                );
                INSERT INTO threads VALUES (
                    'thread-contained', 'Safe title', 'safe-model',
                    'on-request', '{{\"type\":\"workspace-write\"}}', '{}', 200
                );",
                escaped_rollout.display().to_string().replace('\\', "\\\\")
            ))
            .unwrap();
        drop(connection);

        let contexts = collect_codex_session_contexts_from(&database, 10).unwrap();
        let context = &contexts[0];
        let serialized = serde_json::to_string(context).unwrap();

        assert_eq!(context.model.as_deref(), Some("safe-model"));
        assert_eq!(context.approval_policy.as_deref(), Some("on-request"));
        assert_eq!(context.sandbox_mode.as_deref(), Some("workspace-write"));
        assert!(!serialized.contains("TOP_SECRET"));
    }

    #[test]
    fn codex_handshake_kills_a_silent_process_at_the_deadline() {
        let (_directory, script) = sleeping_script();
        let started = Instant::now();
        let result = CodexAppServer::start_with_timeout(script, Duration::from_millis(100));
        let error = match result {
            Ok(_) => panic!("silent fake app-server unexpectedly initialized"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "collector_timeout");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn turn_options_pass_reasoning_effort_per_provider() {
        let args_for = |agent, options: TurnOptions<'_>| {
            let mut command = Command::new("placeholder");
            apply_turn_options(&mut command, agent, &options);
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };

        // Claude Code takes a --effort flag and supports the deep levels.
        assert_eq!(
            args_for(
                crate::domain::AgentType::ClaudeCode,
                TurnOptions {
                    reasoning_effort: Some("xhigh"),
                    ..Default::default()
                },
            ),
            vec!["--effort", "xhigh"]
        );

        // Codex has no --effort flag: effort is passed as a config override.
        assert_eq!(
            args_for(
                crate::domain::AgentType::Codex,
                TurnOptions {
                    reasoning_effort: Some("high"),
                    ..Default::default()
                },
            ),
            vec!["-c", "model_reasoning_effort=\"high\""]
        );

        // Providers without a per-turn effort ignore it rather than passing a
        // flag the CLI would reject.
        assert!(
            args_for(
                crate::domain::AgentType::ZCode,
                TurnOptions {
                    reasoning_effort: Some("high"),
                    ..Default::default()
                },
            )
            .is_empty()
        );

        // Effort still applies when a model override is absent, and blank
        // values never reach the command line.
        assert_eq!(
            args_for(
                crate::domain::AgentType::ClaudeCode,
                TurnOptions {
                    reasoning_effort: Some("  "),
                    ..Default::default()
                },
            ),
            Vec::<String>::new()
        );
        assert_eq!(
            args_for(
                crate::domain::AgentType::ClaudeCode,
                TurnOptions {
                    model: Some("claude-opus-5"),
                    permission_mode: Some("plan"),
                    ..Default::default()
                },
            ),
            vec!["--model", "claude-opus-5", "--permission-mode", "plan"]
        );
    }

    #[test]
    fn turn_options_support_explicit_session_exists_override() {
        let opt_new = TurnOptions {
            session_exists: Some(false),
            ..Default::default()
        };
        assert_eq!(opt_new.session_exists, Some(false));

        let opt_resume = TurnOptions {
            session_exists: Some(true),
            ..Default::default()
        };
        assert_eq!(opt_resume.session_exists, Some(true));
    }

    #[test]
    fn aggregate_zcode_root_stats_rolls_up_subagents_and_sums_assistant_tokens() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                title TEXT,
                directory TEXT,
                time_created INTEGER,
                time_updated INTEGER
            );
            CREATE TABLE message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                data TEXT NOT NULL,
                time_created INTEGER
            );
            INSERT INTO session VALUES ('root_sess_1', NULL, 'Root Session', 'D:/code', 1000, 2000);
            INSERT INTO session VALUES ('sub_sess_2', 'root_sess_1', 'Subagent', 'D:/code', 1100, 1900);
            INSERT INTO session VALUES ('independent_sess_3', '', 'Other Session', 'D:/code', 1200, 2100);

            -- 2 assistant messages in root_sess_1
            INSERT INTO message VALUES ('m1', 'root_sess_1', '{\"role\":\"assistant\",\"tokens\":{\"total\":500,\"input\":400,\"output\":100,\"cache\":{\"read\":350,\"write\":50},\"reasoning\":20}}', 1010);
            INSERT INTO message VALUES ('m2', 'root_sess_1', '{\"role\":\"assistant\",\"tokens\":{\"total\":600,\"input\":500,\"output\":100,\"cache\":{\"read\":450,\"write\":50},\"reasoning\":30}}', 1020);
            -- 1 user message in root_sess_1 (should NOT be aggregated)
            INSERT INTO message VALUES ('m3', 'root_sess_1', '{\"role\":\"user\",\"tokens\":{\"total\":1000,\"input\":1000,\"output\":0}}', 1005);
            -- 1 assistant message in sub_sess_2 (must be rolled up to root_sess_1)
            INSERT INTO message VALUES ('m4', 'sub_sess_2', '{\"role\":\"assistant\",\"tokens\":{\"total\":800,\"input\":700,\"output\":100,\"cache\":{\"read\":600,\"write\":0},\"reasoning\":50}}', 1110);
            -- 1 assistant message in independent_sess_3
            INSERT INTO message VALUES ('m5', 'independent_sess_3', '{\"role\":\"assistant\",\"tokens\":{\"total\":200,\"input\":150,\"output\":50,\"cache\":{\"read\":100,\"write\":0},\"reasoning\":0}}', 1210);",
        ).unwrap();

        let stats = aggregate_zcode_root_stats(&conn);
        let root = stats
            .get("root_sess_1")
            .expect("root_sess_1 should be present");
        assert_eq!(root.calls, 3); // 2 in root + 1 in sub
        assert_eq!(root.total_tokens, 1900); // 500 + 600 + 800
        assert_eq!(root.input_tokens, 1600); // 400 + 500 + 700
        assert_eq!(root.output_tokens, 300); // 100 + 100 + 100
        assert_eq!(root.cache_read_tokens, 1400); // 350 + 450 + 600
        assert_eq!(root.reasoning_tokens, 100); // 20 + 30 + 50

        let other = stats
            .get("independent_sess_3")
            .expect("independent_sess_3 should be present");
        assert_eq!(other.calls, 1);
        assert_eq!(other.total_tokens, 200);
        assert_eq!(other.cache_read_tokens, 100);
    }

    #[test]
    fn a_sessions_subagents_are_read_from_their_own_files() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("projects").join("-p");
        let session = project.join("s1");
        let subs = session.join("subagents");
        fs::create_dir_all(&subs).unwrap();
        let main = project.join("s1.jsonl");
        fs::write(&main, "").unwrap();
        fs::write(
            subs.join("agent-aaa.jsonl"),
            concat!(
                r#"{"type":"assistant","message":{"id":"m1","model":"deepseek-v4.1-flash","usage":{"input_tokens":10,"output_tokens":100,"cache_read_input_tokens":900,"cache_creation_input_tokens":0}}}"#,
                "\n",
                // the same message's next block repeats the usage: one call
                r#"{"type":"assistant","message":{"id":"m1","model":"deepseek-v4.1-flash","usage":{"input_tokens":10,"output_tokens":100,"cache_read_input_tokens":900,"cache_creation_input_tokens":0}}}"#,
                "\n",
            ),
        )
        .unwrap();
        fs::write(
            subs.join("agent-aaa.meta.json"),
            r#"{"agentType":"Explore","description":"Survey the module","spawnDepth":1}"#,
        )
        .unwrap();
        fs::write(
            subs.join("agent-bbb.jsonl"),
            r#"{"type":"assistant","message":{"id":"m2","model":"gemini-3.8-flash-n","usage":{"input_tokens":5,"output_tokens":50}}}"#,
        )
        .unwrap();
        fs::write(
            subs.join("agent-bbb.meta.json"),
            r#"{"agentType":"Plan","description":"Write the plan","parentAgentId":"aaa","spawnDepth":2}"#,
        )
        .unwrap();

        let mut found = collect_claude_subagents(&main);
        found.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(found.len(), 2, "one entry per subagent file");
        assert_eq!(found[0].id, "aaa");
        assert_eq!(found[0].kind, "Explore");
        assert_eq!(found[0].description, "Survey the module");
        assert_eq!(found[0].request_count, 1, "a message counts once");
        assert_eq!(found[0].completion_tokens, 100);
        assert_eq!(found[1].parent, "aaa", "a nested subagent names its parent");
        assert_eq!(found[1].depth, 2);
        fs::remove_dir_all(dir).ok();
    }

    /// The console asks for the session list every few seconds, and the store
    /// holds tens of megabytes of finished subagent transcripts. A file whose
    /// length and last write have not moved is answered from memory; one that
    /// grows is read again.
    #[test]
    fn a_finished_subagent_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("agent-ccc.jsonl");
        let line = r#"{"type":"assistant","message":{"id":"m1","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":42}}}"#;
        // the moment is pinned, so each write does not give the file a new one:
        // the memo's key is (length, moment), and the test is about that key
        // and not about how fast two writes happen to land
        let stamp = SystemTime::now() - Duration::from_secs(600);
        let write_at = |text: &str| {
            fs::write(&file, text).unwrap();
            fs::OpenOptions::new()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(stamp)
                .unwrap();
        };

        write_at(&format!("{line}\n"));
        let first = read_claude_subagent_cached(&file).expect("first read");
        assert_eq!(first.completion_tokens, 42);

        // same length, same moment: the memo answers even though the bytes
        // now say something else
        write_at(&format!("{line}\n").replace("42", "99"));
        let cached = read_claude_subagent_cached(&file).expect("cached read");
        assert_eq!(
            cached.completion_tokens, 42,
            "a file that has not moved is not read again"
        );

        // a longer file has moved: it is read again, whatever its moment
        write_at(&format!(
            "{line}\n{}\n",
            r#"{"type":"assistant","message":{"id":"m2","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":8}}}"#
        ));
        let reread = read_claude_subagent_cached(&file).expect("second read");
        assert_eq!(reread.request_count, 2, "a grown transcript is read again");
        assert_eq!(reread.completion_tokens, 50);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn deleting_a_session_moves_its_files_to_the_trash() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude");
        let project = config.join("projects").join("-p");
        let session = project.join("s1");
        fs::create_dir_all(session.join("subagents")).unwrap();
        fs::write(project.join("s1.jsonl"), "{}\n").unwrap();
        fs::write(session.join("subagents").join("agent-a.jsonl"), "{}\n").unwrap();
        let trash = dir.path().join("trash");
        // a session written just now is still being used
        let refused = delete_claude_session_in(&config, &trash, "s1");
        assert!(refused.is_err(), "a session written this minute is kept");

        // backdate every file past the guard, then delete
        let old = SystemTime::now() - Duration::from_secs(600);
        let backdate = |path: &Path| {
            let file = fs::OpenOptions::new().write(true).open(path).unwrap();
            file.set_modified(old).unwrap();
        };
        backdate(&project.join("s1.jsonl"));
        backdate(&session.join("subagents").join("agent-a.jsonl"));
        let moved = delete_claude_session_in(&config, &trash, "s1").expect("deleted");
        assert!(!project.join("s1.jsonl").exists());
        assert!(!session.exists(), "the subagent folder goes with it");
        assert!(moved.starts_with(&trash));
        assert!(
            moved.join("s1.jsonl").is_file(),
            "the transcript is in the trash, not gone"
        );

        // an id the store never had is refused, not created
        assert!(delete_claude_session_in(&config, &trash, "missing").is_err());
        // and an id that could walk the tree is refused before any lookup
        assert!(delete_claude_session_in(&config, &trash, "../s1").is_err());
        fs::remove_dir_all(dir).ok();
    }

    /// Coding Assistant keeps `file-history/<id>` and `session-env/<id>` beside
    /// the conversation's own `<id>` folder. All three carry the same name, and
    /// a session with all three has to delete in one go: the moves must land
    /// somewhere apart instead of the second one colliding with the first.
    #[test]
    fn deleting_a_session_that_has_two_folders_named_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("claude");
        let project = config.join("projects").join("-p");
        let session = project.join("s1");
        fs::create_dir_all(session.join("subagents")).unwrap();
        fs::write(project.join("s1.jsonl"), "{}\n").unwrap();
        fs::write(session.join("subagents").join("agent-a.jsonl"), "{}\n").unwrap();
        let history = config.join("file-history").join("s1");
        fs::create_dir_all(&history).unwrap();
        fs::write(history.join("old-version.txt"), "before\n").unwrap();
        let env = config.join("session-env").join("s1");
        fs::create_dir_all(&env).unwrap();
        fs::write(env.join("env.json"), "{}").unwrap();
        fs::create_dir_all(config.join("todos")).unwrap();
        fs::write(config.join("todos").join("s1-agent-a.json"), "[]").unwrap();

        // backdate every file past the write guard
        let old = SystemTime::now() - Duration::from_secs(600);
        let backdate = |path: &Path| {
            let file = fs::OpenOptions::new().write(true).open(path).unwrap();
            file.set_modified(old).unwrap();
        };
        for file in [
            project.join("s1.jsonl"),
            session.join("subagents").join("agent-a.jsonl"),
            history.join("old-version.txt"),
            env.join("env.json"),
            config.join("todos").join("s1-agent-a.json"),
        ] {
            backdate(&file);
        }

        let trash = dir.path().join("trash");
        let moved = delete_claude_session_in(&config, &trash, "s1").expect("all of it moves");
        for gone in [&project.join("s1.jsonl"), &session, &history, &env] {
            assert!(!gone.exists(), "{} is out of the store", gone.display());
        }
        // every moved piece is in the trash, none of them merged into another
        assert!(moved.join("s1.jsonl").is_file());
        assert!(moved.join("s1/subagents/agent-a.jsonl").is_file());
        let names: Vec<String> = fs::read_dir(&moved)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names.len(),
            5,
            "each of the five items landed on its own name: {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "session-env-s1"),
            "the env folder is kept under a name of its own: {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "file-history-s1"),
            "the history folder is kept under a name of its own: {names:?}"
        );
        assert!(
            moved.join("session-env-s1/env.json").is_file(),
            "the env file is inside it"
        );
        assert!(
            names.iter().any(|n| n.ends_with("s1-agent-a.json")),
            "the todo is kept: {names:?}"
        );
        fs::remove_dir_all(dir).ok();
    }

    /// A delete that cannot finish must not leave the session half moved: the
    /// transcript that already went to the trash comes back, so the row the
    /// page still shows can be deleted again.
    #[test]
    fn a_delete_that_fails_part_way_puts_the_session_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let transcript = dir.path().join("s1.jsonl");
        fs::write(&transcript, "{}\n").unwrap();
        let folder = dir.path().join("s1");
        fs::create_dir_all(folder.join("subagents")).unwrap();
        fs::write(folder.join("subagents").join("agent-a.jsonl"), "{}\n").unwrap();
        // a source that has gone between the listing and the move
        let vanished = store.join("session-env").join("s1");
        let trash = dir.path().join("trash").join("1-s1");
        fs::create_dir_all(&trash).unwrap();

        let paths = vec![transcript.clone(), folder.clone(), vanished];
        let failure = move_all_into(&trash, &paths).expect_err("the missing one stops the move");

        assert!(transcript.is_file(), "the transcript is back where it was");
        assert!(
            folder.join("subagents").join("agent-a.jsonl").is_file(),
            "the folder is back with its subagents"
        );
        let left: Vec<String> = fs::read_dir(&trash)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            left.is_empty(),
            "nothing is left claimed by the trash: {left:?}"
        );
        let message = failure.to_string();
        assert!(message.contains("put it all back"), "{message}");
        assert!(message.contains("moved 2 of 3"), "{message}");
        fs::remove_dir_all(dir).ok();
    }
}
