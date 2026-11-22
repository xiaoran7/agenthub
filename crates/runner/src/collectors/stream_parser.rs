use std::{
    io::{BufReader, Read},
    thread::{self, JoinHandle},
};

use serde_json::Value;

use super::*;

/// The last usage report a CLI emitted for this turn. Only providers that
/// state consumption in their stream are covered; the rest leave it `None`
/// and the console honestly says the capacity was not reported.
pub(crate) fn extract_context_usage_from_output(
    agent: &crate::domain::AgentType,
    output: &str,
) -> Option<ContextUsage> {
    match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            output.lines().rev().find_map(|line| {
                let value = serde_json::from_str::<Value>(line).ok()?;
                claude_result_context_usage(&value)
            })
        }
        crate::domain::AgentType::Antigravity => {
            let fallback_model = output.lines().find_map(|line| {
                let value = serde_json::from_str::<Value>(line).ok()?;
                value
                    .get("model")
                    .or_else(|| value.pointer("/init/model"))
                    .or_else(|| value.pointer("/result/model"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            });
            output.lines().rev().find_map(|line| {
                let value = serde_json::from_str::<Value>(line).ok()?;
                super::antigravity::extract_antigravity_usage_from_value(
                    &value,
                    fallback_model.as_deref(),
                )
            })
        }
        _ => None,
    }
}

pub(crate) fn extract_session_id_from_output(
    agent: &crate::domain::AgentType,
    output: &str,
) -> Option<String> {
    output.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<Value>(line).ok()?;
        let raw = match agent {
            crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
                ["session_id"]
                    .into_iter()
                    .find_map(|field| value.get(field).and_then(Value::as_str))
                    .filter(|_| {
                        value.get("type").and_then(Value::as_str) == Some("system")
                            && value.get("subtype").and_then(Value::as_str) == Some("init")
                            || value.get("type").and_then(Value::as_str) == Some("result")
                    })
            }
            crate::domain::AgentType::Codex => value
                .get("thread_id")
                .and_then(Value::as_str)
                .filter(|_| value.get("type").and_then(Value::as_str) == Some("thread.started")),
            crate::domain::AgentType::Antigravity => {
                return super::antigravity::extract_antigravity_session_id(&value);
            }
            crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => None,
        }?;
        metadata_label(raw, MAX_CONFIG_VALUE_CHARS)
    })
}

pub(crate) fn read_agent_turn_output(
    stream: impl Read + Send + 'static,
    agent: crate::domain::AgentType,
    mut on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut output = Vec::with_capacity(MAX_AGENT_TURN_OUTPUT_BYTES + 1);
        loop {
            match read_bounded_line(&mut reader) {
                BoundedLine::Line(line) => {
                    let may_emit = output
                        .len()
                        .checked_add(line.len())
                        .is_some_and(|size| size <= MAX_AGENT_TURN_OUTPUT_BYTES);
                    let remaining = (MAX_AGENT_TURN_OUTPUT_BYTES + 1).saturating_sub(output.len());
                    if remaining > 0 {
                        output.extend_from_slice(&line.as_bytes()[..line.len().min(remaining)]);
                    }
                    if may_emit && let Some(update) = parse_agent_turn_update(&agent, &line) {
                        on_update(update);
                    }
                }
                BoundedLine::Eof => return Ok(output),
                BoundedLine::TooLarge => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "agent output line exceeded protocol limit",
                    ));
                }
                BoundedLine::Io(error) => return Err(error),
            }
        }
    })
}

pub(crate) fn parse_agent_turn_update(
    agent: &crate::domain::AgentType,
    line: &str,
) -> Option<AgentTurnUpdate> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            if value.get("type").and_then(Value::as_str) == Some("system") {
                let subtype = value.get("subtype").and_then(Value::as_str);
                if subtype == Some("api_retry") {
                    let attempt = value.get("attempt").and_then(Value::as_u64).unwrap_or(1);
                    let max_retries = value
                        .get("max_retries")
                        .and_then(Value::as_u64)
                        .unwrap_or(10);
                    let status = value
                        .get("error_status")
                        .and_then(Value::as_u64)
                        .unwrap_or(429);
                    let error = value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("rate_limit");
                    return Some(AgentTurnUpdate::AssistantDelta(format!(
                        "\n⚠️ [上游模型响应限流/配额不足 (HTTP {status} {error})，Claude 正在自动重试第 {attempt}/{max_retries} 次...]\n"
                    )));
                }
                if subtype == Some("status")
                    && value.get("status").and_then(Value::as_str) == Some("requesting")
                {
                    return Some(AgentTurnUpdate::Activity("Claude 正在请求模型 API"));
                }
                if subtype == Some("init") {
                    return Some(AgentTurnUpdate::Activity("Claude 正在初始化上下文..."));
                }
                if subtype == Some("compact_boundary") {
                    return Some(AgentTurnUpdate::Activity(
                        "Claude 正在压缩整理历史上下文...",
                    ));
                }
            }
            if value.get("type").and_then(Value::as_str) == Some("stream_event") {
                let event = value.get("event")?;
                if event.get("type").and_then(Value::as_str) == Some("content_block_delta") {
                    let delta_type = event.pointer("/delta/type").and_then(Value::as_str);
                    if delta_type == Some("text_delta") {
                        return event
                            .pointer("/delta/text")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                            .map(|text| AgentTurnUpdate::AssistantDelta(text.to_owned()));
                    }
                    if delta_type == Some("thinking_delta") {
                        return Some(AgentTurnUpdate::Activity("Claude 正在深度思考..."));
                    }
                }
            }
            let assistant_blocks = value
                .get("type")
                .and_then(Value::as_str)
                .filter(|t| *t == "assistant")
                .and_then(|_| value.pointer("/message/content"))
                .and_then(Value::as_array);
            if let Some(blocks) = assistant_blocks {
                let has_tool = blocks
                    .iter()
                    .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"));
                if has_tool {
                    return Some(AgentTurnUpdate::Activity("Claude 正在调用工具"));
                }
                let mut text_acc = String::new();
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) == Some("text") {
                        text_acc.push_str(block.get("text").and_then(Value::as_str).unwrap_or(""));
                    }
                }
                let trimmed = text_acc.trim();
                if !trimmed.is_empty() {
                    return Some(AgentTurnUpdate::AssistantMessage(trimmed.to_owned()));
                }
            }
            None
        }
        crate::domain::AgentType::Codex => {
            let event_type = value.get("type").and_then(Value::as_str)?;
            let item_type = value.pointer("/item/type").and_then(Value::as_str);
            if event_type == "item.completed" && item_type == Some("agent_message") {
                return value
                    .pointer("/item/text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| AgentTurnUpdate::AssistantMessage(text.trim().to_owned()));
            }
            match (event_type, item_type) {
                ("turn.started", _) => Some(AgentTurnUpdate::Activity("Codex 正在思考")),
                ("item.started", Some("command_execution")) => {
                    Some(AgentTurnUpdate::Activity("Codex 正在执行命令"))
                }
                ("item.started", Some("mcp_tool_call")) => {
                    Some(AgentTurnUpdate::Activity("Codex 正在调用工具"))
                }
                ("item.started", Some("web_search")) => {
                    Some(AgentTurnUpdate::Activity("Codex 正在搜索"))
                }
                ("item.completed", Some("file_change")) => {
                    Some(AgentTurnUpdate::Activity("Codex 已更新文件"))
                }
                _ => None,
            }
        }
        crate::domain::AgentType::Antigravity => {
            super::antigravity::parse_antigravity_turn_update(&value)
        }
        crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => None,
    }
}

pub(crate) fn final_agent_message(
    agent: &crate::domain::AgentType,
    output: &str,
) -> Option<String> {
    match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            for line in output.lines().rev() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if value.get("type").and_then(Value::as_str) != Some("result") {
                    continue;
                }
                if let Some(res) = value
                    .get("result")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    return Some(res.to_owned());
                }
            }
            for line in output.lines().rev() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if value.get("type").and_then(Value::as_str) != Some("assistant") {
                    continue;
                }
                if let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) {
                    let mut text_acc = String::new();
                    for block in blocks {
                        if block.get("type").and_then(Value::as_str) == Some("text") {
                            text_acc
                                .push_str(block.get("text").and_then(Value::as_str).unwrap_or(""));
                        }
                    }
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_owned());
                    }
                }
            }
            let mut deltas = String::new();
            for line in output.lines() {
                let Ok(value) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if value.get("type").and_then(Value::as_str) != Some("stream_event") {
                    continue;
                }
                if let Some(delta) = value.pointer("/event/delta/text").and_then(Value::as_str) {
                    deltas.push_str(delta);
                }
            }
            let trimmed = deltas.trim();
            if !trimmed.is_empty() {
                Some(trimmed.to_owned())
            } else {
                None
            }
        }
        crate::domain::AgentType::Codex => output.lines().rev().find_map(|line| {
            let value = serde_json::from_str::<Value>(line).ok()?;
            if value.get("type").and_then(Value::as_str) != Some("item.completed")
                || value.pointer("/item/type").and_then(Value::as_str) != Some("agent_message")
            {
                return None;
            }
            value
                .pointer("/item/text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_owned)
        }),
        crate::domain::AgentType::Antigravity => {
            super::antigravity::final_antigravity_message(output)
        }
        crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => None,
    }
}
