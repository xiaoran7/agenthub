//! Bounded read-only session conversation loading.
//!
//! This is the single transcript reader in the codebase: `agenthubd` owns it
//! and Desktop consumes it over local IPC. Discovery metadata stays in
//! `collectors`; this module only serves conversation previews for sessions
//! that can be resumed or are already managed.

use crate::domain::AgentType;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_SESSION_ID_CHARS: usize = 160;
const MAX_CONVERSATION_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CONVERSATION_MESSAGES: usize = 300;
/// Response budget for one IPC frame (`MAX_DAEMON_FRAME_BYTES` is 1 MiB);
/// keeps the serialized preview comfortably inside a single frame.
const MAX_CONVERSATION_RESPONSE_BYTES: usize = 512 * 1024;

pub fn validate_session_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.chars().count() > MAX_SESSION_ID_CHARS
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("invalid_session_id");
    }
    Ok(())
}

/// Loads a bounded conversation preview for one session, or an empty list when
/// no transcript exists. Never returns tool payloads beyond what the provider
/// already wrote into its own local transcript on this machine.
pub fn load_session_conversation(
    home: &Path,
    agent_type: AgentType,
    session_id: &str,
) -> Vec<Value> {
    if validate_session_id(session_id).is_err() {
        return Vec::new();
    }
    // Remote sessions often arrive prefixed with provider namespaces (e.g. claude_<uuid>, antigravity_<id>, grok_<id>).
    // Strip provider-specific prefixes so on-disk transcript matching finds the raw file name / UUID.
    let raw_session_id = session_id
        .strip_prefix("claude_")
        .or_else(|| session_id.strip_prefix("antigravity_"))
        .or_else(|| session_id.strip_prefix("grok_"))
        .unwrap_or(session_id);

    if agent_type == AgentType::ZCode {
        return load_zcode_session_conversation(home, session_id);
    }
    let mut paths = Vec::new();
    match agent_type {
        AgentType::Codex => {
            collect_provider_transcripts(&home.join(".codex"), raw_session_id, 0, &mut paths)
        }
        AgentType::ClaudeCode => collect_transcript_files(
            &home.join(".claude/projects"),
            raw_session_id,
            0,
            &mut paths,
        ),
        _ => paths.extend(find_agent_transcript(home, agent_type, raw_session_id)),
    }
    paths.sort();
    paths.dedup();
    let mut omitted = paths.len() > 8;
    if paths.len() > 8 {
        let tail = paths.split_off(paths.len() - 4);
        paths.truncate(4);
        paths.extend(tail);
    }
    let file_budget = MAX_CONVERSATION_BYTES / paths.len().max(1) as u64;
    let mut records = Vec::new();
    let mut seen = HashMap::<String, usize>::new();
    for (source, path) in paths.iter().enumerate() {
        let (values, partial) = read_transcript_windows(path, file_budget);
        omitted |= partial;
        // A similarly named rollout must not contribute a different session.
        if values.iter().any(|v| {
            (v.get("type").and_then(Value::as_str) == Some("session_meta")
                && v.pointer("/payload/id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id != raw_session_id && id != session_id))
                || v.get("sessionId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id != raw_session_id && id != session_id)
        }) {
            continue;
        }
        for value in values {
            if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let key = serde_json::to_string(&value).unwrap_or_default();
            if seen.get(&key).is_some_and(|previous| *previous != source) {
                continue;
            }
            seen.insert(key, source);
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
    let mut items = Vec::new();
    let mut tools = HashMap::<String, String>::new();
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
        items.extend(normalized);
    }
    budget_conversation(items, omitted)
}

/// Reads the bounded head/tail windows of one transcript and returns the parsed
/// records. Lines are parsed exactly once here: callers reuse these values
/// instead of re-parsing the same JSONL text.
fn read_transcript_windows(path: &Path, budget: u64) -> (Vec<Value>, bool) {
    let Ok(mut file) = fs::File::open(path) else {
        return (Vec::new(), true);
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let partial = len > budget;
    let head_len = if partial { budget / 4 } else { len };
    let mut head = Vec::new();
    if (&mut file).take(head_len).read_to_end(&mut head).is_err() {
        return (Vec::new(), true);
    }
    let mut chunks = Vec::new();
    if partial {
        if let Some(end) = head.iter().rposition(|b| *b == b'\n') {
            head.truncate(end + 1);
        } else {
            head.clear();
        }
    }
    chunks.push(head);
    if partial {
        let tail_len = budget - head_len;
        if file
            .seek(SeekFrom::Start(len.saturating_sub(tail_len)))
            .is_ok()
        {
            let mut tail = Vec::new();
            if file.take(tail_len).read_to_end(&mut tail).is_ok()
                && let Some(start) = tail.iter().position(|b| *b == b'\n')
            {
                chunks.push(tail[start + 1..].to_vec());
            }
        }
    }
    let mut invalid = partial;
    // One parse per line, reused by the caller. `invalid` still marks any line
    // that failed to parse or was cut mid-record by the window boundary.
    let values = chunks
        .into_iter()
        .flat_map(|bytes| {
            let text = decode_local_text(&bytes);
            text.lines().map(str::to_owned).collect::<Vec<_>>()
        })
        .filter_map(|line| {
            if line.trim().is_empty() {
                return None;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(value) => Some(value),
                Err(_) => {
                    invalid = true;
                    None
                }
            }
        })
        .collect();
    (values, invalid)
}

fn budget_conversation(items: Vec<Value>, omitted: bool) -> Vec<Value> {
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

fn find_agent_transcript(home: &Path, agent_type: AgentType, session_id: &str) -> Option<PathBuf> {
    let raw_session_id = session_id
        .strip_prefix("claude_")
        .or_else(|| session_id.strip_prefix("antigravity_"))
        .or_else(|| session_id.strip_prefix("grok_"))
        .unwrap_or(session_id);

    match agent_type {
        AgentType::ClaudeCode => {
            find_transcript_file(&home.join(".claude").join("projects"), raw_session_id, 0)
        }
        AgentType::Codex => find_provider_transcript(&home.join(".codex"), raw_session_id, 0),
        AgentType::Antigravity => {
            let roots = [
                home.join(".gemini").join("antigravity"),
                home.join(".antigravity"),
            ];
            roots.into_iter().find_map(|root| {
                let direct = root
                    .join("brain")
                    .join(raw_session_id)
                    .join(".system_generated")
                    .join("logs")
                    .join("transcript.jsonl");
                direct.is_file().then_some(direct)
            })
        }
        AgentType::GrokBuild => {
            find_provider_transcript(&home.join(".grok").join("sessions"), raw_session_id, 0)
        }
        AgentType::ZCode => None,
    }
}

fn load_zcode_session_conversation(home: &Path, session_id: &str) -> Vec<Value> {
    let db_path = home.join(".zcode").join("cli").join("db").join("db.sqlite");
    if !db_path.is_file() {
        return Vec::new();
    }
    let Ok(connection) = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };

    // Query the latest messages/parts in windowed subquery, then re-sort ASC for chronological preview
    let sql = r#"
        SELECT
            sub.role,
            sub.part_type,
            sub.data
        FROM (
            SELECT
                json_extract(m.data, '$.role') as role,
                json_extract(p.data, '$.type') as part_type,
                p.data,
                m.sequence as m_seq,
                p.sequence as p_seq
            FROM message m
            JOIN part p ON m.id = p.message_id
            WHERE m.session_id = ?1
            ORDER BY m.sequence DESC, p.sequence DESC
            LIMIT 600
        ) sub
        ORDER BY sub.m_seq ASC, sub.p_seq ASC
    "#;

    let Ok(mut stmt) = connection.prepare(sql) else {
        return Vec::new();
    };

    let rows = match stmt.query_map([session_id], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) {
        Ok(mapped) => mapped.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(_) => return Vec::new(),
    };

    let mut items = Vec::new();

    for (role_opt, part_type_opt, raw_data) in rows {
        let Ok(data) = serde_json::from_str::<Value>(&raw_data) else {
            continue;
        };

        // Filter out synthetic background prompts or model-only prompts (such as todo_reminder)
        if data.get("synthetic").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if let Some(visibility) = data.pointer("/metadata/visibility").and_then(Value::as_str)
            && (visibility == "model-only" || visibility == "hidden")
        {
            continue;
        }
        if let Some(source) = data.pointer("/metadata/source").and_then(Value::as_str)
            && (source == "todo_reminder" || source == "system")
        {
            continue;
        }

        let role = role_opt.as_deref().unwrap_or("assistant");
        let part_type = part_type_opt.as_deref().unwrap_or("");

        let item = match part_type {
            "text" => {
                let text = data.get("text").and_then(Value::as_str).unwrap_or("");
                let clean = clean_message_text(text);
                if clean.is_empty() {
                    continue;
                }
                conversation_item(role, "message", "", &clean, "completed")
            }
            "tool" => {
                let tool_name = data
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("工具调用");
                let state = data.get("state");
                let status = state
                    .and_then(|s| s.get("status"))
                    .and_then(Value::as_str)
                    .map(status_label)
                    .unwrap_or("completed");
                let input = state.and_then(|s| s.get("input"));
                let output = state.and_then(|s| s.get("output"));
                let label = tool_call_label(tool_name, input);
                let details = if let Some(out) = output {
                    compact_json(out)
                } else if let Some(inp) = input {
                    compact_json(inp)
                } else {
                    String::new()
                };
                conversation_item("assistant", "tool", &label, &details, status)
            }
            _ => continue,
        };

        items.push(item);
    }

    budget_conversation(items, false)
}

fn deduplicate_consecutive_user_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut deduped: Vec<Value> = Vec::with_capacity(messages.len());
    for item in messages {
        let is_user = item.get("role").and_then(Value::as_str) == Some("user");
        let content = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();

        if is_user && !content.is_empty() {
            // If the immediately preceding message is also a user message with identical content,
            // overwrite/replace it with this later attempt (or skip duplicates).
            if let Some(prev) = deduped.last_mut() {
                let prev_is_user = prev.get("role").and_then(Value::as_str) == Some("user");
                let prev_content = prev
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if prev_is_user && prev_content == content {
                    // Replace previous duplicate with the updated one
                    *prev = item;
                    continue;
                }
            }
        }
        deduped.push(item);
    }
    deduped
}

fn find_provider_transcript(root: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    let mut matches = Vec::new();
    collect_provider_transcripts(root, session_id, depth, &mut matches);
    // Sort descending by file modification time so that resumed or continued sessions
    // (e.g. Codex rollouts across days or continuation fragments) pick the newest file
    matches.sort_by(|a, b| {
        let mtime_b = b.metadata().and_then(|m| m.modified()).ok();
        let mtime_a = a.metadata().and_then(|m| m.modified()).ok();
        mtime_b.cmp(&mtime_a)
    });
    matches.into_iter().next()
}

fn collect_provider_transcripts(
    root: &Path,
    session_id: &str,
    depth: usize,
    results: &mut Vec<PathBuf>,
) {
    if depth > 7 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let parent_matches = path
                .ancestors()
                .any(|part| part.file_name().and_then(|value| value.to_str()) == Some(session_id));
            if name.contains(session_id) || parent_matches {
                results.push(path);
                continue;
            }
        }
        if path.is_dir() {
            collect_provider_transcripts(&path, session_id, depth + 1, results);
        }
    }
}

pub(crate) fn find_transcript_file(root: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    let mut matches = Vec::new();
    collect_transcript_files(root, session_id, depth, &mut matches);
    matches.sort_by(|a, b| {
        let mtime_b = b.metadata().and_then(|m| m.modified()).ok();
        let mtime_a = a.metadata().and_then(|m| m.modified()).ok();
        mtime_b.cmp(&mtime_a)
    });
    matches.into_iter().next()
}

fn collect_transcript_files(
    root: &Path,
    session_id: &str,
    depth: usize,
    results: &mut Vec<PathBuf>,
) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && path.file_stem().and_then(|value| value.to_str()) == Some(session_id)
        {
            results.push(path);
            continue;
        }
        if path.is_dir() {
            collect_transcript_files(&path, session_id, depth + 1, results);
        }
    }
}

/// Shared local text decoder (UTF-8/UTF-16 with BOM sniffing). Also used by
/// the desktop attachment reader for user-picked text files.
pub fn decode_local_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if bytes.len() >= 8 && bytes.len().is_multiple_of(2) {
        let pairs = bytes.chunks_exact(2).collect::<Vec<_>>();
        let even_nuls = pairs.iter().filter(|pair| pair[0] == 0).count();
        let odd_nuls = pairs.iter().filter(|pair| pair[1] == 0).count();
        let threshold = pairs.len() / 3;
        if odd_nuls > threshold {
            let units = pairs
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            return String::from_utf16_lossy(&units);
        }
        if even_nuls > threshold {
            let units = pairs
                .iter()
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            return String::from_utf16_lossy(&units);
        }
    }
    String::from_utf8_lossy(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)).into_owned()
}

fn local_conversation_message(value: &Value) -> Option<(&'static str, String)> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload_type = value
        .pointer("/payload/type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let raw_role = value
        .get("role")
        .or_else(|| value.pointer("/message/role"))
        .or_else(|| value.pointer("/payload/role"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| match (event_type, payload_type) {
            ("user", _) | ("USER_INPUT", _) | (_, "user_message") => "user",
            ("assistant", _) | ("ASSISTANT", _) | (_, "agent_message") => "assistant",
            (_, "message") => value
                .pointer("/payload/role")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            _ => "",
        });
    let role = match raw_role.to_ascii_lowercase().as_str() {
        "user" | "human" => "user",
        "assistant" | "agent" | "model" => "assistant",
        _ => return None,
    };
    let content = value
        .pointer("/message/content")
        .or_else(|| value.pointer("/payload/content"))
        .or_else(|| value.pointer("/payload/message"))
        .or_else(|| value.get("content"))
        .or_else(|| value.get("text"))?;
    let text = extract_local_text(content);
    (!text.trim().is_empty()).then_some((role, text))
}

fn normalize_local_conversation_items(value: &Value) -> Vec<Value> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = value.get("payload").unwrap_or(value);
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if matches!(
        event_type,
        "summary" | "file-history-snapshot" | "queue-operation"
    ) || matches!(payload_type, "turn_context" | "token_count" | "reasoning")
    {
        return Vec::new();
    }

    // Claude Code writes its auto-compact summary as a `user` record flagged
    // `isCompactSummary` + `isVisibleInTranscriptOnly`. The flag means "this
    // exists for the transcript, not for the reader" - the official client
    // never renders it. Showing it would dump the entire condensed history of
    // the session into the conversation as if the user had typed it.
    if value.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        || value
            .get("isVisibleInTranscriptOnly")
            .and_then(Value::as_bool)
            == Some(true)
    {
        return Vec::new();
    }

    // A compaction boundary carries only the savings, not the summary text.
    // Surface it as one status row so the user still sees what happened.
    if event_type == "system"
        && value.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
    {
        let saved = value
            .pointer("/compactMetadata/cumulativeDroppedTokens")
            .and_then(Value::as_u64);
        return vec![conversation_item(
            "system",
            "status",
            "上下文压缩",
            &match saved {
                Some(tokens) => format!(
                    "已自动压缩上下文，节省约 {} tokens",
                    format_token_count(tokens)
                ),
                None => "已自动压缩上下文".to_owned(),
            },
            "completed",
        )];
    }

    if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return Vec::new();
    }
    let content = value
        .pointer("/message/content")
        .or_else(|| payload.get("content"))
        .or_else(|| payload.get("message"))
        .or_else(|| value.get("content"))
        .or_else(|| value.get("text"));

    if let Some(text) = content.and_then(Value::as_str) {
        if text.contains("<local-command-caveat>") {
            return Vec::new();
        }
        if let Some(command) = xml_tag(text, "command-name") {
            return vec![conversation_item(
                "system",
                "command",
                "本地命令",
                &command,
                "completed",
            )];
        }
        if text.contains("<task-notification>") {
            let summary =
                xml_tag(text, "summary").unwrap_or_else(|| "后台任务状态已更新".to_owned());
            let status = xml_tag(text, "status").unwrap_or_default();
            return vec![conversation_item(
                "system",
                "status",
                "后台任务",
                &summary,
                status_label(&status),
            )];
        }
    }

    let role = normalized_role(value);
    let mut items = Vec::new();

    if let Some(thinking) = value
        .get("thinking")
        .or_else(|| payload.get("thinking"))
        .or_else(|| payload.get("reasoning"))
        .and_then(Value::as_str)
    {
        let clean = clean_message_text(thinking);
        if !clean.is_empty() {
            items.push(conversation_item(
                "assistant",
                "thinking",
                "思考过程",
                &clean,
                "completed",
            ));
        }
    }

    if let Some(blocks) = content.and_then(Value::as_array) {
        let mut text_parts = Vec::new();
        let mut pending_tools: HashMap<String, usize> = HashMap::new();
        for block in blocks {
            let block_type = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match block_type {
                "thinking" | "thought" | "reasoning" => {
                    let text = extract_local_text(block);
                    if !text.trim().is_empty() {
                        items.push(conversation_item(
                            "assistant",
                            "thinking",
                            "思考过程",
                            &text,
                            "completed",
                        ));
                    }
                }
                "text" | "input_text" | "output_text" => {
                    let text = extract_local_text(block);
                    if !text.trim().is_empty() {
                        text_parts.push(text);
                    }
                }
                "tool_use" | "tool_call" | "function_call" | "custom_tool_call" => {
                    let name = block
                        .get("name")
                        .or_else(|| block.get("tool_name"))
                        .and_then(Value::as_str)
                        .unwrap_or("工具调用");
                    let input = block.get("input").or_else(|| block.get("arguments"));
                    let label = tool_call_label(name, input);
                    let details = input.map(compact_json).unwrap_or_default();
                    // Transcripts are historical records: a persisted tool call is
                    // done unless its result below reports an error.
                    let index = items.len();
                    items.push(conversation_item(
                        "assistant",
                        "tool",
                        &label,
                        &details,
                        "completed",
                    ));
                    if let Some(id) = block.get("id").and_then(Value::as_str) {
                        pending_tools.insert(id.to_owned(), index);
                    }
                }
                "tool_result" | "function_call_output" | "custom_tool_call_output" => {
                    let details = block
                        .get("content")
                        .or_else(|| block.get("output"))
                        .map(extract_local_text)
                        .unwrap_or_default();
                    let failed = block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let matched = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .and_then(|id| pending_tools.get(id).copied());
                    if let Some(index) = matched {
                        if !details.trim().is_empty() {
                            let merged = format!(
                                "{}\n\n〔结果〕\n{}",
                                items[index]["content"].as_str().unwrap_or_default(),
                                details.trim()
                            );
                            items[index]["content"] = Value::String(merged);
                        }
                        if failed {
                            items[index]["status"] = Value::String("failed".to_owned());
                        }
                    } else if !details.trim().is_empty() {
                        items.push(conversation_item(
                            "system",
                            "tool",
                            "工具结果",
                            &details,
                            if failed { "failed" } else { "completed" },
                        ));
                    }
                }
                _ => {}
            }
        }
        if let Some(role) = role {
            let text = clean_message_text(&text_parts.join("\n"));
            if !text.is_empty() {
                items.insert(
                    0,
                    conversation_item(role, "message", "", &text, "completed"),
                );
            }
        }
    }

    let tool_type = matches!(
        payload_type,
        "function_call"
            | "custom_tool_call"
            | "commandExecution"
            | "command_execution"
            | "fileChange"
            | "file_change"
    );
    if tool_type {
        let named = payload
            .get("name")
            .or_else(|| payload.get("command"))
            .and_then(Value::as_str);
        let label = match named {
            Some(name) => tool_call_label(name, payload.get("arguments")),
            None if payload_type.contains("file") => "文件变更".to_owned(),
            None => "工具调用".to_owned(),
        };
        let details = payload
            .get("arguments")
            .or_else(|| payload.get("aggregatedOutput"))
            .or_else(|| payload.get("aggregated_output"))
            .or_else(|| payload.get("changes"))
            .map(compact_json)
            .unwrap_or_default();
        // Persisted rollout records are historical: an unfinished marker in an
        // old rollout means the record was cut, not that work is still running.
        let status = match payload
            .get("status")
            .and_then(Value::as_str)
            .map(status_label)
            .unwrap_or("unknown")
        {
            "running" | "unknown" => "completed",
            other => other,
        };
        items.push(conversation_item(
            "assistant",
            "tool",
            &label,
            &details,
            status,
        ));
    }
    if matches!(
        payload_type,
        "function_call_output" | "custom_tool_call_output"
    ) {
        let details = payload
            .get("output")
            .or_else(|| payload.get("content"))
            .map(extract_local_text)
            .unwrap_or_default();
        items.push(conversation_item(
            "system",
            "tool",
            "工具结果",
            &details,
            "completed",
        ));
    }

    if let Some(calls) = value
        .get("tool_calls")
        .or_else(|| payload.get("tool_calls"))
        .and_then(Value::as_array)
    {
        for call in calls {
            let function = call.get("function").unwrap_or(call);
            let label = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("工具调用");
            let details = function
                .get("arguments")
                .or_else(|| call.get("input"))
                .map(compact_json)
                .unwrap_or_default();
            items.push(conversation_item(
                "assistant",
                "tool",
                label,
                &details,
                "completed",
            ));
        }
    }

    if items.is_empty()
        && let Some((role, text)) = local_conversation_message(value)
    {
        let text = clean_message_text(&text);
        if !text.is_empty() {
            items.push(conversation_item(role, "message", "", &text, "completed"));
        }
    }
    items
}

fn normalized_role(value: &Value) -> Option<&'static str> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload_type = value
        .pointer("/payload/type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let role = value
        .get("role")
        .or_else(|| value.pointer("/message/role"))
        .or_else(|| value.pointer("/payload/role"))
        .and_then(Value::as_str)
        .unwrap_or(match (event_type, payload_type) {
            ("user", _) | ("USER_INPUT", _) | (_, "user_message") | (_, "userMessage") => "user",
            ("assistant", _) | ("ASSISTANT", _) | (_, "agent_message") | (_, "agentMessage") => {
                "assistant"
            }
            _ => "",
        });
    match role.to_ascii_lowercase().as_str() {
        "user" | "human" => Some("user"),
        "assistant" | "agent" | "model" => Some("assistant"),
        _ => None,
    }
}

fn conversation_item(role: &str, kind: &str, label: &str, content: &str, status: &str) -> Value {
    let trimmed = content.trim();
    let max_len = if matches!(kind, "tool" | "status" | "thinking") {
        3_500
    } else {
        16_000
    };
    let final_content = if trimmed.chars().count() > max_len {
        let head: String = trimmed.chars().take(max_len / 2).collect();
        let tail: String = trimmed
            .chars()
            .rev()
            .take(max_len / 2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("{head}\n\n〔内容过长，已省略中段，保留首尾〕\n\n{tail}")
    } else {
        trimmed.to_owned()
    };
    json!({ "role": role, "kind": kind, "label": label, "status": status,
        "content": final_content })
}

/// Summarizes a tool call as `Name: key argument` so collapsed rows stay
/// scannable without expanding the payload.
fn tool_call_label(name: &str, input: Option<&Value>) -> String {
    const KEY_FIELDS: [&str; 12] = [
        "command",
        "file_path",
        "path",
        "notebook_path",
        "url",
        "pattern",
        "query",
        "prompt",
        "description",
        "skill",
        "instruction",
        "message",
    ];
    let detail = input.and_then(|input| {
        KEY_FIELDS
            .iter()
            .find_map(|field| {
                input
                    .get(field)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                input
                    .get("questions")
                    .and_then(Value::as_array)
                    .and_then(|qs| qs.first())
                    .and_then(|q| q.get("question").or_else(|| q.get("header")))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                input
                    .get("todos")
                    .and_then(Value::as_array)
                    .map(|todos| format!("{} 项待办", todos.len()))
            })
    });
    match detail {
        Some(detail) if detail.chars().count() > 96 => {
            format!("{name}: {}…", detail.chars().take(96).collect::<String>())
        }
        Some(detail) => format!("{name}: {detail}"),
        None => name.to_owned(),
    }
}

fn compact_json(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default())
}

/// Renders a token count the way the official client does: thousands
/// separated, with a `k` suffix past a million so the status row stays short.
fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}k", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

fn status_label(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "completed" | "complete" | "success" | "succeeded" => "completed",
        "failed" | "error" | "declined" => "failed",
        "running" | "inprogress" | "in_progress" | "pending" => "running",
        _ => "unknown",
    }
}

fn xml_tag(text: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = text.find(&start_tag)? + start_tag.len();
    let end = text[start..].find(&end_tag)? + start;
    Some(text[start..end].trim().to_owned())
}

fn clean_message_text(text: &str) -> String {
    let mut text = text
        .replace("<USER_REQUEST>", "")
        .replace("</USER_REQUEST>", "");

    if text
        .trim_start()
        .starts_with("Base directory for this skill:")
        || text.trim_start().starts_with("<local-command-stdout>")
        || text.trim_start().starts_with("<local-command-stderr>")
        || text.contains("<local-command-caveat>")
    {
        return String::new();
    }

    // 1. Filter out injected system blocks embedded inside a message
    // Strip <recommended_plugins>...</recommended_plugins>
    while let Some(start) = text.find("<recommended_plugins>") {
        if let Some(end) = text[start..].find("</recommended_plugins>") {
            text.replace_range(start..start + end + "</recommended_plugins>".len(), "");
        } else {
            text.truncate(start);
            break;
        }
    }

    // Strip <environment_context>...</environment_context>
    while let Some(start) = text.find("<environment_context>") {
        if let Some(end) = text[start..].find("</environment_context>") {
            text.replace_range(start..start + end + "</environment_context>".len(), "");
        } else {
            text.truncate(start);
            break;
        }
    }

    // Strip <system-reminder>...</system-reminder>
    while let Some(start) = text.find("<system-reminder>") {
        if let Some(end) = text[start..].find("</system-reminder>") {
            text.replace_range(start..start + end + "</system-reminder>".len(), "");
        } else {
            text.truncate(start);
            break;
        }
    }

    // Strip <instructions>...</instructions>
    while let Some(start) = text.find("<INSTRUCTIONS>") {
        if let Some(end) = text[start..].find("</INSTRUCTIONS>") {
            text.replace_range(start..start + end + "</INSTRUCTIONS>".len(), "");
        } else {
            text.truncate(start);
            break;
        }
    }

    // Strip '# AGENTS.md instructions for ...'
    if let Some(pos) = text.find("# AGENTS.md instructions for") {
        let rest = &text[pos..];
        if let Some(newline_pos) = rest.find("\n\n") {
            text.replace_range(pos..pos + newline_pos + 2, "");
        } else {
            text.truncate(pos);
        }
    }

    // 2. Clean up empty lines or residual whitespace, then check if it's pure system prompt
    let cleaned = text.trim();
    if is_pure_system_prompt(cleaned) {
        return String::new();
    }

    cleaned.to_owned()
}

fn is_pure_system_prompt(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }

    // Automated todo reminders
    if text.starts_with("The TodoWrite tool hasn't been used recently") {
        return true;
    }

    // Codex plugin recommendations block
    if text.starts_with("<recommended_plugins>")
        || text.contains("Here is a list of plugins that are available but not installed.")
    {
        return true;
    }

    // Codex AGENTS.md rule injection
    if text.starts_with("# AGENTS.md instructions for")
        || (text.starts_with("<INSTRUCTIONS>") && text.ends_with("</INSTRUCTIONS>"))
    {
        return true;
    }

    // Codex / runner environment context block
    if (text.starts_with("<environment_context>") && text.ends_with("</environment_context>"))
        || (text.contains("<cwd>") && text.contains("<shell>") && text.contains("<timezone>"))
    {
        return true;
    }

    // ZCode system reminder block
    if (text.starts_with("<system-reminder>") && text.ends_with("</system-reminder>"))
        || text.starts_with("The following skills are available for use with the Skill tool:")
        || (text.contains("<untrusted_objective>")
            && text.contains("Budget:\n- Time spent pursuing goal"))
    {
        return true;
    }

    // ZCode internal automated tool execution transcript injected into user turn
    if text.starts_with("Called the Read tool with the following input:")
        || text.starts_with("Called the Bash tool with the following input:")
        || text.starts_with("Called the Edit tool with the following input:")
    {
        return true;
    }

    false
}

fn extract_local_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .filter(|item| {
                !matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("thinking" | "reasoning" | "tool_call" | "tool_result")
                )
            })
            .map(extract_local_text)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
    }
    ["text", "content", "output_text", "input_text", "message"]
        .into_iter()
        .find_map(|field| value.get(field).map(extract_local_text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_validation_rejects_paths() {
        assert!(validate_session_id("019abc-session_1").is_ok());
        assert!(validate_session_id("../auth.json").is_err());
        assert!(validate_session_id("bad\nvalue").is_err());
    }

    #[test]
    fn tool_call_label_extracts_key_arguments_and_questions() {
        let bash_input = json!({"command": "cargo test -p runner"});
        assert_eq!(
            tool_call_label("Bash", Some(&bash_input)),
            "Bash: cargo test -p runner"
        );

        let question_input = json!({
            "questions": [
                {
                    "question": "Which framework to use?",
                    "header": "Framework"
                }
            ]
        });
        assert_eq!(
            tool_call_label("AskUserQuestion", Some(&question_input)),
            "AskUserQuestion: Which framework to use?"
        );

        let todo_input = json!({
            "todos": ["task 1", "task 2", "task 3"]
        });
        assert_eq!(
            tool_call_label("TodoWrite", Some(&todo_input)),
            "TodoWrite: 3 项待办"
        );
    }

    #[test]
    fn local_conversation_parser_supports_provider_native_message_shapes() {
        let claude = json!({"type":"user","message":{"content":[{"type":"text","text":"你好"}]}});
        let codex = json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"已完成"}]}});
        let antigravity =
            json!({"type":"USER_INPUT","content":"<USER_REQUEST>修复乱码</USER_REQUEST>"});

        assert_eq!(
            local_conversation_message(&claude),
            Some(("user", "你好".into()))
        );
        assert_eq!(
            local_conversation_message(&codex),
            Some(("assistant", "已完成".into()))
        );
        assert_eq!(
            local_conversation_message(&antigravity),
            Some(("user", "<USER_REQUEST>修复乱码</USER_REQUEST>".into()))
        );
    }

    #[test]
    fn compact_summary_never_reaches_the_conversation() {
        // Exactly what Claude Code writes when it auto-compacts: a `user`
        // record holding the condensed history of everything said so far.
        let summary = json!({
            "type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,
            "turnOrigin":"sdk","message":{"role":"user","content":
            "This session is being continued from a previous conversation that ran out of context.\n\nSummary:\n1. Primary Request and Intent:\n   - SECRET_CONDENSED_HISTORY"}
        });
        let boundary = json!({
            "type":"system","subtype":"compact_boundary","level":"info",
            "content":"Conversation compacted",
            "compactMetadata":{"trigger":"auto","preTokens":177701,"postTokens":14111,
                "cumulativeDroppedTokens":163590,"durationMs":104470}
        });

        assert!(normalize_local_conversation_items(&summary).is_empty());

        // The boundary itself is a status row, not leaked content.
        let items = normalize_local_conversation_items(&boundary);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["kind"], "status");
        assert_eq!(items[0]["role"], "system");
        assert_eq!(items[0]["label"], "上下文压缩");
        assert!(items[0]["content"].as_str().unwrap().contains("163k"));
        assert!(
            !items[0]["content"]
                .as_str()
                .unwrap()
                .contains("SECRET_CONDENSED_HISTORY")
        );

        // A boundary without a reported token count still renders.
        let bare = json!({"type":"system","subtype":"compact_boundary","content":"Conversation compacted"});
        assert_eq!(normalize_local_conversation_items(&bare).len(), 1);

        // An ordinary user turn is unaffected by the filter.
        let prompt = json!({"type":"user","message":{"content":"你好"}});
        assert_eq!(normalize_local_conversation_items(&prompt).len(), 1);
    }

    #[test]
    fn token_counts_format_like_the_official_client() {
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1_000), "1k");
        assert_eq!(format_token_count(163_590), "163k");
        assert_eq!(format_token_count(1_500_000), "1.5M");
    }

    #[test]
    fn local_conversation_normalizer_filters_control_markup_and_keeps_tools() {
        let caveat = json!({"type":"user","message":{"content":"<local-command-caveat>internal only</local-command-caveat>"}});
        let command = json!({"type":"user","message":{"content":"<command-name>/compact</command-name><command-message>compact</command-message>"}});
        let claude = json!({"type":"assistant","message":{"content":[
            {"type":"text","text":"- 完成一项\n- 完成二项"},
            {"type":"tool_use","name":"Read","input":{"file_path":"README.md"}}
        ]}});
        let codex = json!({"type":"response_item","payload":{"type":"command_execution","command":"cargo test","status":"completed","aggregated_output":"ok"}});

        assert!(normalize_local_conversation_items(&caveat).is_empty());
        assert_eq!(
            normalize_local_conversation_items(&command)[0]["kind"],
            "command"
        );
        let claude_items = normalize_local_conversation_items(&claude);
        assert_eq!(claude_items[0]["kind"], "message");
        assert_eq!(claude_items[1]["kind"], "tool");
        assert_eq!(
            normalize_local_conversation_items(&codex)[0]["label"],
            "cargo test"
        );
    }

    #[test]
    fn local_text_decoder_handles_utf8_and_utf16_without_mojibake() {
        assert_eq!(decode_local_text("会话内容".as_bytes()), "会话内容");
        let mut utf16 = vec![0xff, 0xfe];
        for unit in "会话内容".encode_utf16() {
            utf16.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_local_text(&utf16), "会话内容");
        let utf16_without_bom = "session 内容"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(decode_local_text(&utf16_without_bom), "session 内容");
    }

    #[test]
    fn load_session_conversation_caps_messages_and_returns_empty_without_transcript() {
        let home = tempfile::tempdir().unwrap();
        let codex_dir = home.path().join(".codex").join("sessions");
        fs::create_dir_all(&codex_dir).unwrap();
        let mut rollout = String::new();
        for index in 0..400 {
            rollout.push_str(&format!(
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"msg-{index}\"}}]}}}}\n"
            ));
        }
        fs::write(codex_dir.join("thread-abc.jsonl"), rollout).unwrap();

        let messages = load_session_conversation(home.path(), AgentType::Codex, "thread-abc");
        assert_eq!(messages.len(), 300);
        // Head-and-tail budgeting retains the initial head (msg-0) and latest tail (msg-399)
        assert_eq!(messages[0]["content"], "msg-0");
        assert_eq!(messages[299]["content"], "msg-399");
        assert!(load_session_conversation(home.path(), AgentType::Codex, "missing").is_empty());
        assert!(load_session_conversation(home.path(), AgentType::Codex, "../escape").is_empty());
    }

    #[test]
    fn load_session_conversation_stays_inside_one_ipc_frame() {
        let home = tempfile::tempdir().unwrap();
        let codex_dir = home.path().join(".codex").join("sessions");
        fs::create_dir_all(&codex_dir).unwrap();
        let mut rollout = String::new();
        for _ in 0..40 {
            rollout.push_str(&format!(
                "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{}\"}}]}}}}\n",
                "x".repeat(60_000)
            ));
        }
        fs::write(codex_dir.join("thread-big.jsonl"), rollout).unwrap();

        let messages = load_session_conversation(home.path(), AgentType::Codex, "thread-big");
        let approx: usize = messages
            .iter()
            .map(|item| item["content"].as_str().map(str::len).unwrap_or_default() + 128)
            .sum();
        assert!(messages.len() < 40);
        assert!(approx < 600 * 1024);
        // Head item is preserved even when the transcript is huge
        assert!(!messages.is_empty());
        assert_eq!(messages[0]["role"], "assistant");
    }

    #[test]
    fn load_session_conversation_claude_preserves_opening_prompt() {
        let home = tempfile::tempdir().unwrap();
        let claude_dir = home.path().join(".claude").join("projects").join("proj-1");
        fs::create_dir_all(&claude_dir).unwrap();

        let mut lines = Vec::new();
        // Opening prompt
        lines.push(
            r#"{"type":"user","message":{"role":"user","content":"请开始重构代码架构"}}"#
                .to_string(),
        );
        // Lots of tool calls and outputs in between
        for i in 0..100 {
            lines.push(format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"call_{i}","name":"Read","input":{{"file_path":"src/lib.rs"}}}}]}}}}"#
            ));
            lines.push(format!(
                r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"call_{i}","content":"{}"}}]}}}}"#,
                "output_data_".repeat(200)
            ));
        }
        // Latest assistant turn
        lines.push(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"重构已顺利完成！"}]}}"#.to_string());

        fs::write(claude_dir.join("sess-test.jsonl"), lines.join("\n")).unwrap();

        let messages = load_session_conversation(home.path(), AgentType::ClaudeCode, "sess-test");
        assert!(!messages.is_empty());
        // Opening prompt is ALWAYS preserved at the head!
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "请开始重构代码架构");
        // Latest turn is at the tail
        assert_eq!(messages.last().unwrap()["role"], "assistant");
        assert_eq!(messages.last().unwrap()["content"], "重构已顺利完成！");
    }

    #[test]
    fn clean_message_text_filters_codex_and_zcode_system_prompts() {
        let plugins = "<recommended_plugins>\n- Airtable\n- Granola\n</recommended_plugins>";
        assert!(clean_message_text(plugins).is_empty());

        let agents_md = "# AGENTS.md instructions for D:\\Claudespace\n\n<INSTRUCTIONS>\n遵循 AGENT.md\n</INSTRUCTIONS>";
        assert!(clean_message_text(agents_md).is_empty());

        let env_ctx = "<environment_context>\n  <cwd>D:\\Claudespace</cwd>\n  <shell>powershell</shell>\n  <timezone>Asia/Shanghai</timezone>\n</environment_context>";
        assert!(clean_message_text(env_ctx).is_empty());

        let mixed = format!("{plugins}\n\n{agents_md}\n\n{env_ctx}\n\n真实用户提问：请修复Bug");
        assert_eq!(clean_message_text(&mixed), "真实用户提问：请修复Bug");

        let zcode_reminder = "<system-reminder>\nContinue working toward the active session goal.\n<untrusted_objective>\n任务\n</untrusted_objective>\nBudget:\n- Time spent pursuing goal: 0 seconds\n</system-reminder>";
        assert!(clean_message_text(zcode_reminder).is_empty());

        let zcode_tool_inject = "Called the Read tool with the following input: {\"file_path\":\"test.rs\"}\nResult of calling the Read tool:\n1\tfn main() {}";
        assert!(clean_message_text(zcode_tool_inject).is_empty());
    }

    #[test]
    fn find_provider_transcript_prefers_latest_modified_continuation_file() {
        let home = tempfile::tempdir().unwrap();
        let codex_dir = home.path().join(".codex").join("sessions");
        let day1 = codex_dir.join("2026").join("09").join("15");
        let day2 = codex_dir.join("2026").join("09").join("17");
        fs::create_dir_all(&day1).unwrap();
        fs::create_dir_all(&day2).unwrap();

        let session_id = "01a0a555-5821-71b2-9ce6-efffaffccf8d";
        let file1 = day1.join(format!("rollout-2026-09-15T21-50-28-{session_id}.jsonl"));
        let file2 = day2.join(format!(
            "rollout-2026-09-17T10-18-25-{session_id}_continuation.jsonl"
        ));

        fs::write(&file1, "{\"type\":\"first\"}\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        fs::write(&file2, "{\"type\":\"latest\"}\n").unwrap();

        let matched = find_provider_transcript(&home.path().join(".codex"), session_id, 0);
        assert_eq!(matched, Some(file2));
    }

    #[test]
    fn test_real_claude_sessions_loading() {
        let Some(home) = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
        else {
            return;
        };
        let entries = crate::collectors::collect_claude_sessions_for_remote().unwrap();
        println!("Collected {} claude sessions for remote", entries.len());
        let mut empty_count = 0;
        let mut ok_count = 0;
        for (id, _cwd, _title) in entries.iter() {
            let prefixed = format!("claude_{}", id);
            let messages = load_session_conversation(&home, AgentType::ClaudeCode, &prefixed);
            if messages.is_empty() {
                empty_count += 1;
            } else {
                ok_count += 1;
            }
        }
        println!("WITH PREFIXED ID: ok={}, empty={}", ok_count, empty_count);
        assert!(
            ok_count > 30,
            "Prefixed session IDs must now resolve successfully!"
        );
    }
}

#[cfg(test)]
#[path = "conversation_regression.rs"]
mod regression;
