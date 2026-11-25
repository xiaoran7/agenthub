use super::*;

impl MultiProviderTransport {
    pub(crate) fn handle_drain(&mut self) -> Result<Vec<Value>, NativeError> {
        let mut messages: Vec<Value> = Vec::new();
        let mut codex_lost = false;

        while let Some(evt) = self.codex_buffered_events.pop_front() {
            messages.push(evt);
        }

        if let Some(ref mut codex) = self.codex {
            match codex.drain_messages() {
                Ok(codex_msgs) => {
                    for msg in codex_msgs {
                        let method = msg.get("method").and_then(Value::as_str);
                        let thread_id = msg
                            .pointer("/params/threadId")
                            .or_else(|| msg.pointer("/params/thread/id"))
                            .and_then(Value::as_str);
                        match method {
                            Some("turn/started") => {
                                if let (Some(tid), Some(turn_id)) = (
                                    thread_id,
                                    msg.pointer("/params/turn/id").and_then(Value::as_str),
                                ) {
                                    self.codex_active_turns
                                        .insert(tid.to_string(), turn_id.to_string());
                                }
                            }
                            Some("turn/completed") | Some("thread/closed") => {
                                if let Some(tid) = thread_id {
                                    self.codex_active_turns.remove(tid);
                                    self.codex_pending_requests
                                        .retain(|_, (owner, _)| owner != tid);
                                }
                            }
                            Some("serverRequest/resolved") => {
                                if let Some(req_id) = msg.pointer("/params/requestId") {
                                    self.codex_pending_requests.remove(&req_id.to_string());
                                }
                            }
                            Some(_) if msg.get("id").is_some() => {
                                if let (Some(tid), Some(req_id)) = (thread_id, msg.get("id")) {
                                    self.codex_pending_requests.insert(
                                        req_id.to_string(),
                                        (tid.to_string(), req_id.clone()),
                                    );
                                }
                            }
                            _ => {}
                        }
                        messages.push(msg);
                    }
                }
                Err(e) => {
                    eprintln!("[MultiProviderTransport] codex drain error: {:?}", e);
                    codex_lost = true;
                }
            }
        }
        if codex_lost {
            // Isolate Codex app-server loss so healthy Claude, Antigravity, and ACP
            // sessions keep running without bringing down the whole host, while
            // failing active Codex turns and clearing pending Codex requests.
            self.handle_codex_transport_loss();
            while let Some(evt) = self.codex_buffered_events.pop_front() {
                messages.push(evt);
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

        let mut dead_acp_sessions: Vec<(String, String)> = Vec::new();
        for (session_id, server) in self.acp_servers.iter_mut() {
            if let Some(plugin_id) = self.acp_session_plugins.get(session_id)
                && !crate::plugins::is_plugin_enabled(plugin_id)
            {
                dead_acp_sessions.push((session_id.clone(), "插件已禁用，会话已终止".to_string()));
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
                    dead_acp_sessions.push((session_id.clone(), format!("ACP 协议流异常: {e}")));
                }
            }
        }
        for (sid, err_msg) in dead_acp_sessions {
            self.acp_servers.remove(&sid);
            self.acp_session_plugins.remove(&sid);
            if let Some(turn_id) = self.acp_active_turns.remove(&sid) {
                messages.push(json!({
                    "method": "error",
                    "params": {
                        "threadId": sid,
                        "turnId": turn_id,
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

        Ok(messages)
    }
}
