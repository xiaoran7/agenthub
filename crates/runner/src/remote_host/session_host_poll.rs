use super::*;

impl<T: NativeTransport> NativeSessionHost<T> {
    pub fn poll(&mut self) -> Result<Vec<HostEvent>, NativeError> {
        if !self.alive {
            return Ok(Vec::new());
        }
        let mut events: Vec<HostEvent> = Vec::new();
        while let Some(mut evt) = self.pending_historical_events.pop_front() {
            if let Some(id) = evt.hub_session_id.as_ref() {
                evt.binding = self.sessions.get(id).cloned();
            }
            events.push(evt);
            if events.len() >= 64 {
                break;
            }
        }
        let messages = match self.transport.drain() {
            Ok(messages) => messages,
            Err(error) => {
                self.mark_lost();
                return Err(error);
            }
        };
        for raw in messages {
            let evt = self.consume(raw);
            if evt.hub_session_id.is_none() && is_ephemeral_provider_noise(&evt.kind) {
                continue;
            }
            events.push(evt);
        }
        let now = Instant::now();
        self.recheck_codex_release(now);
        self.release_idle_control(now);
        if let Some(home) = crate::collectors::home_dir() {
            events.extend(self.live_mirror.poll_sessions(
                &home,
                &self.generation,
                &self.sessions,
                now,
            ));
        }
        Ok(events)
    }

    pub(crate) fn mark_lost(&mut self) {
        self.alive = false;
        self.control_idle_since.clear();
        self.control_release_checks.clear();
        self.approvals.clear();
        self.questions.clear();
        for binding in self.sessions.values_mut() {
            binding.control_connected = false;
            binding.execution_state = ExecutionState::Unknown;
        }
    }

    pub(crate) fn consume(&mut self, raw: Value) -> HostEvent {
        let method = raw["method"].as_str().unwrap_or("provider_response");
        let params = &raw["params"];
        let native_id = params["threadId"]
            .as_str()
            .or_else(|| params["thread"]["id"].as_str())
            .map(str::to_owned);
        let hub_id = native_id
            .as_ref()
            .and_then(|id| self.native_to_hub.get(id))
            .cloned();
        let mut approval_id: Option<String> = None;
        let mut approval: Option<PendingApprovalSnapshot> = None;
        let mut resolved_question_id: Option<String> = None;
        let mut question: Option<PendingQuestionSnapshot> = None;
        let mut kind = method.to_string();
        if let Some(binding) = hub_id.as_ref().and_then(|id| self.sessions.get_mut(id)) {
            match method {
                "thread/closed" if binding.execution_state == ExecutionState::ReleasingControl => {
                    self.control_idle_since.remove(&binding.hub_session_id);
                    self.control_release_checks.remove(&binding.hub_session_id);
                    self.newly_created.remove(&binding.hub_session_id);
                    binding.control_connected = false;
                    binding.execution_state = ExecutionState::Unknown;
                    binding.current_turn_id = None;
                    kind = "session_control_changed".into();
                }
                _ if !binding.control_connected
                    && super::session_control::is_codex_thread_id(&binding.provider_session_id) => {
                }
                "thread/status/changed" => {
                    let previous = binding.execution_state;
                    binding.execution_state = execution_state(&params["status"]);
                    if previous != binding.execution_state
                        && let Some(since) =
                            self.control_idle_since.get_mut(&binding.hub_session_id)
                    {
                        *since = Instant::now();
                    }
                }
                "turn/started" => {
                    binding.current_turn_id = params["turn"]["id"].as_str().map(str::to_owned);
                    binding.execution_state = turn_state(&params["turn"]["status"]);
                }
                // Delayed events for an older turn must not end a newer turn.
                "turn/completed" => {
                    if params["turn"]["id"].as_str() == binding.current_turn_id.as_deref()
                        || binding.current_turn_id.is_none()
                    {
                        binding.execution_state = turn_state(&params["turn"]["status"]);
                        binding.current_turn_id = None;
                        if let Some(since) =
                            self.control_idle_since.get_mut(&binding.hub_session_id)
                        {
                            *since = Instant::now();
                        }
                        self.approvals.retain(|_, approval| {
                            approval.hub_session_id != binding.hub_session_id
                        });
                        self.questions
                            .retain(|_, q| q.hub_session_id != binding.hub_session_id);
                        self.live_mirror
                            .defer_after_local_turn(&binding.hub_session_id, Instant::now());
                    }
                    if let Some(real_id) = params.get("realSessionId").and_then(Value::as_str) {
                        let real_id = real_id.trim();
                        if !real_id.is_empty() && binding.provider_session_id != real_id {
                            self.native_to_hub.remove(&binding.provider_session_id);
                            binding.provider_session_id = real_id.to_string();
                            self.native_to_hub
                                .insert(real_id.to_string(), binding.hub_session_id.clone());
                        }
                    }
                }
                "serverRequest/resolved" => {
                    if let Some(id) = self.approvals.iter().find_map(|(id, approval)| {
                        (approval.hub_session_id == binding.hub_session_id
                            && approval.request_id == params["requestId"])
                            .then(|| id.clone())
                    }) {
                        self.approvals.remove(&id);
                        approval_id = Some(id);
                        kind = "approval_resolved".into();
                    }
                    if let Some(id) = self.questions.iter().find_map(|(id, q)| {
                        (q.hub_session_id == binding.hub_session_id
                            && q.request_id == params["requestId"])
                            .then(|| id.clone())
                    }) {
                        self.questions.remove(&id);
                        resolved_question_id = Some(id);
                        kind = "question_resolved".into();
                    }
                }
                _ if raw.get("id").is_some() && raw.get("method").is_some() => {
                    let request_id = raw["id"].clone();
                    let turn_id = params["turnId"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .or(binding.current_turn_id.as_deref())
                        .unwrap_or("");
                    if binding.current_turn_id.as_deref() == Some(turn_id)
                        || binding.current_turn_id.is_none()
                    {
                        let mut digest = Sha256::new();
                        digest.update(b"agenthub.approval.v1\0");
                        digest.update(self.generation.as_bytes());
                        digest.update([0]);
                        digest.update(binding.provider_session_id.as_bytes());
                        digest.update([0]);
                        digest.update(
                            serde_json::to_vec(&request_id).unwrap_or_else(|_| b"invalid".to_vec()),
                        );
                        let id = format!("approval-{:x}", digest.finalize());
                        let is_command_approval = method == "item/commandExecution/requestApproval"
                            || method == "execCommandApproval"
                            || method == "item/mcpToolCall/requestApproval";
                        let is_file_approval = method == "item/fileChange/requestApproval"
                            || method == "applyPatchApproval";
                        let is_question_request = method == "item/askUserQuestion"
                            || method == "item/question"
                            || method == "client/askQuestion"
                            || method == "item/tool/requestUserInput";

                        if is_question_request {
                            let first_q = params
                                .get("questions")
                                .and_then(Value::as_array)
                                .and_then(|arr| arr.first());
                            let codex_question_id = (method == "item/tool/requestUserInput")
                                .then(|| first_q.and_then(|q| q["id"].as_str()).map(str::to_owned))
                                .flatten();
                            let prompt = params["prompt"]
                                .as_str()
                                .or_else(|| params["question"].as_str())
                                .or_else(|| first_q.and_then(|q| q["question"].as_str()))
                                .unwrap_or("Agent 提出了一个问题")
                                .to_string();
                            let header = params["header"]
                                .as_str()
                                .or_else(|| first_q.and_then(|q| q["header"].as_str()))
                                .map(str::to_owned);
                            let multi_select = params["multiSelect"]
                                .as_bool()
                                .or_else(|| first_q.and_then(|q| q["multiSelect"].as_bool()));
                            let raw_opts = params["options"]
                                .as_array()
                                .or_else(|| first_q.and_then(|q| q["options"].as_array()));
                            let options = raw_opts.map(|arr| {
                                arr.iter()
                                    .filter_map(|opt| {
                                        opt["label"].as_str().map(|label| QuestionOptionSnapshot {
                                            label: label.to_string(),
                                            description: opt["description"]
                                                .as_str()
                                                .map(str::to_owned),
                                            preview: opt["preview"].as_str().map(str::to_owned),
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            });
                            let q_snapshot = PendingQuestionSnapshot {
                                question_id: id.clone(),
                                hub_session_id: binding.hub_session_id.clone(),
                                host_generation: self.generation.clone(),
                                turn_id: turn_id.into(),
                                prompt: prompt.clone(),
                                header: header.clone(),
                                options: options.clone(),
                                multi_select,
                            };
                            if !self.answered_requests.contains(&id) {
                                binding.execution_state = ExecutionState::AwaitingInput;
                                self.questions.insert(
                                    id.clone(),
                                    PendingQuestion {
                                        hub_session_id: binding.hub_session_id.clone(),
                                        turn_id: turn_id.into(),
                                        request_id,
                                        prompt,
                                        header,
                                        codex_question_id,
                                        options,
                                        multi_select,
                                    },
                                );
                                question = Some(q_snapshot);
                                kind = "question_requested".into();
                            }
                        } else {
                            let supported = is_command_approval || is_file_approval;
                            if !self.answered_requests.contains(&id) {
                                binding.execution_state = if supported {
                                    ExecutionState::AwaitingPermission
                                } else {
                                    ExecutionState::AwaitingInput
                                };
                                let available_decisions =
                                    params["availableDecisions"].as_array().cloned();
                                let tool_name =
                                    params["toolName"].as_str().map(str::to_owned).or_else(|| {
                                        if is_command_approval {
                                            Some("Bash".to_string())
                                        } else if is_file_approval {
                                            Some("FileChange".to_string())
                                        } else {
                                            None
                                        }
                                    });
                                let command_preview = params["command"].as_str().map(str::to_owned);
                                let path = params["filePath"].as_str().map(str::to_owned);
                                let diff = params["diff"]
                                    .as_str()
                                    .map(|d| sanitize_approval_diff(d, 65_536));
                                let description = params["reason"].as_str().map(str::to_owned);
                                let risk_level = if is_command_approval {
                                    Some("high".to_string())
                                } else if is_file_approval {
                                    Some("medium".to_string())
                                } else {
                                    None
                                };

                                let descriptor = PendingApprovalSnapshot {
                                    approval_id: id.clone(),
                                    hub_session_id: binding.hub_session_id.clone(),
                                    host_generation: self.generation.clone(),
                                    turn_id: turn_id.into(),
                                    can_approve: available_decisions
                                        .as_ref()
                                        .is_none_or(|values| values.contains(&json!("accept"))),
                                    can_decline: available_decisions
                                        .as_ref()
                                        .is_none_or(|values| values.contains(&json!("decline"))),
                                    tool_name: tool_name.clone(),
                                    command_preview: command_preview.clone(),
                                    path: path.clone(),
                                    diff: diff.clone(),
                                    description: description.clone(),
                                    risk_level: risk_level.clone(),
                                };
                                self.approvals.entry(id.clone()).or_insert_with(|| {
                                    PendingApproval {
                                        hub_session_id: binding.hub_session_id.clone(),
                                        turn_id: turn_id.into(),
                                        request_id,
                                        supported,
                                        available_decisions,
                                        tool_name,
                                        command_preview,
                                        path,
                                        diff,
                                        description,
                                        risk_level,
                                    }
                                });
                                approval_id = Some(id);
                                if supported {
                                    approval = Some(descriptor);
                                }
                                kind = if supported {
                                    "approval_requested"
                                } else {
                                    "unsupported_provider_request"
                                }
                                .into();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let question_id = question
            .as_ref()
            .map(|q| q.question_id.clone())
            .or(resolved_question_id);
        let binding = hub_id
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .cloned();
        let effective_native_id = binding
            .as_ref()
            .map(|b| b.provider_session_id.clone())
            .or(native_id);
        HostEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            host_generation: self.generation.clone(),
            binding,
            hub_session_id: hub_id,
            provider_session_id: effective_native_id,
            kind,
            raw,
            approval_id,
            approval,
            question_id,
            question,
        }
    }
}

fn redact_json_strings_recursive(value: &mut Value) {
    match value {
        Value::String(s) => {
            *s = crate::privacy::redact_paths_inside_text(s);
        }
        Value::Array(items) => {
            for item in items {
                redact_json_strings_recursive(item);
            }
        }
        Value::Object(map) => {
            for val in map.values_mut() {
                redact_json_strings_recursive(val);
            }
        }
        _ => {}
    }
}

fn shrink_longest_json_string(value: &mut Value, excess: usize) -> bool {
    fn find_longest_len(value: &Value) -> usize {
        match value {
            Value::String(s) => s.len(),
            Value::Array(items) => items.iter().map(find_longest_len).max().unwrap_or(0),
            Value::Object(map) => map.values().map(find_longest_len).max().unwrap_or(0),
            _ => 0,
        }
    }
    fn shrink_matching(value: &mut Value, target_len: usize, excess: usize) -> bool {
        match value {
            Value::String(s) if s.len() == target_len && !s.is_empty() => {
                let shrink = (excess / 2).max(1).min(s.len());
                let next_len = s.len().saturating_sub(shrink);
                crate::collectors::truncate_utf8_bytes(s, next_len);
                true
            }
            Value::Array(items) => items
                .iter_mut()
                .any(|item| shrink_matching(item, target_len, excess)),
            Value::Object(map) => map
                .values_mut()
                .any(|val| shrink_matching(val, target_len, excess)),
            _ => false,
        }
    }
    let longest = find_longest_len(value);
    longest > 0 && shrink_matching(value, longest, excess)
}

pub(crate) fn sanitize_approval_diff(raw: &str, max_bytes: usize) -> String {
    if let Ok(mut parsed) = serde_json::from_str::<Value>(raw)
        && parsed.is_object()
    {
        redact_json_strings_recursive(&mut parsed);
        for _ in 0..24 {
            let encoded = parsed.to_string();
            if encoded.len() <= max_bytes {
                return encoded;
            }
            let excess = encoded.len() - max_bytes;
            if !shrink_longest_json_string(&mut parsed, excess) {
                break;
            }
        }
        let encoded = parsed.to_string();
        if encoded.len() <= max_bytes {
            return encoded;
        }
        return "{}".to_string();
    }
    let mut sanitized = crate::privacy::redact_paths_inside_text(raw);
    crate::collectors::truncate_utf8_bytes(&mut sanitized, max_bytes);
    sanitized
}

fn is_ephemeral_provider_noise(kind: &str) -> bool {
    matches!(
        kind,
        "skills/changed"
            | "mcpServer/startupStatus/updated"
            | "account/rateLimits/updated"
            | "account/updated"
            | "deprecationNotice"
            | "configWarning"
            | "warning"
            | "fuzzyFileSearch/sessionUpdated"
            | "fuzzyFileSearch/sessionCompleted"
            | "thread/archived"
            | "thread/unarchived"
    )
}

#[cfg(test)]
mod tests {
    use super::is_ephemeral_provider_noise;

    #[test]
    fn json_approval_diff_redacts_nested_paths_and_stays_valid_json_within_budget() {
        let big = format!("see /Users/alice/secret/app.rs {}", "\"".repeat(24_000));
        let raw_json = serde_json::json!({
            "old_string": big,
            "new_string": "updated C:\\Users\\bob\\repo\\main.rs"
        })
        .to_string();
        let sanitized = super::sanitize_approval_diff(&raw_json, 65_536);
        assert!(
            sanitized.len() <= 65_536,
            "sanitized json diff exceeded 65536 bytes: {}",
            sanitized.len()
        );
        let reparsed: serde_json::Value =
            serde_json::from_str(&sanitized).expect("json diff must stay valid JSON");
        assert!(
            !reparsed["old_string"]
                .as_str()
                .unwrap()
                .contains("/Users/alice/secret")
        );
        assert!(!reparsed["new_string"].as_str().unwrap().contains("bob"));
    }

    #[test]
    fn ephemeral_provider_noise_matches_known_housekeeping_notifications() {
        assert!(is_ephemeral_provider_noise("skills/changed"));
        assert!(is_ephemeral_provider_noise(
            "mcpServer/startupStatus/updated"
        ));
        assert!(!is_ephemeral_provider_noise("turn/started"));
        assert!(!is_ephemeral_provider_noise("item/agentMessage/delta"));
    }
}
