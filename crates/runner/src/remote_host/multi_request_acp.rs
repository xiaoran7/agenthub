use super::*;

impl MultiProviderTransport {
    pub(crate) fn handle_acp_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, NativeError> {
        match method {
            "thread/read" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_session_id = session_id.strip_prefix("acp_").unwrap_or(session_id);
                let exists = !raw_session_id.is_empty()
                    && (self.acp_servers.contains_key(session_id)
                        || self.acp_active_turns.contains_key(session_id)
                        || self.acp_cached_cwds.contains_key(session_id));
                if !exists {
                    return Err(NativeError::rejected("native_session_not_found"));
                }
                let cwd = self
                    .acp_cached_cwds
                    .get(session_id)
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from("."));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" },
                        "name": session_id
                    }
                }))
            }
            "thread/resume" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_session_id = session_id.strip_prefix("acp_").unwrap_or(session_id);
                let exists = !raw_session_id.is_empty()
                    && (self.acp_servers.contains_key(session_id)
                        || self.acp_active_turns.contains_key(session_id)
                        || self.acp_cached_cwds.contains_key(session_id));
                if !exists {
                    return Err(NativeError::rejected("native_session_not_found"));
                }
                let cwd = self
                    .acp_cached_cwds
                    .get(session_id)
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from("."));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" }
                    }
                }))
            }
            "turn/start" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_session_id = session_id.strip_prefix("acp_").unwrap_or(session_id);
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("");
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());

                let send_res = {
                    let server = self
                        .acp_servers
                        .get_mut(session_id)
                        .ok_or_else(|| NativeError::rejected("acp_session_not_found"))?;
                    server.send_tracked_prompt(raw_session_id, text, &turn_id)
                };
                if let Err(e) = send_res {
                    self.acp_servers.remove(session_id);
                    self.acp_active_turns.remove(session_id);
                    return Err(NativeError::rejected(format!("acp_send_failed: {}", e)));
                }

                self.acp_active_turns
                    .insert(session_id.to_string(), turn_id.clone());
                self.acp_buffered_events.push_back(json!({
                    "method": "turn/started",
                    "params": {
                        "threadId": session_id,
                        "turn": {
                            "id": turn_id.clone(),
                            "status": { "type": "inProgress" }
                        }
                    }
                }));

                Ok(json!({
                    "turn": {
                        "id": turn_id,
                        "status": { "type": "inProgress" }
                    }
                }))
            }
            "turn/interrupt" => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_session_id = session_id.strip_prefix("acp_").unwrap_or(session_id);
                let has_server = if let Some(server) = self.acp_servers.get_mut(session_id) {
                    let _ = server.session_cancel(raw_session_id);
                    true
                } else {
                    false
                };
                // Keep active turn_id until `turn/completed` drains so NativeSessionHost matches turn.id.
                if !has_server {
                    self.acp_active_turns.remove(session_id);
                }
                Ok(json!({"ok": true}))
            }
            "thread/archive" | "thread/unarchive" => Ok(json!({"ok": true})),
            _ => self.codex_request(method, params),
        }
    }
}
