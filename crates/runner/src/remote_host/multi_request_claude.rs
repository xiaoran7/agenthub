use super::*;

impl MultiProviderTransport {
    pub(crate) fn handle_claude_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, NativeError> {
        match method {
            "thread/read" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_id = session_id.trim_start_matches("claude_");
                let cached = self
                    .claude_cached_cwds
                    .get(session_id)
                    .or_else(|| self.claude_cached_cwds.get(raw_id))
                    .cloned();
                let in_memory = self.claude_sessions.contains_key(session_id)
                    || self.claude_active_turns.contains_key(session_id)
                    || cached.is_some();
                let disk_info = if !in_memory {
                    crate::collectors::find_claude_session_info(raw_id)
                        .ok()
                        .flatten()
                } else {
                    None
                };
                if raw_id.is_empty() || (!in_memory && disk_info.is_none()) {
                    return Err(NativeError::rejected("native_session_not_found"));
                }
                let (cwd, title) = if let Some(c) =
                    cached.filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
                {
                    (c, None)
                } else if let Some((c, t)) = disk_info
                    .or_else(|| {
                        crate::collectors::find_claude_session_info(raw_id)
                            .ok()
                            .flatten()
                    })
                    .filter(|(c, _)| !c.as_os_str().is_empty() && *c != Path::new("."))
                {
                    (c, t)
                } else if let Some(param_cwd) = params["cwd"]
                    .as_str()
                    .filter(|s| !s.is_empty() && *s != ".")
                {
                    let p = PathBuf::from(param_cwd);
                    let canon = dunce::canonicalize(&p).unwrap_or(p);
                    (canon, None)
                } else {
                    (PathBuf::from("."), None)
                };
                self.claude_cached_cwds
                    .insert(session_id.to_string(), cwd.clone());
                self.claude_cached_cwds
                    .insert(raw_id.to_string(), cwd.clone());
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" },
                        "name": title.unwrap_or_else(|| raw_id.to_string())
                    }
                }))
            }
            "thread/resume" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_id = session_id.trim_start_matches("claude_");
                let cached = self
                    .claude_cached_cwds
                    .get(session_id)
                    .or_else(|| self.claude_cached_cwds.get(raw_id))
                    .cloned();
                let in_memory = self.claude_sessions.contains_key(session_id)
                    || self.claude_active_turns.contains_key(session_id)
                    || cached.is_some();
                let disk_info = if !in_memory {
                    crate::collectors::find_claude_session_info(raw_id)
                        .ok()
                        .flatten()
                } else {
                    None
                };
                if raw_id.is_empty() || (!in_memory && disk_info.is_none()) {
                    return Err(NativeError::rejected("native_session_not_found"));
                }
                if let Some(m) = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.claude_cached_models
                        .insert(session_id.to_string(), m.to_string());
                }
                let cwd = cached
                    .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
                    .or_else(|| {
                        disk_info
                            .or_else(|| {
                                crate::collectors::find_claude_session_info(raw_id)
                                    .ok()
                                    .flatten()
                            })
                            .map(|(c, _)| c)
                            .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
                    })
                    .or_else(|| {
                        params["cwd"]
                            .as_str()
                            .filter(|s| !s.is_empty() && *s != ".")
                            .map(|s| {
                                let p = PathBuf::from(s);
                                dunce::canonicalize(&p).unwrap_or(p)
                            })
                    })
                    .unwrap_or_else(|| PathBuf::from("."));
                self.claude_cached_cwds
                    .insert(session_id.to_string(), cwd.clone());
                self.claude_cached_cwds
                    .insert(raw_id.to_string(), cwd.clone());
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" }
                    }
                }))
            }
            "turn/start" => {
                let session_id = params["threadId"].as_str().unwrap_or("").to_string();
                let raw_id = session_id.trim_start_matches("claude_").to_string();
                let session_exists = params.get("sessionExists").and_then(Value::as_bool);
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("")
                    .to_string();
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());
                self.claude_turn_emitted_text.remove(&session_id);
                self.claude_turn_streamed_deltas.remove(&session_id);
                self.claude_active_turns
                    .insert(session_id.clone(), turn_id.clone());
                let cancel = Arc::new(AtomicBool::new(false));
                self.claude_cancels
                    .insert(session_id.clone(), cancel.clone());

                let events_tx = self.claude_buffered_events.clone();
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
                    .claude_cached_cwds
                    .get(&session_id)
                    .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
                    .cloned()
                    .or_else(|| {
                        crate::collectors::find_claude_session_info(&raw_id)
                            .ok()
                            .flatten()
                            .map(|(c, _)| c)
                            .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
                    })
                    .or_else(|| {
                        params["cwd"]
                            .as_str()
                            .filter(|s| !s.is_empty() && *s != ".")
                            .map(PathBuf::from)
                    });
                if let Some(ref c) = cwd {
                    self.claude_cached_cwds
                        .insert(session_id.clone(), c.clone());
                }
                let model = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .or_else(|| self.claude_cached_models.get(&session_id).cloned());
                if let Some(m) = &model {
                    self.claude_cached_models
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
                let claude_desktop = self.is_claude_desktop_session(&session_id);

                let image_blocks: Vec<Value> = params
                    .get("imageBlocks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();

                // Attempt persistent bidirectional session driving
                if let Some(ref cwd_path) = cwd {
                    match self.get_or_spawn_claude_session(
                        &session_id,
                        cwd_path,
                        model.as_deref(),
                        effort.as_deref(),
                        permission_mode.as_deref(),
                        claude_desktop,
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
                            self.claude_sessions.remove(&session_id);
                        }
                        Err(_) => {
                            self.claude_sessions.remove(&session_id);
                        }
                    }
                }

                let binary = self.claude_binary.clone();
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
                                            "itemId": format!("claude_msg:{t_id}"),
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
                                            "itemId": format!("claude_msg:{t_id}"),
                                            "delta": msg
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::Activity(_) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/started",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "item": {
                                                "id": "claude_tool",
                                                "type": "commandExecution"
                                            }
                                        }
                                    }));
                                }
                            }
                        }
                    };

                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::collectors::run_agent_session_turn_streaming(
                            crate::domain::AgentType::ClaudeCode,
                            binary,
                            cwd.as_deref(),
                            &raw_id,
                            &text,
                            crate::collectors::TurnOptions {
                                model: model.as_deref(),
                                permission_mode: permission_mode.as_deref(),
                                reasoning_effort: effort.as_deref(),
                                session_exists,
                                claude_desktop,
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
                                        "itemId": format!("claude_msg:{turn_id_clone}"),
                                        "delta": &res.response
                                    }
                                }));
                            }
                            completed_params["turn"]["output"] = json!(&res.response);
                            if let Some(ref real_id) = res.session_id {
                                completed_params["realSessionId"] =
                                    json!(format!("claude_{}", real_id));
                            }
                            if let Some(usage) = res.usage {
                                q.push_back(json!({
                                    "method": "thread/tokenUsage/updated",
                                    "params": {
                                        "threadId": session_id_clone,
                                        "turnId": turn_id_clone,
                                        "tokenUsage": {
                                            "last": {"inputTokens": usage.used},
                                            "modelContextWindow": usage.window
                                        }
                                    }
                                }));
                            }
                        } else if !cancel.load(Ordering::Relaxed) {
                            let err_str = match outcome {
                                Ok(Err(ref err)) => err.to_string(),
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
                let has_session = if let Some(session) = self.claude_sessions.get_mut(session_id) {
                    let _ = session.interrupt();
                    true
                } else {
                    false
                };
                let has_cancel = if let Some(cancel) = self.claude_cancels.get(session_id) {
                    cancel.store(true, Ordering::SeqCst);
                    true
                } else {
                    false
                };
                // Keep active turn_id until `turn/completed` drains so NativeSessionHost matches turn.id.
                if !has_session && !has_cancel {
                    self.claude_active_turns.remove(session_id);
                }
                Ok(json!({"ok": true}))
            }
            "thread/archive" | "thread/unarchive" => Ok(json!({"ok": true})),
            _ => self.codex_request(method, params),
        }
    }
}
