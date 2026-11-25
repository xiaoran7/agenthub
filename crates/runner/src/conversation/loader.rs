use super::*;

/// Loads a bounded conversation preview for one session, or an empty list when
/// no transcript exists. Never returns tool payloads beyond what the provider
/// already wrote into its own local transcript on this machine.
pub fn load_session_conversation(
    home: &Path,
    agent_type: AgentType,
    session_id: &str,
) -> Vec<Value> {
    load_session_conversation_with_workspace(home, agent_type, session_id, None)
}

pub type TranscriptFileFingerprint = (PathBuf, u64, Option<SystemTime>);

/// Returns `true` when every tracked transcript file still exists with the
/// exact same `(len, mtime)` on disk.
pub fn transcript_fingerprints_unchanged(fingerprints: &[TranscriptFileFingerprint]) -> bool {
    !fingerprints.is_empty()
        && fingerprints.iter().all(|(path, c_len, c_mtime)| {
            let Some(meta) = fs::metadata(path).ok() else {
                return false;
            };
            meta.len() == *c_len && meta.modified().ok() == *c_mtime
        })
}

/// Checks whether a session's on-disk transcript changed relative to caller-held
/// `known_fingerprints`.
///
/// Checking `known_fingerprints` directly via `fs::metadata` avoids relying on
/// the 10-second `conversation_cache()` TTL, so idle sessions perform zero disk
/// reads and zero JSON parses indefinitely.
pub fn load_session_conversation_if_changed(
    home: &Path,
    agent_type: AgentType,
    session_id: &str,
    workspace: Option<&Path>,
    known_fingerprints: &[TranscriptFileFingerprint],
) -> Option<(Vec<TranscriptFileFingerprint>, Vec<Value>)> {
    if validate_session_id(session_id).is_err() {
        return None;
    }
    if transcript_fingerprints_unchanged(known_fingerprints) {
        return None;
    }

    let cache_key = (
        home.to_path_buf(),
        agent_type.clone(),
        session_id.to_string(),
    );
    let items = load_session_conversation_with_workspace(home, agent_type, session_id, workspace);
    let fresh_fingerprints = conversation_cache()
        .read()
        .ok()
        .and_then(|guard| {
            guard
                .get(&cache_key)
                .map(|entry| entry.fingerprints.clone())
        })
        .unwrap_or_default();

    Some((fresh_fingerprints, items))
}

/// Loads a bounded conversation preview for one session with optional workspace path
/// to allow fast O(1) transcript lookup without recursive directory scans.
pub fn load_session_conversation_with_workspace(
    home: &Path,
    agent_type: AgentType,
    session_id: &str,
    workspace: Option<&Path>,
) -> Vec<Value> {
    if validate_session_id(session_id).is_err() {
        return Vec::new();
    }
    // Remote sessions often arrive prefixed with provider namespaces (e.g. claude_<uuid>, antigravity_<id>, grok_<id>).
    // Strip provider-specific prefixes so on-disk transcript matching finds the raw file name / UUID.
    let raw_session_id = session_id
        .strip_prefix("claude_")
        .or_else(|| session_id.strip_prefix("antigravity_"))
        .unwrap_or(session_id);

    if let AgentType::Plugin(ref plugin_id) = agent_type {
        return crate::plugins::load_plugin_conversation(plugin_id, session_id, workspace);
    }

    let cache_key = (
        home.to_path_buf(),
        agent_type.clone(),
        session_id.to_string(),
    );
    if let Ok(guard) = conversation_cache().read()
        && let Some(entry) = guard.get(&cache_key)
        && !entry.fingerprints.is_empty()
        && entry.fingerprints.iter().all(|(p, c_len, c_mtime)| {
            let meta = fs::metadata(p).ok();
            let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = meta.and_then(|m| m.modified().ok());
            len == *c_len && mtime == *c_mtime
        })
    {
        return entry.messages.clone();
    }

    let mut paths = Vec::new();
    match agent_type {
        AgentType::Codex => {
            let codex_root = home.join(".codex");
            ensure_codex_path_index(&codex_root);
            if let Ok(guard) = codex_path_index().read()
                && let Some((_, ref cached_root, ref list)) = *guard
                && cached_root == &codex_root
            {
                for (name, p) in list {
                    if codex_rollout_matches_session_id(name, raw_session_id) && p.is_file() {
                        paths.push(p.clone());
                    }
                }
            }
            if paths.is_empty() {
                let sessions_dir = codex_root.join("sessions");
                let archived_dir = codex_root.join("archived_sessions");
                if sessions_dir.is_dir() {
                    collect_provider_transcripts(&sessions_dir, raw_session_id, 0, &mut paths);
                }
                if archived_dir.is_dir() {
                    collect_provider_transcripts(&archived_dir, raw_session_id, 0, &mut paths);
                }
            }
            if paths.is_empty() {
                // Only scan direct jsonl files in codex_root without recursively traversing subdirectories
                if let Ok(entries) = fs::read_dir(&codex_root) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file()
                            && path.extension().and_then(|v| v.to_str()) == Some("jsonl")
                        {
                            let name = path
                                .file_name()
                                .and_then(|v| v.to_str())
                                .unwrap_or_default();
                            if codex_rollout_matches_session_id(name, raw_session_id) {
                                paths.push(path);
                            }
                        }
                    }
                }
            }
        }
        AgentType::ClaudeCode | AgentType::ClaudeDesktop => {
            if let Some(ws) = workspace {
                if let Some(direct) = find_claude_workspace_transcript(home, ws, raw_session_id) {
                    paths.push(direct);
                }
            }
            if paths.is_empty() {
                let projects_root = home.join(".claude").join("projects");
                ensure_claude_path_index(&projects_root);
                let mut found = false;
                if let Ok(guard) = claude_path_index().read() {
                    if let Some((_, _, ref map)) = *guard {
                        if let Some(p) = map.get(raw_session_id) {
                            if p.is_file() {
                                paths.push(p.clone());
                                found = true;
                            }
                        }
                    }
                }
                if !found {
                    collect_transcript_files(&projects_root, raw_session_id, 0, &mut paths);
                    if let Some(first) = paths.first() {
                        if let Ok(mut guard) = claude_path_index().write() {
                            if let Some((_, _, ref mut map)) = *guard {
                                map.insert(raw_session_id.to_string(), first.clone());
                            }
                        }
                    }
                }
            }
        }
        _ => paths.extend(find_agent_transcript(home, &agent_type, raw_session_id)),
    }
    paths.sort();
    paths.dedup();

    let fingerprints: Vec<(PathBuf, u64, Option<SystemTime>)> = paths
        .iter()
        .map(|p| {
            let meta = fs::metadata(p).ok();
            let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = meta.and_then(|m| m.modified().ok());
            (p.clone(), len, mtime)
        })
        .collect();

    if let Ok(guard) = conversation_cache().read()
        && let Some(entry) = guard.get(&cache_key)
        && entry.fingerprints == fingerprints
    {
        return entry.messages.clone();
    }
    let mut omitted = paths.len() > 8;
    if paths.len() > 8 {
        let tail = paths.split_off(paths.len() - 4);
        paths.truncate(4);
        paths.extend(tail);
    }
    let multi_source = paths.len() > 1;
    let file_budget = MAX_CONVERSATION_BYTES / paths.len().max(1) as u64;
    let mut records = Vec::new();
    let mut seen = HashMap::<String, usize>::new();
    let mut seen_uuids = std::collections::HashSet::<String>::new();
    for (source, path) in paths.iter().enumerate() {
        let (values, partial) = read_transcript_windows(path, file_budget);
        omitted |= partial;
        // A similarly named rollout must not contribute a different session.
        if values.iter().any(|v| {
            v.get("type").and_then(Value::as_str) == Some("session_meta")
                && v.pointer("/payload/id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id != raw_session_id && id != session_id)
        }) {
            continue;
        }
        for value in values {
            if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            // Claude Code resume replay re-appends prior transcript entries verbatim with the same uuid.
            if let Some(uuid) = value
                .get("uuid")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                && !seen_uuids.insert(uuid.to_owned())
            {
                continue;
            }
            if multi_source {
                let key = serde_json::to_string(&value).unwrap_or_default();
                if seen.get(&key).is_some_and(|previous| *previous != source) {
                    continue;
                }
                seen.insert(key, source);
            }
            records.push(value);
        }
    }
    // Untimestamped records retain source order rather than invented chronology.
    if records
        .iter()
        .all(|v| v.get("timestamp").and_then(Value::as_str).is_some())
    {
        records.sort_by(|a, b| a["timestamp"].as_str().cmp(&b["timestamp"].as_str()));
    }
    if super::antigravity::is_antigravity_record_sequence(&records) {
        let items = super::antigravity::normalize_antigravity_records(&records);
        let output = budget_conversation(items, omitted);
        if let Ok(mut guard) = conversation_cache().write() {
            guard.insert(
                cache_key,
                CachedConversationEntry {
                    fingerprints,
                    messages: output.clone(),
                },
            );
        }
        return output;
    }

    let mut items = Vec::new();
    let mut tools = HashMap::<String, String>::new();
    let mut codex_call_map = HashMap::<String, usize>::new();
    for value in records {
        if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
            for block in blocks {
                if block["type"] == "tool_use"
                    && let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str())
                {
                    tools.insert(id.to_owned(), name.to_owned());
                }
            }
        }
        let mut normalized = normalize_local_conversation_items(&value);
        if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
            let mut results = blocks.iter().filter(|b| b["type"] == "tool_result");
            for item in normalized
                .iter_mut()
                .filter(|i| i["role"] == "system" && i["kind"] == "tool")
            {
                if let Some(block) = results.next()
                    && let Some(name) = block["tool_use_id"].as_str().and_then(|id| tools.get(id))
                {
                    item["label"] = Value::String(format!("{name} · 结果"));
                }
            }
        }
        super::codex::merge_codex_call_outputs(&mut codex_call_map, &value, &mut items, normalized);
    }
    let output = budget_conversation(items, omitted);
    if let Ok(mut guard) = conversation_cache().write() {
        guard.insert(
            cache_key,
            CachedConversationEntry {
                fingerprints,
                messages: output.clone(),
            },
        );
    }
    output
}

/// Reads the bounded head/tail windows of one transcript and returns the parsed
/// records. Lines are parsed exactly once here: callers reuse these values
/// instead of re-parsing the same JSONL text.
fn parse_window_bytes(bytes: &[u8], values: &mut Vec<Value>, invalid: &mut bool) {
    if bytes.is_empty() {
        return;
    }
    // Zero-copy UTF-8 fast path when the chunk is plain UTF-8 without BOM or UTF-16 NUL bytes.
    let maybe_utf8 = (!bytes.starts_with(&[0xff, 0xfe])
        && !bytes.starts_with(&[0xfe, 0xff])
        && !bytes.iter().take(64).any(|&b| b == 0))
    .then(|| std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)).ok())
    .flatten();
    if let Some(text) = maybe_utf8 {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(trimmed) {
                Ok(value) => values.push(value),
                Err(_) => *invalid = true,
            }
        }
        return;
    }
    let text = decode_local_text(bytes);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => values.push(value),
            Err(_) => *invalid = true,
        }
    }
}

pub(crate) fn read_transcript_windows(path: &Path, budget: u64) -> (Vec<Value>, bool) {
    let Ok(mut file) = fs::File::open(path) else {
        return (Vec::new(), true);
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let partial = len > budget;
    let head_len = if partial { budget / 4 } else { len };
    let mut head = Vec::with_capacity((head_len as usize).min(256 * 1024));
    if (&mut file).take(head_len).read_to_end(&mut head).is_err() {
        return (Vec::new(), true);
    }
    if partial {
        if let Some(end) = head.iter().rposition(|b| *b == b'\n') {
            head.truncate(end + 1);
        } else {
            head.clear();
        }
    }
    let mut invalid = partial;
    let mut values = Vec::new();
    // Parse head then free its buffer before allocating/reading the tail window.
    parse_window_bytes(&head, &mut values, &mut invalid);
    drop(head);

    if partial {
        let tail_len = budget - head_len;
        if file
            .seek(SeekFrom::Start(len.saturating_sub(tail_len)))
            .is_ok()
        {
            let mut tail = Vec::with_capacity((tail_len as usize).min(256 * 1024));
            if file.take(tail_len).read_to_end(&mut tail).is_ok()
                && let Some(start) = tail.iter().position(|b| *b == b'\n')
            {
                parse_window_bytes(&tail[start + 1..], &mut values, &mut invalid);
            }
        }
    }
    (values, invalid)
}

pub(crate) fn budget_conversation(items: Vec<Value>, omitted: bool) -> Vec<Value> {
    let items = deduplicate_consecutive_user_messages(items);
    let mut selected = std::collections::BTreeSet::new();
    let mut bytes = 2usize;
    let budget = MAX_CONVERSATION_RESPONSE_BYTES - 2048;
    let message = |item: &Value| {
        item["kind"] == "message" && (item["role"] == "user" || item["role"] == "assistant")
    };
    // Protect opening/latest natural-language turns before allocating tool output.
    let mut order = Vec::new();
    order.extend(items.iter().position(message));
    order.extend(items.iter().position(|i| message(i) && i["role"] == "user"));
    order.extend(
        items
            .iter()
            .rposition(|i| message(i) && i["role"] == "assistant"),
    );
    order.extend(
        items
            .iter()
            .rposition(|i| message(i) && i["role"] == "user"),
    );
    order.extend((0..items.len()).rev().filter(|i| message(&items[*i])));
    order.extend((0..items.len()).rev());
    for i in order {
        if selected.contains(&i) {
            continue;
        }
        let size = serde_json::to_vec(&items[i])
            .map(|s| s.len() + 1)
            .unwrap_or(budget);
        if selected.len() < MAX_CONVERSATION_MESSAGES - 1 && bytes + size <= budget {
            selected.insert(i);
            bytes += size;
        }
    }
    let truncated = selected.len() < items.len() || omitted;
    let mut output: Vec<Value> = selected.into_iter().map(|i| items[i].clone()).collect();
    if truncated {
        output.insert(output.len().min(1), conversation_item("system", "status", "历史记录折叠", "当前为有界会话预览，部分记录未载入或已省略；展示已读取的开场与最新对话，原始历史未修改。", "completed"));
    }
    output
}
