use std::{
    collections::{HashSet, VecDeque},
    env, fs,
    io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, UNIX_EPOCH},
};

use chrono::DateTime;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

const MAX_PROTOCOL_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(15);
const CHILD_WAIT_INTERVAL: Duration = Duration::from_millis(10);
const CLAUDE_RECENT_SESSION_LIMIT: usize = 100;
const CLAUDE_ALL_SESSION_LIMIT: usize = 1_000;
const MAX_AGENT_TURN_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const LOCAL_CONTEXT_HEAD_BYTES: u64 = 512 * 1024;
const LOCAL_CONTEXT_TAIL_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SESSION_TITLE_CHARS: usize = 160;
const MAX_CONFIG_VALUE_CHARS: usize = 96;

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
}

/// Outcome of a streamed agent process: final message plus the session id the
/// CLI reported for freshly started conversations.
#[derive(Debug, Clone)]
pub struct AgentStreamOutcome {
    pub response: String,
    pub session_id: Option<String>,
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
    content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaudeTranscriptEntry {
    pub session_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub started_at_ms: Option<i64>,
    pub last_activity_at_ms: Option<i64>,
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
            if let Some(entry) = read_claude_transcript_metadata(&path, &program)? {
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
        apply_claude_context_line(&line, context);
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
            if let Some(usage) = value.pointer("/message/usage") {
                let input = usage_u64(usage, "input_tokens")
                    .saturating_add(usage_u64(usage, "cache_creation_input_tokens"))
                    .saturating_add(usage_u64(usage, "cache_read_input_tokens"));
                if input > 0 {
                    context.context_used_tokens = Some(input);
                }
            }
        }
        Some("user") if context.title.is_none() => {
            let prompt_text = value
                .pointer("/message/content")
                .and_then(|c| {
                    if let Some(text) = c.as_str() {
                        Some(text)
                    } else if let Some(arr) = c.as_array() {
                        arr.iter()
                            .find_map(|item| item.get("text").and_then(Value::as_str))
                    } else {
                        None
                    }
                })
                .or_else(|| value.get("prompt").and_then(Value::as_str));
            if let Some(text) = prompt_text {
                context.title = local_prompt_title(text);
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
    .or(context.context_window_tokens);
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

fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("HOME").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
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

    // Metadata is repeated on transcript entries. Bound the scan so a damaged
    // or adversarial transcript cannot make background collection unbounded.
    for _ in 0..64 {
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
        let Ok(metadata) = serde_json::from_str::<ClaudeTranscriptMetadata>(&line) else {
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
                .or(metadata.content)
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
        if session_id.is_some() && cwd.is_some() && started_at_ms.is_some() && title.is_some() {
            break;
        }
    }

    let session_id = session_id
        .or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .map(str::to_owned)
        })
        .filter(|value| !value.trim().is_empty());
    Ok(session_id.map(|session_id| ClaudeTranscriptEntry {
        session_id,
        cwd: cwd.unwrap_or_default(),
        title,
        started_at_ms,
        last_activity_at_ms,
    }))
}

pub fn discover_claude_binary() -> PathBuf {
    if let Some(path) = env::var_os("CLAUDE_BINARY").filter(|v| !v.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(user_profile) = env::var_os("USERPROFILE").map(PathBuf::from) {
        let local_bin = user_profile.join(".local").join("bin").join("claude.exe");
        if local_bin.is_file() {
            return local_bin;
        }
        let local_cmd = user_profile.join(".local").join("bin").join("claude.cmd");
        if local_cmd.is_file() {
            return local_cmd;
        }
    }
    PathBuf::from("claude")
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

pub fn collect_claude_sessions_for_remote()
-> Result<Vec<(String, String, Option<String>)>, CollectorError> {
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
        project_candidates.truncate(30);

        for (_, path) in project_candidates {
            if let Some(entry) = read_claude_transcript_metadata(&path, &program)? {
                entries.push((
                    entry.session_id,
                    entry.cwd,
                    entry.title,
                    entry.last_activity_at_ms,
                ));
            }
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.3));
    entries.truncate(100);
    Ok(entries
        .into_iter()
        .map(|(id, cwd, title, _)| (id, cwd, title))
        .collect())
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

pub fn collect_codex_local_threads_from(
    database: &Path,
    limit: u32,
) -> Result<Value, CollectorError> {
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
    let sql = format!(
        "SELECT id, cwd, {created}, {updated}, {recency}, {source} FROM threads{archived_filter} ORDER BY {order_by} DESC LIMIT ?1"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    let rows = statement
        .query_map([i64::from(limit)], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "cwd": row.get::<_, String>(1).unwrap_or_default(),
                "createdAt": row.get::<_, Option<i64>>(2)?.unwrap_or_default(),
                "updatedAt": row.get::<_, Option<i64>>(3)?.unwrap_or_default(),
                "recencyAt": row.get::<_, Option<i64>>(4)?.unwrap_or_default(),
                "source": row.get::<_, String>(5).unwrap_or_else(|_| "localStore".to_owned()),
                "status": { "type": "persisted" }
            }))
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CollectorError::Database { program, source })?;
    Ok(json!({ "data": rows, "nextCursor": null }))
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
        context.title = title.and_then(|value| metadata_label(&value, MAX_SESSION_TITLE_CHARS));
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
    let start = length.saturating_sub(LOCAL_CONTEXT_TAIL_BYTES);
    let mut reader = BufReader::new(file);
    reader
        .seek(SeekFrom::Start(start))
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?;
    let mut consumed = 0_u64;
    while consumed < length.saturating_sub(start) {
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
                let used = usage_u64(last, "total_tokens").max(usage_u64(last, "input_tokens"));
                if used > 0 {
                    context.context_used_tokens = Some(used);
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
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let clean = line.trim_start_matches(['#', ' ', '>', '-', '*']).trim();
    if clean.is_empty() || clean.starts_with('<') {
        return None;
    }
    metadata_label(clean, MAX_SESSION_TITLE_CHARS)
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

const ZCODE_RECENT_SESSION_LIMIT: usize = 100;
const ZCODE_ALL_SESSION_LIMIT: usize = 1_000;

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

pub fn collect_zcode_sessions_from(
    database: &Path,
    include_completed: bool,
) -> Result<String, CollectorError> {
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
            s.time_updated
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

    serde_json::to_string(&rows).map_err(|source| CollectorError::InvalidJson { program, source })
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
            (SELECT json_extract(data, '$.modelID') FROM message WHERE session_id = s.id AND json_extract(data, '$.role') = 'assistant' ORDER BY time_created DESC LIMIT 1) as model,
            (SELECT json_extract(data, '$.mode') FROM message WHERE session_id = s.id AND json_extract(data, '$.role') = 'assistant' ORDER BY time_created DESC LIMIT 1) as mode,
            (SELECT json_extract(data, '$.tokens.total') FROM message WHERE session_id = s.id AND json_extract(data, '$.role') = 'assistant' AND json_extract(data, '$.tokens.total') > 0 ORDER BY time_created DESC LIMIT 1) as total_tokens,
            (SELECT json_extract(data, '$.tokens.input') FROM message WHERE session_id = s.id AND json_extract(data, '$.role') = 'assistant' AND json_extract(data, '$.tokens.total') > 0 ORDER BY time_created DESC LIMIT 1) as input_tokens
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
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
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
    for (session_id, title, model, mode, total_tokens, input_tokens) in rows {
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
        contexts.push(context);
    }
    Ok(contexts)
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
#[allow(clippy::too_many_arguments)]
/// Resumes an existing agent conversation and streams the turn's updates.
pub fn run_agent_session_turn_streaming(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: Option<&Path>,
    session_id: &str,
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
        crate::domain::AgentType::ClaudeCode => {
            command.args([
                "--resume",
                session_id,
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
    apply_turn_options(&mut command, agent, &options);
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
    apply_turn_options(&mut command, agent, &options);
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
    let Some(model) = options
        .model
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    match agent {
        crate::domain::AgentType::ClaudeCode => {
            command.args(["--model", model]);
            if let Some(mode) = options
                .permission_mode
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                command.args(["--permission-mode", mode]);
            }
        }
        crate::domain::AgentType::Codex => {
            command.args(["-m", model]);
            if let Some(mode) = options
                .permission_mode
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let sandbox = match mode {
                    "read-only" | "plan" => "read-only",
                    "bypassPermissions" | "danger-full-access" => "danger-full-access",
                    _ => "workspace-write",
                };
                command.args(["--sandbox", sandbox]);
            }
        }
        crate::domain::AgentType::ZCode => {
            command.args(["--model", model]);
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
    let response = final_agent_message(agent, &output).ok_or_else(|| CollectorError::Protocol {
        program,
        code: "missing_final_message".to_owned(),
        message: "missing final agent message".to_owned(),
    })?;
    let session_id = extract_session_id_from_output(agent, &output);
    Ok(AgentStreamOutcome {
        response,
        session_id,
    })
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
            if value.get("type").and_then(Value::as_str) == Some("stream_event") {
                let event = value.get("event")?;
                if event.get("type").and_then(Value::as_str) == Some("content_block_delta")
                    && event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
                {
                    return event
                        .pointer("/delta/text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                        .map(|text| AgentTurnUpdate::AssistantDelta(text.to_owned()));
                }
            }
            let has_tool = value.get("type").and_then(Value::as_str) == Some("assistant")
                && value
                    .pointer("/message/content")
                    .and_then(Value::as_array)
                    .is_some_and(|content| {
                        content.iter().any(|block| {
                            block.get("type").and_then(Value::as_str) == Some("tool_use")
                        })
                    });
            has_tool.then_some(AgentTurnUpdate::Activity("Claude 正在调用工具"))
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
        crate::domain::AgentType::ClaudeCode => output.lines().rev().find_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            if value.get("type").and_then(Value::as_str) != Some("result") {
                return None;
            }
            value
                .get("result")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_owned)
        }),
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

enum ProtocolRead {
    Message(Value),
    InvalidJson(serde_json::Error),
    LineTooLarge,
    Io(std::io::Error),
    Eof,
}

fn resolve_codex_app_server_binary(binary: &Path) -> PathBuf {
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

pub struct CodexAppServer {
    program: String,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    receiver: Receiver<ProtocolRead>,
    reader: Option<JoinHandle<()>>,
    pending_messages: VecDeque<Value>,
    next_request_id: u64,
    response_timeout: Duration,
    job: Option<ChildJob>,
}

impl CodexAppServer {
    pub fn start(binary: impl AsRef<Path>) -> Result<Self, CollectorError> {
        Self::start_with_timeout(binary, DEFAULT_COLLECTOR_TIMEOUT)
    }

    pub fn start_with_timeout(
        binary: impl AsRef<Path>,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        Self::start_mode(binary, response_timeout, false)
    }

    /// A long-lived control connection must receive content and tool events.
    /// Metadata collectors retain their original opt-out list.
    pub fn start_interactive(
        binary: impl AsRef<Path>,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        Self::start_mode(binary, response_timeout, true)
    }

    fn start_mode(
        binary: impl AsRef<Path>,
        response_timeout: Duration,
        interactive: bool,
    ) -> Result<Self, CollectorError> {
        let configured_binary = binary.as_ref();
        let binary = resolve_codex_app_server_binary(configured_binary);
        let program = binary.display().to_string();
        let mut command = Command::new(&binary);
        command
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || read_protocol(stdout, sender));
        let mut server = Self {
            program,
            child,
            stdin: BufWriter::new(stdin),
            receiver,
            reader: Some(reader),
            pending_messages: VecDeque::new(),
            next_request_id: 1,
            response_timeout,
            job: Some(job),
        };
        server.initialize(interactive)?;
        Ok(server)
    }

    fn initialize(&mut self, interactive: bool) -> Result<(), CollectorError> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "agent_platform",
                    "title": "AgentPulse Runner",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": false,
                    "optOutNotificationMethods": if interactive { json!([]) } else { json!([
                        "thread/started",
                        "thread/name/updated",
                        "thread/tokenUsage/updated",
                        "turn/diff/updated",
                        "turn/plan/updated",
                        "item/started",
                        "item/completed",
                        "item/agentMessage/delta",
                        "item/plan/delta",
                        "item/reasoning/textDelta",
                        "item/reasoning/summaryTextDelta",
                        "item/reasoning/summaryPartAdded",
                        "item/commandExecution/outputDelta",
                        "item/fileChange/outputDelta",
                        "warning",
                        "configWarning",
                        "error"
                    ]) }
                }
            }),
        )?;
        self.send(&json!({"method": "initialized", "params": {}}))?;
        Ok(())
    }

    pub fn list_threads(&mut self, limit: u32) -> Result<Value, CollectorError> {
        self.request(
            "thread/list",
            json!({
                "cursor": null,
                "limit": limit,
                "sortKey": "recency_at",
                "sortDirection": "desc",
                "archived": false,
                "useStateDbOnly": false,
                "sourceKinds": [
                    "cli",
                    "vscode",
                    "exec",
                    "appServer",
                    "subAgent",
                    "subAgentReview",
                    "subAgentCompact",
                    "subAgentThreadSpawn",
                    "subAgentOther",
                    "unknown"
                ]
            }),
        )
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, CollectorError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.send(&json!({
            "method": method,
            "id": request_id,
            "params": params
        }))?;

        let deadline = Instant::now() + self.response_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CollectorError::Timeout {
                    program: self.program.clone(),
                    timeout_ms: self.response_timeout.as_millis(),
                });
            }
            let message = self.read_message(request_id, remaining)?;
            // Server-to-client requests and notifications carry a `method` and
            // use an id namespace independent from ours; a colliding numeric id
            // must not be mistaken for the response to this request.
            if message.get("method").is_some()
                || message.get("id").and_then(Value::as_u64) != Some(request_id)
            {
                self.pending_messages.push_back(message);
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(CollectorError::Protocol {
                    program: self.program.clone(),
                    code: error
                        .get("code")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "unknown".into()),
                    message: sanitize_protocol_message(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error"),
                    ),
                });
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Wait for a server notification while preserving unrelated messages for
    /// the normal collector loop. Auth login uses this to keep the app-server
    /// alive until the browser/device-code flow reports completion.
    pub fn wait_for_notification(
        &mut self,
        method: &str,
        timeout: Duration,
    ) -> Result<Option<Value>, CollectorError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CollectorError::Timeout {
                    program: self.program.clone(),
                    timeout_ms: timeout.as_millis(),
                });
            }
            let message = self.read_message(0, remaining)?;
            if message.get("method").and_then(Value::as_str) == Some(method) {
                return Ok(message.get("params").cloned());
            }
            self.pending_messages.push_back(message);
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), CollectorError> {
        serde_json::to_writer(&mut self.stdin, message).map_err(|source| {
            CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }
        })?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|source| CollectorError::Io {
                program: self.program.clone(),
                source,
            })
    }

    /// Answer a server request in its own ID namespace; never allocate a new ID.
    pub fn respond(&mut self, request_id: Value, result: Value) -> Result<(), CollectorError> {
        self.send(&json!({"id": request_id, "result": result}))
    }

    pub fn drain_messages(&mut self) -> Result<Vec<Value>, CollectorError> {
        let mut messages: Vec<Value> = self.pending_messages.drain(..).collect();
        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(message)) => messages.push(message),
                Ok(ProtocolRead::InvalidJson(source)) => {
                    return Err(CollectorError::InvalidJson {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::LineTooLarge) => {
                    return Err(CollectorError::ProtocolLineTooLarge {
                        program: self.program.clone(),
                        limit: MAX_PROTOCOL_LINE_BYTES,
                    });
                }
                Ok(ProtocolRead::Io(source)) => {
                    return Err(CollectorError::Io {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::Eof) => {
                    return Err(CollectorError::UnexpectedEof {
                        program: self.program.clone(),
                        request_id: self.next_request_id,
                    });
                }
                Err(TryRecvError::Empty) => return Ok(messages),
                Err(TryRecvError::Disconnected) => {
                    return Err(CollectorError::ReaderStopped {
                        program: self.program.clone(),
                    });
                }
            }
        }
    }

    fn read_message(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<Value, CollectorError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(ProtocolRead::Message(message)) => Ok(message),
            Ok(ProtocolRead::InvalidJson(source)) => Err(CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }),
            Ok(ProtocolRead::LineTooLarge) => Err(CollectorError::ProtocolLineTooLarge {
                program: self.program.clone(),
                limit: MAX_PROTOCOL_LINE_BYTES,
            }),
            Ok(ProtocolRead::Io(source)) => Err(CollectorError::Io {
                program: self.program.clone(),
                source,
            }),
            Ok(ProtocolRead::Eof) => Err(CollectorError::UnexpectedEof {
                program: self.program.clone(),
                request_id,
            }),
            Err(RecvTimeoutError::Timeout) => Err(CollectorError::Timeout {
                program: self.program.clone(),
                timeout_ms: timeout.as_millis(),
            }),
            Err(RecvTimeoutError::Disconnected) => Err(CollectorError::ReaderStopped {
                program: self.program.clone(),
            }),
        }
    }
}

impl Drop for CodexAppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.job.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub struct ZCodeAppServer {
    program: String,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    receiver: Receiver<ProtocolRead>,
    reader: Option<JoinHandle<()>>,
    pending_messages: VecDeque<Value>,
    next_request_id: u64,
    response_timeout: Duration,
    job: Option<ChildJob>,
}

pub fn resolve_zcode_app_server_script() -> Option<PathBuf> {
    if let Some(path) = env::var_os("ZCODE_APP_SERVER_PATH").filter(|v| !v.is_empty()) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    let candidates = [
        PathBuf::from(r"D:\zcode\resources\glm\zcode.cjs"),
        PathBuf::from(r"C:\Program Files\zcode\resources\glm\zcode.cjs"),
        PathBuf::from(r"C:\Program Files (x86)\zcode\resources\glm\zcode.cjs"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    if let Some(home) = home_dir() {
        let local_app = home.join(r"AppData\Local\Programs\zcode\resources\glm\zcode.cjs");
        if local_app.is_file() {
            return local_app.canonicalize().ok().or(Some(local_app));
        }
    }
    None
}

pub fn resolve_node_executable() -> PathBuf {
    if let Some(path) = env::var_os("NODE_BINARY").filter(|v| !v.is_empty()) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    let candidates = [
        PathBuf::from(r"D:\nodejs\node.exe"),
        PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("node")
}

impl ZCodeAppServer {
    pub fn start_if_available(response_timeout: Duration) -> Result<Self, CollectorError> {
        let script = resolve_zcode_app_server_script().ok_or_else(|| CollectorError::Start {
            program: "zcode".to_owned(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "zcode_script_not_found"),
        })?;
        Self::start_with_script(script, response_timeout)
    }

    pub fn start_with_script(
        script: impl AsRef<Path>,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        let script_path = script.as_ref();
        let node_bin = resolve_node_executable();
        let program = format!("node {}", script_path.display());
        let mut command = Command::new(&node_bin);
        command
            .arg(script_path)
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
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
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || read_zcode_protocol(stdout, sender));
        let server = Self {
            program,
            child,
            stdin: BufWriter::new(stdin),
            receiver,
            reader: Some(reader),
            pending_messages: VecDeque::new(),
            next_request_id: 1,
            response_timeout,
            job: Some(job),
        };
        Ok(server)
    }

    fn handle_internal_query(&mut self, message: &Value) -> Result<bool, CollectorError> {
        if let (Some(req_id), Some(method)) = (
            message.get("id"),
            message.get("method").and_then(Value::as_str),
        ) {
            if method == "session/requestRuntimePreferences" {
                self.respond(
                    req_id.clone(),
                    json!({
                        "nativeSearchEnhancementsEnabled": false,
                        "memoryEnabled": false,
                        "askUserQuestionAutoResolutionEnabled": true,
                        "modelContextBudgetStrategy": "legacy"
                    }),
                )?;
                return Ok(true);
            } else if method == "interaction/requestOfficialMcpAuthHeaders" {
                self.respond(
                    req_id.clone(),
                    json!({"ok": false, "reason": "official_auth_unavailable"}),
                )?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, CollectorError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let request_id_str = request_id.to_string();
        self.send(&json!({
            "method": method,
            "id": request_id_str,
            "params": params
        }))?;

        let deadline = Instant::now() + self.response_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CollectorError::Timeout {
                    program: self.program.clone(),
                    timeout_ms: self.response_timeout.as_millis(),
                });
            }
            let message = self.read_message(request_id, remaining)?;
            if self.handle_internal_query(&message)? {
                continue;
            }
            let is_match = message
                .get("id")
                .and_then(Value::as_str)
                .map(|id| id == request_id_str)
                .unwrap_or(false)
                || message.get("id").and_then(Value::as_u64) == Some(request_id);
            if message.get("method").is_some() || !is_match {
                self.pending_messages.push_back(message);
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(CollectorError::Protocol {
                    program: self.program.clone(),
                    code: error
                        .get("code")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "unknown".into()),
                    message: sanitize_protocol_message(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error"),
                    ),
                });
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), CollectorError> {
        serde_json::to_writer(&mut self.stdin, message).map_err(|source| {
            CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }
        })?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|source| CollectorError::Io {
                program: self.program.clone(),
                source,
            })
    }

    pub fn respond(&mut self, request_id: Value, result: Value) -> Result<(), CollectorError> {
        self.send(&json!({"id": request_id, "result": result}))
    }

    pub fn drain_messages(&mut self) -> Result<Vec<Value>, CollectorError> {
        let mut messages: Vec<Value> = Vec::new();
        while let Some(msg) = self.pending_messages.pop_front() {
            if !self.handle_internal_query(&msg)? {
                messages.push(msg);
            }
        }
        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(message)) => {
                    if !self.handle_internal_query(&message)? {
                        messages.push(message);
                    }
                }
                Ok(ProtocolRead::InvalidJson(source)) => {
                    return Err(CollectorError::InvalidJson {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::LineTooLarge) => {
                    return Err(CollectorError::ProtocolLineTooLarge {
                        program: self.program.clone(),
                        limit: MAX_PROTOCOL_LINE_BYTES,
                    });
                }
                Ok(ProtocolRead::Io(source)) => {
                    return Err(CollectorError::Io {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::Eof)
                | Err(TryRecvError::Empty)
                | Err(TryRecvError::Disconnected) => break,
            }
        }
        Ok(messages)
    }

    pub fn session_resume(&mut self, session_id: &str) -> Result<Value, CollectorError> {
        let resume_res = self.request("session/resume", json!({"sessionId": session_id}))?;
        let _ = self.request(
            "session/subscribe",
            json!({"sessionId": session_id, "deliveryKind": "desktop-continuous"}),
        );
        Ok(resume_res)
    }

    pub fn session_send(
        &mut self,
        session_id: &str,
        content: &str,
    ) -> Result<Value, CollectorError> {
        self.request(
            "session/send",
            json!({"sessionId": session_id, "content": content}),
        )
    }

    pub fn session_stop(&mut self, session_id: &str) -> Result<Value, CollectorError> {
        self.request("session/stop", json!({"sessionId": session_id}))
    }

    fn read_message(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<Value, CollectorError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(ProtocolRead::Message(message)) => Ok(message),
            Ok(ProtocolRead::InvalidJson(source)) => Err(CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }),
            Ok(ProtocolRead::LineTooLarge) => Err(CollectorError::ProtocolLineTooLarge {
                program: self.program.clone(),
                limit: MAX_PROTOCOL_LINE_BYTES,
            }),
            Ok(ProtocolRead::Io(source)) => Err(CollectorError::Io {
                program: self.program.clone(),
                source,
            }),
            Ok(ProtocolRead::Eof) => Err(CollectorError::UnexpectedEof {
                program: self.program.clone(),
                request_id,
            }),
            Err(RecvTimeoutError::Timeout) => Err(CollectorError::Timeout {
                program: self.program.clone(),
                timeout_ms: timeout.as_millis(),
            }),
            Err(RecvTimeoutError::Disconnected) => Err(CollectorError::ReaderStopped {
                program: self.program.clone(),
            }),
        }
    }
}

impl Drop for ZCodeAppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.job.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_zcode_protocol(stdout: impl Read, sender: mpsc::Sender<ProtocolRead>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_bounded_line(&mut reader) {
            BoundedLine::Eof => {
                let _ = sender.send(ProtocolRead::Eof);
                return;
            }
            BoundedLine::TooLarge => {
                let _ = sender.send(ProtocolRead::LineTooLarge);
                return;
            }
            BoundedLine::Line(line) => {
                let trimmed = line.trim();
                if !trimmed.starts_with('{') {
                    continue;
                }
                match serde_json::from_str(trimmed) {
                    Ok(message) => {
                        if sender.send(ProtocolRead::Message(message)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(ProtocolRead::InvalidJson(error));
                        return;
                    }
                }
            }
            BoundedLine::Io(error) => {
                let _ = sender.send(ProtocolRead::Io(error));
                return;
            }
        }
    }
}

fn read_protocol(stdout: impl Read, sender: mpsc::Sender<ProtocolRead>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_bounded_line(&mut reader) {
            BoundedLine::Eof => {
                let _ = sender.send(ProtocolRead::Eof);
                return;
            }
            BoundedLine::TooLarge => {
                let _ = sender.send(ProtocolRead::LineTooLarge);
                return;
            }
            BoundedLine::Line(line) => match serde_json::from_str(&line) {
                Ok(message) => {
                    if sender.send(ProtocolRead::Message(message)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(ProtocolRead::InvalidJson(error));
                    return;
                }
            },
            BoundedLine::Io(error) => {
                let _ = sender.send(ProtocolRead::Io(error));
                return;
            }
        }
    }
}

enum BoundedLine {
    Line(String),
    TooLarge,
    Eof,
    Io(std::io::Error),
}

/// Read one newline-terminated line while enforcing the size limit during the
/// read. `read_line` would buffer an arbitrarily long line before any length
/// check could run, letting a misbehaving child exhaust memory.
fn read_bounded_line(reader: &mut impl BufRead) -> BoundedLine {
    let mut line = Vec::new();
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return BoundedLine::Io(error),
        };
        if buffer.is_empty() {
            if line.is_empty() {
                return BoundedLine::Eof;
            }
            return finish_line(line);
        }
        let (take, found_newline) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (buffer.len(), false),
        };
        if line.len() + take > MAX_PROTOCOL_LINE_BYTES {
            return BoundedLine::TooLarge;
        }
        line.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if found_newline {
            return finish_line(line);
        }
    }
}

fn finish_line(line: Vec<u8>) -> BoundedLine {
    match String::from_utf8(line) {
        Ok(line) => BoundedLine::Line(line),
        Err(error) => BoundedLine::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.utf8_error(),
        )),
    }
}

#[cfg(windows)]
struct ChildJob(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: ChildJob exclusively owns a Windows kernel handle. Kernel handles
// are valid across threads in the owning process, and Drop only calls
// CloseHandle on that same owned value.
#[cfg(windows)]
unsafe impl Send for ChildJob {}

#[cfg(windows)]
impl ChildJob {
    fn attach(child: &Child) -> std::io::Result<Self> {
        use std::{
            ffi::c_void,
            mem::{size_of, zeroed},
            os::windows::io::AsRawHandle,
            ptr,
        };
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
        };

        // SAFETY: All pointers refer to initialized local data for the duration of each
        // call. Every successful job handle is owned by ChildJob and closed once.
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const information).cast::<c_void>(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .expect("job information size fits u32"),
            ) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            if AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            Ok(Self(handle))
        }
    }
}

#[cfg(windows)]
impl Drop for ChildJob {
    fn drop(&mut self) {
        // SAFETY: The handle is created and exclusively owned by this ChildJob.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Place the child in its own process group on Unix so the whole tree can be
/// signalled at teardown; on Windows, detach console windows so processes run silently.
fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // CREATE_NO_WINDOW: prevent popping up console windows on desktop.
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
    }
}

#[cfg(not(windows))]
struct ChildJob {
    process_group: i32,
}

#[cfg(not(windows))]
impl ChildJob {
    fn attach(child: &Child) -> std::io::Result<Self> {
        // `configure_process_group` made the child a process-group leader, so
        // its PID doubles as the process-group ID.
        Ok(Self {
            process_group: i32::try_from(child.id())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?,
        })
    }
}

#[cfg(not(windows))]
impl Drop for ChildJob {
    fn drop(&mut self) {
        // SAFETY: kill(2) with a negative PID signals the process group the
        // runner created for this collector child; it never signals our own
        // group because the child was spawned as a group leader.
        unsafe {
            libc::kill(-self.process_group, libc::SIGKILL);
        }
    }
}

fn sanitize_protocol_message(message: &str) -> String {
    let sanitized: String = message
        .chars()
        .take(160)
        .filter(|ch| !ch.is_control())
        .collect();
    if sanitized.is_empty() {
        "protocol error".into()
    } else {
        sanitized
    }
}

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
    fn claude_stream_events_emit_text_without_thinking_or_tool_input() {
        let text = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}}"#;
        let thinking = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"private"}}}"#;

        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::ClaudeCode, text),
            Some(AgentTurnUpdate::AssistantDelta("hello".to_owned()))
        );
        assert_eq!(
            parse_agent_turn_update(crate::domain::AgentType::ClaudeCode, thinking),
            None
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
        assert!(started.elapsed() < Duration::from_secs(3));
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
                "\"message\":{\"content\":\"TOP_SECRET_TRANSCRIPT_CONTENT\"}}\n"
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
        assert!(!payload.contains("message"));
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
}
