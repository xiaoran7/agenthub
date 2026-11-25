use super::*;

const MAX_DIFF_JSON_BYTES: usize = 65_536;

fn serialize_bounded_json_diff(
    mut obj: serde_json::Map<String, Value>,
    max_bytes: usize,
) -> String {
    for _ in 0..16 {
        let encoded = Value::Object(obj.clone()).to_string();
        if encoded.len() <= max_bytes {
            return encoded;
        }
        let excess = encoded.len() - max_bytes;
        let longest_key = obj
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.len())))
            .max_by_key(|(_, len)| *len)
            .map(|(k, _)| k);
        let Some(key) = longest_key else {
            break;
        };
        if let Some(Value::String(s)) = obj.get_mut(&key) {
            if s.is_empty() {
                break;
            }
            let shrink = (excess / 2).max(1).min(s.len());
            let target = s.len().saturating_sub(shrink);
            crate::collectors::truncate_utf8_bytes(s, target);
        }
    }
    for value in obj.values_mut() {
        if let Value::String(s) = value {
            s.clear();
        }
    }
    Value::Object(obj).to_string()
}

impl MultiProviderTransport {
    pub(crate) fn drain_claude_process_sessions(&mut self, messages: &mut Vec<Value>) {
        let mut claude_completed = Vec::new();
        let mut claude_dead_sessions = Vec::new();
        for (session_id, session) in self.claude_sessions.iter_mut() {
            let active_turn_id = self
                .claude_active_turns
                .get(session_id)
                .map(String::as_str)
                .unwrap_or_default();
            match session.drain_messages() {
                Ok(msgs) => {
                    for msg in msgs {
                        if let Some(msg_type) = msg.get("type").and_then(Value::as_str) {
                            match msg_type {
                                "stream_event" => {
                                    if let Some(delta) = msg
                                        .pointer("/event/delta/text")
                                        .or_else(|| msg.pointer("/delta/text"))
                                        .and_then(Value::as_str)
                                        && !delta.is_empty()
                                    {
                                        self.claude_turn_emitted_text.insert(session_id.clone());
                                        self.claude_turn_streamed_deltas.insert(session_id.clone());
                                        messages.push(json!({
                                            "method": "item/agentMessage/delta",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": active_turn_id,
                                                "itemId": format!("claude_msg:{active_turn_id}"),
                                                "delta": delta
                                            }
                                        }));
                                    } else if let Some(thinking_delta) = msg
                                        .pointer("/event/delta/thinking")
                                        .or_else(|| msg.pointer("/delta/thinking"))
                                        .and_then(Value::as_str)
                                        && !thinking_delta.is_empty()
                                    {
                                        self.claude_turn_streamed_deltas.insert(session_id.clone());
                                        messages.push(json!({
                                            "method": "item/reasoning/delta",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": active_turn_id,
                                                "itemId": format!("claude_thinking:{active_turn_id}"),
                                                "delta": thinking_delta
                                            }
                                        }));
                                    }
                                }
                                "assistant" => {
                                    if let Some(usage_obj) = msg.pointer("/message/usage") {
                                        let model_win = msg
                                            .pointer("/message/model")
                                            .and_then(Value::as_str)
                                            .or_else(|| {
                                                self.claude_cached_models
                                                    .get(session_id)
                                                    .map(String::as_str)
                                            })
                                            .map(crate::collectors::claude_context_window);
                                        if let Some(usage) =
                                            crate::collectors::context_usage_from_values(
                                                usage_obj, model_win,
                                            )
                                        {
                                            messages.push(json!({
                                                "method": "thread/tokenUsage/updated",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": self.claude_active_turns.get(session_id).cloned().unwrap_or_default(),
                                                    "tokenUsage": {
                                                        "used": usage.used,
                                                        "last": {"inputTokens": usage.used},
                                                        "window": usage.window,
                                                        "modelContextWindow": usage.window
                                                    }
                                                }
                                            }));
                                        }
                                    }
                                    if let Some(content) =
                                        msg.pointer("/message/content").and_then(Value::as_array)
                                    {
                                        let already_streamed =
                                            self.claude_turn_streamed_deltas.contains(session_id);
                                        for block in content {
                                            if block.get("type").and_then(Value::as_str)
                                                == Some("tool_use")
                                            {
                                                let tool_name = block
                                                    .get("name")
                                                    .and_then(Value::as_str)
                                                    .unwrap_or("unknown");
                                                messages.push(json!({
                                                    "method": "item/started",
                                                    "params": {
                                                        "threadId": session_id,
                                                        "turnId": self.claude_active_turns.get(session_id).cloned().unwrap_or_default(),
                                                        "item": {
                                                            "id": format!("claude_tool_{}", tool_name),
                                                            "type": "commandExecution",
                                                            "toolName": tool_name
                                                        }
                                                    }
                                                }));
                                            } else if !already_streamed
                                                && block.get("type").and_then(Value::as_str)
                                                    == Some("text")
                                                && let Some(text) =
                                                    block.get("text").and_then(Value::as_str)
                                                && !text.is_empty()
                                            {
                                                self.claude_turn_emitted_text
                                                    .insert(session_id.clone());
                                                messages.push(json!({
                                                    "method": "item/agentMessage/delta",
                                                    "params": {
                                                        "threadId": session_id,
                                                        "turnId": active_turn_id,
                                                        "itemId": format!("claude_msg:{active_turn_id}"),
                                                        "delta": text
                                                    }
                                                }));
                                            }
                                        }
                                    }
                                }
                                "control_request" => {
                                    if let Some(req_id) =
                                        msg.get("request_id").and_then(Value::as_str)
                                    {
                                        let req = &msg["request"];
                                        let subtype = req
                                            .get("subtype")
                                            .and_then(Value::as_str)
                                            .unwrap_or("unknown");
                                        let active_turn_id = self
                                            .claude_active_turns
                                            .get(session_id)
                                            .cloned()
                                            .unwrap_or_default();
                                        let tool_name =
                                            req.get("tool_name").and_then(Value::as_str);
                                        let is_question = subtype == "ask_user_question"
                                            || tool_name == Some("AskUserQuestion");

                                        if is_question {
                                            self.claude_pending_requests
                                                .insert(req_id.to_string(), session_id.clone());
                                            let input = &req["input"];
                                            let questions =
                                                input.get("questions").and_then(Value::as_array);
                                            let first_q = questions.and_then(|q| q.first());
                                            let prompt = first_q
                                                .and_then(|q| {
                                                    q.get("question").and_then(Value::as_str)
                                                })
                                                .or_else(|| {
                                                    input.get("question").and_then(Value::as_str)
                                                })
                                                .or_else(|| {
                                                    input.get("prompt").and_then(Value::as_str)
                                                })
                                                .unwrap_or("Agent 提出了一个问题");
                                            let header = first_q.and_then(|q| {
                                                q.get("header").and_then(Value::as_str)
                                            });
                                            let multi_select = first_q.and_then(|q| {
                                                q.get("multiSelect").and_then(Value::as_bool)
                                            });
                                            let options = first_q
                                            .and_then(|q| q.get("options").and_then(Value::as_array))
                                            .map(|opts| {
                                                opts.iter()
                                                    .filter_map(|opt| {
                                                        opt.get("label")
                                                            .and_then(Value::as_str)
                                                            .map(|lbl| {
                                                                json!({
                                                                    "label": lbl,
                                                                    "description": opt.get("description").and_then(Value::as_str),
                                                                    "preview": opt.get("preview").and_then(Value::as_str)
                                                                })
                                                            })
                                                    })
                                                    .collect::<Vec<_>>()
                                            });
                                            messages.push(json!({
                                                "id": format!("claude_req_{}", req_id),
                                                "method": "client/askQuestion",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": active_turn_id,
                                                    "requestId": req_id,
                                                    "prompt": prompt,
                                                    "header": header,
                                                    "options": options,
                                                    "multiSelect": multi_select
                                                }
                                            }));
                                        } else {
                                            let tool_name_str = tool_name.unwrap_or("Bash");
                                            let is_file = matches!(
                                                tool_name_str,
                                                "Edit" | "Write" | "NotebookEdit" | "fileChange"
                                            );
                                            let input = &req["input"];
                                            let file_path = input
                                                .get("file_path")
                                                .or_else(|| input.get("path"))
                                                .or_else(|| input.get("notebook_path"))
                                                .and_then(Value::as_str);
                                            let command = input
                                                .get("command")
                                                .or_else(|| input.get("cmd"))
                                                .and_then(Value::as_str);
                                            let reason = req
                                                .get("description")
                                                .or_else(|| req.get("reason"))
                                                .and_then(Value::as_str);

                                            if is_file {
                                                let diff = if let (Some(old_str), Some(new_str)) = (
                                                    input.get("old_string").and_then(Value::as_str),
                                                    input.get("new_string").and_then(Value::as_str),
                                                ) {
                                                    let mut sanitized_old =
                                                        crate::privacy::redact_paths_inside_text(
                                                            old_str,
                                                        );
                                                    let mut sanitized_new =
                                                        crate::privacy::redact_paths_inside_text(
                                                            new_str,
                                                        );
                                                    crate::collectors::truncate_utf8_bytes(
                                                        &mut sanitized_old,
                                                        24_000,
                                                    );
                                                    crate::collectors::truncate_utf8_bytes(
                                                        &mut sanitized_new,
                                                        24_000,
                                                    );
                                                    let mut obj = serde_json::Map::new();
                                                    obj.insert(
                                                        "old_string".into(),
                                                        Value::String(sanitized_old),
                                                    );
                                                    obj.insert(
                                                        "new_string".into(),
                                                        Value::String(sanitized_new),
                                                    );
                                                    Some(serialize_bounded_json_diff(
                                                        obj,
                                                        MAX_DIFF_JSON_BYTES,
                                                    ))
                                                } else if let Some(content) =
                                                    input.get("content").and_then(Value::as_str)
                                                {
                                                    let mut sanitized =
                                                        crate::privacy::redact_paths_inside_text(
                                                            content,
                                                        );
                                                    crate::collectors::truncate_utf8_bytes(
                                                        &mut sanitized,
                                                        48_000,
                                                    );
                                                    let mut obj = serde_json::Map::new();
                                                    obj.insert(
                                                        "new_string".into(),
                                                        Value::String(sanitized),
                                                    );
                                                    Some(serialize_bounded_json_diff(
                                                        obj,
                                                        MAX_DIFF_JSON_BYTES,
                                                    ))
                                                } else if let Some(new_source) =
                                                    input.get("new_source").and_then(Value::as_str)
                                                {
                                                    let mut sanitized =
                                                        crate::privacy::redact_paths_inside_text(
                                                            new_source,
                                                        );
                                                    crate::collectors::truncate_utf8_bytes(
                                                        &mut sanitized,
                                                        48_000,
                                                    );
                                                    let mut obj = serde_json::Map::new();
                                                    obj.insert(
                                                        "new_string".into(),
                                                        Value::String(sanitized),
                                                    );
                                                    Some(serialize_bounded_json_diff(
                                                        obj,
                                                        MAX_DIFF_JSON_BYTES,
                                                    ))
                                                } else if let Some(raw_diff) = input
                                                    .get("diff")
                                                    .or_else(|| input.get("patch"))
                                                    .and_then(Value::as_str)
                                                {
                                                    let mut sanitized =
                                                        crate::privacy::redact_paths_inside_text(
                                                            raw_diff,
                                                        );
                                                    crate::collectors::truncate_utf8_bytes(
                                                        &mut sanitized,
                                                        65536,
                                                    );
                                                    Some(sanitized)
                                                } else {
                                                    None
                                                };

                                                self.claude_pending_requests
                                                    .insert(req_id.to_string(), session_id.clone());
                                                messages.push(json!({
                                                    "id": format!("claude_req_{}", req_id),
                                                    "method": "item/fileChange/requestApproval",
                                                    "params": {
                                                        "threadId": session_id,
                                                        "turnId": active_turn_id,
                                                        "requestId": req_id,
                                                        "toolName": tool_name_str,
                                                        "filePath": file_path,
                                                        "diff": diff,
                                                        "reason": reason,
                                                        "availableDecisions": ["accept", "decline"]
                                                    }
                                                }));
                                            } else {
                                                self.claude_pending_requests
                                                    .insert(req_id.to_string(), session_id.clone());
                                                let cmd_preview = command.unwrap_or(tool_name_str);
                                                messages.push(json!({
                                                "id": format!("claude_req_{}", req_id),
                                                "method": "item/commandExecution/requestApproval",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": active_turn_id,
                                                    "requestId": req_id,
                                                    "toolName": tool_name_str,
                                                    "command": cmd_preview,
                                                    "reason": reason,
                                                    "availableDecisions": ["accept", "decline"]
                                                }
                                            }));
                                            }
                                        }
                                    }
                                }
                                "result" => {
                                    let is_error = msg
                                        .get("is_error")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false)
                                        || msg
                                            .get("subtype")
                                            .and_then(Value::as_str)
                                            .is_some_and(|s| s.starts_with("error"));
                                    let result_text = msg
                                        .get("result")
                                        .and_then(Value::as_str)
                                        .filter(|s| !s.trim().is_empty())
                                        .map(str::to_string)
                                        .or_else(|| {
                                            msg.get("errors")
                                                .and_then(Value::as_array)
                                                .map(|arr| {
                                                    arr.iter()
                                                        .filter_map(Value::as_str)
                                                        .collect::<Vec<_>>()
                                                        .join("; ")
                                                })
                                                .filter(|s| !s.trim().is_empty())
                                        })
                                        .unwrap_or_else(|| session.last_stderr());
                                    let turn_id = self
                                        .claude_active_turns
                                        .get(session_id)
                                        .cloned()
                                        .unwrap_or_default();
                                    if let Some(usage) =
                                        crate::collectors::claude_result_context_usage(&msg)
                                    {
                                        let window = usage.window.or_else(|| {
                                            self.claude_cached_models.get(session_id).map(|m| {
                                                crate::collectors::claude_context_window(m)
                                            })
                                        });
                                        messages.push(json!({
                                            "method": "thread/tokenUsage/updated",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": turn_id,
                                                "tokenUsage": {
                                                    "used": usage.used,
                                                    "last": {"inputTokens": usage.used},
                                                    "window": window,
                                                    "modelContextWindow": window
                                                }
                                            }
                                        }));
                                    }
                                    let real_sid = msg
                                        .get("session_id")
                                        .and_then(Value::as_str)
                                        .filter(|s| !s.is_empty())
                                        .map(|s| format!("claude_{s}"));

                                    if is_error {
                                        let err_msg = if result_text.trim().is_empty() {
                                            "Claude 执行失败，请检查网关或模型配置".to_string()
                                        } else {
                                            result_text
                                        };
                                        messages.push(json!({
                                            "method": "error",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": turn_id,
                                                "willRetry": false,
                                                "error": { "message": &err_msg }
                                            }
                                        }));
                                        let mut completed_params = json!({
                                            "threadId": session_id,
                                            "turn": {
                                                "id": turn_id,
                                                "status": "failed",
                                                "error": err_msg
                                            }
                                        });
                                        if let Some(real) = real_sid {
                                            completed_params["realSessionId"] = json!(real);
                                        }
                                        messages.push(json!({
                                            "method": "turn/completed",
                                            "params": completed_params
                                        }));
                                    } else {
                                        if !self.claude_turn_emitted_text.contains(session_id)
                                            && !result_text.trim().is_empty()
                                        {
                                            messages.push(json!({
                                                "method": "item/agentMessage/delta",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": turn_id,
                                                    "itemId": format!("claude_msg:{turn_id}"),
                                                    "delta": &result_text
                                                }
                                            }));
                                        }
                                        let mut completed_params = json!({
                                            "threadId": session_id,
                                            "turn": {
                                                "id": turn_id,
                                                "status": "completed",
                                                "output": result_text
                                            }
                                        });
                                        if let Some(real) = real_sid {
                                            completed_params["realSessionId"] = json!(real);
                                        }
                                        messages.push(json!({
                                            "method": "turn/completed",
                                            "params": completed_params
                                        }));
                                    }
                                    claude_completed.push(session_id.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    if session.is_dead() {
                        claude_dead_sessions.push(session_id.clone());
                    }
                }
                Err(err) => {
                    let turn_id = self
                        .claude_active_turns
                        .get(session_id)
                        .cloned()
                        .unwrap_or_default();
                    let err_msg = format!("Claude 协议流异常: {err}");
                    if !turn_id.is_empty() {
                        messages.push(json!({
                            "method": "error",
                            "params": {
                                "threadId": session_id,
                                "turnId": turn_id,
                                "willRetry": false,
                                "error": { "message": &err_msg }
                            }
                        }));
                        messages.push(json!({
                            "method": "turn/completed",
                            "params": {
                                "threadId": session_id,
                                "turn": {
                                    "id": turn_id,
                                    "status": "failed",
                                    "error": err_msg
                                }
                            }
                        }));
                    }
                    claude_completed.push(session_id.clone());
                    claude_dead_sessions.push(session_id.clone());
                }
            }
        }
        for sid in claude_completed {
            self.claude_active_turns.remove(&sid);
            self.claude_cancels.remove(&sid);
            self.claude_turn_emitted_text.remove(&sid);
            self.claude_turn_streamed_deltas.remove(&sid);
        }
        for sid in claude_dead_sessions {
            let stderr_hint = self
                .claude_sessions
                .remove(&sid)
                .map(|s| s.last_stderr())
                .unwrap_or_default();
            let orphaned_reqs: Vec<String> = self
                .claude_pending_requests
                .iter()
                .filter(|(_, owner)| *owner == &sid)
                .map(|(req_id, _)| req_id.clone())
                .collect();
            for req_id in orphaned_reqs {
                self.claude_pending_requests.remove(&req_id);
                messages.push(json!({
                    "method": "serverRequest/resolved",
                    "params": {
                        "threadId": &sid,
                        "requestId": format!("claude_req_{req_id}")
                    }
                }));
            }
            if let Some(turn_id) = self.claude_active_turns.remove(&sid) {
                self.claude_cancels.remove(&sid);
                self.claude_turn_emitted_text.remove(&sid);
                self.claude_turn_streamed_deltas.remove(&sid);
                let err_msg = if stderr_hint.trim().is_empty() {
                    "Claude 进程意外退出".to_string()
                } else {
                    format!("Claude 进程意外退出: {}", stderr_hint.trim())
                };
                messages.push(json!({
                    "method": "error",
                    "params": {
                        "threadId": &sid,
                        "turnId": &turn_id,
                        "willRetry": false,
                        "error": { "message": &err_msg }
                    }
                }));
                messages.push(json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": sid,
                        "turn": {
                            "id": turn_id,
                            "status": "failed",
                            "error": err_msg
                        }
                    }
                }));
            }
        }
    }
}
