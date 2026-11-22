use super::*;

pub(crate) const LEDGER_FILE_LIMIT: usize = 4_000;
pub(crate) const LEDGER_LINE_LIMIT: usize = 8 * 1024 * 1024;

pub(crate) fn claude_config_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    home_dir().map(|home| home.join(".claude"))
}

/// Discovers all Claude Desktop data folders on this machine, including
/// Windows MSIX packaged `LocalCache` folders (port of Magpie's
/// `desktopdir.Everywhere()` in `internal/desktopdir/desktopdir.go`).
pub(crate) fn claude_desktop_data_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |p: PathBuf| {
        if p.is_dir() {
            let canon = dunce::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if seen.insert(canon) {
                out.push(p);
            }
        }
    };

    for root in crate::claude_gateway::gateway_search_roots() {
        push(root);
    }

    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join("AppData").join("Local")))
        {
            let packages = local.join("Packages");
            if let Ok(entries) = fs::read_dir(&packages) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("Claude_") || name.starts_with("AnthropicPBC.Claude_") {
                        let cache = entry.path().join("LocalCache");
                        push(cache.join("Roaming").join("Claude"));
                        push(cache.join("Local").join("Claude"));
                        push(cache.join("Local").join("Claude-3p"));
                    }
                }
            }
        }
    }

    out
}

/// All `.claude` config roots holding `projects/` transcripts, paired with their
/// default agent (`claude_code` for `~/.claude`, `claude_desktop` for Cowork's
/// `local-agent-mode-sessions/*/*/local_*/.claude`, port of Magpie's `callFileLists`).
pub(crate) fn claude_transcript_roots() -> Vec<(PathBuf, &'static str)> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |dir: PathBuf, agent: &'static str| {
        if dir.join("projects").is_dir() {
            let canon = dunce::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            if seen.insert(canon) {
                roots.push((dir, agent));
            }
        }
    };

    if let Some(cfg) = claude_config_dir() {
        push(cfg, "claude_code");
    }

    for desktop_dir in claude_desktop_data_dirs() {
        let sessions_root = desktop_dir.join("local-agent-mode-sessions");
        let Ok(lvl1) = fs::read_dir(&sessions_root) else {
            continue;
        };
        for d1 in lvl1.flatten() {
            let Ok(lvl2) = fs::read_dir(d1.path()) else {
                continue;
            };
            for d2 in lvl2.flatten() {
                let Ok(lvl3) = fs::read_dir(d2.path()) else {
                    continue;
                };
                for d3 in lvl3.flatten() {
                    let name = d3.file_name().to_string_lossy().to_string();
                    if name.starts_with("local_") {
                        push(d3.path().join(".claude"), "claude_desktop");
                    }
                }
            }
        }
    }

    roots
}

/// Every `*.jsonl` under `projects/` across all discovered Claude roots,
/// newest first, bounded, with live handle metadata `(path, default_agent, len, modified_ms)`.
pub(crate) fn transcript_files_from_roots(
    roots: &[(PathBuf, &'static str)],
) -> Vec<(PathBuf, &'static str, u64, i64)> {
    let mut files: Vec<(i64, u64, PathBuf, &'static str)> = Vec::new();
    let mut seen_paths = HashSet::new();

    for (config_dir, default_agent) in roots {
        let projects = config_dir.join("projects");
        let Ok(dirs) = fs::read_dir(&projects) else {
            continue;
        };
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
                    collect_subagent_files(&path, default_agent, &mut files, &mut seen_paths);
                    continue;
                }
                if path.extension().and_then(|v| v.to_str()) != Some("jsonl") {
                    continue;
                }
                push_transcript(&path, default_agent, &mut files, &mut seen_paths);
            }
        }
    }

    files.sort_by_key(|(m, _, _, _)| std::cmp::Reverse(*m));
    files.truncate(LEDGER_FILE_LIMIT);
    files
        .into_iter()
        .map(|(m, len, path, agent)| (path, agent, len, m))
        .collect()
}

/// The subagent transcripts under one session's folder, if it has any.
fn collect_subagent_files(
    session_dir: &Path,
    default_agent: &'static str,
    files: &mut Vec<(i64, u64, PathBuf, &'static str)>,
    seen: &mut HashSet<PathBuf>,
) {
    let subagents = session_dir.join("subagents");
    if !subagents.is_dir() {
        return;
    }
    if let Ok(entries) = fs::read_dir(&subagents) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) == Some("jsonl") {
                push_transcript(&path, default_agent, files, seen);
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
                push_transcript(&path, default_agent, files, seen);
            }
        }
    }
}

fn push_transcript(
    path: &Path,
    default_agent: &'static str,
    files: &mut Vec<(i64, u64, PathBuf, &'static str)>,
    seen: &mut HashSet<PathBuf>,
) {
    if !seen.insert(path.to_path_buf()) {
        return;
    }
    let Some((len, modified)) = live_file_mark(path) else {
        return;
    };
    files.push((modified, len, path.to_path_buf(), default_agent));
}

/// Resolves the parent conversation session ID from a transcript path (port of
/// Magpie's `sessionOfPath` in `internal/sessions/calls.go:566`):
/// `<id>.jsonl`, `<id>/subagents/<agent>.jsonl`, or
/// `<id>/subagents/workflows/<run>/<agent>.jsonl`.
pub(crate) fn session_of_path(path: &Path) -> String {
    if let Some(parent) = path.parent() {
        if parent.file_name().and_then(|n| n.to_str()) == Some("subagents")
            && let Some(session_dir) = parent.parent()
            && let Some(name) = session_dir.file_name().and_then(|n| n.to_str())
        {
            return name.to_owned();
        }
        if let Some(workflows) = parent.parent()
            && workflows.file_name().and_then(|n| n.to_str()) == Some("workflows")
            && let Some(subagents) = workflows.parent()
            && subagents.file_name().and_then(|n| n.to_str()) == Some("subagents")
            && let Some(session_dir) = subagents.parent()
            && let Some(name) = session_dir.file_name().and_then(|n| n.to_str())
        {
            return name.to_owned();
        }
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned()
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
    #[serde(default, rename = "requestRef")]
    request_ref: Option<String>,
    #[serde(default, rename = "requestedModel")]
    requested_model: Option<String>,
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
struct CacheCreationDetail {
    #[serde(default)]
    ephemeral_5m_input_tokens: u64,
    #[serde(default)]
    ephemeral_1h_input_tokens: u64,
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
    #[serde(default)]
    cache_creation: Option<CacheCreationDetail>,
}

fn strip_context_suffix(model: &str) -> &str {
    model
        .trim()
        .trim_end_matches(" [1m]")
        .trim_end_matches("[1m]")
        .trim_end_matches(" [1M]")
        .trim_end_matches("[1M]")
        .trim()
}

/// Reads newly appended lines of one transcript from `state.offset` into
/// `state.calls`, seeking past everything already parsed on previous walks.
pub(crate) fn read_transcript_incremental(
    path: &Path,
    state: &mut TranscriptState,
    routed: &HashMap<String, String>,
) -> Result<(), CollectorError> {
    let program = "Claude transcript store";
    let mut file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let file_len = file
        .metadata()
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?
        .len();
    if file_len < state.offset {
        let default_agent = state.default_agent;
        *state = TranscriptState {
            default_agent,
            ..TranscriptState::default()
        };
    } else if file_len == state.offset {
        return Ok(());
    }
    if state.offset > 0 {
        file.seek(SeekFrom::Start(state.offset))
            .map_err(|source| CollectorError::Io {
                program: program.to_owned(),
                source,
            })?;
    }

    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let fallback_session = session_of_path(path);

    loop {
        line.clear();
        let (bytes_read, terminated) = match read_line_with_delim(&mut reader, &mut line) {
            Ok((0, _)) => break,
            Ok(pair) => pair,
            Err(_) => break,
        };
        if !terminated {
            // An unterminated tail at EOF is only committed if it already ends a
            // complete JSON object (`}`); otherwise a writer is mid-flush and
            // we keep `state.offset` right before it for the next poll.
            let trimmed_end = line
                .iter()
                .rev()
                .find(|&&b| !b.is_ascii_whitespace())
                .copied();
            if trimmed_end != Some(b'}') {
                break;
            }
        }
        state.offset = state.offset.saturating_add(bytes_read as u64);
        if line.len() > LEDGER_LINE_LIMIT {
            continue;
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct IdentityModel<'a> {
            #[serde(borrow, rename = "modelId")]
            model_id: crate::collectors::JsonOptStr<'a>,
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct AttachmentIdentity<'a> {
            #[serde(borrow)]
            identity: crate::collectors::JsonOptMap<IdentityModel<'a>>,
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct AttachmentIdentityLine<'a> {
            #[serde(borrow)]
            attachment: crate::collectors::JsonOptMap<AttachmentIdentity<'a>>,
        }

        if contains(&line, b"\"identity\"")
            && contains(&line, b"\"modelId\"")
            && let Ok(att) = serde_json::from_slice::<AttachmentIdentityLine>(&line)
            && let Some(mid) = att
                .attachment
                .0
                .and_then(|a| a.identity.0)
                .and_then(|id| id.model_id.0)
                .map(|s| strip_context_suffix(s.as_ref()).to_owned())
                .filter(|s| !s.is_empty())
        {
            state.requested_model = Some(mid);
        }
        if !contains(&line, b"\"assistant\"") {
            if contains(&line, b"\"user\"")
                && let Some(ms) = line_timestamp_ms(&line)
            {
                state.last_user_ms = state.last_user_ms.max(ms);
            }
            continue;
        }
        let Ok(parsed) = serde_json::from_slice::<AssistantLine>(&line) else {
            continue;
        };
        if parsed.kind != "assistant" {
            continue;
        }
        if let Some(req_mdl) = parsed
            .requested_model
            .as_deref()
            .map(strip_context_suffix)
            .filter(|s| !s.is_empty())
        {
            state.requested_model = Some(req_mdl.to_owned());
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
        let raw_msg_model = message.model.as_deref().map(str::trim).unwrap_or_default();
        let failed = parsed.api_error;
        let model = if !raw_msg_model.is_empty() && raw_msg_model != "<synthetic>" {
            strip_context_suffix(raw_msg_model).to_owned()
        } else if !failed {
            match state.requested_model.as_deref() {
                Some(req_m) if !req_m.is_empty() => req_m.to_owned(),
                _ => continue,
            }
        } else {
            String::new()
        };
        if model.is_empty() && !failed {
            continue;
        }
        let agent = if state.default_agent == "claude_desktop"
            || parsed
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
        let req_id = parsed
            .request_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| parsed.request_ref.as_deref().filter(|s| !s.is_empty()));
        let key = match (message.id.as_deref(), req_id) {
            (Some(id), Some(req)) if !id.is_empty() => format!("{id}\u{1f}{req}"),
            (Some(id), None) if !id.is_empty() => id.to_owned(),
            (_, Some(req)) => req.to_owned(),
            _ => format!("{time}\u{1f}{model}"),
        };
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
        // Port of Magpie's `ccTokens` (`sessions.go:80-86`)
        let cache_write =
            usage
                .cache_creation
                .as_ref()
                .map_or(usage.cache_creation_input_tokens, |cc| {
                    usage.cache_creation_input_tokens.max(
                        cc.ephemeral_5m_input_tokens
                            .saturating_add(cc.ephemeral_1h_input_tokens),
                    )
                });
        let asked = state.last_user_ms.max(state.last_assistant_ms).min(time);
        let provider = resolve_provider_label(agent, &model, routed);
        let record = LedgerRecord {
            t: time,
            asked,
            agent,
            provider,
            model: if failed { intern("") } else { intern(&model) },
            session: intern(&session),
            cwd: intern(parsed.cwd.as_deref().unwrap_or("")),
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_read: usage.cache_read_input_tokens,
            cache_write,
            duration_ms: turn_duration_ms(asked, time),
            error: error.map(|e| intern(&e)),
            cost: if failed {
                0.0
            } else {
                calc_cost(
                    &model,
                    usage
                        .input_tokens
                        .saturating_add(usage.cache_read_input_tokens)
                        .saturating_add(cache_write),
                    usage.output_tokens,
                    usage.cache_read_input_tokens,
                )
            },
            retried: false,
            recovered: false,
        };
        match state.index.get(&key) {
            Some(&at) => {
                let asked = state.calls[at].asked;
                state.calls[at] = record;
                state.calls[at].asked = asked;
                state.calls[at].duration_ms = turn_duration_ms(asked, time);
            }
            None => {
                state.index.insert(key, state.calls.len());
                state.calls.push(record);
            }
        }
        state.last_assistant_ms = state.last_assistant_ms.max(time);
    }

    // Bound memory used by the chunk deduplication index:
    // Historical transcript files (>10 minutes inactive) never receive new chunks for old messages.
    let now_ms = chrono::Utc::now().timestamp_millis();
    if now_ms.saturating_sub(state.modified_ms) > 10 * 60 * 1000 {
        state.index.clear();
        state.index.shrink_to_fit();
    } else if state.index.len() > 32 {
        let min_idx = state.calls.len().saturating_sub(32);
        state.index.retain(|_, &mut idx| idx >= min_idx);
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn read_transcript(path: &Path) -> Result<Vec<LedgerRecord>, CollectorError> {
    let mut state = TranscriptState::default();
    read_transcript_incremental(path, &mut state, &HashMap::new())?;
    Ok(state.calls)
}

/// A prompt or tool result's moment, taken off its bytes.
///
/// A user line is the prompt or a tool result, and a tool result carries the
/// whole output of the command — screenshots as base64 run to megabytes. The
/// whole-turn floor needs nothing from it but when it happened, so the line is
/// scanned for `"timestamp":"…"` and that one field is parsed; the rest of the
/// line is never handed to the JSON parser.
pub(crate) fn line_timestamp_ms(line: &[u8]) -> Option<i64> {
    const MARK: &[u8] = b"\"timestamp\":\"";
    let at = find_bytes(line, MARK)? + MARK.len();
    let rest = &line[at..];
    let end = rest.iter().position(|b| *b == b'"')?;
    let stamp = std::str::from_utf8(&rest[..end]).ok()?;
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|d| d.timestamp_millis())
}

pub(crate) fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Does the line hold this byte sequence? A small memchr so a scan of the
/// store does not build a String for every line.
pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find_bytes(haystack, needle).is_some()
}
/// Streams one LF-delimited line up to `limit` bytes without letting an
/// oversized record expand `buf` unboundedly. Returns `(total_bytes, terminated, oversized)`.
pub(crate) fn read_bounded_ledger_line<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    limit: usize,
) -> std::io::Result<(usize, bool, bool)> {
    let mut total = 0usize;
    let mut oversized = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok((total, false, oversized));
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(at) => {
                let slice = &available[..at];
                if !oversized && buf.len().saturating_add(slice.len()) <= limit {
                    buf.extend_from_slice(slice);
                } else {
                    oversized = true;
                }
                reader.consume(at + 1);
                return Ok((total + at + 1, true, oversized));
            }
            None => {
                let n = available.len();
                if !oversized && buf.len().saturating_add(n) <= limit {
                    buf.extend_from_slice(available);
                } else {
                    oversized = true;
                }
                reader.consume(n);
                total += n;
            }
        }
    }
}

pub(crate) fn read_line_with_delim<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<(usize, bool)> {
    let mut total = 0;
    loop {
        let available = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok((total, false));
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(at) => {
                buf.extend_from_slice(&available[..at]);
                reader.consume(at + 1);
                return Ok((total + at + 1, true));
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
