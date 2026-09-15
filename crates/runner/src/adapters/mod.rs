pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod grok;
pub mod zcode;

use std::collections::BTreeMap;

use serde_json::Value;

pub(crate) fn string_field<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    cursor.as_str()
}

pub(crate) fn safe_tool_category(tool_name: Option<&str>) -> Option<String> {
    let tool_name = tool_name?;
    let category = match tool_name {
        "Bash" | "exec_command" | "shell_command" | "run_terminal_cmd" | "run_command" => "shell",
        "Edit"
        | "Write"
        | "apply_patch"
        | "search_replace"
        | "write"
        | "write_to_file"
        | "replace_file_content" => "file_change",
        "Read" | "Glob" | "Grep" | "read_file" | "list_dir" | "grep" | "view_file"
        | "grep_search" | "find_by_name" => "file_read",
        "AskUserQuestion" | "request_user_input" | "ask_user_question" | "ask_question" => {
            "user_input"
        }
        "ExitPlanMode" => "plan_approval",
        "Agent" | "Task" | "spawn_agent" | "invoke_subagent" | "manage_subagents" => "subagent",
        name if name.starts_with("mcp__") || name.starts_with("call_mcp_tool") => "mcp",
        _ => "other",
    };
    Some(category.to_owned())
}

pub(crate) fn detail(
    entries: impl IntoIterator<Item = (&'static str, Option<String>)>,
) -> BTreeMap<String, String> {
    entries
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value)))
        .collect()
}

/// Detail values must stay enum-like: agent CLIs may grow richer payloads in
/// these fields, and anything outside the fixed charset or length budget is
/// dropped rather than synced.
pub(crate) fn sanitize_detail_value(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .take(64)
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ' ' | '/'))
        .collect();
    if sanitized.is_empty() {
        "unknown".into()
    } else {
        sanitized
    }
}

/// Raw status values are protocol-bounded (128 chars at the gateway); clamp
/// locally so one oversized upstream value cannot wedge outbox syncing.
pub(crate) fn clamp_raw_status(value: &str) -> String {
    value.chars().take(128).collect()
}
