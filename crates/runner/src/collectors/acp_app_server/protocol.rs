use super::*;

pub fn discover_acp_binary() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("ACP_SERVER_PATH").filter(|v| !v.is_empty()) {
        let candidate = std::path::PathBuf::from(path);
        if candidate.is_file() {
            return dunce::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }
    None
}

pub fn build_permission_outcome(
    options: Option<&Vec<AcpPermissionOption>>,
    decision: &str,
) -> Value {
    if decision == "accept" {
        let chosen_opt = options.and_then(|opts| {
            opts.iter()
                .find(|o| o.kind == "allow_once" || o.kind == "allow_always")
                .map(|o| o.option_id.clone())
        });
        if let Some(option_id) = chosen_opt {
            json!({ "outcome": "selected", "optionId": option_id })
        } else {
            json!({ "outcome": "selected" })
        }
    } else {
        let chosen_opt = options.and_then(|opts| {
            opts.iter()
                .find(|o| o.kind == "reject_once" || o.kind == "reject_always")
                .map(|o| o.option_id.clone())
        });
        if let Some(option_id) = chosen_opt {
            json!({ "outcome": "selected", "optionId": option_id })
        } else {
            json!({ "outcome": "cancelled" })
        }
    }
}

fn resolve_active_turn_id(active_turns: &HashMap<String, String>, session_id: &str) -> String {
    if let Some(turn) = active_turns.get(session_id) {
        return turn.clone();
    }
    if !session_id.is_empty()
        && let Some((_, turn)) = active_turns.iter().find(|(key, _)| {
            !key.is_empty() && (session_id.ends_with(key.as_str()) || key.ends_with(session_id))
        })
    {
        return turn.clone();
    }
    if active_turns.len() == 1
        && let Some((_, turn)) = active_turns.iter().next()
    {
        return turn.clone();
    }
    "turn_unknown".to_string()
}

pub fn normalize_acp_message(
    msg: Value,
    pending_prompts: &mut HashMap<u64, PendingPromptInfo>,
    pending_permissions: &mut HashMap<String, Vec<AcpPermissionOption>>,
    active_turns: &mut HashMap<String, String>,
) -> Vec<Value> {
    let mut normalized = Vec::new();
    if msg.get("method").is_none() {
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            if let Some(prompt_info) = pending_prompts.remove(&id) {
                let err_msg = msg
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(sanitize_protocol_message)
                    .filter(|s| !s.trim().is_empty());
                let is_err = msg.get("error").is_some();
                let stop_reason = msg
                    .get("result")
                    .and_then(|r| r.get("stopReason"))
                    .and_then(Value::as_str)
                    .unwrap_or("end_turn");

                let status_type = if is_err {
                    "failed"
                } else if stop_reason == "cancelled" {
                    "interrupted"
                } else {
                    "completed"
                };

                if is_err {
                    let message = err_msg
                        .clone()
                        .unwrap_or_else(|| "ACP 执行失败".to_string());
                    normalized.push(json!({
                        "method": "error",
                        "params": {
                            "threadId": prompt_info.session_id,
                            "turnId": prompt_info.turn_id,
                            "willRetry": false,
                            "error": { "message": message }
                        }
                    }));
                }

                let mut turn_obj = json!({
                    "id": prompt_info.turn_id,
                    "status": {
                        "type": status_type
                    }
                });
                if let Some(message) = err_msg {
                    turn_obj["error"] = json!(message);
                }

                normalized.push(json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": prompt_info.session_id,
                        "turn": turn_obj
                    }
                }));
                active_turns.remove(&prompt_info.session_id);
                return normalized;
            }
        }
    }

    if let Some(method) = msg.get("method").and_then(Value::as_str) {
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let active_turn_id = resolve_active_turn_id(active_turns, &session_id);

        match method {
            "session/update" => {
                let update = &params["update"];
                let update_type = update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match update_type {
                    "agent_message_chunk" => {
                        let text = update
                            .get("content")
                            .and_then(|c| c.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        normalized.push(json!({
                            "method": "item/agentMessage/delta",
                            "params": {
                                "threadId": session_id,
                                "turnId": active_turn_id,
                                "itemId": format!("acp_msg:{active_turn_id}"),
                                "delta": text
                            }
                        }));
                    }
                    "agent_thought_chunk" => {
                        let text = update
                            .get("content")
                            .and_then(|c| c.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !text.is_empty() {
                            normalized.push(json!({
                                "method": "item/reasoning/delta",
                                "params": {
                                    "threadId": session_id,
                                    "turnId": active_turn_id,
                                    "itemId": format!("acp_thinking:{active_turn_id}"),
                                    "delta": text
                                }
                            }));
                        }
                    }
                    "tool_call" => {
                        let tool_call_id = update
                            .get("toolCallId")
                            .and_then(Value::as_str)
                            .unwrap_or("tool-1");
                        let kind = update
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or("other");
                        let item_type = match kind {
                            "edit" | "write" | "delete" | "move" => "fileChange",
                            "execute" => "commandExecution",
                            _ => "commandExecution",
                        };
                        normalized.push(json!({
                            "method": "item/started",
                            "params": {
                                "threadId": session_id,
                                "turnId": active_turn_id,
                                "item": {
                                    "id": tool_call_id,
                                    "type": item_type
                                }
                            }
                        }));
                    }
                    "tool_call_update" => {
                        let tool_call_id = update
                            .get("toolCallId")
                            .and_then(Value::as_str)
                            .unwrap_or("tool-1");
                        let status = update
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("completed");
                        normalized.push(json!({
                            "method": "item/completed",
                            "params": {
                                "threadId": session_id,
                                "turnId": active_turn_id,
                                "item": {
                                    "id": tool_call_id,
                                    "status": status
                                }
                            }
                        }));
                    }
                    "usage_update" => {
                        let used = update
                            .get("used")
                            .or_else(|| update.get("inputTokens"))
                            .or_else(|| update.get("input_tokens"))
                            .and_then(Value::as_u64)
                            .filter(|n| *n > 0);
                        let size = update
                            .get("size")
                            .or_else(|| update.get("modelContextWindow"))
                            .or_else(|| update.get("window"))
                            .and_then(Value::as_u64)
                            .filter(|n| *n > 0);
                        if let Some(used) = used {
                            let mut usage_obj = json!({
                                "last": {"inputTokens": used}
                            });
                            if let Some(size) = size {
                                usage_obj["modelContextWindow"] = json!(size);
                            }
                            normalized.push(json!({
                                "method": "thread/tokenUsage/updated",
                                "params": {
                                    "threadId": session_id,
                                    "turnId": active_turn_id,
                                    "tokenUsage": usage_obj
                                }
                            }));
                        }
                    }
                    _ => {}
                }
            }
            "session/request_permission" => {
                let req_id_raw = msg.get("id").cloned().unwrap_or(json!("1"));
                let req_id_str = req_id_raw
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| req_id_raw.as_i64().map(|n| n.to_string()))
                    .or_else(|| req_id_raw.as_u64().map(|n| n.to_string()))
                    .unwrap_or_else(|| "1".to_string());

                let tool_call = &params["toolCall"];
                let kind = tool_call
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("execute");
                let title = tool_call
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Execute tool");
                let raw_input = &tool_call["rawInput"];

                let is_file = matches!(kind, "edit" | "write" | "fileChange");

                if let Some(options_array) = params.get("options").and_then(Value::as_array) {
                    let mut opts = Vec::new();
                    for opt in options_array {
                        if let (Some(opt_id), Some(opt_kind)) = (
                            opt.get("optionId").and_then(Value::as_str),
                            opt.get("kind").and_then(Value::as_str),
                        ) {
                            opts.push(AcpPermissionOption {
                                option_id: opt_id.to_string(),
                                kind: opt_kind.to_string(),
                            });
                        }
                    }
                    pending_permissions.insert(req_id_str.clone(), opts);
                }

                if is_file {
                    let file_path = raw_input
                        .get("file_path")
                        .or_else(|| raw_input.get("path"))
                        .or_else(|| raw_input.get("filePath"))
                        .and_then(Value::as_str);

                    let diff = if let (Some(old_str), Some(new_str)) = (
                        raw_input.get("old_string").and_then(Value::as_str),
                        raw_input.get("new_string").and_then(Value::as_str),
                    ) {
                        let mut payload = json!({
                            "old_string": crate::privacy::redact_paths_inside_text(old_str),
                            "new_string": crate::privacy::redact_paths_inside_text(new_str),
                        })
                        .to_string();
                        crate::collectors::truncate_utf8_bytes(&mut payload, 65536);
                        Some(payload)
                    } else if let Some(content) = raw_input.get("content").and_then(Value::as_str) {
                        let mut payload = json!({
                            "new_string": crate::privacy::redact_paths_inside_text(content),
                        })
                        .to_string();
                        crate::collectors::truncate_utf8_bytes(&mut payload, 65536);
                        Some(payload)
                    } else if let Some(raw_diff) = raw_input
                        .get("diff")
                        .or_else(|| raw_input.get("patch"))
                        .and_then(Value::as_str)
                    {
                        let mut sanitized = crate::privacy::redact_paths_inside_text(raw_diff);
                        crate::collectors::truncate_utf8_bytes(&mut sanitized, 65536);
                        Some(sanitized)
                    } else {
                        None
                    };

                    normalized.push(json!({
                        "id": format!("acp_req_{}", req_id_str),
                        "method": "item/fileChange/requestApproval",
                        "params": {
                            "threadId": session_id,
                            "turnId": active_turn_id,
                            "requestId": req_id_raw,
                            "toolName": "FileChange",
                            "filePath": file_path,
                            "diff": diff,
                            "reason": title,
                            "availableDecisions": ["accept", "decline"]
                        }
                    }));
                } else {
                    let command = raw_input
                        .get("command")
                        .or_else(|| raw_input.get("cmd"))
                        .and_then(Value::as_str)
                        .unwrap_or(title);

                    normalized.push(json!({
                        "id": format!("acp_req_{}", req_id_str),
                        "method": "item/commandExecution/requestApproval",
                        "params": {
                            "threadId": session_id,
                            "turnId": active_turn_id,
                            "requestId": req_id_raw,
                            "toolName": "Bash",
                            "command": command,
                            "reason": title,
                            "availableDecisions": ["accept", "decline"]
                        }
                    }));
                }
            }
            _ => {}
        }
    }

    normalized
}
