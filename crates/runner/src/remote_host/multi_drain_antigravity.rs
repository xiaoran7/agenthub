use super::*;

fn truncate_utf8_bytes(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

impl MultiProviderTransport {
    pub(crate) fn normalize_antigravity_stream_msg(
        &mut self,
        session_id: &str,
        turn_id: &str,
        msg: &Value,
        stderr_hint: &str,
        messages: &mut Vec<Value>,
    ) -> bool {
        let mut completed = false;
        let kind = msg
            .get("event")
            .or_else(|| msg.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");

        if let Some(usage) = crate::collectors::antigravity::extract_antigravity_usage_from_value(
            msg,
            self.antigravity_cached_models
                .get(session_id)
                .map(String::as_str),
        ) {
            messages.push(json!({
                "method": "thread/tokenUsage/updated",
                "params": {
                    "threadId": session_id,
                    "turnId": turn_id,
                    "tokenUsage": {
                        "used": usage.used,
                        "last": {"inputTokens": usage.used},
                        "window": usage.window,
                        "modelContextWindow": usage.window
                    }
                }
            }));
        }

        match kind {
            "init" => {
                if let Some(m) = msg
                    .get("model")
                    .or_else(|| msg.pointer("/init/model"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.antigravity_cached_models
                        .insert(session_id.to_string(), m.to_string());
                }
            }
            "step_update" | "stream_event" | "PLANNER_RESPONSE" | "assistant" => {
                let su = msg.get("step_update").unwrap_or(msg);
                if let Some(thinking) = su
                    .get("thinking_delta")
                    .or_else(|| su.get("thinking"))
                    .or_else(|| su.get("thought"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    messages.push(json!({
                        "method": "item/reasoning/delta",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "itemId": "antigravity_reason",
                            "delta": thinking
                        }
                    }));
                }
                if let Some(delta) = su
                    .get("text_delta")
                    .or_else(|| su.get("delta"))
                    .or_else(|| msg.pointer("/delta/text"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    self.antigravity_turn_emitted_text
                        .insert(session_id.to_string());
                    self.antigravity_turn_streamed_deltas
                        .insert(session_id.to_string());
                    messages.push(json!({
                        "method": "item/agentMessage/delta",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "itemId": "antigravity_msg",
                            "delta": delta
                        }
                    }));
                } else if !self.antigravity_turn_streamed_deltas.contains(session_id)
                    && let Some(full_text) = su
                        .get("text")
                        .or_else(|| su.get("content"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                {
                    self.antigravity_turn_emitted_text
                        .insert(session_id.to_string());
                    messages.push(json!({
                        "method": "item/agentMessage/delta",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "itemId": "antigravity_msg",
                            "delta": full_text
                        }
                    }));
                }
                if let Some(tool_name) = su
                    .get("tool_name")
                    .or_else(|| su.get("toolName"))
                    .and_then(Value::as_str)
                {
                    let status = su
                        .get("state")
                        .or_else(|| su.get("status"))
                        .and_then(Value::as_str);
                    let method_name = if matches!(status, Some("DONE" | "done" | "completed")) {
                        "item/completed"
                    } else {
                        "item/started"
                    };
                    messages.push(json!({
                        "method": method_name,
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "item": {
                                "id": format!("antigravity_tool_{tool_name}"),
                                "type": "commandExecution",
                                "toolName": tool_name
                            }
                        }
                    }));
                }
                if let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        let name = call.get("name").and_then(Value::as_str).unwrap_or("tool");
                        messages.push(json!({
                            "method": "item/started",
                            "params": {
                                "threadId": session_id,
                                "turnId": turn_id,
                                "item": {
                                    "id": format!("antigravity_tool_{name}"),
                                    "type": "commandExecution",
                                    "toolName": name
                                }
                            }
                        }));
                    }
                }
            }
            "control_request" | "request_permission" => {
                if let Some(req_id) = msg
                    .get("request_id")
                    .or_else(|| msg.get("id"))
                    .and_then(Value::as_str)
                {
                    self.antigravity_pending_requests
                        .insert(req_id.to_string(), session_id.to_string());
                    let req = msg.get("request").unwrap_or(msg);
                    let tool_name = req
                        .get("tool_name")
                        .or_else(|| req.get("toolName"))
                        .and_then(Value::as_str)
                        .unwrap_or("Bash");
                    let subtype = req.get("subtype").and_then(Value::as_str).unwrap_or("");
                    if subtype == "ask_user_question" || tool_name == "AskUserQuestion" {
                        let input = &req["input"];
                        let prompt = input
                            .get("question")
                            .or_else(|| input.get("prompt"))
                            .and_then(Value::as_str)
                            .unwrap_or("Antigravity 提出了一个问题");
                        messages.push(json!({
                            "id": format!("antigravity_req_{req_id}"),
                            "method": "client/askQuestion",
                            "params": {
                                "threadId": session_id,
                                "turnId": turn_id,
                                "requestId": req_id,
                                "prompt": prompt
                            }
                        }));
                    } else {
                        let is_file = matches!(
                            crate::adapters::safe_tool_category(Some(tool_name)).as_deref(),
                            Some("file_change")
                        );
                        let input = &req["input"];
                        let file_path = input
                            .get("file_path")
                            .or_else(|| input.get("path"))
                            .or_else(|| input.get("TargetFile"))
                            .and_then(Value::as_str);
                        let command = input
                            .get("command")
                            .or_else(|| input.get("CommandLine"))
                            .and_then(Value::as_str);
                        let reason = req
                            .get("reason")
                            .or_else(|| req.get("description"))
                            .and_then(Value::as_str);
                        if is_file {
                            let raw_diff = input
                                .get("diff")
                                .or_else(|| input.get("patch"))
                                .and_then(Value::as_str);
                            let diff = raw_diff.map(|d| {
                                let mut s = crate::privacy::redact_paths_inside_text(d);
                                truncate_utf8_bytes(&mut s, 65536);
                                s
                            });
                            messages.push(json!({
                                "id": format!("antigravity_req_{req_id}"),
                                "method": "item/fileChange/requestApproval",
                                "params": {
                                    "threadId": session_id,
                                    "turnId": turn_id,
                                    "requestId": req_id,
                                    "toolName": tool_name,
                                    "filePath": file_path,
                                    "diff": diff,
                                    "reason": reason,
                                    "availableDecisions": ["accept", "decline"]
                                }
                            }));
                        } else {
                            messages.push(json!({
                                "id": format!("antigravity_req_{req_id}"),
                                "method": "item/commandExecution/requestApproval",
                                "params": {
                                    "threadId": session_id,
                                    "turnId": turn_id,
                                    "requestId": req_id,
                                    "toolName": tool_name,
                                    "command": command.unwrap_or(tool_name),
                                    "reason": reason,
                                    "availableDecisions": ["accept", "decline"]
                                }
                            }));
                        }
                    }
                }
            }
            "result" => {
                let interrupted = msg
                    .get("interrupted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || msg.get("status").and_then(Value::as_str) == Some("INTERRUPTED");
                let is_error = !interrupted
                    && (msg
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                        || msg.get("status").and_then(Value::as_str).is_some_and(|s| {
                            s.eq_ignore_ascii_case("ERROR") || s.eq_ignore_ascii_case("FAILED")
                        }));
                let result_text = msg
                    .pointer("/result/response")
                    .or_else(|| msg.get("response"))
                    .or_else(|| msg.get("output"))
                    .or_else(|| msg.get("result"))
                    .or_else(|| msg.pointer("/result/error/message"))
                    .or_else(|| msg.pointer("/error/message"))
                    .or_else(|| msg.get("message"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        if is_error && !stderr_hint.trim().is_empty() {
                            stderr_hint.to_string()
                        } else {
                            String::new()
                        }
                    });
                let real_sid = crate::collectors::antigravity::extract_antigravity_session_id(msg)
                    .map(|raw| {
                        if raw.starts_with("antigravity_") {
                            raw
                        } else {
                            format!("antigravity_{raw}")
                        }
                    });
                let status = if interrupted {
                    "interrupted"
                } else if is_error {
                    "failed"
                } else {
                    "completed"
                };
                if is_error {
                    let err_msg = if result_text.trim().is_empty() {
                        "Antigravity 执行失败".to_string()
                    } else {
                        result_text.clone()
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
                } else if !interrupted
                    && !self.antigravity_turn_emitted_text.contains(session_id)
                    && !result_text.trim().is_empty()
                {
                    messages.push(json!({
                        "method": "item/agentMessage/delta",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "itemId": "antigravity_msg",
                            "delta": &result_text
                        }
                    }));
                }
                let mut completed_params = json!({
                    "threadId": session_id,
                    "turn": {
                        "id": turn_id,
                        "status": status
                    }
                });
                if !is_error && !interrupted {
                    completed_params["turn"]["output"] = json!(result_text);
                } else if is_error {
                    completed_params["turn"]["error"] = json!(result_text);
                }
                if let Some(real) = real_sid {
                    completed_params["realSessionId"] = json!(real);
                }
                messages.push(json!({
                    "method": "turn/completed",
                    "params": completed_params
                }));
                completed = true;
            }
            _ => {}
        }
        completed
    }

    pub(crate) fn drain_antigravity_process_sessions(&mut self, messages: &mut Vec<Value>) {
        let mut completed_sids = Vec::new();
        let mut dead_sids = Vec::new();
        let session_ids: Vec<String> = self.antigravity_sessions.keys().cloned().collect();
        for session_id in session_ids {
            let drained = match self.antigravity_sessions.get_mut(&session_id) {
                Some(session) => session.drain_messages(),
                None => continue,
            };
            let (is_dead, session_turn_id, stderr_hint, known_session_id) = self
                .antigravity_sessions
                .get(&session_id)
                .map(|s| {
                    (
                        s.is_dead(),
                        s.current_turn_id().map(str::to_string),
                        s.last_stderr(),
                        s.session_id().trim().to_string(),
                    )
                })
                .unwrap_or((false, None, String::new(), String::new()));
            let current_raw = session_id
                .strip_prefix("antigravity_")
                .unwrap_or(&session_id);
            let fallback_real = (!known_session_id.is_empty()
                && known_session_id
                    .strip_prefix("antigravity_")
                    .unwrap_or(&known_session_id)
                    != current_raw)
                .then(|| {
                    if known_session_id.starts_with("antigravity_") {
                        known_session_id
                    } else {
                        format!("antigravity_{known_session_id}")
                    }
                });
            let turn_id = self
                .antigravity_active_turns
                .get(&session_id)
                .cloned()
                .or(session_turn_id)
                .unwrap_or_default();
            match drained {
                Ok(msgs) => {
                    let mut promoted_real: Option<String> = None;
                    for msg in msgs {
                        if self.normalize_antigravity_stream_msg(
                            &session_id,
                            &turn_id,
                            &msg,
                            &stderr_hint,
                            messages,
                        ) {
                            completed_sids.push(session_id.clone());
                            if let Some(last) = messages.last_mut() {
                                if last["params"].get("realSessionId").is_none()
                                    && let Some(ref real) = fallback_real
                                {
                                    last["params"]["realSessionId"] = json!(real);
                                }
                                if let Some(real) =
                                    last["params"].get("realSessionId").and_then(Value::as_str)
                                {
                                    promoted_real = Some(real.to_string());
                                }
                            }
                        }
                    }
                    if let Some(ref real) = promoted_real {
                        self.promote_antigravity_real_session_id(&session_id, real);
                    }
                    if is_dead {
                        dead_sids.push(promoted_real.unwrap_or(session_id));
                    }
                }
                Err(err) => {
                    let err_msg = format!("Antigravity 协议流异常: {err}");
                    if !turn_id.is_empty() {
                        messages.push(json!({
                            "method": "error",
                            "params": {
                                "threadId": &session_id,
                                "turnId": &turn_id,
                                "willRetry": false,
                                "error": { "message": &err_msg }
                            }
                        }));
                        messages.push(json!({
                            "method": "turn/completed",
                            "params": {
                                "threadId": &session_id,
                                "turn": {
                                    "id": &turn_id,
                                    "status": "failed",
                                    "error": err_msg
                                }
                            }
                        }));
                    }
                    completed_sids.push(session_id.clone());
                    dead_sids.push(session_id);
                }
            }
        }
        for sid in completed_sids {
            self.antigravity_active_turns.remove(&sid);
            self.antigravity_cancels.remove(&sid);
            self.antigravity_turn_emitted_text.remove(&sid);
            self.antigravity_turn_streamed_deltas.remove(&sid);
        }
        for sid in dead_sids {
            self.antigravity_sessions.remove(&sid);
        }
    }

    pub(crate) fn promote_antigravity_real_session_id(&mut self, temp_sid: &str, real_sid: &str) {
        let clean_real = real_sid.trim();
        if clean_real.is_empty() || clean_real == temp_sid {
            return;
        }
        let raw_real = clean_real
            .strip_prefix("antigravity_")
            .unwrap_or(clean_real);
        self.antigravity_known_real_ids
            .insert(clean_real.to_string());
        self.antigravity_known_real_ids.insert(raw_real.to_string());
        if let Some(session) = self.antigravity_sessions.remove(temp_sid) {
            self.antigravity_sessions
                .insert(clean_real.to_string(), session);
        }
        if let Some(cwd) = self.antigravity_cached_cwds.get(temp_sid).cloned() {
            self.antigravity_cached_cwds
                .insert(clean_real.to_string(), cwd.clone());
            self.antigravity_cached_cwds
                .insert(raw_real.to_string(), cwd);
        }
        if let Some(model) = self.antigravity_cached_models.get(temp_sid).cloned() {
            self.antigravity_cached_models
                .insert(clean_real.to_string(), model.clone());
            self.antigravity_cached_models
                .insert(raw_real.to_string(), model);
        }
    }
}
