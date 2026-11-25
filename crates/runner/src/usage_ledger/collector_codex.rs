use super::*;

const CODEX_ROLLOUT_LIMIT: usize = 4_000;
const CODEX_ROLLOUT_MAX_DEPTH: usize = 8;

/// Discovers every Codex home folder on this machine (port of Magpie's
/// `appdir.CodexHomesIn`): `$CODEX_HOME`, `~/.codex`, and any `~/.codex*`
/// profile directory holding `auth.json` or `sessions/`.
pub(crate) fn codex_homes(home: &Path) -> Vec<PathBuf> {
    let main = std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".codex"));
    let mut out = vec![main.clone()];
    let mut seen = HashSet::new();
    seen.insert(dunce::canonicalize(&main).unwrap_or_else(|_| main.clone()));

    let mut add = |dir: PathBuf| {
        let canon = dunce::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if seen.insert(canon) {
            out.push(dir);
        }
    };
    let is_codex_home =
        |dir: &Path| dir.join("auth.json").is_file() || dir.join("sessions").is_dir();

    if let Ok(entries) = fs::read_dir(home) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|k| k.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let trimmed = name.trim_start_matches('.');
            if !trimmed.starts_with("codex") {
                continue;
            }
            if is_codex_home(&path) {
                add(path);
                continue;
            }
            if let Ok(subs) = fs::read_dir(&path) {
                for sub in subs.flatten() {
                    let sub_path = sub.path();
                    if is_codex_home(&sub_path) {
                        add(sub_path);
                    }
                }
            }
        }
    }
    out
}

/// Codex's sessions and per-turn calls, from all rollouts (`.jsonl` and
/// `.jsonl.zst`) across all Codex homes (port of Magpie's `codexFiles` and
/// `codexCallLine` in `internal/sessions/calls.go`).
pub(crate) fn update_codex_calls(home: &Path, routed: &HashMap<String, String>) -> bool {
    let mut files = Vec::new();
    let mut seen_paths = HashSet::new();
    for root in codex_homes(home) {
        for directory in ["sessions", "archived_sessions"] {
            collect_rollouts(&root.join(directory), 0, &mut files, &mut seen_paths);
        }
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    if files.len() > CODEX_ROLLOUT_LIMIT {
        files.drain(..files.len() - CODEX_ROLLOUT_LIMIT);
    }

    let mut file_marks = Vec::with_capacity(files.len());
    for path in files {
        let Some((len, modified_ms)) = live_file_mark(&path) else {
            continue;
        };
        let mark = FileMark { len, modified_ms };
        file_marks.push((path, mark));
    }

    // Zero-clone fast path when all rollout files match their cached FileMark.
    if let Ok(guard) = ledger_cache().read()
        && guard.walked_at.is_some()
        && guard.codex.len() == file_marks.len()
        && file_marks
            .iter()
            .all(|(p, mark)| guard.codex.get(p).is_some_and(|(old, _)| old == mark))
    {
        return false;
    }

    let Ok(mut guard) = ledger_cache().write() else {
        return false;
    };
    let mark_set: HashSet<&Path> = file_marks.iter().map(|(p, _)| p.as_path()).collect();
    let mut any_changed = guard.codex.len() != file_marks.len();
    guard.codex.retain(|p, _| mark_set.contains(p.as_path()));
    any_changed |= guard.remove_missing_cold_paths("codex", &mark_set);

    for (path, mark) in file_marks {
        if let Some((old_mark, _)) = guard.codex.get(&path) {
            if *old_mark == mark {
                continue;
            }
        }
        if !guard.codex.contains_key(&path)
            && guard.cold_manifest.get(&path).is_some_and(|m| {
                m.source == "codex" && m.file_len == mark.len && m.modified_ms == mark.modified_ms
            })
        {
            guard.codex.insert(path, (mark, Vec::new()));
            any_changed = true;
            continue;
        }
        guard.mark_cold_path_dirty(&path);
        let records = read_codex_rollout_calls(&path, routed);
        guard.codex.insert(path, (mark, records));
        any_changed = true;
    }
    any_changed
}

#[cfg(test)]
pub(crate) fn codex_calls(
    home: &Path,
    routed: &HashMap<String, String>,
) -> (Vec<LedgerRecord>, bool) {
    let changed = update_codex_calls(home, routed);
    let mut out = Vec::new();
    if let Ok(guard) = ledger_cache().read() {
        for (_, recs) in guard.codex.values() {
            out.extend(recs.iter().cloned());
        }
    }
    out.sort_by_key(|r| std::cmp::Reverse(r.t));
    (out, changed)
}

fn collect_rollouts(
    directory: &Path,
    depth: usize,
    files: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    if depth > CODEX_ROLLOUT_MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            collect_rollouts(&path, depth + 1, files, seen);
            continue;
        }
        let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
        if name.starts_with("rollout-")
            && (name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
            && seen.insert(path.clone())
        {
            files.push(path);
        }
    }
}

fn extract_rollout_thread_id(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    let stem = name
        .strip_suffix(".jsonl.zst")
        .or_else(|| name.strip_suffix(".jsonl"))
        .unwrap_or(name);
    // rollout-YYYY-MM-DDTHH-MM-SS-<thread_id>
    if let Some(rest) = stem.strip_prefix("rollout-")
        && rest.len() > 20
        && rest.as_bytes().get(19) == Some(&b'-')
    {
        return rest[20..].to_owned();
    }
    stem.to_owned()
}

/// Reads every model call in one Codex rollout (`.jsonl` or `.jsonl.zst`),
/// tracking per-turn `model` changes, `token_usage_record`, `compacted`, and
/// `token_count` deltas, and subtracting `cached_input_tokens` +
/// `cache_write_input_tokens` from `input_tokens` (port of Magpie's
/// `codexCallLine` in `internal/sessions/calls.go` and `codex_usage.go`).
#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexUsageFields {
    input_tokens: crate::collectors::JsonOptU64,
    output_tokens: crate::collectors::JsonOptU64,
    cached_input_tokens: crate::collectors::JsonOptU64,
    cache_read_input_tokens: crate::collectors::JsonOptU64,
    cache_write_input_tokens: crate::collectors::JsonOptU64,
    cache_creation_input_tokens: crate::collectors::JsonOptU64,
}

impl CodexUsageFields {
    fn into_tuple(self) -> (u64, u64, u64, u64) {
        (
            self.input_tokens.0.unwrap_or(0),
            self.output_tokens.0.unwrap_or(0),
            self.cached_input_tokens
                .0
                .or(self.cache_read_input_tokens.0)
                .unwrap_or(0),
            self.cache_write_input_tokens
                .0
                .or(self.cache_creation_input_tokens.0)
                .unwrap_or(0),
        )
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexProvenance<'a> {
    #[serde(borrow)]
    model: crate::collectors::JsonOptStr<'a>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexBaseInstructions<'a> {
    #[serde(borrow)]
    provenance: crate::collectors::JsonOptMap<CodexProvenance<'a>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexThreadSettings<'a> {
    #[serde(borrow)]
    model: crate::collectors::JsonOptStr<'a>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexTokenCountInfo {
    total_token_usage: crate::collectors::JsonOptMap<CodexUsageFields>,
    last_token_usage: crate::collectors::JsonOptMap<CodexUsageFields>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexCompactUsageRecord<'a> {
    usage: crate::collectors::JsonOptMap<CodexUsageFields>,
    #[serde(borrow)]
    response_id: crate::collectors::JsonOptStr<'a>,
    thread_token_usage: crate::collectors::JsonOptMap<CodexUsageFields>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexPayload<'a> {
    #[serde(borrow)]
    session_id: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    id: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    cwd: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    model: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    base_instructions: crate::collectors::JsonOptMap<CodexBaseInstructions<'a>>,
    #[serde(borrow)]
    turn_id: crate::collectors::JsonOptStr<'a>,
    usage: crate::collectors::JsonOptMap<CodexUsageFields>,
    #[serde(borrow)]
    response_id: crate::collectors::JsonOptStr<'a>,
    thread_token_usage: crate::collectors::JsonOptMap<CodexUsageFields>,
    #[serde(borrow)]
    latest_token_usage_record: crate::collectors::JsonOptMap<Box<CodexCompactUsageRecord<'a>>>,
    #[serde(rename = "type", borrow)]
    kind: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    thread_settings: crate::collectors::JsonOptMap<CodexThreadSettings<'a>>,
    info: crate::collectors::JsonOptMap<CodexTokenCountInfo>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexRolloutLine<'a> {
    #[serde(rename = "type", borrow)]
    kind: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    timestamp: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    payload: crate::collectors::JsonOptMap<CodexPayload<'a>>,
}

fn read_codex_rollout_calls(path: &Path, routed: &HashMap<String, String>) -> Vec<LedgerRecord> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let is_zst = path
        .file_name()
        .and_then(|v| v.to_str())
        .is_some_and(|n| n.ends_with(".zst"));
    let mut reader: BufReader<Box<dyn std::io::Read>> = if is_zst {
        let Ok(decoder) = zstd::stream::Decoder::new(BufReader::new(file)) else {
            return Vec::new();
        };
        BufReader::new(Box::new(decoder))
    } else {
        BufReader::new(Box::new(file))
    };

    let mut model = String::new();
    let mut session = extract_rollout_thread_id(path);
    let mut cwd = String::new();
    let mut last_in_ms: i64 = 0;
    let mut last_call_ms: i64 = 0;
    let mut turn_models: HashMap<String, String> = HashMap::new();
    let mut current_turn = String::new();

    // (input, output, cached, cache_write)
    let mut high_total: Option<(u64, u64, u64, u64)> = None;
    let mut seen_responses: HashSet<String> = HashSet::new();
    let mut recorded_totals: HashSet<(u64, u64)> = HashSet::new();
    let mut calls: Vec<LedgerRecord> = Vec::new();

    let push_codex_call = |calls: &mut Vec<LedgerRecord>,
                           t: i64,
                           asked_ms: i64,
                           sess: &str,
                           dir: &str,
                           mdl: &str,
                           raw: (u64, u64, u64, u64)| {
        let (raw_in, raw_out, cached, cw) = raw;
        if raw_in == 0 && raw_out == 0 {
            return;
        }
        // Magpie's `spent()`: Codex `input_tokens` includes cached and cache_write tokens.
        let uncached_in = raw_in.saturating_sub(cached.saturating_add(cw));
        let eff_model = if mdl.is_empty() { "gpt-5-codex" } else { mdl };
        let provider = resolve_provider_label("codex", eff_model, routed);
        calls.push(LedgerRecord {
            t,
            asked: asked_ms,
            agent: "codex",
            provider,
            model: intern(eff_model),
            session: intern(sess),
            cwd: intern(dir),
            input: uncached_in,
            output: raw_out,
            cache_read: cached,
            cache_write: cw,
            duration_ms: turn_duration_ms(asked_ms, t),
            error: None,
            cost: calc_cost(eff_model, raw_in, raw_out, cached),
            retried: false,
            recovered: false,
        });
    };

    let mut line_buf = Vec::new();
    loop {
        line_buf.clear();
        if line_buf.capacity() > 512 * 1024 {
            line_buf = Vec::new();
        }
        match read_bounded_ledger_line(&mut reader, &mut line_buf, LEDGER_LINE_LIMIT) {
            Ok((0, _, _)) => break,
            Ok((_, _, true)) => continue,
            Ok(_) => {}
            Err(_) => break,
        }
        let line = line_buf.as_slice();
        if line.is_empty() {
            continue;
        }
        // Fast check for input events (user prompt or tool output) to track turn start time
        if (contains(line, b"\"user_message\"")
            || contains(line, b"\"function_call_output\"")
            || contains(line, b"\"custom_tool_call_output\"")
            || contains(line, b"\"role\":\"user\""))
            && let Some(ms) = line_timestamp_ms(line)
        {
            last_in_ms = last_in_ms.max(ms);
        }
        let relevant = contains(line, b"\"session_meta\"")
            || contains(line, b"\"turn_context\"")
            || contains(line, b"\"thread_settings_applied\"")
            || contains(line, b"\"task_started\"")
            || contains(line, b"\"token_usage_record\"")
            || contains(line, b"\"compacted\"")
            || contains(line, b"\"token_count\"");
        if !relevant {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<CodexRolloutLine>(line) else {
            continue;
        };
        let line_ts = value
            .timestamp
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
            .or_else(|| line_timestamp_ms(line))
            .unwrap_or(0);

        match value.kind.as_deref() {
            Some("session_meta") => {
                if let Some(payload) = value.payload.0 {
                    if let Some(id) = payload
                        .session_id
                        .as_deref()
                        .or(payload.id.as_deref())
                        .filter(|s| !s.is_empty())
                    {
                        session = id.to_owned();
                    }
                    if let Some(c) = payload.cwd.as_deref().filter(|s| !s.is_empty()) {
                        cwd = c.to_owned();
                    }
                    if let Some(name) = payload
                        .base_instructions
                        .0
                        .and_then(|bi| bi.provenance.0)
                        .and_then(|prov| prov.model.0)
                        .or(payload.model.0)
                        .as_deref()
                        .filter(|s| !s.is_empty())
                    {
                        model = name.to_owned();
                    }
                }
            }
            Some("turn_context") => {
                if let Some(payload) = value.payload.0 {
                    if let Some(tid) = payload.turn_id.as_deref().filter(|s| !s.is_empty()) {
                        current_turn = tid.to_owned();
                    }
                    if let Some(c) = payload.cwd.as_deref().filter(|s| !s.is_empty()) {
                        cwd = c.to_owned();
                    }
                    if let Some(name) = payload.model.as_deref().filter(|s| !s.is_empty()) {
                        model = name.to_owned();
                        if !current_turn.is_empty() {
                            turn_models.insert(current_turn.clone(), model.clone());
                        }
                    }
                }
            }
            Some("token_usage_record") => {
                let Some(payload) = value.payload.0 else {
                    continue;
                };
                let Some(usage) = payload.usage.0 else {
                    continue;
                };
                if line_ts <= 0 {
                    continue;
                }
                let resp_id = payload.response_id.as_deref().unwrap_or("");
                if !resp_id.is_empty() && !seen_responses.insert(resp_id.to_owned()) {
                    continue;
                }
                let turn_id = payload.turn_id.as_deref().unwrap_or(&current_turn);
                let call_model = turn_models
                    .get(turn_id)
                    .cloned()
                    .unwrap_or_else(|| model.clone());
                let raw = usage.into_tuple();
                if let Some(tot) = payload.thread_token_usage.0 {
                    let (t_in, t_out, t_c, t_cw) = tot.into_tuple();
                    recorded_totals.insert((t_in, t_out));
                    high_total = Some((t_in, t_out, t_c, t_cw));
                }
                let asked = last_in_ms.max(last_call_ms).min(line_ts);
                push_codex_call(&mut calls, line_ts, asked, &session, &cwd, &call_model, raw);
                last_call_ms = last_call_ms.max(line_ts);
            }
            Some("compacted") => {
                let Some(rec) = value.payload.0.and_then(|p| p.latest_token_usage_record.0) else {
                    continue;
                };
                let Some(usage) = rec.usage.0 else {
                    continue;
                };
                if line_ts <= 0 {
                    continue;
                }
                let resp_id = rec.response_id.as_deref().unwrap_or("");
                if !resp_id.is_empty() && !seen_responses.insert(resp_id.to_owned()) {
                    continue;
                }
                let raw = usage.into_tuple();
                if let Some(tot) = rec.thread_token_usage.0 {
                    let (t_in, t_out, t_c, t_cw) = tot.into_tuple();
                    recorded_totals.insert((t_in, t_out));
                    high_total = Some((t_in, t_out, t_c, t_cw));
                }
                let asked = last_in_ms.max(last_call_ms).min(line_ts);
                push_codex_call(&mut calls, line_ts, asked, &session, &cwd, &model, raw);
                last_call_ms = last_call_ms.max(line_ts);
            }
            Some("event_msg") => {
                let Some(payload) = value.payload.0 else {
                    continue;
                };
                match payload.kind.as_deref() {
                    Some("task_started") => {
                        if let Some(tid) = payload.turn_id.as_deref().filter(|s| !s.is_empty()) {
                            current_turn = tid.to_owned();
                        }
                        if line_ts > 0 {
                            last_in_ms = last_in_ms.max(line_ts);
                        }
                    }
                    Some("thread_settings_applied") => {
                        if let Some(m) = payload
                            .thread_settings
                            .0
                            .and_then(|ts| ts.model.0)
                            .as_deref()
                            .filter(|s| !s.is_empty())
                        {
                            model = m.to_owned();
                            if !current_turn.is_empty() {
                                turn_models.insert(current_turn.clone(), model.clone());
                            }
                        }
                    }
                    Some("token_count") => {
                        if line_ts <= 0 {
                            continue;
                        }
                        let Some(info) = payload.info.0 else {
                            continue;
                        };
                        let Some(total_val) = info.total_token_usage.0 else {
                            continue;
                        };
                        let (t_in, t_out, t_c, t_cw) = total_val.into_tuple();
                        if t_in == 0 && t_out == 0 {
                            continue;
                        }
                        if recorded_totals.contains(&(t_in, t_out))
                            || high_total == Some((t_in, t_out, t_c, t_cw))
                        {
                            high_total = Some((t_in, t_out, t_c, t_cw));
                            continue;
                        }
                        let last_tuple = info.last_token_usage.0.map(CodexUsageFields::into_tuple);
                        // Counter reset check (`codex_usage.go:559`)
                        if let Some((p_in, p_out, _, _)) = high_total
                            && (t_in < p_in || t_out < p_out)
                        {
                            high_total = None;
                        }
                        let call_raw = if let Some((l_in, l_out, l_c, l_cw)) = last_tuple {
                            if l_in == 0 && l_out == 0 {
                                // `codex_usage.go:625`: last=0 on a fork checkpoint books nothing
                                high_total = Some((t_in, t_out, t_c, t_cw));
                                continue;
                            }
                            (l_in, l_out, l_c, l_cw)
                        } else if let Some((p_in, p_out, p_c, p_cw)) = high_total {
                            (
                                t_in.saturating_sub(p_in),
                                t_out.saturating_sub(p_out),
                                t_c.saturating_sub(p_c),
                                t_cw.saturating_sub(p_cw),
                            )
                        } else {
                            (t_in, t_out, t_c, t_cw)
                        };
                        high_total = Some((t_in, t_out, t_c, t_cw));
                        let call_model = turn_models
                            .get(&current_turn)
                            .cloned()
                            .unwrap_or_else(|| model.clone());
                        let asked = last_in_ms.max(last_call_ms).min(line_ts);
                        push_codex_call(
                            &mut calls,
                            line_ts,
                            asked,
                            &session,
                            &cwd,
                            &call_model,
                            call_raw,
                        );
                        last_call_ms = last_call_ms.max(line_ts);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    calls
}
