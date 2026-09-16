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
    io::Read,
    path::{Path, PathBuf},
};

const MAX_SESSION_ID_CHARS: usize = 160;
const MAX_CONVERSATION_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CONVERSATION_LINES: usize = 2_000;
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
    if agent_type == AgentType::ZCode {
        return load_zcode_session_conversation(home, session_id);
    }
    let Some(path) = find_agent_transcript(home, agent_type, session_id) else {
        return Vec::new();
    };
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_CONVERSATION_BYTES)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Vec::new();
    }
    let content = decode_local_text(&bytes);
    let mut messages = Vec::new();
    let mut approx_bytes = 0usize;
    for line in content.lines().take(MAX_CONVERSATION_LINES) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        for item in normalize_local_conversation_items(&value) {
            approx_bytes += item
                .get("content")
                .and_then(Value::as_str)
                .map(str::len)
                .unwrap_or_default()
                + 128;
            messages.push(item);
            if messages.len() >= MAX_CONVERSATION_MESSAGES
                || approx_bytes >= MAX_CONVERSATION_RESPONSE_BYTES
            {
                return messages;
            }
        }
    }
    messages
}

fn find_agent_transcript(home: &Path, agent_type: AgentType, session_id: &str) -> Option<PathBuf> {
    match agent_type {
        AgentType::ClaudeCode => {
            find_transcript_file(&home.join(".claude").join("projects"), session_id, 0)
        }
        AgentType::Codex => find_provider_transcript(&home.join(".codex"), session_id, 0),
        AgentType::Antigravity => {
            let roots = [
                home.join(".gemini").join("antigravity"),
                home.join(".antigravity"),
            ];
            roots.into_iter().find_map(|root| {
                let direct = root
                    .join("brain")
                    .join(session_id)
                    .join(".system_generated")
                    .join("logs")
                    .join("transcript.jsonl");
                direct.is_file().then_some(direct)
            })
        }
        AgentType::GrokBuild => {
            find_provider_transcript(&home.join(".grok").join("sessions"), session_id, 0)
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

    let sql = r#"
        SELECT
            json_extract(m.data, '$.role') as role,
            json_extract(p.data, '$.type') as part_type,
            p.data
        FROM message m
        JOIN part p ON m.id = p.message_id
        WHERE m.session_id = ?1
        ORDER BY m.sequence ASC, p.sequence ASC
        LIMIT 300
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
    let mut approx_bytes = 0usize;

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

        approx_bytes += item
            .get("content")
            .and_then(Value::as_str)
            .map(str::len)
            .unwrap_or_default()
            + 128;
        items.push(item);
        if items.len() >= MAX_CONVERSATION_MESSAGES
            || approx_bytes >= MAX_CONVERSATION_RESPONSE_BYTES
        {
            break;
        }
    }

    items
}

fn find_provider_transcript(root: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    if depth > 7 {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
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
                return Some(path);
            }
        }
        if path.is_dir()
            && let Some(found) = find_provider_transcript(&path, session_id, depth + 1)
        {
            return Some(found);
        }
    }
    None
}

fn find_transcript_file(root: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    if depth > 3 {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && path.file_stem().and_then(|value| value.to_str()) == Some(session_id)
        {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_transcript_file(&path, session_id, depth + 1)
        {
            return Some(found);
        }
    }
    None
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
    json!({ "role": role, "kind": kind, "label": label, "status": status,
        "content": content.trim().chars().take(16_000).collect::<String>() })
}

/// Summarizes a tool call as `Name: key argument` so collapsed rows stay
/// scannable without expanding the payload.
fn tool_call_label(name: &str, input: Option<&Value>) -> String {
    const KEY_FIELDS: [&str; 10] = [
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
    ];
    let detail = input.and_then(|input| {
        KEY_FIELDS.iter().find_map(|field| {
            input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
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
    let text = text
        .replace("<USER_REQUEST>", "")
        .replace("</USER_REQUEST>", "");
    if text.contains("<local-command-caveat>") {
        return String::new();
    }
    // Filter out background automated reminders injected by runners/codex/zcode
    if text.starts_with("The TodoWrite tool hasn't been used recently") {
        return String::new();
    }
    text.trim().to_owned()
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
        assert_eq!(messages[0]["content"], "msg-0");
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
    }
}
