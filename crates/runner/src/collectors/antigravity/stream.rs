use serde_json::Value;

use super::super::{AgentTurnUpdate, ContextUsage, metadata_label, usage_u64};
use super::config::resolve_antigravity_context_window;

/// Extracts a native Antigravity conversation/session id from nested `init`/`result`/`step_update` JSON.
pub(crate) fn extract_antigravity_session_id(value: &Value) -> Option<String> {
    [
        value.get("conversation_id"),
        value.get("conversationId"),
        value.get("session_id"),
        value.get("sessionId"),
        value.pointer("/result/conversation_id"),
        value.pointer("/result/conversationId"),
        value.pointer("/result/session_id"),
        value.pointer("/init/conversation_id"),
        value.pointer("/init/session_id"),
        value.pointer("/step_update/conversation_id"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    .and_then(|raw| metadata_label(raw, super::super::MAX_CONFIG_VALUE_CHARS))
}

/// Parses Antigravity/Gemini token usage (`usage`, `result.usage`, or `usageMetadata`).
pub(crate) fn extract_antigravity_usage_from_value(
    value: &Value,
    fallback_model: Option<&str>,
) -> Option<ContextUsage> {
    let usage = value
        .get("usage")
        .or_else(|| value.pointer("/result/usage"))
        .or_else(|| value.get("usageMetadata"))
        .or_else(|| {
            value
                .get("input_tokens")
                .or_else(|| value.get("total_tokens"))
                .map(|_| value)
        })?;
    let input = usage_u64(usage, "input_tokens")
        .max(usage_u64(usage, "promptTokenCount"))
        .max(usage_u64(usage, "inputTokens"));
    let cache_read = usage_u64(usage, "cache_read_tokens")
        .max(usage_u64(usage, "cache_read_input_tokens"))
        .max(usage_u64(usage, "cachedContentTokenCount"));
    let cache_write = usage_u64(usage, "cache_creation_input_tokens");
    let total = usage_u64(usage, "total_tokens").max(usage_u64(usage, "totalTokenCount"));
    let used = input
        .saturating_add(cache_read)
        .saturating_add(cache_write)
        .max(total);
    if used == 0 {
        return None;
    }
    let model = value
        .get("model")
        .or_else(|| value.pointer("/result/model"))
        .or_else(|| value.pointer("/init/model"))
        .and_then(Value::as_str)
        .or(fallback_model);
    let reported_window = usage
        .get("context_window_tokens")
        .or_else(|| usage.get("model_context_window"))
        .or_else(|| usage.get("modelContextWindow"))
        .and_then(Value::as_u64)
        .filter(|w| *w > 0)
        .or_else(|| model.map(resolve_antigravity_context_window));
    Some(ContextUsage {
        used,
        window: reported_window,
    })
}

/// Extracts incremental turn activity/text deltas from `agy` / Antigravity NDJSON events.
pub(crate) fn parse_antigravity_turn_update(value: &Value) -> Option<AgentTurnUpdate> {
    let su = value.get("step_update").unwrap_or(value);
    if let Some(delta) = su
        .get("text_delta")
        .or_else(|| su.get("delta"))
        .or_else(|| su.get("text"))
        .or_else(|| su.get("content"))
        .or_else(|| value.pointer("/result/response"))
        .or_else(|| value.get("response"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        return Some(AgentTurnUpdate::AssistantDelta(delta.to_owned()));
    }
    if su
        .get("thinking_delta")
        .or_else(|| su.get("thinking"))
        .or_else(|| su.get("thought"))
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
    {
        return Some(AgentTurnUpdate::Activity("Antigravity 正在深度思考"));
    }
    if let Some(tool) = su
        .get("tool_name")
        .or_else(|| su.get("toolName"))
        .and_then(Value::as_str)
    {
        return Some(AgentTurnUpdate::Activity(
            match crate::adapters::safe_tool_category(Some(tool)).as_deref() {
                Some("shell") => "Antigravity 正在执行命令",
                Some("file_change") => "Antigravity 正在修改文件",
                Some("file_read") => "Antigravity 正在读取文件",
                Some("user_input") => "Antigravity 正在等待输入",
                _ => "Antigravity 正在调用工具",
            },
        ));
    }
    if let Some(step_type) = su.get("step_type").and_then(Value::as_str) {
        return Some(AgentTurnUpdate::Activity(match step_type {
            "tool" | "tool_call" | "run_command" => "Antigravity 正在调用工具",
            "thinking" | "planner" | "PLANNER_RESPONSE" => "Antigravity 正在规划",
            _ => "Antigravity 正在处理任务",
        }));
    }
    if value.get("type").and_then(Value::as_str) == Some("PLANNER_RESPONSE") {
        return Some(AgentTurnUpdate::Activity("Antigravity 正在规划"));
    }
    if value.get("event").and_then(Value::as_str) == Some("init") {
        return Some(AgentTurnUpdate::Activity("Antigravity 正在初始化上下文"));
    }
    None
}

/// Extracts the final assistant message from Antigravity NDJSON or accumulated text deltas.
pub(crate) fn final_antigravity_message(output: &str) -> Option<String> {
    let mut deltas = String::new();
    for line in output.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let su = value.get("step_update").unwrap_or(&value);
        if let Some(d) = su
            .get("text_delta")
            .or_else(|| su.get("delta"))
            .and_then(Value::as_str)
        {
            deltas.push_str(d);
        }
    }
    for line in output.lines().rev() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        for candidate in [
            value.pointer("/result/response"),
            value.get("response"),
            value.get("output"),
            value.get("content"),
            value.get("result"),
            value.pointer("/error/message"),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(trimmed) = candidate.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                return Some(trimmed.to_owned());
            }
        }
    }
    let delta_trimmed = deltas.trim();
    if !delta_trimmed.is_empty() {
        return Some(delta_trimmed.to_owned());
    }
    let trimmed = output.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}
