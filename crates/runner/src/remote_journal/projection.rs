use super::*;

pub(crate) fn projected_remote_receipt(receipt: &CommandReceipt) -> Value {
    let result = receipt.result.as_ref().and_then(|result| {
        let mut projected = serde_json::Map::new();
        if let Some(binding) = result.get("binding") {
            projected.insert("binding".into(), binding.clone());
        }
        if let Some(turn_id) = result.pointer("/turn/id").and_then(Value::as_str) {
            projected.insert("turn".into(), json!({"id":turn_id}));
        }
        for key in ["approval_id", "response_sent"] {
            if let Some(value) = result.get(key) {
                projected.insert(key.into(), value.clone());
            }
        }
        (!projected.is_empty()).then_some(Value::Object(projected))
    });
    let mut projected = json!({
        "client_cmd_id": receipt.client_cmd_id,
        "hub_session_id": receipt.hub_session_id,
        "status": receipt.status,
        "code": receipt.code,
        "result": result,
    });
    crate::privacy::sanitize_workspace_paths_in_json(&mut projected);
    projected
}

pub(crate) fn projected_remote_event(event: &crate::remote_host::HostEvent) -> Result<Value> {
    let mut projected = serde_json::to_value(event)?;
    if let Some(object) = projected.as_object_mut() {
        object.remove("raw");
        if let Some(raw) = projected_event_raw(event) {
            object.insert("raw".into(), raw);
        }
    }
    crate::privacy::sanitize_workspace_paths_in_json(&mut projected);
    Ok(json!({"type":"event","event":projected}))
}

/// One-line summary of a `codexErrorInfo` kind object, e.g.
/// `{"usageLimitReached":{"httpStatusCode":429}}`.
///
/// Only the small scalars that say what to do about the failure are kept, so an
/// internal detail object cannot ride along with them. Anything unrecognised is
/// forwarded as-is rather than guessed at.
pub(crate) fn codex_error_summary(detail: &Value) -> Value {
    const KEEP: [&str; 4] = ["httpStatusCode", "retryAfterSeconds", "code", "message"];
    let Some(object) = detail.as_object() else {
        return detail.clone();
    };
    let mut summary = serde_json::Map::new();
    for key in KEEP {
        if let Some(value) = object.get(key)
            && (value.is_string() || value.is_number() || value.is_boolean())
        {
            let bounded = value
                .as_str()
                .map(|text| {
                    text.chars()
                        .take(MAX_ERROR_MESSAGE_CHARS)
                        .collect::<String>()
                })
                .map(Value::String)
                .unwrap_or_else(|| value.clone());
            summary.insert(key.into(), bounded);
        }
    }
    if summary.is_empty() {
        return detail.clone();
    }
    Value::Object(summary)
}

pub(crate) fn extract_text_from_value(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = value.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(s) = item.as_str() {
                parts.push(s.to_string());
            } else if let Some(text) = item.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            } else if let Some(text) = item.get("output_text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("\n"));
        }
    }
    None
}

pub(crate) fn projected_event_raw(event: &crate::remote_host::HostEvent) -> Option<Value> {
    let params = event.raw.get("params")?;
    match event.kind.as_str() {
        "history/snapshot" => params.get("items").and_then(Value::as_array).map(|items| {
            // `conversation::load_session_conversation` stores message text
            // under `content`, while every client reads `text` (the same
            // key the live item events use). Project one canonical shape so
            // a resumed session shows its history instead of an empty
            // transcript, and drop fields no client understands.
            let normalized: Vec<Value> = items
                .iter()
                .map(|item| {
                    let mut out = serde_json::Map::new();
                    for key in ["role", "kind", "label", "status"] {
                        if let Some(value) = item.get(key).and_then(Value::as_str) {
                            out.insert(key.into(), json!(value));
                        }
                    }
                    let text = item
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| item.get("content").and_then(extract_text_from_value))
                        .unwrap_or_default();
                    out.insert("text".into(), json!(text));
                    Value::Object(out)
                })
                .collect();
            json!({"params":{"items": normalized}})
        }),
        "turn/started" => params
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(|id| json!({"params":{"turn":{"id":id}}})),
        "turn/completed" => params
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(|id| {
                let mut turn = serde_json::Map::new();
                turn.insert("id".into(), json!(id));
                if let Some(status) = params.pointer("/turn/status").and_then(Value::as_str) {
                    turn.insert("status".into(), json!(status));
                }
                let error_msg = params.pointer("/turn/error").and_then(|v| {
                    v.as_str()
                        .or_else(|| v.get("message").and_then(Value::as_str))
                });
                if let Some(message) = error_msg {
                    let scrubbed = crate::privacy::redact_paths_inside_text(message);
                    let bounded = scrubbed
                        .chars()
                        .take(MAX_ERROR_MESSAGE_CHARS)
                        .collect::<String>();
                    if !bounded.trim().is_empty() {
                        turn.insert("error".into(), json!(bounded));
                    }
                }
                json!({"params":{"turn": Value::Object(turn)}})
            }),
        // How full the context window is. Clients get one canonical pair so a
        // Codex camelCase report and a normalised Claude/ACP report read the
        // same; the capacity stays optional because a provider may state
        // consumption without stating the window.
        "thread/tokenUsage/updated" => {
            let usage = params.get("tokenUsage")?;
            let positive =
                |value: Option<&Value>| value.and_then(Value::as_u64).filter(|number| *number > 0);
            let last = usage.get("last").or_else(|| usage.get("last_token_usage"));
            let used = positive(usage.get("used"))
                .or_else(|| last.and_then(|last| positive(last.get("inputTokens"))))
                .or_else(|| last.and_then(|last| positive(last.get("input_tokens"))))
                .or_else(|| positive(usage.get("inputTokens")))
                .or_else(|| positive(usage.get("input_tokens")))?;
            let window = positive(usage.get("window"))
                .or_else(|| positive(usage.get("modelContextWindow")))
                .or_else(|| positive(usage.get("model_context_window")))
                .or_else(|| positive(usage.get("context_window_tokens")))
                .or_else(|| positive(params.get("model_context_window")));
            let mut projected = json!({"params":{"tokenUsage":{"used":used}}});
            if let Some(window) = window {
                projected["params"]["tokenUsage"]["window"] = json!(window);
            }
            Some(projected)
        }
        "item/agentMessage/delta" => {
            let item_id = params.get("itemId").and_then(Value::as_str)?;
            let delta = params.get("delta").and_then(Value::as_str)?;
            Some(json!({"params":{"itemId":item_id,"delta":delta}}))
        }
        // A turn that died must say why. Without this the remote console shows
        // "执行出错" with no cause, and the only way to learn the real reason is
        // to walk back to the machine. Only the already allowlisted, bounded
        // fields are projected; the message is then scrubbed token by token so
        // an absolute path inside the prose does not take the whole explanation
        // down with it.
        "error" => {
            let error = params.get("error")?;
            let mut projected_error = serde_json::Map::new();
            if let Some(message) = error.get("message").and_then(Value::as_str) {
                let scrubbed = crate::privacy::redact_paths_inside_text(message);
                let bounded = scrubbed
                    .chars()
                    .take(MAX_ERROR_MESSAGE_CHARS)
                    .collect::<String>();
                projected_error.insert("message".into(), json!(bounded));
            }
            // `codexErrorInfo` is an object keyed by kind (`{"usageLimitReached":{...}}`),
            // not a string, so it is projected as its kind plus whatever the kind
            // object can say in one line. An unknown shape keeps its own shape
            // rather than being guessed at.
            if let Some(info) = error.get("codexErrorInfo") {
                match info {
                    Value::Object(kinds) => {
                        let mut projected_info = serde_json::Map::new();
                        for (kind, detail) in kinds {
                            projected_info.insert(kind.clone(), json!(codex_error_summary(detail)));
                        }
                        if !projected_info.is_empty() {
                            projected_error
                                .insert("codexErrorInfo".into(), Value::Object(projected_info));
                        }
                    }
                    Value::String(text) => {
                        projected_error.insert("codexErrorInfo".into(), json!(text));
                    }
                    _ => {}
                }
            }
            if let Some(value) = error.get("type").and_then(Value::as_str) {
                projected_error.insert("type".into(), json!(value));
            }
            // An error object with nothing a human can act on is not a cause;
            // dropping it keeps the console from rendering an empty banner.
            if projected_error.is_empty() {
                return None;
            }
            let mut projected_params = serde_json::Map::new();
            projected_params.insert("error".into(), Value::Object(projected_error));
            if let Some(turn_id) = params.get("turnId").and_then(Value::as_str) {
                projected_params.insert("turnId".into(), json!(turn_id));
            }
            // `willRetry: true` means the provider is still working on it, so the
            // console must not present this as the turn's final outcome.
            if let Some(will_retry) = params.get("willRetry").and_then(Value::as_bool) {
                projected_params.insert("willRetry".into(), json!(will_retry));
            }
            Some(json!({"params": Value::Object(projected_params)}))
        }

        // A tool that has started but has no result yet. Without this the
        // remote console cannot show a live "执行中" row; it only learns about
        // the operation once it finishes. Only the fields a client renders
        // are projected, and no provider payload is copied wholesale.
        "item/started" => {
            let item = params.get("item")?;
            let id = item.get("id").and_then(Value::as_str)?;
            let item_type = item.get("type").and_then(Value::as_str)?;
            if !matches!(
                item_type,
                "commandExecution" | "fileChange" | "reasoning" | "thinking"
            ) {
                return None;
            }
            let mut started = json!({"id": id, "type": item_type});
            for key in [
                "command",
                "commandLine",
                "toolName",
                "name",
                "path",
                "status",
                "text",
            ] {
                if let Some(value) = item.get(key).and_then(Value::as_str) {
                    started[key] = json!(value);
                }
            }
            Some(json!({"params":{"item": started}}))
        }
        "item/completed" => {
            let item = params.get("item")?;
            let id = item.get("id").and_then(Value::as_str)?;
            match item.get("type").and_then(Value::as_str)? {
                "userMessage" => {
                    let mut user_item = json!({"id": id, "type": "userMessage"});
                    if let Some(content) = item.get("content") {
                        user_item["content"] = content.clone();
                    }
                    let text = item
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| item.get("content").and_then(extract_text_from_value));
                    if let Some(text) = text {
                        user_item["text"] = json!(text);
                    }
                    Some(json!({"params":{"item": user_item}}))
                }
                "agentMessage" => {
                    let text = item
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| item.get("content").and_then(extract_text_from_value))
                        .unwrap_or_default();
                    Some(json!({"params":{"item":{
                        "id": id,
                        "type": "agentMessage",
                        "text": text
                    }}}))
                }
                "commandExecution" => {
                    let mut cmd_item = json!({"id": id, "type": "commandExecution"});
                    if let Some(cmd) = item.get("command") {
                        cmd_item["command"] = cmd.clone();
                    }
                    if let Some(output) = item.get("output") {
                        cmd_item["output"] = output.clone();
                    }
                    if let Some(status) = item.get("status") {
                        cmd_item["status"] = status.clone();
                    }
                    if let Some(exit_code) = item.get("exitCode") {
                        cmd_item["exitCode"] = exit_code.clone();
                    }
                    Some(json!({"params":{"item": cmd_item}}))
                }
                "thinking" => {
                    let text = item
                        .get("text")
                        .or_else(|| item.get("thought"))
                        .or_else(|| item.get("thinking"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    Some(json!({"params":{"item": {
                        "id": id,
                        "type": "thinking",
                        "text": text
                    }}}))
                }
                _ => None,
            }
        }
        _ => None,
    }
}
