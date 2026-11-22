use super::*;

impl MultiProviderTransport {
    pub(crate) fn handle_drain(&mut self) -> Result<Vec<Value>, NativeError> {
        let mut messages: Vec<Value> = Vec::new();
        let mut codex_lost = false;

        if let Some(ref mut codex) = self.codex {
            match codex.drain_messages() {
                Ok(codex_msgs) => messages.extend(codex_msgs),
                Err(e) => {
                    eprintln!("[MultiProviderTransport] codex drain error: {:?}", e);
                    codex_lost = true;
                }
            }
        }

        if let Ok(mut claude_events) = self.claude_buffered_events.lock() {
            while let Some(evt) = claude_events.pop_front() {
                if evt.get("method").and_then(Value::as_str) == Some("turn/completed")
                    && let Some(sid) = evt["params"]["threadId"].as_str()
                {
                    self.claude_active_turns.remove(sid);
                    self.claude_cancels.remove(sid);
                }
                messages.push(evt);
            }
        }

        // Drain active persistent Claude sessions

        self.drain_claude_process_sessions(&mut messages);

        let mut buffered_promotions = Vec::new();
        if let Ok(mut antigravity_events) = self.antigravity_buffered_events.lock() {
            while let Some(evt) = antigravity_events.pop_front() {
                if evt.get("method").and_then(Value::as_str) == Some("turn/completed")
                    && let Some(sid) = evt["params"]["threadId"].as_str()
                {
                    self.antigravity_active_turns.remove(sid);
                    self.antigravity_cancels.remove(sid);
                    if let Some(real) = evt["params"].get("realSessionId").and_then(Value::as_str) {
                        buffered_promotions.push((sid.to_string(), real.to_string()));
                    }
                }
                messages.push(evt);
            }
        }
        for (temp_sid, real_sid) in buffered_promotions {
            self.promote_antigravity_real_session_id(&temp_sid, &real_sid);
        }

        self.drain_antigravity_process_sessions(&mut messages);

        while let Some(evt) = self.acp_buffered_events.pop_front() {
            messages.push(evt);
        }

        let mut dead_acp_sessions = Vec::new();
        for (session_id, server) in self.acp_servers.iter_mut() {
            if let Some(plugin_id) = self.acp_session_plugins.get(session_id)
                && !crate::plugins::is_plugin_enabled(plugin_id)
            {
                dead_acp_sessions.push(session_id.clone());
                continue;
            }
            match server.drain_messages() {
                Ok(acp_msgs) => {
                    for mut msg in acp_msgs {
                        if let Some(params) = msg.get_mut("params") {
                            params["threadId"] = json!(session_id);
                        }
                        if let Some(req_id_str) = msg.get("id").and_then(Value::as_str)
                            && let Some(actual_id) = req_id_str.strip_prefix("acp_req_")
                        {
                            self.acp_pending_requests
                                .insert(actual_id.to_string(), session_id.clone());
                        }
                        if msg.get("method").and_then(Value::as_str) == Some("turn/completed") {
                            self.acp_active_turns.remove(session_id);
                        }
                        messages.push(msg);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[MultiProviderTransport] acp drain error for {}: {:?}",
                        session_id, e
                    );
                    dead_acp_sessions.push(session_id.clone());
                }
            }
        }
        for sid in dead_acp_sessions {
            self.acp_servers.remove(&sid);
            self.acp_session_plugins.remove(&sid);
            self.acp_active_turns.remove(&sid);
        }

        // Only mark transport lost if codex itself dropped AND there are no other active provider streams
        if codex_lost
            && self.acp_active_turns.is_empty()
            && self.claude_active_turns.is_empty()
            && self.antigravity_active_turns.is_empty()
        {
            return Err(NativeError::uncertain("provider_transport_lost"));
        }

        Ok(messages)
    }
}
