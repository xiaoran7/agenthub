use super::*;

impl MultiProviderTransport {
    pub(crate) fn handle_plugin_request(
        &mut self,
        plugin_id_hint: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, NativeError> {
        let session_id_param = params
            .get("threadId")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .unwrap_or("");

        let rec = if !plugin_id_hint.trim().is_empty() {
            crate::plugins::get_enabled_plugin(plugin_id_hint)
        } else if !session_id_param.is_empty() {
            crate::plugins::find_enabled_plugin_for_session(session_id_param)
        } else {
            None
        }
        .ok_or_else(|| NativeError::rejected("plugin_disabled"))?;

        let prefix = rec.manifest.effective_session_prefix();

        match method {
            "thread/start" => {
                if !rec.manifest.capabilities.launch {
                    return Err(NativeError::rejected("plugin_launch_unsupported"));
                }
                let cwd_str = params["cwd"].as_str().unwrap_or(".");
                let cwd_path = PathBuf::from(cwd_str);
                let model = params
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let permission_mode = params
                    .get("permissionMode")
                    .or_else(|| params.get("approvalPolicy"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());

                let (_, mut server) = crate::plugins::global_manager()
                    .spawn_acp_server(&rec.id, Duration::from_secs(20))
                    .map_err(|err| NativeError::rejected(format!("plugin_start_failed:{err}")))?;

                let raw_id = server
                    .session_new_with_options(&cwd_path, model, permission_mode)
                    .map_err(|err| {
                        NativeError::rejected(format!("plugin_session_new_failed:{err}"))
                    })?;

                let session_id = if raw_id.starts_with(&prefix) {
                    raw_id
                } else {
                    format!("{prefix}{raw_id}")
                };
                self.acp_cached_cwds.insert(session_id.clone(), cwd_path);
                self.acp_session_plugins
                    .insert(session_id.clone(), rec.id.clone());
                self.acp_servers.insert(session_id.clone(), server);

                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd_str,
                        "provider": rec.id,
                        "agent_name": rec.id,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/read" => {
                let raw_id = session_id_param
                    .strip_prefix(&prefix)
                    .unwrap_or(session_id_param);
                if raw_id.is_empty() {
                    return Err(NativeError::rejected("native_session_not_found"));
                }
                let in_memory = self.acp_servers.contains_key(session_id_param)
                    || self.acp_active_turns.contains_key(session_id_param)
                    || self.acp_cached_cwds.contains_key(session_id_param);
                if !in_memory {
                    let discovered_cwd =
                        self.find_discovered_plugin_session_cwd(&rec, session_id_param, raw_id);
                    let Some(cwd_opt) = discovered_cwd else {
                        return Err(NativeError::rejected("native_session_not_found"));
                    };
                    if let Some(discovered) = cwd_opt {
                        self.acp_cached_cwds
                            .insert(session_id_param.to_string(), discovered);
                    }
                }
                let cwd = self.resolve_plugin_cwd(session_id_param, &params);
                self.acp_cached_cwds
                    .insert(session_id_param.to_string(), cwd.clone());
                Ok(json!({
                    "thread": {
                        "id": session_id_param,
                        "cwd": cwd.to_string_lossy(),
                        "provider": rec.id,
                        "agent_name": rec.id,
                        "status": { "type": "idle" },
                        "name": session_id_param
                    }
                }))
            }
            "thread/resume" => {
                if !rec.manifest.capabilities.resume {
                    return Err(NativeError::rejected("plugin_resume_unsupported"));
                }
                let raw_id = session_id_param
                    .strip_prefix(&prefix)
                    .unwrap_or(session_id_param);
                if raw_id.is_empty() {
                    return Err(NativeError::rejected("native_session_not_found"));
                }
                let cwd = self.resolve_plugin_cwd(session_id_param, &params);
                if !self.acp_servers.contains_key(session_id_param) {
                    let (_, mut server) = crate::plugins::global_manager()
                        .spawn_acp_server(&rec.id, Duration::from_secs(20))
                        .map_err(|err| {
                            NativeError::rejected(format!("plugin_start_failed:{err}"))
                        })?;
                    let cwd_opt = (!cwd.as_os_str().is_empty() && cwd != Path::new("."))
                        .then_some(cwd.as_path());
                    server
                        .session_load_with_cwd(raw_id, cwd_opt)
                        .map_err(|err| {
                            NativeError::rejected(format!("plugin_resume_failed:{err}"))
                        })?;
                    self.acp_session_plugins
                        .insert(session_id_param.to_string(), rec.id.clone());
                    self.acp_servers
                        .insert(session_id_param.to_string(), server);
                }
                self.acp_cached_cwds
                    .insert(session_id_param.to_string(), cwd.clone());
                Ok(json!({
                    "thread": {
                        "id": session_id_param,
                        "cwd": cwd.to_string_lossy(),
                        "provider": rec.id,
                        "agent_name": rec.id,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "turn/start" => {
                let cwd = self.resolve_plugin_cwd(session_id_param, &params);
                let raw_id = session_id_param
                    .strip_prefix(&prefix)
                    .unwrap_or(session_id_param);
                if !self.acp_servers.contains_key(session_id_param) {
                    let (_, mut server) = crate::plugins::global_manager()
                        .spawn_acp_server(&rec.id, Duration::from_secs(20))
                        .map_err(|err| {
                            NativeError::rejected(format!("plugin_start_failed:{err}"))
                        })?;
                    let cwd_opt = (!cwd.as_os_str().is_empty() && cwd != Path::new("."))
                        .then_some(cwd.as_path());
                    server
                        .session_load_with_cwd(raw_id, cwd_opt)
                        .map_err(|err| {
                            NativeError::rejected(format!("plugin_resume_failed:{err}"))
                        })?;
                    self.acp_session_plugins
                        .insert(session_id_param.to_string(), rec.id.clone());
                    self.acp_servers
                        .insert(session_id_param.to_string(), server);
                }
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("");
                let model = params
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let permission_mode = params
                    .get("permissionMode")
                    .or_else(|| params.get("approvalPolicy"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());

                let send_res = {
                    let server = self
                        .acp_servers
                        .get_mut(session_id_param)
                        .ok_or_else(|| NativeError::rejected("plugin_session_not_found"))?;
                    server.send_tracked_prompt_with_options(
                        raw_id,
                        text,
                        &turn_id,
                        model,
                        permission_mode,
                    )
                };
                if let Err(err) = send_res {
                    self.acp_servers.remove(session_id_param);
                    self.acp_session_plugins.remove(session_id_param);
                    self.acp_active_turns.remove(session_id_param);
                    return Err(NativeError::rejected(format!("plugin_send_failed:{err}")));
                }

                self.acp_active_turns
                    .insert(session_id_param.to_string(), turn_id.clone());
                self.acp_buffered_events.push_back(json!({
                    "method": "turn/started",
                    "params": {
                        "threadId": session_id_param,
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
                let raw_id = session_id_param
                    .strip_prefix(&prefix)
                    .unwrap_or(session_id_param);
                let has_server = if let Some(server) = self.acp_servers.get_mut(session_id_param) {
                    let _ = server.session_cancel(raw_id);
                    true
                } else {
                    false
                };
                // Keep active turn_id until `turn/completed` drains so NativeSessionHost matches turn.id.
                if !has_server {
                    self.acp_active_turns.remove(session_id_param);
                }
                Ok(json!({ "ok": true }))
            }
            "thread/archive" | "thread/unarchive" => Ok(json!({ "ok": true })),
            _ => Err(NativeError::rejected(format!(
                "unsupported_plugin_method:{method}"
            ))),
        }
    }

    fn find_discovered_plugin_session_cwd(
        &self,
        rec: &crate::plugins::InstalledPluginRecord,
        session_id: &str,
        raw_id: &str,
    ) -> Option<Option<PathBuf>> {
        let match_item = |item: &Value| -> Option<Option<PathBuf>> {
            let id = item.get("id").and_then(Value::as_str)?;
            if id != session_id && id != raw_id {
                return None;
            }
            let cwd = item
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != ".")
                .map(|s| {
                    let p = PathBuf::from(s);
                    dunce::canonicalize(&p).unwrap_or(p)
                });
            Some(cwd)
        };
        for item in cached_local_store_sessions().iter() {
            if let Some(found) = match_item(item) {
                return Some(found);
            }
        }
        if rec.manifest.capabilities.discover {
            let meta = crate::plugins::global_manager().query_metadata_for(rec);
            for item in &meta.sessions {
                if let Some(found) = match_item(item) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn resolve_plugin_cwd(&self, session_id: &str, params: &Value) -> PathBuf {
        if let Some(cached) = self
            .acp_cached_cwds
            .get(session_id)
            .filter(|p| !p.as_os_str().is_empty() && *p != Path::new("."))
        {
            return cached.clone();
        }
        if let Some(param_cwd) = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != ".")
        {
            let p = PathBuf::from(param_cwd);
            return dunce::canonicalize(&p).unwrap_or(p);
        }
        PathBuf::from(".")
    }
}
