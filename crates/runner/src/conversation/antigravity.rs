use std::collections::VecDeque;

use super::parser::{clean_message_text, compact_json, conversation_item, tool_call_label};
use super::*;

fn decode_string_arg(val: Option<&Value>) -> Option<String> {
    let s = val?.as_str()?;
    let trimmed = s.trim();
    if trimmed.starts_with('"')
        && trimmed.ends_with('"')
        && let Ok(unquoted) = serde_json::from_str::<String>(trimmed)
    {
        return Some(unquoted);
    }
    Some(s.to_owned())
}

/// Cleans Antigravity user prompt wrappers (`<USER_REQUEST>`, `<ADDITIONAL_METADATA>`,
/// `<SYSTEM_MESSAGE>`, `<USER_SETTINGS_CHANGE>`).
pub(crate) fn clean_antigravity_user_text(raw: &str) -> String {
    let mut text = raw.to_owned();
    if let Some(start) = text.find("<USER_REQUEST>") {
        let after = start + "<USER_REQUEST>".len();
        if let Some(end_rel) = text[after..].find("</USER_REQUEST>") {
            text = text[after..after + end_rel].to_owned();
        }
    }
    for (open, close) in [
        ("<ADDITIONAL_METADATA>", "</ADDITIONAL_METADATA>"),
        ("<SYSTEM_MESSAGE>", "</SYSTEM_MESSAGE>"),
        ("<USER_SETTINGS_CHANGE>", "</USER_SETTINGS_CHANGE>"),
        ("<CONTEXT_SUMMARY>", "</CONTEXT_SUMMARY>"),
    ] {
        while let Some(start) = text.find(open) {
            if let Some(end) = text[start..].find(close) {
                text.replace_range(start..start + end + close.len(), "");
            } else {
                text.truncate(start);
                break;
            }
        }
    }
    clean_message_text(&text)
}

/// Strips Antigravity `Created At:` / `Completed At:` headers and trailing `<SYSTEM_MESSAGE>`
/// blocks from `GENERIC` tool outputs, returning `(clean_output, failed)`.
pub fn clean_antigravity_tool_output(raw: &str) -> (String, bool) {
    let mut body = raw.to_owned();
    while let Some(start) = body.find("<SYSTEM_MESSAGE>") {
        if let Some(end) = body[start..].find("</SYSTEM_MESSAGE>") {
            body.replace_range(start..start + end + "</SYSTEM_MESSAGE>".len(), "");
        } else {
            body.truncate(start);
            break;
        }
    }
    if let Some(pos) = body.find("\nThe following is a <SYSTEM_MESSAGE>") {
        body.truncate(pos);
    }

    let mut failed = false;
    let mut kept_lines = Vec::new();
    for (idx, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if idx < 4 && (trimmed.starts_with("Created At:") || trimmed.starts_with("Completed At:")) {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Command exited with code ") {
            let code_str = rest.trim_end_matches('.');
            if let Ok(code) = code_str.parse::<i32>()
                && code != 0
            {
                failed = true;
            }
        } else if trimmed.starts_with("Command failed to run:") || trimmed.starts_with("Error:") {
            failed = true;
        }
        kept_lines.push(line);
    }
    let cleaned = kept_lines.join("\n").trim().to_owned();
    (cleaned, failed)
}

/// Formats Antigravity tool call arguments, synthesizing standard unified diffs (`---`/`+++`/`@@`)
/// for `edit_file` and `write_file` so Web and Android diff viewers highlight additions and removals.
pub fn format_antigravity_tool_details(name: &str, args: Option<&Value>) -> String {
    let Some(args) = args else {
        return String::new();
    };
    match name {
        "edit_file" => {
            let path = decode_string_arg(args.get("Path").or_else(|| args.get("TargetFile")))
                .unwrap_or_else(|| "file".to_owned());
            let target = decode_string_arg(args.get("Target"));
            let replacement = decode_string_arg(args.get("Replacement"));
            if target.is_some() || replacement.is_some() {
                let old_s = target.unwrap_or_default();
                let new_s = replacement.unwrap_or_default();
                let old_lines: Vec<&str> = old_s.lines().collect();
                let new_lines: Vec<&str> = new_s.lines().collect();
                let mut diff = format!(
                    "--- a/{path}\n+++ b/{path}\n@@ -1,{} +1,{} @@\n",
                    old_lines.len().max(1),
                    new_lines.len().max(1)
                );
                for l in old_lines.into_iter().take(80) {
                    diff.push('-');
                    diff.push_str(l);
                    diff.push('\n');
                }
                for l in new_lines.into_iter().take(80) {
                    diff.push('+');
                    diff.push_str(l);
                    diff.push('\n');
                }
                return diff.trim_end().to_owned();
            }
        }
        "write_file" => {
            let path = decode_string_arg(args.get("Path").or_else(|| args.get("TargetFile")))
                .unwrap_or_else(|| "file".to_owned());
            if let Some(content) = decode_string_arg(args.get("Content")) {
                let lines: Vec<&str> = content.lines().collect();
                let shown = lines.len().min(80);
                let mut diff = format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{shown} @@\n");
                for l in lines.into_iter().take(80) {
                    diff.push('+');
                    diff.push_str(l);
                    diff.push('\n');
                }
                return diff.trim_end().to_owned();
            }
        }
        "run_command" => {
            if let Some(cmd) =
                decode_string_arg(args.get("CommandLine").or_else(|| args.get("command")))
            {
                return cmd;
            }
        }
        _ => {}
    }
    compact_json(args)
}

/// Returns true when the JSON record sequence originates from an Antigravity `transcript*.jsonl`.
pub(crate) fn is_antigravity_record_sequence(records: &[Value]) -> bool {
    records.iter().take(12).any(|v| {
        matches!(
            v.get("type").and_then(Value::as_str),
            Some("USER_INPUT" | "PLANNER_RESPONSE" | "SYSTEM_MESSAGE" | "CHECKPOINT")
        )
    })
}

/// Stateful normalizer for an Antigravity conversation's step list, pairing each
/// `PLANNER_RESPONSE` tool call with its subsequent `GENERIC` tool execution output.
pub(crate) fn normalize_antigravity_records(records: &[Value]) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();
    let mut pending_tools: VecDeque<usize> = VecDeque::new();

    for value in records {
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            continue;
        };
        let source = value
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match event_type {
            "SYSTEM_MESSAGE" => {}
            "CHECKPOINT" => {
                pending_tools.clear();
                items.push(conversation_item(
                    "system",
                    "status",
                    "上下文检查点",
                    "已创建会话上下文检查点",
                    "completed",
                ));
            }
            "USER_INPUT" => {
                pending_tools.clear();
                let raw = value
                    .get("content")
                    .or_else(|| value.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let clean = clean_antigravity_user_text(raw);
                if !clean.is_empty() {
                    items.push(conversation_item(
                        "user",
                        "message",
                        "",
                        &clean,
                        "completed",
                    ));
                }
            }
            "PLANNER_RESPONSE" => {
                pending_tools.clear();
                if let Some(thinking) = value.get("thinking").and_then(Value::as_str) {
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
                if let Some(calls) = value.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        let function = call.get("function").unwrap_or(call);
                        let name = call
                            .get("name")
                            .or_else(|| call.get("tool_name"))
                            .or_else(|| function.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("工具调用");
                        let args = call.get("args").or_else(|| {
                            call.get("parameters")
                                .or_else(|| call.get("input"))
                                .or_else(|| function.get("arguments"))
                        });
                        let label = tool_call_label(name, args);
                        let details = format_antigravity_tool_details(name, args);
                        let idx = items.len();
                        items.push(conversation_item(
                            "assistant",
                            "tool",
                            &label,
                            &details,
                            "completed",
                        ));
                        pending_tools.push_back(idx);
                    }
                }
                if let Some(content) = value
                    .get("content")
                    .or_else(|| value.get("response"))
                    .and_then(Value::as_str)
                {
                    let clean = clean_message_text(content);
                    if !clean.is_empty() {
                        items.push(conversation_item(
                            "assistant",
                            "message",
                            "",
                            &clean,
                            "completed",
                        ));
                    }
                }
            }
            "GENERIC" => {
                if source != "MODEL" {
                    continue;
                }
                let raw = value
                    .get("content")
                    .or_else(|| value.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let trimmed_raw = raw.trim_start();
                let looks_like_tool_output = trimmed_raw.starts_with("Created At:")
                    || trimmed_raw.starts_with("Tool is running as a background task");
                let (clean_out, failed) = clean_antigravity_tool_output(raw);
                let status_val = if failed
                    || value
                        .get("status")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s.eq_ignore_ascii_case("ERROR"))
                {
                    "failed"
                } else {
                    "completed"
                };
                if let Some(tool_idx) = pending_tools.pop_front() {
                    if !clean_out.is_empty() {
                        let prev = items[tool_idx]["content"].as_str().unwrap_or_default();
                        let merged = if prev.is_empty() {
                            clean_out
                        } else {
                            format!("{prev}\n\n〔结果〕\n{clean_out}")
                        };
                        let rebuilt = conversation_item(
                            "assistant",
                            "tool",
                            items[tool_idx]["label"].as_str().unwrap_or("工具调用"),
                            &merged,
                            status_val,
                        );
                        items[tool_idx] = rebuilt;
                    } else if status_val == "failed" {
                        items[tool_idx]["status"] = Value::String("failed".to_owned());
                    }
                } else if looks_like_tool_output {
                    if !clean_out.is_empty() {
                        items.push(conversation_item(
                            "system",
                            "tool",
                            "工具结果",
                            &clean_out,
                            status_val,
                        ));
                    }
                } else {
                    let clean_msg = clean_message_text(&clean_out);
                    if !clean_msg.is_empty() {
                        items.push(conversation_item(
                            "assistant",
                            "message",
                            "",
                            &clean_msg,
                            status_val,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    items
}

/// Single-step fallback normalizer for Antigravity records.
pub(crate) fn normalize_antigravity_step(value: &Value) -> Option<Vec<Value>> {
    let event_type = value.get("type").and_then(Value::as_str)?;
    if !matches!(
        event_type,
        "SYSTEM_MESSAGE" | "CHECKPOINT" | "USER_INPUT" | "PLANNER_RESPONSE" | "GENERIC"
    ) {
        return None;
    }
    Some(normalize_antigravity_records(std::slice::from_ref(value)))
}
