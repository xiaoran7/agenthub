use super::session_control::{configure_codex_unload, is_codex_thread_id};
use super::*;

const MAX_PROMPT_ATTACHMENTS: usize = 16;
const MAX_TOTAL_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

fn write_private_attachment(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes)
    }
}

impl<T: NativeTransport> NativeSessionHost<T> {
    pub(crate) fn execute(&mut self, command: &RemoteCommand) -> Result<Value, NativeError> {
        let hub_id = &command.hub_session_id;
        if command.host_generation.as_deref() != Some(&self.generation) {
            return Err(NativeError::rejected("host_generation_mismatch"));
        }
        if let RemoteAction::Connect {
            mode,
            workspace_path,
            provider_session_id,
            model,
            sandbox,
            approval_policy,
            agent_type,
            permission_mode,
        } = &command.command
        {
            let is_codex = provider_session_id
                .as_deref()
                .is_some_and(is_codex_thread_id);
            if is_codex && *mode == ConnectMode::Resume {
                return Err(NativeError::rejected("explicit_control_required"));
            }
            if !is_codex && matches!(mode, ConnectMode::Preview | ConnectMode::TakeControl) {
                return Err(NativeError::rejected("session_control_unsupported"));
            }
            if *mode == ConnectMode::Attach {
                return Err(NativeError::rejected("attach_not_supported"));
            }
            if let Some(existing) = self.sessions.get(hub_id).cloned() {
                if *mode == ConnectMode::Create
                    || provider_session_id.as_ref() != Some(&existing.provider_session_id)
                {
                    return Err(NativeError::rejected("session_already_bound"));
                }
                if *mode != ConnectMode::Preview
                    && existing.execution_state == ExecutionState::ReleasingControl
                {
                    return Err(NativeError::rejected("session_control_releasing"));
                }
                if existing.control_connected && *mode != ConnectMode::Preview {
                    // Already connected to this exact native session; re-enqueue history and context usage idempotently.
                    self.enqueue_session_history_and_usage(&existing, None);
                    return Ok(json!({
                        "binding": existing,
                        "provider_response": {
                            "thread": {
                                "id": existing.provider_session_id,
                                "cwd": existing.workspace_path,
                                "status": { "type": "idle" }
                            }
                        }
                    }));
                }
            }
            let ws_str = workspace_path.to_string_lossy();
            let norm_path = |p: &str| -> String {
                p.trim_start_matches(r"\\?\")
                    .replace('/', "\\")
                    .trim_end_matches('\\')
                    .to_lowercase()
            };
            let ws_norm = norm_path(&ws_str);
            let cwd = self
                .workspaces
                .iter()
                .find(|w| {
                    let w_str = w.to_string_lossy();
                    crate::privacy::workspace_id_for_path(&w_str) == ws_str
                        || (*mode != ConnectMode::Create && norm_path(&w_str) == ws_norm)
                })
                .cloned()
                .or_else(|| {
                    if *mode != ConnectMode::Create {
                        self.discovered_workspaces
                            .get(ws_str.as_ref())
                            .or_else(|| self.discovered_workspaces.get(&ws_norm))
                            .or_else(|| {
                                let clean = ws_str.trim_start_matches(r"\\?\");
                                self.discovered_workspaces.get(clean)
                            })
                            .or_else(|| {
                                let computed_id = crate::privacy::workspace_id_for_path(&ws_str);
                                self.discovered_workspaces.get(&computed_id)
                            })
                            .or_else(|| {
                                self.discovered_workspaces
                                    .values()
                                    .find(|p| norm_path(&p.to_string_lossy()) == ws_norm)
                            })
                            .filter(|p| p.is_dir())
                            .cloned()
                    } else {
                        None
                    }
                })
                .filter(|cwd| cwd.is_dir())
                .ok_or_else(|| NativeError::rejected("workspace_not_authorized"))?;
            if model.as_ref().is_some_and(|model| model.trim().is_empty()) {
                return Err(NativeError::rejected("invalid_model"));
            }
            let (effective_sandbox, effective_policy) = match permission_mode.as_deref() {
                Some("plan") => (CodexSandbox::ReadOnly, CodexApprovalPolicy::OnRequest),
                Some("acceptEdits") => {
                    (CodexSandbox::WorkspaceWrite, CodexApprovalPolicy::OnRequest)
                }
                Some("bypassPermissions") => {
                    (CodexSandbox::DangerFullAccess, CodexApprovalPolicy::Never)
                }
                _ => (*sandbox, *approval_policy),
            };
            let mut params = json!({"cwd": cwd, "sandbox": effective_sandbox, "approvalPolicy": effective_policy,
                "approvalsReviewer": "user"});
            if let Some(model) = model {
                params["model"] = json!(model);
            }
            if let Some(at) = agent_type {
                params["agentType"] = json!(at);
            }
            if let Some(pm) = permission_mode
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                params["permissionMode"] = json!(pm);
            }
            let mut resume_turns = None;
            let response = match mode {
                ConnectMode::Create => {
                    if provider_session_id.is_some() {
                        return Err(NativeError::rejected("create_cannot_set_native_id"));
                    }
                    params["experimentalRawEvents"] = json!(false);
                    if agent_type.as_deref().is_none_or(|kind| kind == "codex") {
                        configure_codex_unload(&mut params);
                    }
                    self.transport.request("thread/start", params)?
                }
                ConnectMode::Resume | ConnectMode::Preview | ConnectMode::TakeControl => {
                    let id = provider_session_id
                        .as_ref()
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| NativeError::rejected("native_id_required"))?;
                    if self
                        .native_to_hub
                        .get(id)
                        .is_some_and(|bound| bound != hub_id)
                    {
                        return Err(NativeError::rejected("native_session_already_bound"));
                    }
                    // Read does not take control. Never resume a known active thread.
                    let history = self.transport.request(
                        "thread/read",
                        json!({"threadId": id, "includeTurns": !is_codex, "cwd": cwd}),
                    )?;
                    resume_turns = history.get("thread").and_then(|t| t.get("turns")).cloned();
                    let thread = &history["thread"];
                    if thread["id"].as_str() != Some(id) {
                        return Err(NativeError::rejected("native_id_mismatch"));
                    }
                    if *mode != ConnectMode::Preview
                        && !matches!(
                            thread["status"]["type"].as_str(),
                            Some("notLoaded" | "idle")
                        )
                    {
                        return Err(NativeError::rejected("external_session_not_resumable"));
                    }
                    let native_cwd = thread["cwd"]
                        .as_str()
                        .filter(|path| !path.is_empty() && *path != ".");
                    if is_codex && native_cwd.is_none() {
                        return Err(NativeError::rejected("resume_workspace_mismatch"));
                    }
                    if let Some(native_cwd) = native_cwd {
                        let matches = if cfg!(windows) {
                            norm_path(native_cwd) == ws_norm
                                || norm_path(native_cwd) == norm_path(&cwd.to_string_lossy())
                        } else {
                            Path::new(native_cwd) == cwd
                        };
                        if !matches && dunce::canonicalize(native_cwd).ok().as_ref() != Some(&cwd) {
                            return Err(NativeError::rejected("resume_workspace_mismatch"));
                        }
                    }
                    if *mode == ConnectMode::Preview {
                        history
                    } else {
                        params["threadId"] = json!(id);
                        if is_codex {
                            configure_codex_unload(&mut params);
                            params["excludeTurns"] = json!(true);
                        }
                        self.transport.request("thread/resume", params)?
                    }
                }
                ConnectMode::Attach => unreachable!(),
            };
            let id = response["thread"]["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| NativeError::uncertain("provider_missing_native_id"))?
                .to_string();
            if provider_session_id
                .as_ref()
                .is_some_and(|expected| expected != &id)
                || self
                    .native_to_hub
                    .get(&id)
                    .is_some_and(|bound| bound != hub_id)
            {
                return Err(NativeError::uncertain("provider_identity_conflict"));
            }
            let existing = self.sessions.get(hub_id).cloned();
            let binding = if *mode == ConnectMode::Preview {
                existing.unwrap_or_else(|| SessionBinding {
                    hub_session_id: hub_id.clone(),
                    provider_session_id: id.clone(),
                    workspace_path: cwd.clone(),
                    host_generation: self.generation.clone(),
                    control_connected: false,
                    execution_state: ExecutionState::Unknown,
                    current_turn_id: None,
                })
            } else {
                SessionBinding {
                    hub_session_id: hub_id.clone(),
                    provider_session_id: id.clone(),
                    workspace_path: cwd.clone(),
                    host_generation: self.generation.clone(),
                    control_connected: true,
                    execution_state: execution_state(&response["thread"]["status"]),
                    current_turn_id: None,
                }
            };
            self.native_to_hub.insert(id.clone(), hub_id.clone());
            self.sessions.insert(hub_id.clone(), binding.clone());
            if *mode == ConnectMode::TakeControl {
                self.control_idle_since
                    .insert(hub_id.clone(), Instant::now());
                self.enqueue_control_change(&binding);
            }
            if *mode == ConnectMode::Create {
                self.newly_created.insert(hub_id.clone());
            }

            let turns_source = response["thread"]["turns"]
                .as_array()
                .filter(|turns| !turns.is_empty())
                .or_else(|| resume_turns.as_ref().and_then(Value::as_array));
            if *mode != ConnectMode::Create || turns_source.is_some() {
                self.enqueue_session_history_and_usage(&binding, turns_source);
            }

            // Bound provider_response in receipt: IPC frame limit is MAX_DAEMON_FRAME_BYTES (1MB).
            // Retain thread summary (id, cwd, status) and discard large historical turns array.
            let mut sanitized_response = response.clone();
            if let Some(thread_obj) = sanitized_response
                .get_mut("thread")
                .and_then(Value::as_object_mut)
            {
                thread_obj.remove("turns");
            }
            return Ok(json!({"binding": binding, "provider_response": sanitized_response}));
        }

        let session = self
            .sessions
            .get(hub_id)
            .cloned()
            .ok_or_else(|| NativeError::rejected("session_not_bound"))?;
        if matches!(command.command, RemoteAction::ReleaseControl) {
            return self
                .release_codex_control(&session)
                .map(|binding| json!({ "binding": binding }));
        }
        if !session.control_connected
            && !matches!(
                command.command,
                RemoteAction::SendPrompt {
                    acquire_control: true,
                    ..
                }
            )
        {
            return Err(NativeError::rejected("session_control_lost"));
        }
        match &command.command {
            RemoteAction::SendPrompt {
                text,
                acquire_control,
                effort,
                model,
                permission_mode,
                attachments,
            } => {
                if (text.trim().is_empty() && attachments.is_empty()) || text.len() > 512 * 1024 {
                    return Err(NativeError::rejected("invalid_prompt"));
                }
                if attachments.len() > MAX_PROMPT_ATTACHMENTS {
                    return Err(NativeError::rejected("too_many_attachments"));
                }
                let total_attachment_bytes: usize = attachments
                    .iter()
                    .try_fold(0usize, |acc, att| acc.checked_add(att.data.len()))
                    .unwrap_or(usize::MAX);
                if total_attachment_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
                    return Err(NativeError::rejected("attachments_too_large"));
                }
                if text.trim_start().starts_with('/') {
                    return Err(NativeError::rejected("slash_not_supported"));
                }
                if session.current_turn_id.is_some()
                    && matches!(
                        session.execution_state,
                        ExecutionState::Running
                            | ExecutionState::AwaitingPermission
                            | ExecutionState::AwaitingInput
                    )
                {
                    return Err(NativeError::rejected("session_busy_or_unknown"));
                }
                if !session.workspace_path.is_dir() {
                    return Err(NativeError::rejected("workspace_unavailable"));
                }
                let acquired = !session.control_connected && *acquire_control;
                if acquired {
                    self.take_control_for_prompt(
                        command,
                        &session,
                        model.clone(),
                        permission_mode.clone(),
                    )?;
                }
                if let Some(binding) = self.sessions.get_mut(hub_id) {
                    binding.current_turn_id = None;
                }
                let is_newly_created = self.newly_created.remove(hub_id);
                let mut effective_text = text.clone();
                let mut image_blocks: Vec<Value> = Vec::new();
                let mut codex_extra_inputs: Vec<Value> = Vec::new();
                if !attachments.is_empty() {
                    let attach_base = crate::collectors::home_dir()
                        .unwrap_or_else(std::env::temp_dir)
                        .join(".agenthub")
                        .join("tmp")
                        .join("attachments");
                    let attach_dir = attach_base.join(hub_id);
                    let _ = std::fs::create_dir_all(&attach_dir);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(
                            &attach_base,
                            std::fs::Permissions::from_mode(0o700),
                        );
                        let _ = std::fs::set_permissions(
                            &attach_dir,
                            std::fs::Permissions::from_mode(0o700),
                        );
                    }
                    // Prune stale temporary attachments older than 24 hours.
                    if let Ok(entries) = std::fs::read_dir(&attach_dir) {
                        let cutoff = std::time::SystemTime::now()
                            .checked_sub(std::time::Duration::from_secs(24 * 3600));
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_file()
                                && let Some(cutoff_ts) = cutoff
                                && entry
                                    .metadata()
                                    .and_then(|m| m.modified())
                                    .is_ok_and(|mtime| mtime < cutoff_ts)
                            {
                                let _ = std::fs::remove_file(&path);
                            }
                        }
                    }
                    let cmd_prefix: String = command
                        .client_cmd_id
                        .chars()
                        .filter(char::is_ascii_alphanumeric)
                        .take(12)
                        .collect();
                    let cmd_prefix = if cmd_prefix.is_empty() {
                        "turn".to_string()
                    } else {
                        cmd_prefix
                    };
                    let mut attachment_notes = Vec::new();
                    for (idx, att) in attachments.iter().enumerate() {
                        let safe_name: String = att
                            .name
                            .chars()
                            .map(|c| {
                                if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                                    c
                                } else {
                                    '_'
                                }
                            })
                            .take(96)
                            .collect();
                        let file_name = if safe_name.is_empty() {
                            format!("{cmd_prefix}_attachment_{idx}")
                        } else {
                            format!("{cmd_prefix}_{idx}_{safe_name}")
                        };
                        let file_path = attach_dir.join(&file_name);
                        let is_base64 = att.encoding.as_deref() == Some("base64");
                        if is_base64 {
                            let saved_ok = decode_base64_bytes(&att.data).is_some_and(|bytes| {
                                write_private_attachment(&file_path, &bytes).is_ok()
                            });
                            if att.media_type.starts_with("image/") {
                                image_blocks.push(json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": att.media_type,
                                        "data": att.data.trim(),
                                    }
                                }));
                                if saved_ok {
                                    codex_extra_inputs.push(json!({
                                        "type": "localImage",
                                        "path": file_path.to_string_lossy(),
                                    }));
                                }
                            }
                            if saved_ok {
                                attachment_notes.push(format!(
                                    "[附件 {}: {} (已保存至本地临时路径: {})]",
                                    att.name,
                                    att.media_type,
                                    file_path.display()
                                ));
                            } else {
                                attachment_notes
                                    .push(format!("[附件 {}: {}]", att.name, att.media_type));
                            }
                        } else {
                            let saved_ok =
                                write_private_attachment(&file_path, att.data.as_bytes()).is_ok();
                            if att.data.len() <= 64 * 1024 {
                                attachment_notes.push(format!(
                                    "[附件 {} ({})]:\n```\n{}\n```",
                                    att.name,
                                    file_path.display(),
                                    att.data
                                ));
                            } else if saved_ok {
                                attachment_notes.push(format!(
                                    "[附件 {}: 已保存至本地临时路径 {}]",
                                    att.name,
                                    file_path.display()
                                ));
                            }
                        }
                    }
                    if !attachment_notes.is_empty() {
                        let prefix = attachment_notes.join("\n\n");
                        effective_text = if effective_text.trim().is_empty() {
                            prefix
                        } else {
                            format!("{prefix}\n\n{effective_text}")
                        };
                    }
                }
                let mut input_items = vec![json!({
                    "type": "text",
                    "text": effective_text,
                    "text_elements": []
                })];
                input_items.extend(codex_extra_inputs);
                let mut turn_params = json!({
                    "threadId": session.provider_session_id,
                    "cwd": session.workspace_path.to_string_lossy(),
                    "input": input_items
                });
                if !image_blocks.is_empty() {
                    turn_params["imageBlocks"] = json!(image_blocks);
                }
                if is_newly_created {
                    turn_params["sessionExists"] = json!(false);
                }
                // Reasoning effort is a per-turn override on Codex; omitting it
                // keeps whatever the thread was started with.
                if let Some(effort) = effort
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    turn_params["effort"] = json!(effort);
                }
                if let Some(model) = model
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    turn_params["model"] = json!(model);
                }
                if let Some(mode) = permission_mode
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    turn_params["permissionMode"] = json!(mode);
                }
                let mut result = self
                    .transport
                    .request("turn/start", turn_params)
                    .inspect_err(|error| {
                        if error.outcome_unknown
                            && let Some(binding) = self.sessions.get_mut(hub_id)
                        {
                            binding.execution_state = ExecutionState::Unknown;
                        }
                    })?;
                let turn_id = result["turn"]["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| NativeError::uncertain("provider_missing_turn_id"))?;
                let binding = self.sessions.get_mut(hub_id).expect("binding checked");
                binding.current_turn_id = Some(turn_id.to_string());
                binding.execution_state = turn_state(&result["turn"]["status"]);
                if acquired {
                    result["binding"] = json!(binding);
                }
                Ok(result)
            }
            RemoteAction::ResolveApproval {
                approval_id,
                approved,
            } => {
                let approval = self
                    .approvals
                    .get(approval_id)
                    .ok_or_else(|| NativeError::rejected("approval_not_pending"))?;
                if approval.hub_session_id != *hub_id
                    || session.current_turn_id.as_ref() != Some(&approval.turn_id)
                {
                    return Err(NativeError::rejected("approval_session_mismatch"));
                }
                if !approval.supported {
                    return Err(NativeError::rejected("approval_type_not_supported"));
                }
                let decision = if *approved { "accept" } else { "decline" };
                if approval
                    .available_decisions
                    .as_ref()
                    .is_some_and(|allowed| !allowed.contains(&json!(decision)))
                {
                    return Err(NativeError::rejected("approval_decision_not_available"));
                }
                self.transport
                    .respond(approval.request_id.clone(), json!({"decision": decision}))?;
                self.approvals.remove(approval_id);
                self.mark_answered(&approval_id.clone());
                // Native notifications, not the act of sending a response, update execution state.
                Ok(json!({"approval_id": approval_id, "response_sent": true}))
            }
            RemoteAction::AnswerQuestion {
                question_id,
                answer,
            } => {
                let question = self
                    .questions
                    .get(question_id)
                    .ok_or_else(|| NativeError::rejected("question_not_pending"))?;
                if question.hub_session_id != *hub_id
                    || session.current_turn_id.as_ref() != Some(&question.turn_id)
                {
                    return Err(NativeError::rejected("question_session_mismatch"));
                }
                // Build multi-provider compatible user-input answer payload (Claude + Codex v2 + Antigravity)
                let answer_vec: Vec<Value> = if let Some(arr) = answer.as_array() {
                    arr.clone()
                } else if let Some(s) = answer.as_str() {
                    vec![json!(s)]
                } else {
                    vec![answer.clone()]
                };
                let mut answers_map = serde_json::Map::new();
                if let Some(ref qid) = question.codex_question_id
                    && !qid.is_empty()
                {
                    answers_map.insert(qid.clone(), json!({ "answers": answer_vec }));
                } else {
                    answers_map.insert(question.prompt.clone(), answer.clone());
                }
                self.transport.respond(
                    question.request_id.clone(),
                    json!({
                        "answers": answers_map,
                        "response": answer
                    }),
                )?;
                self.questions.remove(question_id);
                self.mark_answered(&question_id.clone());
                Ok(json!({"question_id": question_id, "response_sent": true}))
            }
            RemoteAction::SetArchived { archived } => {
                if session.current_turn_id.is_some()
                    && matches!(
                        session.execution_state,
                        ExecutionState::Running
                            | ExecutionState::AwaitingPermission
                            | ExecutionState::AwaitingInput
                    )
                {
                    return Err(NativeError::rejected("session_busy_or_unknown"));
                }
                self.transport.request(
                    if *archived {
                        "thread/archive"
                    } else {
                        "thread/unarchive"
                    },
                    json!({"threadId": session.provider_session_id}),
                )
            }
            RemoteAction::Interrupt => {
                if let Some(ref turn_id) = session.current_turn_id {
                    self.transport.request(
                        "turn/interrupt",
                        json!({"threadId": session.provider_session_id, "turnId": turn_id}),
                    )
                } else {
                    let _ = self.transport.request(
                        "turn/interrupt",
                        json!({"threadId": session.provider_session_id}),
                    );
                    if let Some(binding) = self.sessions.get_mut(hub_id) {
                        binding.current_turn_id = None;
                        binding.execution_state = ExecutionState::Idle;
                    }
                    self.approvals
                        .retain(|_, approval| approval.hub_session_id != *hub_id);
                    self.questions.retain(|_, q| q.hub_session_id != *hub_id);
                    Ok(json!({"ok": true, "interrupted": true}))
                }
            }
            RemoteAction::Connect { .. } | RemoteAction::ReleaseControl => unreachable!(),
        }
    }
}
