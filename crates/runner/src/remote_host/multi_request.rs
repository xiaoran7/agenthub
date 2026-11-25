use super::*;

impl MultiProviderTransport {
    pub(crate) fn codex_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, NativeError> {
        let req_thread_id = params
            .get("threadId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(ref mut codex) = self.codex {
            match codex.request(method, params) {
                Ok(resp) => {
                    if method == "turn/start"
                        && let (Some(tid), Some(turn_id)) = (
                            req_thread_id,
                            resp.pointer("/turn/id").and_then(Value::as_str),
                        )
                    {
                        self.codex_active_turns.insert(tid, turn_id.to_string());
                    }
                    Ok(resp)
                }
                Err(crate::collectors::CollectorError::Protocol { ref message, .. }) => {
                    if method == "thread/resume" && message.contains("already has an active writer")
                    {
                        Err(NativeError::rejected("native_writer_busy"))
                    } else {
                        Err(NativeError::rejected("provider_rejected"))
                    }
                }
                Err(_) => {
                    self.handle_codex_transport_loss();
                    Err(NativeError::rejected("codex_provider_lost"))
                }
            }
        } else {
            Err(NativeError::rejected("provider_not_available"))
        }
    }

    pub(crate) fn remember_discovered_session(
        &mut self,
        id: &str,
        cwd: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) {
        let provider_id = |prefix| id.trim_start_matches(prefix).to_owned();
        if id.starts_with("claude_") {
            if !self.claude_cached_cwds.contains_key(id) {
                self.claude_cached_cwds
                    .insert_if_room(id.to_owned(), PathBuf::from(cwd));
                self.claude_cached_cwds
                    .insert_if_room(provider_id("claude_"), PathBuf::from(cwd));
            }
            if provider == Some("claude_desktop") && !self.claude_desktop_sessions.contains(id) {
                self.claude_desktop_sessions.insert_if_room(id.to_owned());
                self.claude_desktop_sessions
                    .insert_if_room(provider_id("claude_"));
            }
        } else if id.starts_with("antigravity_") {
            if !self.antigravity_cached_cwds.contains_key(id) {
                self.antigravity_cached_cwds
                    .insert_if_room(id.to_owned(), PathBuf::from(cwd));
                self.antigravity_cached_cwds
                    .insert_if_room(provider_id("antigravity_"), PathBuf::from(cwd));
            }
            if !self.antigravity_cached_models.contains_key(id)
                && let Some(model) = model.map(str::trim).filter(|value| !value.is_empty())
            {
                self.antigravity_cached_models
                    .insert_if_room(id.to_owned(), model.to_owned());
                self.antigravity_cached_models
                    .insert_if_room(provider_id("antigravity_"), model.to_owned());
            }
        } else if let Some(plugin) = crate::plugins::find_enabled_plugin_for_session(id) {
            let prefix = plugin.manifest.effective_session_prefix();
            if !self.acp_cached_cwds.contains_key(id) {
                self.acp_cached_cwds
                    .insert_if_room(id.to_owned(), PathBuf::from(cwd));
            }
            if !prefix.is_empty() && !id.starts_with(&prefix) {
                self.acp_cached_cwds
                    .insert_if_room(format!("{prefix}{id}"), PathBuf::from(cwd));
            }
        }
    }

    pub(crate) fn handle_request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, NativeError> {
        let session_id = params
            .get("threadId")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .unwrap_or("");

        let is_claude = session_id.starts_with("claude_");
        let is_antigravity = session_id.starts_with("antigravity_");
        let is_acp = session_id.starts_with("acp_");
        let agent_type = params
            .get("agentType")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        if is_claude {
            return self.handle_claude_request(method, params);
        }
        if is_antigravity {
            return self.handle_antigravity_request(method, params);
        }
        if is_acp {
            return self.handle_acp_request(method, params);
        }
        if !session_id.is_empty()
            && crate::plugins::find_enabled_plugin_for_session(session_id).is_some()
        {
            return self.handle_plugin_request("", method, params);
        }

        match method {
            "thread/start"
                if !agent_type.is_empty()
                    && !matches!(
                        agent_type.as_str(),
                        "codex" | "claude_code" | "claude_desktop" | "antigravity" | "acp"
                    ) =>
            {
                if agent_type == "deepseek_harness" {
                    return Err(NativeError::rejected(
                        "deepseek_harness_runtime_not_implemented",
                    ));
                }
                self.handle_plugin_request(&agent_type, method, params)
            }
            "thread/start" if agent_type == "claude_code" || agent_type == "claude_desktop" => {
                let cwd = params["cwd"].as_str().unwrap_or(".").to_string();
                let session_id = format!("claude_{}", uuid::Uuid::new_v4());
                self.claude_cached_cwds
                    .insert(session_id.clone(), PathBuf::from(&cwd));
                if agent_type == "claude_desktop" {
                    self.claude_desktop_sessions.insert(session_id.clone());
                }
                if let Some(m) = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.claude_cached_models
                        .insert(session_id.clone(), m.to_string());
                }
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/start" if agent_type == "antigravity" => {
                let cwd = params["cwd"].as_str().unwrap_or(".").to_string();
                let session_id = format!("antigravity_{}", uuid::Uuid::new_v4());
                self.antigravity_cached_cwds
                    .insert(session_id.clone(), PathBuf::from(&cwd));
                if let Some(m) = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.antigravity_cached_models
                        .insert(session_id.clone(), m.to_string());
                }
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/start" if agent_type == "acp" => {
                let cwd = params["cwd"].as_str().unwrap_or(".").to_string();
                let cwd_path = PathBuf::from(&cwd);
                let binary_path = crate::collectors::discover_acp_binary()
                    .ok_or_else(|| NativeError::rejected("acp_binary_not_found"))?;

                let mut server = AcpAppServer::start(&binary_path, &[], Duration::from_secs(20))
                    .map_err(|e| NativeError::rejected(format!("acp_start_failed: {}", e)))?;

                let native_id = server
                    .session_new(&cwd_path)
                    .map_err(|e| NativeError::rejected(format!("acp_session_new_failed: {}", e)))?;

                let session_id = format!("acp_{}", native_id);
                self.acp_cached_cwds.insert(session_id.clone(), cwd_path);
                self.acp_servers.insert(session_id.clone(), server);

                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/list" => {
                let is_first_page = params
                    .get("cursor")
                    .is_none_or(|c| c.is_null() || c.as_str().is_none_or(|s| s.is_empty()));
                let mut response = if let Some(ref mut codex) = self.codex {
                    match codex.request("thread/list", params) {
                        Ok(resp) => resp,
                        Err(crate::collectors::CollectorError::Protocol { .. }) => {
                            return Err(NativeError::rejected("provider_rejected"));
                        }
                        Err(_) => {
                            self.handle_codex_transport_loss();
                            crate::collectors::collect_codex_local_threads(5000)
                                .unwrap_or_else(|_| json!({"data": [], "nextCursor": null}))
                        }
                    }
                } else {
                    // A missing app-server must not hide locally discoverable Codex
                    // sessions. Metadata-only candidates cannot be resumed here.
                    crate::collectors::collect_codex_local_threads(5000)
                        .unwrap_or_else(|_| json!({"data": [], "nextCursor": null}))
                };
                if let Some(items) = response["data"].as_array_mut() {
                    for item in items.iter_mut() {
                        item["provider"] = json!("codex");
                        item["agent_name"] = json!("codex");
                        item["resumable"] = json!(self.codex.is_some());
                    }
                    crate::collectors::enrich_codex_thread_items(items);
                }
                if is_first_page {
                    // Ensure the initial local store scan has completed before returning,
                    // so providers (like Codex) and local stores (Claude, Grok, ZCode) are
                    // synchronized and returned together without partial display.
                    ensure_initial_local_store_scan(Duration::from_millis(2000));
                    start_local_store_scan();
                    wait_for_active_local_store_scan(Duration::from_millis(120));
                    if let Some(items) = response["data"].as_array_mut() {
                        items.extend(cached_local_store_sessions().iter().cloned());
                    }
                }
                if let Some(items) = response["data"].as_array_mut() {
                    items.sort_by_key(|item| {
                        let ms = item
                            .get("last_activity_at_ms")
                            .and_then(Value::as_i64)
                            .or_else(|| item.get("updatedAt").and_then(Value::as_i64))
                            .or_else(|| item.get("time_updated").and_then(Value::as_i64))
                            .or_else(|| item.get("recencyAt").and_then(Value::as_i64))
                            .or_else(|| item.get("createdAt").and_then(Value::as_i64))
                            .unwrap_or(0);
                        std::cmp::Reverse(ms)
                    });
                    for item in items.iter() {
                        if let (Some(id), Some(cwd)) = (item["id"].as_str(), item["cwd"].as_str()) {
                            self.remember_discovered_session(
                                id,
                                cwd,
                                item["provider"].as_str(),
                                item["model"].as_str(),
                            );
                        }
                    }
                }
                Ok(response)
            }
            "model/list" => {
                if let Some((cached_at, ref cached)) = self.codex_models_cache
                    && cached_at.elapsed() < Duration::from_secs(30)
                {
                    return Ok(cached.clone());
                }
                if let Some(ref mut codex) = self.codex {
                    let mut resp = match codex.request(method, params) {
                        Ok(resp) => resp,
                        Err(crate::collectors::CollectorError::Protocol { .. }) => {
                            return Err(NativeError::rejected("provider_rejected"));
                        }
                        Err(_) => {
                            self.handle_codex_transport_loss();
                            return Err(NativeError::rejected("codex_provider_lost"));
                        }
                    };
                    crate::model_catalog::enrich_codex_live_models(&mut resp);
                    self.codex_models_cache = Some((Instant::now(), resp.clone()));
                    Ok(resp)
                } else {
                    Err(NativeError::rejected("provider_not_available"))
                }
            }
            _ => self.codex_request(method, params),
        }
    }
}
