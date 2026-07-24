pub mod claude;
pub mod codex;

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
        "Bash" | "exec_command" | "shell_command" => "shell",
        "Edit" | "Write" | "apply_patch" => "file_change",
        "Read" | "Glob" | "Grep" => "file_read",
        "AskUserQuestion" | "request_user_input" => "user_input",
        "ExitPlanMode" => "plan_approval",
        "Agent" | "Task" | "spawn_agent" => "subagent",
        name if name.starts_with("mcp__") => "mcp",
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
