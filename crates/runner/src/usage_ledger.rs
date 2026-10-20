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
//! Codex and ZCode keep per-session totals rather than per-call rows, so they
//! are folded in at day granularity by [`agent_totals`] and never appear as
//! individual requests.

use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant, UNIX_EPOCH},
};

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::collectors::{CollectorError, home_dir};

/// How many transcript files one scan walks, newest first. The store holds
/// hundreds of files; the bound keeps a first paint from reading all of it.
const LEDGER_FILE_LIMIT: usize = 4_000;
/// One JSONL line beyond this is not a transcript record worth keeping.
const LEDGER_LINE_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Today,
    Days(u32),
    All,
}

impl Period {
    /// The period a query string names; anything else reads as "all".
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("") {
            "today" => Self::Today,
            "7d" => Self::Days(7),
            "30d" => Self::Days(30),
            _ => Self::All,
        }
    }

    /// The first moment of the period, in milliseconds since the epoch. The
    /// day boundaries are local: "today" means the reader's today.
    pub fn since_ms(self, now_ms: i64) -> i64 {
        match self {
            Self::All => i64::MIN,
            Self::Days(days) => {
                let day = 86_400_000i64;
                let local_offset = local_offset_ms(now_ms);
                let local_now = now_ms + local_offset;
                let start_local = (local_now / day - i64::from(days) + 1) * day;
                start_local - local_offset
            }
            Self::Today => {
                let day = 86_400_000i64;
                let local_offset = local_offset_ms(now_ms);
                let local_now = now_ms + local_offset;
                (local_now / day) * day - local_offset
            }
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Days(7) => "7d",
            Self::Days(30) => "30d",
            Self::Days(_) => "all",
            Self::All => "all",
        }
    }
}

/// The machine's UTC offset, read from the clock at `now_ms` rather than from
/// a fixed table: a laptop that travels changes its answer between boots.
fn local_offset_ms(now_ms: i64) -> i64 {
    use chrono::Local;
    let Some(utc) = DateTime::from_timestamp_millis(now_ms) else {
        return 0;
    };
    let local = utc.with_timezone(&Local);
    i64::from(local.offset().local_minus_utc()) * 1000
}

/// One model call as an agent's own file records it.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerRecord {
    /// The call's last write, in milliseconds since the epoch.
    pub t: i64,
    /// The moment the reply was asked for: the last prompt or tool result
    /// before its first line, or the end of the previous reply. It is what
    /// [`LedgerRecord::floor_ms`] measures a whole turn from, so it never
    /// enters the wire — the page reads the floor the daemon computed.
    #[serde(skip)]
    pub asked: i64,
    /// `claude_code`, `claude_desktop`, `codex`, `grok_build`, `zcode`,
    /// `gemini_cli`.
    pub agent: &'static str,
    /// The model as the provider answered, e.g. `claude-sonnet-5`.
    pub model: String,
    /// The session the call belongs to.
    pub session: String,
    /// The workspace the call ran in, when the transcript names one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cwd: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// The API error a failed call was written with (`rate_limit`,
    /// `authentication_failed`, …). Such a call has no tokens and no cost; it
    /// exists so the request list can say a request failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Pre-computed by the daemon: the page never prices a call itself.
    pub cost: f64,
    /// A successful call that answered a failed one in the same session
    /// shortly before it: the retry, not a failure, is what the list says.
    #[serde(skip_serializing_if = "is_false")]
    pub retried: bool,
    /// A failed call a later try answered. It keeps its error kind, but the
    /// digest counts it as answered rather than failed.
    #[serde(skip_serializing_if = "is_false")]
    pub recovered: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl LedgerRecord {
    pub fn total_tokens(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    /// What the model was handed: every input token, cached or not.
    pub fn prompt_tokens(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    /// The whole turn a reply read from a transcript file can be timed over:
    /// from the moment it was asked for to its last line, the wait for the
    /// first token included.
    ///
    /// A transcript file has no first-token time. Its message lines each carry
    /// the finished usage, and a message's first line is its first *content
    /// block*, not its first token — the blocks land together, so the span
    /// from the first line to the last says nothing about how long the model
    /// wrote. The only honest timing is the whole turn: magpie's `Row.Floor`.
    /// It is the least the reply wrote at, not its speed, and is told only for
    /// a reply of 1,000 tokens or more over a second or more (a shorter one is
    /// mostly wait, and would read far under its real speed) and no faster
    /// than 10,000 tok/s.
    pub fn floor_ms(&self) -> Option<u64> {
        if self.asked <= 0 || self.asked >= self.t {
            return None;
        }
        let ms: u64 = self.t.saturating_sub(self.asked).try_into().ok()?;
        if self.output < FLOOR_MIN_OUTPUT || ms < FLOOR_MIN_MS {
            return None;
        }
        // output * 1000 <= 10_000 * ms, without overflowing on a huge reply
        if self.output.saturating_mul(1000) > MAX_DECODE_SPEED.saturating_mul(ms) {
            return None;
        }
        Some(ms)
    }
}

/// The fastest a real stream writes: 10,000 tokens a second (magpie's
/// `usage.MaxDecodeSpeed`). A whole turn that reads faster than this timed
/// something other than the writing.
const MAX_DECODE_SPEED: u64 = 10_000;
/// The least a whole-turn floor is told for: a shorter reply is mostly the
/// wait for its first token (magpie's `Row.Floor`).
const FLOOR_MIN_OUTPUT: u64 = 1_000;
const FLOOR_MIN_MS: u64 = 1_000;

/// The per-model rate card, one place for both the summary and the ledger.
/// Values are US dollars per million tokens.
pub fn rates(model_name: &str) -> (f64, f64, f64) {
    let m = model_name.to_ascii_lowercase();
    if m.contains("opus") {
        (15.0, 75.0, 1.5)
    } else if m.contains("sonnet") || m.contains("fable") || m.contains("mythos") {
        (3.0, 15.0, 0.3)
    } else if m.contains("haiku") || m.contains("flash") || m.contains("mini") {
        (0.25, 1.25, 0.025)
    } else if m.contains("deepseek") {
        (0.14, 0.28, 0.014)
    } else if m.contains("codex") || m.contains("gpt-5") || m.contains("gpt-4") {
        (2.5, 10.0, 1.25)
    } else {
        (3.0, 15.0, 0.3)
    }
}

/// What a call cost at the rate card. Cache reads bill at the cache rate;
/// regular input is everything that was not read from the cache.
pub fn calc_cost(model_name: &str, prompt: u64, completion: u64, cache_read: u64) -> f64 {
    let (input_rate, output_rate, cache_read_rate) = rates(model_name);
    let regular_prompt = prompt.saturating_sub(cache_read);
    (regular_prompt as f64 * input_rate
        + completion as f64 * output_rate
        + cache_read as f64 * cache_read_rate)
        / 1_000_000.0
}

/// A transcript file as the ledger last read it: its size and modification
/// time decide whether reading it again is worth anything.
#[derive(Debug, Clone)]
struct FileMark {
    len: u64,
    modified_ms: i64,
}

/// One file's calls, kept across walks: a store this size should not be
/// re-read because one unrelated transcript grew.
#[derive(Debug, Default)]
struct LedgerCache {
    /// When the store was last walked.
    walked_at: Option<Instant>,
    /// Each file's fingerprint and the calls it held at that walk.
    files: HashMap<PathBuf, (FileMark, Vec<LedgerRecord>)>,
    /// The merged, sorted answer the last walk produced.
    records: Vec<LedgerRecord>,
}

fn ledger_cache() -> &'static RwLock<LedgerCache> {
    static CACHE: OnceLock<RwLock<LedgerCache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(LedgerCache::default()))
}

/// How long a scan is reused. The page asks on every poll; transcripts only
/// change when an agent writes one, and a few seconds of staleness costs
/// nothing when the fastest poll is two seconds.
const LEDGER_TTL: Duration = Duration::from_secs(5);

/// Drops the memoised walk. A session moved to the trash is still on disk at
/// its old path in the cache until the next park, and the fingerprint of a
/// path that no longer exists never matches, so the calls of a deleted
/// conversation would otherwise stay in the ledger for its TTL.
pub fn forget_transcripts() {
    if let Ok(mut guard) = ledger_cache().write() {
        *guard = LedgerCache::default();
    }
}

/// Every call in Coding Assistant's transcript store, newest first.
///
/// A file whose size and modification time have not moved since the last walk
/// keeps the calls the last walk read from it; only a changed file is read
/// again. The whole scan is memoised for [`LEDGER_TTL`], so a page that polls
/// every two seconds does not rewalk a 2 GB store on every tick.
pub fn claude_records() -> Vec<LedgerRecord> {
    if let Ok(guard) = ledger_cache().read() {
        if let Some(at) = guard.walked_at {
            if at.elapsed() < LEDGER_TTL {
                return guard.records.clone();
            }
        }
    }

    let Some(config_dir) = claude_config_dir() else {
        return Vec::new();
    };
    let Some(files) = transcript_files(&config_dir) else {
        return Vec::new();
    };

    let previous = ledger_cache()
        .read()
        .map(|g| g.files.clone())
        .unwrap_or_default();
    let mut walked = HashMap::with_capacity(files.len());
    let mut records = Vec::new();
    for path in files {
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        let mark = FileMark {
            len: meta.len(),
            modified_ms: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        };
        let cached = previous.get(&path).and_then(|(old, calls)| {
            (old.len == mark.len && old.modified_ms == mark.modified_ms).then(|| calls.clone())
        });
        let calls = cached.unwrap_or_else(|| read_transcript(&path).unwrap_or_default());
        records.extend(calls.iter().cloned());
        walked.insert(path, (mark, calls));
    }

    // One message repeated across resumed sessions belongs to the session that
    // wrote it first; later copies would double every figure.
    records.sort_by_key(|r| std::cmp::Reverse(r.t));
    dedupe(&mut records);
    mark_retries(&mut records);
    let merged = records.clone();
    if let Ok(mut guard) = ledger_cache().write() {
        *guard = LedgerCache {
            walked_at: Some(Instant::now()),
            files: walked,
            records: merged,
        };
    }
    records
}

fn dedupe(records: &mut Vec<LedgerRecord>) {
    // Records carry `t` to the millisecond; a resumed transcript can replay a
    // message with a later timestamp, so identity is the message itself.
    // `read_transcript` already keys a file's messages; across files, the
    // same call is the same session, model and four token counts at the same
    // moment, which is the closest thing to a portable identity.
    let mut seen: std::collections::HashSet<(i64, &'static str, u64, u64, u64, u64)> =
        std::collections::HashSet::new();
    records.retain(|r| seen.insert((r.t, r.agent, r.input, r.output, r.cache_read, r.cache_write)));
}

/// How soon after a failure the next successful call counts as its retry.
/// The CLI retries a rate limit or a server error within seconds to a few
/// minutes; a call a day later is a new question, not a second try.
const RETRY_WINDOW_MS: i64 = 10 * 60 * 1000;

/// Marks the pairs a failed request and the call that answered it form: the
/// failure reads as answered on a retry, and the success reads as a retry.
/// Both keep their own record; the page's status filter tells them apart.
fn mark_retries(records: &mut [LedgerRecord]) {
    // oldest first, so the first success after a failure is the one that
    // answers it
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&i| records[i].t);
    let mut last_failure: HashMap<String, usize> = HashMap::new();
    for i in order {
        let session = records[i].session.clone();
        if records[i].error.is_some() {
            // a later failure replaces an earlier one: only the latest failure
            // before a success is the one that success answers
            last_failure.insert(session, i);
            continue;
        }
        if let Some(failed_at) = last_failure.remove(&session) {
            let gap = records[i].t.saturating_sub(records[failed_at].t);
            if (0..=RETRY_WINDOW_MS).contains(&gap) {
                records[failed_at].recovered = true;
                records[i].retried = true;
            }
        }
    }
}

fn claude_config_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    home_dir().map(|home| home.join(".claude"))
}

/// Every `*.jsonl` under `projects/`, newest first, bounded.
///
/// A session's own file sits in its project's folder; its subagents' files
/// sit beside it under `<session>/subagents/agent-*.jsonl`, and a workflow's
/// agents under `<session>/subagents/workflows/<run>/agent-*.jsonl` (the
/// run's `journal.jsonl` is not a transcript and is left out). Those calls
/// are real model calls with usage of their own, and the session they belong
/// to is the conversation that spawned them.
fn transcript_files(config_dir: &Path) -> Option<Vec<PathBuf>> {
    let projects = config_dir.join("projects");
    if !projects.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    let dirs = fs::read_dir(&projects).ok()?;
    for project in dirs.flatten() {
        if !project.file_type().ok().is_some_and(|k| k.is_dir()) {
            continue;
        }
        let root = project.path();
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for file in entries.flatten() {
            let path = file.path();
            if file.file_type().is_ok_and(|k| k.is_dir()) {
                collect_subagent_files(&path, &mut files);
                continue;
            }
            if path.extension().and_then(|v| v.to_str()) != Some("jsonl") {
                continue;
            }
            push_transcript(&path, &mut files);
        }
    }
    files.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
    files.truncate(LEDGER_FILE_LIMIT);
    Some(files.into_iter().map(|(_, path)| path).collect())
}

/// The subagent transcripts under one session's folder, if it has any.
fn collect_subagent_files(session_dir: &Path, files: &mut Vec<(i64, PathBuf)>) {
    let subagents = session_dir.join("subagents");
    if !subagents.is_dir() {
        return;
    }
    if let Ok(entries) = fs::read_dir(&subagents) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) == Some("jsonl") {
                push_transcript(&path, files);
            }
        }
    }
    let workflows = subagents.join("workflows");
    let Ok(runs) = fs::read_dir(&workflows) else {
        return;
    };
    for run in runs.flatten() {
        if !run.file_type().is_ok_and(|k| k.is_dir()) {
            continue;
        }
        let Ok(agents) = fs::read_dir(run.path()) else {
            continue;
        };
        for agent in agents.flatten() {
            let path = agent.path();
            let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if name.starts_with("agent-")
                && path.extension().and_then(|v| v.to_str()) == Some("jsonl")
            {
                push_transcript(&path, files);
            }
        }
    }
}

fn push_transcript(path: &Path, files: &mut Vec<(i64, PathBuf)>) {
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

/// A transcript line's assistant record, reduced to what the ledger keeps.
#[derive(Debug, Deserialize)]
struct AssistantLine {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default, rename = "requestId")]
    request_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    entrypoint: Option<String>,
    #[serde(default, rename = "isApiErrorMessage")]
    api_error: bool,
    /// The error kind of a failed call (`rate_limit`, `server_error`, …).
    #[serde(default)]
    error: Option<String>,
    /// True on a subagent's lines: its own transcript, not the conversation's.
    #[serde(default, rename = "isSidechain")]
    sidechain: bool,
    /// The subagent a line belongs to, when it is one.
    #[serde(default, rename = "agentId")]
    agent_id: Option<String>,
    #[serde(default)]
    message: Option<AssistantMessage>,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<AssistantUsage>,
}

#[derive(Debug, Default, Deserialize)]
struct AssistantUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

/// Reads one transcript into per-call records.
fn read_transcript(path: &Path) -> Result<Vec<LedgerRecord>, CollectorError> {
    let program = "Claude transcript store";
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();

    // A message's lines repeat its usage; the latest line carries the final
    // figures. Identity is `message.id`, falling back to `requestId`.
    let mut calls: Vec<LedgerRecord> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let fallback_session = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned();
    // What a reply is asked by, and when the reply before it ended: the two
    // moments a whole turn can be timed from (magpie's `state.LastUser` and
    // `state.LastAsst`). The transcript records no first-token time, so this
    // is the only way to time a reply that was written in one line.
    let mut last_user_ms: i64 = 0;
    let mut last_assistant_ms: i64 = 0;

    loop {
        line.clear();
        match read_line_bounded(&mut reader, &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if line.len() > LEDGER_LINE_LIMIT {
            continue;
        }
        // The cheap prefilter keeps the JSON parser away from the many
        // lifecycle lines a transcript also holds. A prompt is read for its
        // moment alone, and a tool result can be megabytes of base64: the
        // moment is taken off the line's bytes, never by parsing the line.
        if !contains(&line, b"\"type\":\"assistant\"") {
            if contains(&line, b"\"type\":\"user\"")
                && let Some(ms) = line_timestamp_ms(&line)
            {
                last_user_ms = last_user_ms.max(ms);
            }
            continue;
        }
        let Ok(parsed) = serde_json::from_slice::<AssistantLine>(&line) else {
            continue;
        };
        if parsed.kind != "assistant" {
            continue;
        }
        let Some(message) = parsed.message.as_ref() else {
            continue;
        };
        let Some(usage) = message.usage.as_ref() else {
            continue;
        };
        let Some(time) = parsed
            .timestamp
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
        else {
            continue;
        };
        let model = message.model.clone().unwrap_or_default();
        // `<synthetic>` lines are the CLI's own bookkeeping, never a call —
        // except a failed one, which the CLI records the same way and which
        // the request list still wants to count as failed.
        let failed = parsed.api_error;
        if model.is_empty() || (model == "<synthetic>" && !failed) {
            continue;
        }
        let agent = if parsed
            .entrypoint
            .as_deref()
            .is_some_and(|e| e.starts_with("claude-desktop"))
        {
            "claude_desktop"
        } else {
            "claude_code"
        };
        let session = parsed
            .session_id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fallback_session.clone());
        let key = match (message.id.as_deref(), parsed.request_id.as_deref()) {
            (Some(id), Some(req)) if !id.is_empty() => format!("{id}\u{1f}{req}"),
            (Some(id), None) if !id.is_empty() => id.to_owned(),
            (_, Some(req)) if !req.is_empty() => req.to_owned(),
            _ => format!("{time}\u{1f}{model}"),
        };
        // A failed call says which kind of error it was and bills nothing.
        let error = if failed {
            Some(
                parsed
                    .error
                    .clone()
                    .filter(|e| !e.is_empty())
                    .unwrap_or_else(|| "unknown".to_owned()),
            )
        } else {
            None
        };
        // The moment the reply was asked for: the last prompt or tool result
        // before it, or, with none since, the end of the reply before it.
        let asked = last_user_ms.max(last_assistant_ms).min(time);
        let record = LedgerRecord {
            t: time,
            asked,
            agent,
            model: if failed { String::new() } else { model.clone() },
            session,
            cwd: parsed.cwd.clone().unwrap_or_default(),
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_read: usage.cache_read_input_tokens,
            cache_write: usage.cache_creation_input_tokens,
            error,
            cost: if failed {
                0.0
            } else {
                calc_cost(
                    &model,
                    usage
                        .input_tokens
                        .saturating_add(usage.cache_read_input_tokens)
                        .saturating_add(usage.cache_creation_input_tokens),
                    usage.output_tokens,
                    usage.cache_read_input_tokens,
                )
            },
            retried: false,
            recovered: false,
        };
        match index.get(&key) {
            Some(&at) => {
                // the message's latest line carries the final usage; the ask
                // it answers was fixed when the message began
                let asked = calls[at].asked;
                calls[at] = record;
                calls[at].asked = asked;
            }
            None => {
                index.insert(key, calls.len());
                calls.push(record);
            }
        }
        last_assistant_ms = last_assistant_ms.max(time);
    }
    Ok(calls)
}

/// A prompt or tool result's moment, taken off its bytes.
///
/// A user line is the prompt or a tool result, and a tool result carries the
/// whole output of the command — screenshots as base64 run to megabytes. The
/// whole-turn floor needs nothing from it but when it happened, so the line is
/// scanned for `"timestamp":"…"` and that one field is parsed; the rest of the
/// line is never handed to the JSON parser.
fn line_timestamp_ms(line: &[u8]) -> Option<i64> {
    const MARK: &[u8] = b"\"timestamp\":\"";
    let at = find_bytes(line, MARK)? + MARK.len();
    let rest = &line[at..];
    let end = rest.iter().position(|b| *b == b'"')?;
    let stamp = std::str::from_utf8(&rest[..end]).ok()?;
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|d| d.timestamp_millis())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Does the line hold this byte sequence? A small memchr so a scan of the
/// store does not build a String for every line.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find_bytes(haystack, needle).is_some()
}
fn read_line_bounded<R: BufRead>(reader: &mut R, buf: &mut Vec<u8>) -> std::io::Result<usize> {
    let mut total = 0;
    loop {
        let available = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok(total);
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(at) => {
                buf.extend_from_slice(&available[..at]);
                reader.consume(at + 1);
                return Ok(total + at + 1);
            }
            None => {
                buf.extend_from_slice(available);
                let n = available.len();
                reader.consume(n);
                total += n;
            }
        }
    }
}

/// What the page's overview needs from the calls of one period: the totals,
/// the series the trend is drawn from, the ranking dimensions, the heatmap's
/// days, the sessions' rows and the request list.
#[derive(Debug, Default, Serialize)]
pub struct LedgerDigest {
    pub period: &'static str,
    /// The first and last days the period covers, as `YYYY-MM-DD` in local
    /// time.
    pub from: String,
    pub to: String,
    /// The heatmap's window: 53 weeks ending today, whatever the period is —
    /// the calendar is about the year, the totals are about the period.
    pub heat_from: String,
    pub heat_to: String,
    pub calls: u64,
    pub sessions: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens: u64,
    pub cost: f64,
    /// Calls that failed and nothing answered.
    pub errors: u64,
    /// Failed calls a later try answered.
    pub recovered: u64,
    /// Calls that answered an earlier failure.
    pub retried: u64,
    /// The period's measured speed and whole-turn floor, summed over every
    /// reply the same way the rankings are (see [`Timing`]).
    #[serde(flatten)]
    pub timing: Timing,
    /// Calls grouped into the period's buckets: hours for a day, days for a
    /// week, weeks for longer spans.
    pub series: Vec<SeriesPoint>,
    /// One entry per ranking dimension: model, model at its agent, agent.
    pub by: Breakdown,
    /// One row per local day with activity, for the heatmap.
    pub heat: Vec<DayPoint>,
    /// One row per session, newest first.
    pub session_rows: Vec<SessionRow>,
    /// The most recent calls, newest first, for the request table.
    pub recent: Vec<LedgerRecord>,
}

#[derive(Debug, Default, Serialize)]
pub struct SeriesPoint {
    /// The bucket's start, in milliseconds since the epoch.
    pub t: i64,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
    /// The bucket's own floor, over every reply in it (see [`Timing`]). A
    /// speed is not a share of anything, so the chart draws it as a track
    /// with each model's mark on it.
    #[serde(flatten)]
    pub timing: Timing,
    /// The bucket's calls by each ranking dimension, so the chart can stack
    /// them in the ranking's colours. Only the ids that were active in the
    /// bucket are here; the chart fills the rest with zero.
    pub by: SeriesBy,
}

#[derive(Debug, Default, Serialize)]
pub struct SeriesBy {
    pub model: HashMap<String, Part>,
    pub model_at: HashMap<String, Part>,
    pub agent: HashMap<String, Part>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Part {
    pub calls: u64,
    pub tokens: u64,
    pub output: u64,
    pub cost: f64,
    /// The floor of the replies of this part: their output over the whole
    /// turn each was asked in (see [`Timing`]).
    #[serde(flatten)]
    pub timing: Timing,
}

/// The speed a group of replies reads at, as magpie tells it for a file
/// source: one figure, a floor.
///
/// A transcript file never says when an answer began — its first line is the
/// first content block, not the first token — so a reply's speed after its
/// first token cannot be measured from it. What the file does say is when the
/// reply was asked for and when it ended, and the tokens over that whole turn
/// are the least the reply wrote at. A speed is summed, not averaged: the
/// group's output over the group's time. An average of per-reply rates lets
/// one short, fast reply stand for a long, slow one.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Timing {
    pub floor_out: u64,
    pub floor_ms: u64,
    pub floor_n: u64,
}

impl Timing {
    fn observe(&mut self, record: &LedgerRecord) {
        if let Some(ms) = record.floor_ms() {
            self.floor_out = self.floor_out.saturating_add(record.output);
            self.floor_ms = self.floor_ms.saturating_add(ms);
            self.floor_n = self.floor_n.saturating_add(1);
        }
    }

    /// The least the replies wrote at, in tokens a second; 0 for none timed.
    pub fn floor(&self) -> f64 {
        if self.floor_ms == 0 {
            0.0
        } else {
            self.floor_out as f64 / (self.floor_ms as f64 / 1000.0)
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Breakdown {
    pub model: Vec<RankRow>,
    pub model_at: Vec<RankRow>,
    pub agent: Vec<RankRow>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RankRow {
    pub id: String,
    pub name: String,
    /// The agent of a `model_at` row (`claude_code` and so on).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent: String,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
    /// The row's speed and floor (see [`Timing`]).
    #[serde(flatten)]
    pub timing: Timing,
}

#[derive(Debug, Default, Serialize)]
pub struct DayPoint {
    /// `YYYY-MM-DD`, local.
    pub date: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
}

/// One session of the period, as the 会话 and 上下文 tabs read it.
#[derive(Debug, Default, Serialize)]
pub struct SessionRow {
    pub id: String,
    /// The agent the session's calls belong to; a session that changed agent
    /// (a transcript resumed in another entrypoint) says so by its latest.
    pub agent: &'static str,
    pub agent_name: &'static str,
    pub cwd: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
    /// The first and last call of the session within the period.
    pub first: i64,
    pub last: i64,
    /// The models the session used, most tokens first.
    pub models: Vec<String>,
    /// The model of the session's latest call.
    pub model: String,
    /// The prompt size of the session's latest successful call: what the
    /// model was handed at the end, i.e. how full the context had grown.
    pub context: u64,
}

/// Aggregates the records of a period.
///
/// `recent_limit` bounds the request table's rows; the series, rankings and
/// heatmap are always computed from every call in the period.
pub fn digest(records: &[LedgerRecord], period: Period, recent_limit: usize) -> LedgerDigest {
    let now = chrono::Local::now().timestamp_millis();
    let since = period.since_ms(now);
    let selected: Vec<&LedgerRecord> = records.iter().filter(|r| r.t >= since).collect();

    let mut digest = LedgerDigest {
        period: period.key(),
        ..LedgerDigest::default()
    };
    let (from, to) = local_day_bounds(period, now, &selected);
    digest.from = from;
    digest.to = to;
    digest.heat_from = local_day(heat_window_start(now));
    digest.heat_to = local_day(now);

    let bucket = bucket_of(period);
    let mut points: std::collections::BTreeMap<i64, SeriesPoint> = Default::default();
    let mut heat: std::collections::BTreeMap<String, DayPoint> = Default::default();
    let mut models: HashMap<String, RankRow> = HashMap::new();
    let mut model_at: HashMap<(String, &'static str), RankRow> = HashMap::new();
    let mut agents: HashMap<&'static str, RankRow> = HashMap::new();
    let mut sessions: HashMap<&str, SessionAgg> = HashMap::new();

    let mut recent: Vec<&LedgerRecord> = Vec::new();
    for record in &selected {
        let failed = record.error.is_some() && !record.recovered;
        digest.calls = digest.calls.saturating_add(1);
        digest.input = digest.input.saturating_add(record.input);
        digest.output = digest.output.saturating_add(record.output);
        digest.cache_read = digest.cache_read.saturating_add(record.cache_read);
        digest.cache_write = digest.cache_write.saturating_add(record.cache_write);
        digest.cost += record.cost;
        if failed {
            digest.errors = digest.errors.saturating_add(1);
        }
        if record.recovered {
            digest.recovered = digest.recovered.saturating_add(1);
        }
        if record.retried {
            digest.retried = digest.retried.saturating_add(1);
        }
        digest.timing.observe(record);

        let at = bucket_start(record.t, bucket);
        let point = points.entry(at).or_insert_with(|| SeriesPoint {
            t: at,
            ..SeriesPoint::default()
        });
        accumulate(point, record);

        let day = local_day(record.t);
        let day_point = heat.entry(day.clone()).or_insert_with(|| DayPoint {
            date: day,
            ..DayPoint::default()
        });
        day_point.calls = day_point.calls.saturating_add(1);
        day_point.tokens = day_point.tokens.saturating_add(record.total_tokens());
        day_point.cost += record.cost;
        if failed {
            day_point.errors = day_point.errors.saturating_add(1);
        }

        sessions
            .entry(record.session.as_str())
            .or_insert_with(|| SessionAgg {
                id: record.session.clone(),
                first: record.t,
                ..SessionAgg::default()
            })
            .observe(record, failed);

        // A failed call names no model; the rankings are about what answered.
        if record.error.is_none() {
            rank(
                models
                    .entry(record.model.clone())
                    .or_insert_with(|| RankRow {
                        id: record.model.clone(),
                        name: record.model.clone(),
                        ..RankRow::default()
                    }),
                record,
            );
            rank(
                model_at
                    .entry((record.model.clone(), record.agent))
                    .or_insert_with(|| RankRow {
                        id: format!("{}@{}", record.model, record.agent),
                        name: record.model.clone(),
                        agent: record.agent.to_owned(),
                        ..RankRow::default()
                    }),
                record,
            );
            rank(
                agents.entry(record.agent).or_insert_with(|| RankRow {
                    id: record.agent.to_owned(),
                    name: agent_label(record.agent).to_owned(),
                    agent: record.agent.to_owned(),
                    ..RankRow::default()
                }),
                record,
            );
        }
        recent.push(record);
    }

    digest.sessions = sessions.len() as u64;
    digest.tokens = digest
        .input
        .saturating_add(digest.output)
        .saturating_add(digest.cache_read)
        .saturating_add(digest.cache_write);
    digest.cost = round_money(digest.cost);
    // the trend reads left to right: a HashMap's order would scatter the
    // buckets across the chart
    digest.series = points.into_values().collect();
    digest.series.sort_by_key(|p| p.t);
    digest.heat = heat
        .into_values()
        .filter(|d| d.date.as_str() >= digest.heat_from.as_str())
        .collect();

    // every ranking sorts by tokens, then by name so a tie never reshuffles
    let sorted = |mut rows: Vec<RankRow>| {
        rows.sort_by(|a, b| {
            b.tokens
                .cmp(&a.tokens)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.agent.cmp(&b.agent))
        });
        rows
    };
    digest.by.model = sorted(models.into_values().collect());
    digest.by.model_at = sorted(model_at.into_values().collect());
    digest.by.agent = sorted(agents.into_values().collect());

    let mut session_rows: Vec<SessionRow> = sessions.into_values().map(SessionRow::from).collect();
    session_rows.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.id.cmp(&b.id)));
    session_rows.truncate(SESSION_ROW_LIMIT);
    digest.session_rows = session_rows;

    recent.truncate(recent_limit);
    digest.recent = recent.into_iter().cloned().collect();
    digest
}

/// The most session rows the page is given; the list is a reading list, not
/// the store's index.
const SESSION_ROW_LIMIT: usize = 300;

#[derive(Debug, Default)]
struct SessionAgg {
    id: String,
    agent: &'static str,
    cwd: String,
    calls: u64,
    tokens: u64,
    cost: f64,
    errors: u64,
    first: i64,
    last: i64,
    latest_t: i64,
    model: String,
    context: u64,
    models: HashMap<String, u64>,
}

impl SessionAgg {
    fn observe(&mut self, record: &LedgerRecord, failed: bool) {
        if self.agent.is_empty() {
            self.agent = record.agent;
        }
        if self.cwd.is_empty() && !record.cwd.is_empty() {
            self.cwd = record.cwd.clone();
        }
        self.calls = self.calls.saturating_add(1);
        self.tokens = self.tokens.saturating_add(record.total_tokens());
        self.cost += record.cost;
        if failed {
            self.errors = self.errors.saturating_add(1);
        }
        self.first = self.first.min(record.t);
        self.last = self.last.max(record.t);
        // the latest successful call is the session's last state: its model
        // and the prompt it was handed
        if record.error.is_none() && record.t >= self.latest_t {
            self.latest_t = record.t;
            self.agent = record.agent;
            self.model = record.model.clone();
            self.context = record.prompt_tokens();
        }
        if record.error.is_none() && !record.model.is_empty() {
            *self.models.entry(record.model.clone()).or_insert(0) += record.total_tokens();
        }
    }
}

impl From<SessionAgg> for SessionRow {
    fn from(mut agg: SessionAgg) -> Self {
        let mut models: Vec<(String, u64)> = agg.models.drain().collect();
        models.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        SessionRow {
            id: agg.id,
            agent: agg.agent,
            agent_name: agent_label(agg.agent),
            cwd: agg.cwd,
            calls: agg.calls,
            tokens: agg.tokens,
            cost: round_money(agg.cost),
            errors: agg.errors,
            first: agg.first,
            last: agg.last,
            models: models.into_iter().take(3).map(|(m, _)| m).collect(),
            model: agg.model,
            context: agg.context,
        }
    }
}

/// The heatmap's first day: 53 weeks back, Monday-first, so the grid is whole.
fn heat_window_start(now: i64) -> i64 {
    let day = 86_400_000i64;
    let offset = local_offset_ms(now);
    let local = now + offset;
    let day_index = local.div_euclid(day);
    let monday_index = day_index - (day_index + 3).rem_euclid(7);
    (monday_index - 52 * 7) * day - offset
}

fn accumulate(point: &mut SeriesPoint, record: &LedgerRecord) {
    point.calls = point.calls.saturating_add(1);
    point.input = point.input.saturating_add(record.input);
    point.output = point.output.saturating_add(record.output);
    point.cache_read = point.cache_read.saturating_add(record.cache_read);
    point.cache_write = point.cache_write.saturating_add(record.cache_write);
    point.tokens = point.tokens.saturating_add(record.total_tokens());
    point.cost += record.cost;
    if record.error.is_some() && !record.recovered {
        point.errors = point.errors.saturating_add(1);
    }
    point.timing.observe(record);
    if record.error.is_none() {
        part(
            point.by.model.entry(record.model.clone()).or_default(),
            record,
        );
        part(
            point
                .by
                .model_at
                .entry(format!("{}@{}", record.model, record.agent))
                .or_default(),
            record,
        );
        part(
            point.by.agent.entry(record.agent.to_owned()).or_default(),
            record,
        );
    }
}

fn part(p: &mut Part, record: &LedgerRecord) {
    p.calls = p.calls.saturating_add(1);
    p.tokens = p.tokens.saturating_add(record.total_tokens());
    p.output = p.output.saturating_add(record.output);
    p.cost += record.cost;
    p.timing.observe(record);
}

fn rank(row: &mut RankRow, record: &LedgerRecord) {
    row.calls = row.calls.saturating_add(1);
    row.input = row.input.saturating_add(record.input);
    row.output = row.output.saturating_add(record.output);
    row.cache_read = row.cache_read.saturating_add(record.cache_read);
    row.cache_write = row.cache_write.saturating_add(record.cache_write);
    row.tokens = row.tokens.saturating_add(record.total_tokens());
    row.cost += record.cost;
    if record.error.is_some() && !record.recovered {
        row.errors = row.errors.saturating_add(1);
    }
    row.timing.observe(record);
}

fn round_money(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The agent ids the console shows, with the names it shows them under.
pub fn agent_label(agent: &str) -> &'static str {
    match agent {
        "claude_code" => "Coding Assistant",
        "claude_desktop" => "Claude Desktop",
        "codex" => "Codex",
        "grok_build" => "Grok",
        "gemini_cli" => "Gemini CLI",
        "zcode" => "ZCode",
        _ => "Other",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Hour,
    Day,
    Week,
}

fn bucket_of(period: Period) -> Bucket {
    match period {
        Period::Today => Bucket::Hour,
        Period::Days(days) if days <= 31 => Bucket::Day,
        _ => Bucket::Week,
    }
}

fn bucket_start(t: i64, bucket: Bucket) -> i64 {
    let day = 86_400_000i64;
    match bucket {
        Bucket::Hour => {
            let offset = local_offset_ms(t);
            let local = t + offset;
            (local / 3_600_000) * 3_600_000 - offset
        }
        Bucket::Day => {
            let offset = local_offset_ms(t);
            let local = t + offset;
            (local / day) * day - offset
        }
        Bucket::Week => {
            // Monday first, as magpie's heatmap and every weekly chart read
            let offset = local_offset_ms(t);
            let local = t + offset;
            let day_index = local.div_euclid(day);
            let monday_index = day_index - (day_index + 3).rem_euclid(7);
            monday_index * day - offset
        }
    }
}

fn local_day(t: i64) -> String {
    let offset = local_offset_ms(t);
    chrono::DateTime::from_timestamp_millis(t + offset)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// The first and last local day the period covers. For a rolling window the
/// first is the window's own edge; the last is the latest call's day, or
/// today — the heatmap should reach the reader's now, not stop at the last
/// quiet day.
fn local_day_bounds(period: Period, now: i64, selected: &[&LedgerRecord]) -> (String, String) {
    let since = period.since_ms(now);
    let from = local_day(since.max(0));
    let latest = selected.iter().map(|r| r.t).max().unwrap_or(now);
    (from, local_day(latest.max(now)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(t: i64, agent: &'static str, model: &str, input: u64, output: u64) -> LedgerRecord {
        LedgerRecord {
            t,
            asked: 0,
            agent,
            model: model.to_owned(),
            session: "s".to_owned(),
            cwd: String::new(),
            input,
            output,
            cache_read: 0,
            cache_write: 0,
            error: None,
            cost: calc_cost(model, input, output, 0),
            retried: false,
            recovered: false,
        }
    }

    #[test]
    fn a_failure_and_its_retry_are_paired() {
        let now = chrono::Local::now().timestamp_millis();
        let mut failed = record(now - 20_000, "claude_code", "", 0, 0);
        failed.error = Some("rate_limit".to_owned());
        let answered = record(now, "claude_code", "claude-sonnet-5", 10, 20);
        let mut records = vec![answered, failed];
        records.sort_by_key(|r| std::cmp::Reverse(r.t));
        mark_retries(&mut records);
        let d = digest(&records, Period::Today, 10);
        assert_eq!(d.calls, 2);
        assert_eq!(d.retried, 1);
        assert_eq!(d.recovered, 1);
        assert_eq!(d.errors, 0, "a failure a retry answered is not counted");
    }

    #[test]
    fn a_failure_a_day_later_is_its_own() {
        let now = chrono::Local::now().timestamp_millis();
        let mut failed = record(now - 86_400_000, "claude_code", "", 0, 0);
        failed.error = Some("server_error".to_owned());
        let mut records = vec![record(now, "claude_code", "claude-opus-5", 10, 20), failed];
        records.sort_by_key(|r| std::cmp::Reverse(r.t));
        mark_retries(&mut records);
        let d = digest(&records, Period::Days(7), 10);
        assert_eq!(d.retried, 0);
        assert_eq!(d.errors, 1);
    }

    #[test]
    fn sessions_carry_their_last_context() {
        let now = chrono::Local::now().timestamp_millis();
        let mut first = record(now - 5_000, "claude_code", "claude-sonnet-5", 1_000, 10);
        first.session = "s1".to_owned();
        let mut last = record(now, "claude_desktop", "claude-opus-5", 4_000, 20);
        last.session = "s1".to_owned();
        last.cache_read = 6_000;
        let d = digest(&[first, last], Period::Today, 10);
        assert_eq!(d.sessions, 1);
        assert_eq!(d.session_rows.len(), 1);
        let row = &d.session_rows[0];
        assert_eq!(row.agent, "claude_desktop");
        assert_eq!(row.model, "claude-opus-5");
        assert_eq!(row.context, 10_000, "the prompt it was last handed");
        assert_eq!(row.calls, 2);
        assert_eq!(d.heat.len(), 1);
    }

    #[test]
    fn digest_ranks_every_dimension() {
        let now = chrono::Local::now().timestamp_millis();
        let records = vec![
            record(now, "claude_code", "claude-sonnet-5", 1_000_000, 100_000),
            record(now, "claude_desktop", "claude-sonnet-5", 500_000, 50_000),
            record(now, "claude_desktop", "claude-opus-5", 100_000, 10_000),
        ];
        let d = digest(&records, Period::Today, 10);
        assert_eq!(d.calls, 3);
        assert_eq!(d.by.model.len(), 2);
        assert_eq!(d.by.model[0].name, "claude-sonnet-5");
        assert_eq!(d.by.model_at.len(), 3);
        assert_eq!(d.by.agent.len(), 2);
        assert_eq!(d.by.agent[0].name, "Coding Assistant");
        assert_eq!(d.recent.len(), 3);
        assert!(d.cost > 0.0);
    }

    #[test]
    fn the_series_reads_left_to_right() {
        // the trend is drawn in the order the daemon hands it over; a
        // HashMap's order would scatter the days across the chart
        let now = chrono::Local::now().timestamp_millis();
        let day = 86_400_000i64;
        let records = vec![
            record(now, "claude_code", "claude-sonnet-5", 10, 5),
            record(now - 2 * day, "claude_code", "claude-sonnet-5", 10, 5),
            record(now - day, "claude_code", "claude-sonnet-5", 10, 5),
        ];
        // a week reads by the day, so three calls on three days are three
        // buckets — and they must arrive in time order
        let d = digest(&records, Period::Days(7), 10);
        assert_eq!(d.series.len(), 3);
        assert!(d.series.windows(2).all(|w| w[0].t < w[1].t));
    }

    #[test]
    fn failed_calls_are_counted_but_not_ranked() {
        let now = chrono::Local::now().timestamp_millis();
        let mut failed = record(now, "claude_code", "", 0, 0);
        failed.error = Some("rate_limit".to_owned());
        let d = digest(&[failed], Period::Today, 10);
        assert_eq!(d.calls, 1);
        assert_eq!(d.errors, 1);
        assert!(d.by.model.is_empty());
        assert_eq!(d.recent.len(), 1);
    }

    #[test]
    fn weeks_start_on_monday() {
        // 2026-10-18 is a Sunday; the week it ends runs from the 12th
        let sunday = 1_792_281_600_000i64; // 2026-10-18T00:00:00Z
        let monday = bucket_start(sunday, Bucket::Week);
        assert_eq!(local_day(monday), "2026-10-12");
        assert_ne!(local_day(monday), "2026-10-18");
    }

    #[test]
    fn a_replys_lines_collapse_to_one_call() {
        let dir = tempdir();
        let file = dir.join("s1.jsonl");
        let line = |uuid: &str, out: u64, req: &str| {
            format!(
                r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"2026-10-01T10:00:00.000Z","sessionId":"s1","requestId":"{req}","cwd":"D:/p","entrypoint":"sdk-cli","message":{{"id":"msg_1","model":"claude-sonnet-5","usage":{{"input_tokens":10,"output_tokens":{out},"cache_read_input_tokens":5}}}}}}"#
            )
        };
        fs::write(
            &file,
            format!(
                "{}\n{}\n{}\n",
                line("a", 1, "req_1"),
                line("b", 7, "req_1"),
                r#"{"type":"user","timestamp":"2026-10-01T10:00:01.000Z"}"#
            ),
        )
        .unwrap();
        let calls = read_transcript(&file).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].output, 7, "the last line carries the final usage");
        assert_eq!(calls[0].agent, "claude_code");
        assert!(calls[0].cost > 0.0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_transcript_tells_only_the_whole_turn() {
        // A message's first line is its first content block, not its first
        // token: the blocks land together. The line span of a burst like the
        // 17k-token / 77 ms one seen in the store is no writing window at all,
        // and the turn under a second is refused by the duration floor before
        // any speed is computed.
        let mut burst = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 17_145);
        burst.asked = 1_000_000 - 77;
        assert_eq!(
            burst.floor_ms(),
            None,
            "a turn under a second is mostly wait"
        );
        assert!(
            burst.output >= FLOOR_MIN_OUTPUT,
            "the burst is refused for its duration, not for its token count"
        );

        // The whole turn is what the file can time, and it is a floor.
        let mut turn = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 5_000);
        turn.asked = 1_000_000 - 20_000;
        assert_eq!(turn.floor_ms(), Some(20_000), "5,000 tokens in 20 s");

        // A turn long enough to be timed, still faster than any real stream:
        // the same 17k reply over 1.5 s reads 11,430 tok/s, and only the
        // speed cap rejects it.
        let mut too_fast = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 17_145);
        too_fast.asked = 1_000_000 - 1_500; // 11,430 tok/s
        assert_eq!(
            too_fast.floor_ms(),
            None,
            "past the duration floor, refused by the 10,000 tok/s cap"
        );

        // Just under the cap at the same length is told.
        let mut fast = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 14_000);
        fast.asked = 1_000_000 - 1_500; // 9,333 tok/s
        assert_eq!(
            fast.floor_ms(),
            Some(1_500),
            "the cap is a ceiling, not a target"
        );
    }

    #[test]
    fn a_whole_turn_is_a_floor_only_for_a_long_reply() {
        let mut long = record(20_000, "claude_code", "claude-sonnet-5", 0, 1_500);
        long.asked = 15_000; // asked 5 s before its last line
        assert_eq!(long.floor_ms(), Some(5_000));

        let mut short = record(20_000, "claude_code", "claude-sonnet-5", 0, 999);
        short.asked = 15_000;
        assert_eq!(short.floor_ms(), None, "a short reply is mostly wait");

        let unasked = record(20_000, "claude_code", "claude-sonnet-5", 0, 2_000);
        assert_eq!(unasked.floor_ms(), None, "no ask, no whole turn");
    }

    #[test]
    fn floors_are_summed_not_averaged() {
        // A fast short reply and a slow long one: the group's floor is its
        // tokens over its time, not the mean of two rates.
        let now = chrono::Local::now().timestamp_millis();
        let mut fast = record(now, "claude_code", "claude-sonnet-5", 0, 2_000);
        fast.asked = now - 2_000; // 1,000 tok/s over the turn
        let mut slow = record(now, "claude_code", "claude-sonnet-5", 0, 2_000);
        slow.asked = now - 20_000; // 100 tok/s over the turn
        let d = digest(&[fast, slow], Period::Today, 10);
        assert_eq!(d.timing.floor_n, 2);
        assert!(
            (d.timing.floor() - 181.8).abs() < 0.5,
            "4,000 tokens in 22 s"
        );
    }

    #[test]
    fn a_replys_ask_is_the_prompt_before_it() {
        let dir = tempdir();
        let file = dir.join("s3.jsonl");
        fs::write(
            &file,
            [
                r#"{"type":"user","timestamp":"2026-10-01T10:00:00.000Z"}"#,
                r#"{"type":"assistant","timestamp":"2026-10-01T10:00:02.000Z","sessionId":"s3","entrypoint":"sdk-cli","message":{"id":"m1","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":2000}}}"#,
                r#"{"type":"user","timestamp":"2026-10-01T10:00:05.000Z"}"#,
                r#"{"type":"assistant","timestamp":"2026-10-01T10:00:08.000Z","sessionId":"s3","entrypoint":"sdk-cli","message":{"id":"m2","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":3000}}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        let calls = read_transcript(&file).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].asked, 1_790_848_800_000, "asked by the prompt");
        assert_eq!(
            calls[0].floor_ms(),
            Some(2_000),
            "two seconds from ask to last line"
        );
        assert_eq!(
            calls[1].asked, 1_790_848_805_000,
            "asked by the next prompt"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn subagent_transcripts_are_read() {
        let dir = tempdir();
        let project = dir.join("projects").join("-p");
        let session = project.join("s1");
        let subs = session.join("subagents");
        let run = subs.join("workflows").join("wf_1");
        fs::create_dir_all(&run).unwrap();
        let line = |session: &str, agent: &str, out: u64| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-10-01T10:00:00.000Z","sessionId":"{session}","isSidechain":true,"agentId":"{agent}","entrypoint":"sdk-cli","message":{{"id":"msg_{agent}","model":"claude-haiku-5","usage":{{"input_tokens":10,"output_tokens":{out}}}}}}}"#
            )
        };
        fs::write(project.join("s1.jsonl"), "").unwrap();
        fs::write(subs.join("agent-a1.jsonl"), line("s1", "a1", 7)).unwrap();
        fs::write(subs.join("agent-a1.meta.json"), "{}").unwrap();
        fs::write(run.join("agent-a2.jsonl"), line("s1", "a2", 9)).unwrap();
        fs::write(run.join("journal.jsonl"), "").unwrap();

        let files = transcript_files(&dir).unwrap();
        assert_eq!(files.len(), 3, "the session and two of its subagents");
        assert!(
            files
                .iter()
                .all(|p| p.file_name().and_then(|n| n.to_str()) != Some("journal.jsonl")),
            "a workflow's journal is not a transcript"
        );
        let calls: Vec<LedgerRecord> = files
            .iter()
            .flat_map(|p| read_transcript(p).unwrap())
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|c| c.session == "s1"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn desktop_entrypoints_are_their_own_agent() {
        let dir = tempdir();
        let file = dir.join("s2.jsonl");
        let line = |ep: &str, id: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-10-01T10:00:00.000Z","sessionId":"s2","entrypoint":"{ep}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1,"output_tokens":2}}}}}}"#
            )
        };
        fs::write(
            &file,
            format!(
                "{}\n{}\n",
                line("claude-desktop-3p", "m1"),
                line("sdk-cli", "m2")
            ),
        )
        .unwrap();
        let calls = read_transcript(&file).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].agent, "claude_desktop");
        assert_eq!(calls[1].agent, "claude_code");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn periods_start_at_the_local_day() {
        // 2026-10-19T12:00:00Z, whatever the machine's zone is
        let now = 1_792_411_200_000i64;
        let today = Period::Today.since_ms(now);
        assert!(today <= now);
        assert!(now - today <= 86_400_000);
        let week = Period::Days(7).since_ms(now);
        assert!(today - week <= 7 * 86_400_000);
        assert_eq!(Period::parse(Some("30d")), Period::Days(30));
        assert_eq!(Period::parse(Some("nonsense")), Period::All);
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agenthub-ledger-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
