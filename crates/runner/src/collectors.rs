use std::{
    collections::{HashSet, VecDeque},
    env, fs,
    io::{BufRead, BufReader, BufWriter, Read, Write},
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
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

const MAX_PROTOCOL_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(15);
const CHILD_WAIT_INTERVAL: Duration = Duration::from_millis(10);
const CLAUDE_RECENT_SESSION_LIMIT: usize = 100;
const CLAUDE_ALL_SESSION_LIMIT: usize = 1_000;
const MAX_AGENT_TURN_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeTranscriptMetadata {
    session_id: Option<String>,
    cwd: Option<String>,
    timestamp: Option<String>,
}

#[derive(Debug)]
struct ClaudeTranscriptEntry {
    session_id: String,
    cwd: String,
    started_at_ms: Option<i64>,
    last_activity_at_ms: Option<i64>,
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
        if session_id.is_some() && cwd.is_some() && started_at_ms.is_some() {
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
        started_at_ms,
        last_activity_at_ms,
    }))
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

pub fn run_agent_task(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: &Path,
    prompt: &str,
    timeout: Duration,
) -> Result<(), CollectorError> {
    let binary = binary.as_ref();
    let program = binary.display().to_string();
    let mut command = Command::new(binary);
    match agent {
        crate::domain::AgentType::ClaudeCode => {
            command.args(["-p", "--output-format", "text"]);
        }
        crate::domain::AgentType::Codex => {
            command.args(["exec", "-"]);
        }
    }
    command
        .current_dir(project_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
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
    let status = wait_for_child(&mut child, &program, timeout)?;
    drop(job);
    if !status.success() {
        return Err(CollectorError::Exit {
            program,
            status: status.to_string(),
        });
    }
    Ok(())
}

/// Resume an existing local Agent session and return only the child process'
/// bounded stdout. The prompt is always written through stdin so it never
/// appears in the process list. Callers must keep the returned content local;
/// unlike normal collection, this function intentionally handles conversation
/// content after an explicit user action.
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
        timeout,
        &AtomicBool::new(false),
        |_| {},
    )
}

/// Resume an Agent session while forwarding safe, user-visible progress.
/// Conversation text is never logged or persisted by this function. The
/// caller owns the callback destination and cancellation flag.
#[allow(clippy::too_many_arguments)]
pub fn run_agent_session_turn_streaming(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: Option<&Path>,
    session_id: &str,
    prompt: &str,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> Result<String, CollectorError> {
    let binary = binary.as_ref();
    let program = binary.display().to_string();
    let mut command = Command::new(binary);
    match agent {
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
        }
    }
    if let Some(project_path) = project_path {
        command.current_dir(project_path);
    }
    command
        .stdin(Stdio::piped())
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
    final_agent_message(agent, &output).ok_or_else(|| CollectorError::Protocol {
        program,
        code: "missing_final_message".to_owned(),
        message: "missing final agent message".to_owned(),
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

fn wait_for_child(
    child: &mut Child,
    program: &str,
    timeout: Duration,
) -> Result<std::process::ExitStatus, CollectorError> {
    wait_for_child_with_cancel(child, program, timeout, None)
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
        let binary = binary.as_ref();
        let program = binary.display().to_string();
        let mut command = Command::new(binary);
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
        server.initialize()?;
        Ok(server)
    }

    fn initialize(&mut self) -> Result<(), CollectorError> {
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
                    "optOutNotificationMethods": [
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
                    ]
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
/// signalled at teardown; the Windows equivalent is the Job Object.
fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        command.process_group(0);
    }
    #[cfg(not(unix))]
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

    fn recording_script(
        directory: &tempfile::TempDir,
        arguments_path: &Path,
        input_path: &Path,
    ) -> PathBuf {
        #[cfg(windows)]
        {
            let path = directory.path().join("agent.cmd");
            fs::write(
                &path,
                format!(
                    "@echo off\r\necho %* > \"{}\"\r\nmore > \"{}\"\r\n",
                    arguments_path.display(),
                    input_path.display(),
                ),
            )
            .unwrap();
            path
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let path = directory.path().join("agent");
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s' \"$*\" > '{}'\ncat > '{}'\n",
                    arguments_path.display(),
                    input_path.display(),
                ),
            )
            .unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }
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
            Duration::from_secs(10),
            &cancelled,
            |_| {},
        )
        .unwrap_err();

        assert!(matches!(error, CollectorError::Cancelled { .. }));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn task_prompt_uses_stdin_instead_of_process_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let arguments_path = directory.path().join("arguments.txt");
        let input_path = directory.path().join("input.txt");
        let script = recording_script(&directory, &arguments_path, &input_path);
        let prompt = "private prompt marker";

        run_agent_task(
            crate::domain::AgentType::Codex,
            script,
            directory.path(),
            prompt,
            Duration::from_secs(2),
        )
        .unwrap();

        let arguments = fs::read_to_string(arguments_path).unwrap();
        let input = fs::read_to_string(input_path).unwrap();
        assert!(arguments.contains("exec"));
        assert!(!arguments.contains(prompt));
        assert_eq!(input.trim_end(), prompt);
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
