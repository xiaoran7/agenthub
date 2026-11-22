use super::parser::{clean_message_text, compact_json, conversation_item, extract_local_text};
use super::*;

/// Extracts reasoning summary or text from a Codex rollout `"reasoning"` payload.
pub(crate) fn extract_codex_reasoning_item(payload: &Value) -> Option<Value> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(summary) = payload.get("summary").and_then(Value::as_array) {
        for entry in summary {
            let t = extract_local_text(entry);
            if !t.trim().is_empty() {
                parts.push(t.trim().to_owned());
            }
        }
    }
    if parts.is_empty()
        && let Some(content) = payload.get("content").and_then(Value::as_array)
    {
        for entry in content {
            let t = extract_local_text(entry);
            if !t.trim().is_empty() {
                parts.push(t.trim().to_owned());
            }
        }
    }
    if parts.is_empty()
        && let Some(text) = payload.get("text").and_then(Value::as_str)
        && !text.trim().is_empty()
    {
        parts.push(text.trim().to_owned());
    }
    let joined = clean_message_text(&parts.join("\n\n"));
    (!joined.is_empty())
        .then(|| conversation_item("assistant", "thinking", "思考过程", &joined, "completed"))
}

/// Converts a Codex `apply_patch` body (`*** Begin Patch` / `*** Update File: ...`)
/// into standard unified diff headers (`--- a/...`, `+++ b/...`) so Web & Android
/// diff viewers highlight +/- hunks cleanly.
pub fn format_codex_apply_patch(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if !trimmed.contains("*** Begin Patch")
        && !trimmed.starts_with("*** Update File:")
        && !trimmed.starts_with("*** Add File:")
    {
        return None;
    }
    let mut out = String::with_capacity(trimmed.len() + 64);
    for line in trimmed.lines() {
        if line.starts_with("*** Begin Patch") || line.starts_with("*** End Patch") {
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Update File:") {
            let p = path.trim();
            out.push_str(&format!("--- a/{p}\n+++ b/{p}\n"));
        } else if let Some(path) = line.strip_prefix("*** Add File:") {
            let p = path.trim();
            out.push_str(&format!("--- /dev/null\n+++ b/{p}\n@@ -0,0 +1 @@\n"));
        } else if let Some(path) = line.strip_prefix("*** Delete File:") {
            let p = path.trim();
            out.push_str(&format!("--- a/{p}\n+++ /dev/null\n@@ -1 +0,0 @@\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    let result = out.trim().to_owned();
    (!result.is_empty()).then_some(result)
}

/// Formats Codex `function_call` / `custom_tool_call` details, extracting `cmd` or `apply_patch` diffs.
pub(crate) fn format_codex_tool_payload(payload: &Value) -> (Option<String>, String) {
    let name = payload
        .get("name")
        .or_else(|| payload.get("command"))
        .and_then(Value::as_str);
    let raw_input = payload
        .get("arguments")
        .or_else(|| payload.get("input"))
        .or_else(|| payload.get("aggregatedOutput"))
        .or_else(|| payload.get("aggregated_output"))
        .or_else(|| payload.get("changes"));

    let mut patch_title: Option<String> = None;
    if let Some(raw_val) = raw_input {
        if let Some(s) = raw_val.as_str() {
            if let Some(diff) = format_codex_apply_patch(s) {
                patch_title = diff
                    .lines()
                    .find_map(|l| l.strip_prefix("+++ b/"))
                    .map(|p| format!("apply_patch: {p}"));
                return (patch_title, diff);
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                if let Some(patch_s) = parsed
                    .get("patch")
                    .or_else(|| parsed.get("input"))
                    .and_then(Value::as_str)
                    && let Some(diff) = format_codex_apply_patch(patch_s)
                {
                    patch_title = diff
                        .lines()
                        .find_map(|l| l.strip_prefix("+++ b/"))
                        .map(|p| format!("apply_patch: {p}"));
                    return (patch_title, diff);
                }
                if let Some(cmd) = parsed
                    .get("cmd")
                    .or_else(|| parsed.get("command"))
                    .and_then(Value::as_str)
                {
                    return (None, cmd.to_owned());
                }
                return (None, compact_json(&parsed));
            }
        }
        return (patch_title, compact_json(raw_val));
    }
    let _ = name;
    (None, String::new())
}

/// Pairs Codex `function_call` / `custom_tool_call` (`call_id`) with subsequent
/// `function_call_output` / `custom_tool_call_output` (`call_id`) across normalized records.
pub(crate) fn merge_codex_call_outputs(
    call_id_to_item_idx: &mut HashMap<String, usize>,
    record: &Value,
    items: &mut Vec<Value>,
    normalized: Vec<Value>,
) {
    let payload = record.get("payload").unwrap_or(record);
    let payload_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let call_id = payload
        .get("call_id")
        .or_else(|| payload.get("callId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if matches!(payload_type, "function_call" | "custom_tool_call") {
        let start_idx = items.len();
        items.extend(normalized);
        if let Some(cid) = call_id
            && items.len() > start_idx
        {
            call_id_to_item_idx.insert(cid.to_owned(), start_idx);
        }
        return;
    }

    if matches!(
        payload_type,
        "function_call_output" | "custom_tool_call_output"
    ) && let Some(cid) = call_id
        && let Some(&tool_idx) = call_id_to_item_idx.get(cid)
        && tool_idx < items.len()
    {
        let out_text = payload
            .get("output")
            .or_else(|| payload.get("content"))
            .map(extract_local_text)
            .unwrap_or_default();
        let trimmed = out_text.trim();
        if !trimmed.is_empty() {
            let prev = items[tool_idx]["content"].as_str().unwrap_or_default();
            let merged = if prev.is_empty() {
                trimmed.to_owned()
            } else {
                format!("{prev}\n\n〔结果〕\n{trimmed}")
            };
            let failed = trimmed.contains("Process exited with code ")
                && !trimmed.contains("Process exited with code 0");
            let status = if failed { "failed" } else { "completed" };
            let rebuilt = conversation_item(
                "assistant",
                "tool",
                items[tool_idx]["label"].as_str().unwrap_or("工具调用"),
                &merged,
                status,
            );
            items[tool_idx] = rebuilt;
        }
        return;
    }

    items.extend(normalized);
}
