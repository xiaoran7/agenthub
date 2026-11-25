use super::*;

fn format_antigravity_turn_error(err: &crate::collectors::CollectorError) -> String {
    match err {
        crate::collectors::CollectorError::Start { program, source }
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            format!(
                "未找到 Antigravity CLI 可执行文件 ({program})：当前仅发现桌面端历史会话，请安装官方 `agy` CLI 或设置环境变量 `ANTIGRAVITY_BINARY` 后重试。"
            )
        }
        other => other.to_string(),
    }
}

impl MultiProviderTransport {
    fn resolve_antigravity_cwd(
        &mut self,
        session_id: &str,
        param_cwd: Option<&str>,
    ) -> Option<(PathBuf, Option<String>, bool)> {
        let raw_id = session_id
            .strip_prefix("antigravity_")
            .unwrap_or(session_id);
        let cached = self
            .antigravity_cached_cwds
            .get(session_id)
            .or_else(|| self.antigravity_cached_cwds.get(raw_id))
            .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
            .cloned();
        let param_path = param_cwd
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != ".")
            .map(|s| {
                let p = PathBuf::from(s);
                dunce::canonicalize(&p).unwrap_or(p)
            });
        let detail = crate::collectors::find_antigravity_session_detail(raw_id)
            .ok()
            .flatten();

        if let Some(ref det) = detail
            && let Some(ref model) = det.model
            && !self.antigravity_cached_models.contains_key(session_id)
        {
            self.antigravity_cached_models
                .insert(session_id.to_string(), model.clone());
            self.antigravity_cached_models
                .insert(raw_id.to_string(), model.clone());
        }

        let resolved = cached
            .or_else(|| {
                detail
                    .as_ref()
                    .map(|d| d.cwd.clone())
                    .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
            })
            .or(param_path);
        if let Some(ref cwd) = resolved {
            self.antigravity_cached_cwds
                .insert(session_id.to_string(), cwd.clone());
            self.antigravity_cached_cwds
                .insert(raw_id.to_string(), cwd.clone());
        }
        let title = detail.as_ref().and_then(|d| d.title.clone());
        let is_running = detail.as_ref().is_some_and(|d| d.is_running)
            || self.antigravity_active_turns.contains_key(session_id);
        resolved.map(|cwd| (cwd, title, is_running))
    }

    fn resolve_existing_antigravity_session(
        &mut self,
        session_id: &str,
        param_cwd: Option<&str>,
    ) -> Result<(PathBuf, Option<String>, bool), NativeError> {
        let raw_id = session_id
            .strip_prefix("antigravity_")
            .unwrap_or(session_id);
        let in_memory = self.antigravity_sessions.contains_key(session_id)
            || self.antigravity_active_turns.contains_key(session_id)
            || self.antigravity_known_real_ids.contains(session_id)
            || self.antigravity_known_real_ids.contains(raw_id)
            || self.antigravity_cached_cwds.contains_key(session_id)
            || self.antigravity_cached_cwds.contains_key(raw_id);
        let detail = crate::collectors::find_antigravity_session_detail(raw_id)
            .ok()
            .flatten();
        let running_on_disk = crate::collectors::is_antigravity_session_running(raw_id);
        if raw_id.is_empty() || (!in_memory && detail.is_none() && !running_on_disk) {
            return Err(NativeError::rejected("native_session_not_found"));
        }
        let info = self.resolve_antigravity_cwd(session_id, param_cwd);
        Ok(match info {
            Some((cwd, title, is_running)) => (cwd, title, is_running || running_on_disk),
            None => (
                PathBuf::from("."),
                detail.and_then(|d| d.title),
                running_on_disk || self.antigravity_active_turns.contains_key(session_id),
            ),
        })
    }

    pub(crate) fn handle_antigravity_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, NativeError> {
        match method {
            "thread/read" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_id = session_id
                    .strip_prefix("antigravity_")
                    .unwrap_or(session_id);
                let (cwd, title, is_running) =
                    self.resolve_existing_antigravity_session(session_id, params["cwd"].as_str())?;
                let status = if is_running {
                    json!({ "type": "running" })
                } else {
                    json!({ "type": "idle" })
                };
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": status,
                        "name": title.unwrap_or_else(|| raw_id.to_string())
                    }
                }))
            }
            "thread/resume" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_id = session_id
                    .strip_prefix("antigravity_")
                    .unwrap_or(session_id);
                let (cwd, _, is_running) =
                    self.resolve_existing_antigravity_session(session_id, params["cwd"].as_str())?;
                if let Some(m) = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.antigravity_cached_models
                        .insert(session_id.to_string(), m.to_string());
                    self.antigravity_cached_models
                        .insert(raw_id.to_string(), m.to_string());
                }
                let status = if is_running {
                    json!({ "type": "running" })
                } else {
                    json!({ "type": "idle" })
                };
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": status
                    }
                }))
            }
            "turn/start" => {
                let session_id = params["threadId"].as_str().unwrap_or("").to_string();
                let raw_id = session_id
                    .strip_prefix("antigravity_")
                    .unwrap_or(&session_id)
                    .to_string();
                let session_exists = params.get("sessionExists").and_then(Value::as_bool);
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("")
                    .to_string();
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());
                self.antigravity_turn_emitted_text.remove(&session_id);
                self.antigravity_turn_streamed_deltas.remove(&session_id);
                self.antigravity_active_turns
                    .insert(session_id.clone(), turn_id.clone());

                let cancel = Arc::new(AtomicBool::new(false));
                self.antigravity_cancels
                    .insert(session_id.clone(), cancel.clone());

                let events_tx = self.antigravity_buffered_events.clone();
                if let Ok(mut q) = events_tx.lock() {
                    q.push_back(json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": session_id,
                            "turn": {
                                "id": turn_id.clone(),
                                "status": { "type": "inProgress" }
                            }
                        }
                    }));
                }

                let cwd = self
                    .resolve_antigravity_cwd(&session_id, params["cwd"].as_str())
                    .map(|(c, _, _)| c);
                let model = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .or_else(|| self.antigravity_cached_models.get(&session_id).cloned())
                    .or_else(|| self.antigravity_cached_models.get(&raw_id).cloned());
                if let Some(ref m) = model {
                    self.antigravity_cached_models
                        .insert(session_id.clone(), m.clone());
                }
                let effort = params["effort"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                let permission_mode = params["permissionMode"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                let image_blocks: Vec<Value> = params
                    .get("imageBlocks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();

                // Attempt persistent bidirectional `agy --input-format stream-json` session first.
                if let Some(ref cwd_path) = cwd
                    && crate::collectors::is_antigravity_cli_available(&self.antigravity_binary)
                {
                    match self.get_or_spawn_antigravity_session(
                        &session_id,
                        cwd_path,
                        model.as_deref(),
                        effort.as_deref(),
                        permission_mode.as_deref(),
                        session_exists,
                    ) {
                        Ok(session) => {
                            if session
                                .send_prompt_with_blocks(&turn_id, &text, &image_blocks)
                                .is_ok()
                            {
                                return Ok(json!({
                                    "turn": {
                                        "id": turn_id,
                                        "status": { "type": "inProgress" }
                                    }
                                }));
                            }
                            self.antigravity_sessions.remove(&session_id);
                        }
                        Err(_) => {
                            self.antigravity_sessions.remove(&session_id);
                        }
                    }
                }

                // Native Antigravity 2.0 Desktop daemon (`language_server` ConnectRPC) path
                if self.allow_antigravity_daemon_fallback()
                    && crate::collectors::antigravity::discover_live_antigravity_daemon().is_some()
                {
                    let s_id = session_id.clone();
                    let r_id = raw_id.clone();
                    let t_id = turn_id.clone();
                    let cwd_clone = cwd.clone();
                    let model_clone = model.clone();
                    let img_clone = image_blocks.clone();
                    std::thread::spawn(move || {
                        let outcome =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                crate::collectors::antigravity::run_antigravity_daemon_turn(
                                    crate::collectors::antigravity::DaemonTurnSpec {
                                        session_id: &s_id,
                                        raw_id: &r_id,
                                        turn_id: &t_id,
                                        cwd: cwd_clone.as_deref(),
                                        text: &text,
                                        image_blocks: &img_clone,
                                        model: model_clone.as_deref(),
                                        session_exists,
                                        cancel: &cancel,
                                        events_queue: &events_tx,
                                    },
                                )
                            }));
                        if let Ok(mut q) = events_tx.lock() {
                            let status_type = if cancel.load(Ordering::Relaxed)
                                || matches!(
                                    outcome,
                                    Ok(Err(crate::collectors::CollectorError::Cancelled { .. }))
                                ) {
                                "interrupted"
                            } else if matches!(outcome, Ok(Ok(_))) {
                                "completed"
                            } else {
                                "failed"
                            };
                            let mut completed_params = json!({
                                "threadId": s_id,
                                "turn": {
                                    "id": t_id,
                                    "status": status_type
                                }
                            });
                            if let Ok(Ok(ref real_raw)) = outcome {
                                completed_params["realSessionId"] =
                                    json!(format!("antigravity_{real_raw}"));
                            } else if status_type == "failed" {
                                let err_str = match outcome {
                                    Ok(Err(ref err)) => format_antigravity_turn_error(err),
                                    _ => "Antigravity 桌面语言服务执行异常中断".to_string(),
                                };
                                q.push_back(json!({
                                    "method": "error",
                                    "params": {
                                        "threadId": s_id,
                                        "turnId": t_id,
                                        "willRetry": false,
                                        "error": { "message": &err_str }
                                    }
                                }));
                                completed_params["turn"]["error"] = json!(err_str);
                            }
                            q.push_back(json!({
                                "method": "turn/completed",
                                "params": completed_params
                            }));
                        }
                    });
                    return Ok(json!({
                        "turn": {
                            "id": turn_id,
                            "status": { "type": "inProgress" }
                        }
                    }));
                }

                // Fallback: single-turn streaming child process.
                let binary = self.antigravity_binary.clone();
                let session_id_clone = session_id.clone();
                let turn_id_clone = turn_id.clone();
                let emitted_text = Arc::new(AtomicBool::new(false));
                let emitted_text_cb = emitted_text.clone();

                std::thread::spawn(move || {
                    let events_queue = events_tx.clone();
                    let s_id = session_id_clone.clone();
                    let t_id = turn_id_clone.clone();
                    let on_update = {
                        let events_queue = events_queue.clone();
                        let s_id = s_id.clone();
                        let t_id = t_id.clone();
                        let emitted_text_cb = emitted_text_cb.clone();
                        move |update: crate::collectors::AgentTurnUpdate| match update {
                            crate::collectors::AgentTurnUpdate::AssistantDelta(delta) => {
                                if !delta.is_empty() {
                                    emitted_text_cb.store(true, Ordering::Relaxed);
                                }
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "antigravity_msg",
                                            "delta": delta
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::AssistantMessage(msg) => {
                                if !msg.is_empty() {
                                    emitted_text_cb.store(true, Ordering::Relaxed);
                                }
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "antigravity_msg",
                                            "delta": msg
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::Activity(activity) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/started",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "item": {
                                                "id": "antigravity_tool",
                                                "type": "commandExecution",
                                                "toolName": activity
                                            }
                                        }
                                    }));
                                }
                            }
                        }
                    };

                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::collectors::run_agent_session_turn_streaming(
                            crate::domain::AgentType::Antigravity,
                            binary,
                            cwd.as_deref(),
                            &raw_id,
                            &text,
                            crate::collectors::TurnOptions {
                                model: model.as_deref(),
                                permission_mode: permission_mode.as_deref(),
                                reasoning_effort: effort.as_deref(),
                                session_exists,
                                claude_desktop: false,
                            },
                            Duration::from_secs(600),
                            &cancel,
                            on_update,
                        )
                    }));

                    if let Ok(mut q) = events_queue.lock() {
                        let status_type = if cancel.load(Ordering::Relaxed) {
                            "interrupted"
                        } else if matches!(outcome, Ok(Ok(_))) {
                            "completed"
                        } else {
                            "failed"
                        };
                        let mut completed_params = json!({
                            "threadId": session_id_clone,
                            "turn": {
                                "id": turn_id_clone,
                                "status": status_type
                            }
                        });
                        if let Ok(Ok(ref res)) = outcome {
                            if !emitted_text.load(Ordering::Relaxed) && !res.response.is_empty() {
                                q.push_back(json!({
                                    "method": "item/agentMessage/delta",
                                    "params": {
                                        "threadId": session_id_clone,
                                        "turnId": turn_id_clone,
                                        "itemId": "antigravity_msg",
                                        "delta": &res.response
                                    }
                                }));
                            }
                            completed_params["turn"]["output"] = json!(&res.response);
                            if let Some(ref real_id) = res.session_id {
                                let normalized = if real_id.starts_with("antigravity_") {
                                    real_id.clone()
                                } else {
                                    format!("antigravity_{real_id}")
                                };
                                completed_params["realSessionId"] = json!(normalized);
                            }
                            if let Some(usage) = res.usage {
                                let window = usage.window.or_else(|| {
                                    model.as_deref().map(
                                        crate::collectors::antigravity::resolve_antigravity_context_window,
                                    )
                                });
                                q.push_back(json!({
                                    "method": "thread/tokenUsage/updated",
                                    "params": {
                                        "threadId": session_id_clone,
                                        "turnId": turn_id_clone,
                                        "tokenUsage": {
                                            "used": usage.used,
                                            "last": {"inputTokens": usage.used},
                                            "window": window,
                                            "modelContextWindow": window
                                        }
                                    }
                                }));
                            }
                        } else if !cancel.load(Ordering::Relaxed) {
                            let err_str = match outcome {
                                Ok(Err(ref err)) => format_antigravity_turn_error(err),
                                _ => "进程执行异常中断".to_string(),
                            };
                            q.push_back(json!({
                                "method": "error",
                                "params": {
                                    "threadId": session_id_clone,
                                    "turnId": turn_id_clone,
                                    "willRetry": false,
                                    "error": { "message": &err_str }
                                }
                            }));
                            completed_params["turn"]["error"] = json!(err_str);
                        }
                        q.push_back(json!({
                            "method": "turn/completed",
                            "params": completed_params
                        }));
                    }
                });

                Ok(json!({
                    "turn": {
                        "id": turn_id,
                        "status": { "type": "inProgress" }
                    }
                }))
            }
            "turn/interrupt" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let has_session =
                    if let Some(session) = self.antigravity_sessions.get_mut(session_id) {
                        let _ = session.interrupt();
                        true
                    } else {
                        false
                    };
                let has_cancel = if let Some(cancel) = self.antigravity_cancels.get(session_id) {
                    cancel.store(true, Ordering::SeqCst);
                    true
                } else {
                    false
                };
                let daemon_cancelled =
                    crate::collectors::antigravity::cancel_daemon_cascade(session_id);
                // Keep active turn_id until `turn/completed` drains so NativeSessionHost matches turn.id.
                if !has_session && !has_cancel && !daemon_cancelled {
                    self.antigravity_active_turns.remove(session_id);
                }
                Ok(json!({"ok": true}))
            }
            "thread/archive" | "thread/unarchive" => Ok(json!({"ok": true})),
            _ => self.codex_request(method, params),
        }
    }
}
