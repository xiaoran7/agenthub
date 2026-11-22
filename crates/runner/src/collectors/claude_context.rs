use std::{
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
    time::UNIX_EPOCH,
};

use serde::de::DeserializeSeed;
use serde_json::Value;

use super::*;

pub(crate) fn claude_result_context_usage(value: &Value) -> Option<ContextUsage> {
    if value.get("type").and_then(Value::as_str) != Some("result") {
        return None;
    }
    let usage = value.get("usage")?;
    let window = value
        .get("model_context_window")
        .and_then(Value::as_u64)
        .or_else(|| value.get("modelContextWindow").and_then(Value::as_u64))
        .or_else(|| {
            value
                .get("modelUsage")
                .and_then(Value::as_object)
                .and_then(|map| {
                    map.iter().find_map(|(model_name, stats)| {
                        stats
                            .get("contextWindow")
                            .or_else(|| stats.get("context_window"))
                            .and_then(Value::as_u64)
                            .filter(|v| *v > 0)
                            .or_else(|| Some(claude_context_window(model_name)))
                    })
                })
        });
    context_usage_from_values(usage, window)
}

pub(crate) fn read_claude_session_context(
    path: &Path,
    program: &str,
) -> Result<Option<LocalSessionContext>, CollectorError> {
    let meta = fs::metadata(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let length = meta.len();
    let modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    if let Ok(guard) = session_context_cache().read()
        && let Some((c_len, c_mod, cached)) = guard.get(path)
        && *c_len == length
        && *c_mod == modified_ms
    {
        return Ok(cached.clone());
    }

    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let fallback_id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS));
    let mut context = LocalSessionContext::new(
        crate::domain::AgentType::ClaudeCode,
        fallback_id.unwrap_or_default(),
    );
    let mut reader = BufReader::new(file);
    scan_claude_context_region(
        &mut reader,
        LOCAL_CONTEXT_HEAD_BYTES.min(length),
        &mut context,
        program,
    )?;
    if length > LOCAL_CONTEXT_HEAD_BYTES {
        let tail_start = length.saturating_sub(LOCAL_CONTEXT_TAIL_BYTES);
        reader
            .seek(SeekFrom::Start(tail_start))
            .map_err(|source| CollectorError::Io {
                program: program.to_owned(),
                source,
            })?;
        scan_claude_context_region(
            &mut reader,
            length.saturating_sub(tail_start),
            &mut context,
            program,
        )?;
    }
    let result = if context.agent_session_id.is_empty() {
        None
    } else {
        Some(context)
    };
    if let Ok(mut guard) = session_context_cache().write() {
        guard.insert(path.to_path_buf(), (length, modified_ms, result.clone()));
    }
    Ok(result)
}

pub(crate) fn scan_claude_context_region(
    reader: &mut impl BufRead,
    byte_limit: u64,
    context: &mut LocalSessionContext,
    program: &str,
) -> Result<(), CollectorError> {
    let mut consumed = 0_u64;
    while consumed < byte_limit {
        match read_spliced_line(reader) {
            SplicedLine::Line {
                text,
                bytes_consumed,
            } => {
                consumed = consumed.saturating_add(bytes_consumed);
                apply_claude_context_line(&text, context);
            }
            SplicedLine::Oversized {
                head,
                tail,
                bytes_consumed,
            } => {
                consumed = consumed.saturating_add(bytes_consumed);
                apply_oversized_claude_record(&head, &tail, context);
            }
            SplicedLine::Eof => break,
            SplicedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: program.to_owned(),
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn apply_claude_context_line(line: &str, context: &mut LocalSessionContext) {
    let seed = ClaudeContextLineSeed {
        parse_array: line.contains("\"user\""),
    };
    let mut de = serde_json::Deserializer::from_str(line);
    let Ok(record) = seed.deserialize(&mut de) else {
        return;
    };
    if context.agent_session_id.is_empty()
        && let Some(session_id) = record
            .session_id
            .as_deref()
            .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
    {
        context.agent_session_id = session_id;
    }
    match record.record_type.as_deref() {
        Some("custom-title") => {
            context.title = record
                .custom_title
                .as_deref()
                .or(record.title.as_deref())
                .or(record.name.as_deref())
                .and_then(|value| metadata_label(value, MAX_SESSION_TITLE_CHARS));
        }
        Some("ai-title") if context.title.is_none() => {
            context.title = record
                .ai_title
                .as_deref()
                .and_then(|value| metadata_label(value, MAX_SESSION_TITLE_CHARS));
        }
        Some("permission-mode" | "mode") => {
            context.permission_mode = record
                .mode
                .as_deref()
                .or(record.permission_mode.as_deref())
                .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS));
        }
        Some("assistant") => {
            context.model = record
                .message
                .as_ref()
                .and_then(|m| m.model.as_deref())
                .or(record.model.as_deref())
                .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
                .or_else(|| context.model.clone());
            context.reasoning_effort = record
                .message
                .as_ref()
                .and_then(|m| m.effort.as_deref())
                .or(record.effort.as_deref())
                .and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
                .or_else(|| context.reasoning_effort.clone());

            if let Some(reason) = record
                .message
                .as_ref()
                .and_then(|m| m.stop_reason.as_deref())
                .or(record.stop_reason.as_deref())
                .filter(|r| !r.is_empty())
            {
                context.stop_reason = Some(reason.to_owned());
                context.turn_ended = Some(reason != "tool_use");
            }

            if let Some(usage) = record.message.as_ref().and_then(|m| m.usage.as_ref()) {
                let input = usage.input_tokens.unwrap_or(0);
                let output = usage.output_tokens.unwrap_or(0);
                let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                let cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
                let total_input = input.saturating_add(cache_write).saturating_add(cache_read);
                if total_input > 0 || output > 0 {
                    context.context_used_tokens = Some(total_input);
                    context.input_tokens = Some(
                        context
                            .input_tokens
                            .unwrap_or(0)
                            .saturating_add(total_input),
                    );
                    context.output_tokens =
                        Some(context.output_tokens.unwrap_or(0).saturating_add(output));
                    context.cache_read_tokens = Some(
                        context
                            .cache_read_tokens
                            .unwrap_or(0)
                            .saturating_add(cache_read),
                    );
                    context.cache_write_tokens = Some(
                        context
                            .cache_write_tokens
                            .unwrap_or(0)
                            .saturating_add(cache_write),
                    );
                }
            }
        }
        Some("queue-operation") if context.title.is_none() => {
            if let Some(content) = record.content.as_ref().and_then(PromptContent::as_raw_str) {
                context.title = local_prompt_title(content);
            }
        }
        Some("user") => {
            if record.is_meta != Some(true)
                && record.is_compact_summary != Some(true)
                && let Some(prompt) = record
                    .message
                    .as_ref()
                    .and_then(|m| m.content.as_ref())
                    .or(record.content.as_ref())
                    .and_then(PromptContent::as_extracted)
            {
                context.turn_ended = Some(false);
                if context.title.is_none() {
                    context.title = local_prompt_title(&prompt);
                }
            }
        }
        _ => {}
    }
    context.context_window_tokens = record
        .context_window
        .as_ref()
        .and_then(|c| c.context_window_size)
        .or_else(|| {
            record
                .context_window_camel
                .as_ref()
                .and_then(|c| c.context_window_size)
        })
        .or_else(|| {
            record
                .message
                .as_ref()
                .and_then(|m| m.usage.as_ref())
                .and_then(|u| u.context_window)
        })
        .or(context.context_window_tokens)
        .or_else(|| context.model.as_deref().map(claude_context_window))
        .or(Some(200_000));
}

/// Bounded extractor for oversized JSONL lines that exceed METADATA_MAX_LINE_BYTES.
/// Uses head and tail slices to extract model, stop_reason and usage without parsing MBs of JSON.
pub(crate) fn apply_oversized_claude_record(
    head: &str,
    tail: &str,
    context: &mut LocalSessionContext,
) {
    let is_assistant = head.contains(r#""role":"assistant""#)
        || head.contains(r#""role": "assistant""#)
        || head.contains(r#""type":"assistant""#)
        || head.contains(r#""type": "assistant""#)
        || tail.contains(r#""type":"assistant""#)
        || tail.contains(r#""type": "assistant""#);

    if is_assistant {
        // 1. Model extraction (head preferred, tail fallback)
        if let Some(model) = extract_quoted_json_field(head, "model")
            .or_else(|| extract_quoted_json_field(tail, "model"))
            && let Some(label) = metadata_label(&model, MAX_CONFIG_VALUE_CHARS)
        {
            context.model = Some(label);
        }

        // 2. Stop reason extraction (tail preferred as it trails the message)
        if let Some(reason) = extract_quoted_json_field(tail, "stop_reason")
            .or_else(|| extract_quoted_json_field(head, "stop_reason"))
            && !reason.is_empty()
        {
            context.turn_ended = Some(reason != "tool_use");
            context.stop_reason = Some(reason);
        }

        // 3. Usage tokens extraction from closed usage object in tail
        if let Some(usage_json) = extract_closed_json_object(tail, "usage")
            && let Ok(usage) = serde_json::from_str::<ClaudeUsageFields>(&usage_json)
        {
            let input = usage.input_tokens.unwrap_or(0);
            let output = usage.output_tokens.unwrap_or(0);
            let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
            let cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
            let total_input = input.saturating_add(cache_write).saturating_add(cache_read);
            if total_input > 0 {
                context.context_used_tokens = Some(total_input);
                context.input_tokens = Some(total_input);
                context.output_tokens = Some(output);
                context.cache_read_tokens = Some(cache_read);
                context.cache_write_tokens = Some(cache_write);
            }
        }

        // Update context window tokens if model was updated
        if let Some(model) = context.model.as_deref() {
            context.context_window_tokens = Some(claude_context_window(model));
        }
        return;
    }

    let is_user = head.contains(r#""role":"user""#)
        || head.contains(r#""role": "user""#)
        || head.contains(r#""type":"user""#)
        || head.contains(r#""type": "user""#);

    if is_user {
        // A user line with tool_result is not a human prompt
        if head.contains(r#""type":"tool_result""#) || head.contains(r#""type": "tool_result""#) {
            return;
        }
        if head.contains(r#""isMeta":true"#) || tail.contains(r#""isMeta":true"#) {
            return;
        }
        // Genuine user prompt waiting for completion clears turn_ended
        context.turn_ended = Some(false);
    }
}

pub(crate) fn extract_quoted_json_field(text: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\"", field);
    let mut search_from = 0;
    let mut last_match = None;
    while let Some(pos) = text[search_from..].find(&pattern) {
        let actual_pos = search_from + pos;
        let rest = &text[actual_pos + pattern.len()..];
        // Skip whitespace and colon
        let after_colon = rest.trim_start();
        if let Some(after_colon) = after_colon.strip_prefix(':') {
            let value_part = after_colon.trim_start();
            if let Some(value_part) = value_part.strip_prefix('"')
                && let Some(end_quote) = value_part.find('"')
            {
                last_match = Some(value_part[..end_quote].to_owned());
            }
        }
        search_from = actual_pos + pattern.len();
    }
    last_match
}

pub(crate) fn extract_closed_json_object(text: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\"", field);
    let pos = text.rfind(&pattern)?;
    let rest = &text[pos + pattern.len()..];
    let open_brace = rest.find('{')?;
    let start_idx = pos + pattern.len() + open_brace;

    let bytes = text.as_bytes();
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;

    for i in start_idx..bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start_idx..=i].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn claude_context_window(model: &str) -> u64 {
    let lower = model.to_ascii_lowercase();
    if lower.contains("[1m]") || lower.contains("-1m") || lower.ends_with("1m") {
        return 1_000_000;
    }
    // Claude 5-series (Opus 5, Sonnet 5, Fable 5, Mythos 5) have native 1M context
    if lower.starts_with("claude-opus-5")
        || lower.starts_with("claude-sonnet-5")
        || lower.starts_with("claude-fable-5")
        || lower.starts_with("claude-mythos-5")
        || lower.contains("anthropic.claude-sonnet-5")
    {
        return 1_000_000;
    }
    200_000
}
