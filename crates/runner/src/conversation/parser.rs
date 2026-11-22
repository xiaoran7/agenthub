use super::*;

/// Shared local text decoder (UTF-8/UTF-16 with BOM sniffing). Also used by
/// the desktop attachment reader for user-picked text files.
#[allow(unknown_lints)]
#[allow(clippy::chunks_exact_to_as_chunks)]
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

pub(crate) fn local_conversation_message(value: &Value) -> Option<(&'static str, String)> {
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
            ("assistant", _)
            | ("ASSISTANT", _)
            | ("PLANNER_RESPONSE", _)
            | (_, "agent_message") => "assistant",
            ("GENERIC", _) => "",
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

pub(crate) fn normalize_local_conversation_items(value: &Value) -> Vec<Value> {
    if let Some(items) = super::antigravity::normalize_antigravity_step(value) {
        return items;
    }

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
    ) || matches!(payload_type, "turn_context" | "token_count")
    {
        return Vec::new();
    }
    if payload_type == "reasoning" {
        return super::codex::extract_codex_reasoning_item(payload)
            .into_iter()
            .collect();
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

    // Claude Code writes synthetic rate-limit or API error messages into the transcript
    // (e.g. `isApiErrorMessage: true`, `error: "rate_limit"`, `apiErrorStatus: 429`).
    // Never surface them as normal assistant dialogue bubbles to avoid confusing users.
    if value.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true)
        || value.pointer("/message/model").and_then(Value::as_str) == Some("<synthetic>")
    {
        let raw_content = value
            .pointer("/message/content")
            .or_else(|| value.get("content"));
        let error_text = raw_content.map(extract_local_text).unwrap_or_default();
        if error_text.contains("No response requested") {
            return Vec::new();
        }
        if !error_text.trim().is_empty() {
            return vec![conversation_item(
                "system",
                "status",
                "API 异常记录",
                &format!("历史请求异常: {}", error_text.trim()),
                "failed",
            )];
        }
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
        let (patch_title, details) = super::codex::format_codex_tool_payload(payload);
        let named = payload
            .get("name")
            .or_else(|| payload.get("command"))
            .and_then(Value::as_str);
        let label = if let Some(pt) = patch_title {
            pt
        } else {
            match named {
                Some(name) => tool_call_label(
                    name,
                    payload.get("arguments").or_else(|| payload.get("input")),
                ),
                None if payload_type.contains("file") => "文件变更".to_owned(),
                None => "工具调用".to_owned(),
            }
        };
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

pub(crate) fn normalized_role(value: &Value) -> Option<&'static str> {
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

pub(crate) fn conversation_item(
    role: &str,
    kind: &str,
    label: &str,
    content: &str,
    status: &str,
) -> Value {
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
        "content": final_content, "text": final_content })
}

/// Summarizes a tool call as `Name: key argument` so collapsed rows stay
/// scannable without expanding the payload.
pub(crate) fn tool_call_label(name: &str, input: Option<&Value>) -> String {
    const KEY_FIELDS: [&str; 22] = [
        "command",
        "CommandLine",
        "cmd",
        "file_path",
        "Path",
        "TargetFile",
        "AbsolutePath",
        "path",
        "SearchDirectory",
        "notebook_path",
        "url",
        "Url",
        "pattern",
        "query",
        "Query",
        "prompt",
        "Prompt",
        "Role",
        "description",
        "skill",
        "instruction",
        "message",
    ];
    let parsed_str_obj = input
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    let effective_input = parsed_str_obj.as_ref().or(input);
    let detail = effective_input.and_then(|input| {
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

pub(crate) fn compact_json(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default())
}

/// Renders a token count the way the official client does: thousands
/// separated, with a `k` suffix past a million so the status row stays short.
pub(crate) fn format_token_count(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}k", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

pub(crate) fn status_label(status: &str) -> &'static str {
    match status.to_ascii_lowercase().as_str() {
        "completed" | "complete" | "success" | "succeeded" => "completed",
        "failed" | "error" | "declined" => "failed",
        "running" | "inprogress" | "in_progress" | "pending" => "running",
        _ => "unknown",
    }
}

pub(crate) fn xml_tag(text: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");
    let start = text.find(&start_tag)? + start_tag.len();
    let end = text[start..].find(&end_tag)? + start;
    Some(text[start..end].trim().to_owned())
}

pub(crate) fn clean_message_text(text: &str) -> String {
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

pub(crate) fn is_pure_system_prompt(text: &str) -> bool {
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

pub(crate) fn extract_local_text(value: &Value) -> String {
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
