use super::*;

const MAX_TRANSPORT_SESSION_CACHE: usize = 4096;
const MAX_ACTIVE_TURN_FLAGS: usize = 512;

/// FIFO-bounded map keeping transport metadata caches capped across long uptimes.
pub(crate) struct BoundedMap<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    max_entries: usize,
}

impl<K: Clone + Eq + std::hash::Hash, V> BoundedMap<K, V> {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            max_entries: max_entries.max(1),
        }
    }

    pub(crate) fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + std::hash::Hash + Eq,
    {
        self.map.get(key)
    }

    pub(crate) fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + std::hash::Hash + Eq,
    {
        self.map.contains_key(key)
    }

    pub(crate) fn iter(&self) -> std::collections::hash_map::Iter<'_, K, V> {
        self.map.iter()
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        if let Some(existing) = self.map.get_mut(&key) {
            *existing = value;
            return;
        }
        while self.map.len() >= self.max_entries {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.map.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    pub(crate) fn insert_if_room(&mut self, key: K, value: V) {
        if self.map.len() < self.max_entries && !self.map.contains_key(&key) {
            self.order.push_back(key.clone());
            self.map.insert(key, value);
        }
    }

    fn compact_order(&mut self) {
        self.order.retain(|item| self.map.contains_key(item));
    }

    pub(crate) fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + std::hash::Hash + Eq,
    {
        let removed = self.map.remove(key);
        if removed.is_some() && self.order.len() > self.max_entries * 2 {
            self.compact_order();
        }
        removed
    }
}

/// FIFO-bounded set keeping session classification / turn marker sets capped.
pub(crate) struct BoundedSet<K> {
    set: HashSet<K>,
    order: VecDeque<K>,
    max_entries: usize,
}

impl<K: Clone + Eq + std::hash::Hash> BoundedSet<K> {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            set: HashSet::new(),
            order: VecDeque::new(),
            max_entries: max_entries.max(1),
        }
    }

    pub(crate) fn contains<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + std::hash::Hash + Eq,
    {
        self.set.contains(key)
    }

    pub(crate) fn insert(&mut self, key: K) {
        if self.set.contains(&key) {
            return;
        }
        while self.set.len() >= self.max_entries {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.set.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.set.insert(key);
    }

    pub(crate) fn insert_if_room(&mut self, key: K) {
        if self.set.len() < self.max_entries && !self.set.contains(&key) {
            self.order.push_back(key.clone());
            self.set.insert(key);
        }
    }

    fn compact_order(&mut self) {
        self.order.retain(|item| self.set.contains(item));
    }

    pub(crate) fn remove<Q>(&mut self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + std::hash::Hash + Eq,
    {
        let removed = self.set.remove(key);
        if removed && self.order.len() > self.max_entries * 2 {
            self.compact_order();
        }
        removed
    }
}

pub struct MultiProviderTransport {
    pub(crate) codex: Option<CodexAppServer>,
    pub(crate) codex_boot: Option<std::sync::mpsc::Receiver<Option<CodexAppServer>>>,
    pub(crate) codex_models_cache: Option<(Instant, Value)>,
    pub(crate) codex_active_turns: HashMap<String, String>,
    pub(crate) codex_pending_requests: HashMap<String, (String, Value)>,
    pub(crate) codex_buffered_events: VecDeque<Value>,
    pub(crate) claude_binary: PathBuf,
    pub(crate) claude_active_turns: HashMap<String, String>,
    pub(crate) claude_cancels: HashMap<String, Arc<AtomicBool>>,
    pub(crate) claude_buffered_events: Arc<Mutex<VecDeque<Value>>>,
    pub(crate) claude_cached_cwds: BoundedMap<String, PathBuf>,
    pub(crate) claude_cached_models: BoundedMap<String, String>,
    pub(crate) claude_desktop_sessions: BoundedSet<String>,
    pub(crate) claude_turn_emitted_text: BoundedSet<String>,
    pub(crate) claude_turn_streamed_deltas: BoundedSet<String>,
    pub(crate) claude_pending_requests: BoundedMap<String, String>,
    pub(crate) claude_sessions: HashMap<String, ClaudeProcessSession>,
    pub(crate) antigravity_binary: PathBuf,
    pub(crate) antigravity_active_turns: HashMap<String, String>,
    pub(crate) antigravity_cancels: HashMap<String, Arc<AtomicBool>>,
    pub(crate) antigravity_buffered_events: Arc<Mutex<VecDeque<Value>>>,
    pub(crate) antigravity_cached_cwds: BoundedMap<String, PathBuf>,
    pub(crate) antigravity_cached_models: BoundedMap<String, String>,
    pub(crate) antigravity_turn_emitted_text: BoundedSet<String>,
    pub(crate) antigravity_turn_streamed_deltas: BoundedSet<String>,
    pub(crate) antigravity_pending_requests: BoundedMap<String, String>,
    pub(crate) antigravity_known_real_ids: BoundedSet<String>,
    pub(crate) antigravity_sessions: HashMap<String, AntigravityProcessSession>,
    pub(crate) acp_servers: HashMap<String, AcpAppServer>,
    pub(crate) acp_session_plugins: HashMap<String, String>,
    pub(crate) acp_active_turns: HashMap<String, String>,
    pub(crate) acp_pending_requests: BoundedMap<String, String>,
    pub(crate) acp_buffered_events: VecDeque<Value>,
    pub(crate) acp_cached_cwds: BoundedMap<String, PathBuf>,
    #[cfg(test)]
    pub(crate) skip_antigravity_daemon: bool,
}

impl MultiProviderTransport {
    pub fn new(codex: Option<CodexAppServer>) -> Self {
        Self {
            codex,
            codex_boot: None,
            codex_models_cache: None,
            codex_active_turns: HashMap::new(),
            codex_pending_requests: HashMap::new(),
            codex_buffered_events: VecDeque::new(),
            claude_binary: crate::collectors::discover_claude_binary(),
            claude_active_turns: HashMap::new(),
            claude_cancels: HashMap::new(),
            claude_buffered_events: Arc::new(Mutex::new(VecDeque::new())),
            claude_cached_cwds: BoundedMap::new(MAX_TRANSPORT_SESSION_CACHE),
            claude_cached_models: BoundedMap::new(MAX_TRANSPORT_SESSION_CACHE),
            claude_desktop_sessions: BoundedSet::new(MAX_TRANSPORT_SESSION_CACHE),
            claude_turn_emitted_text: BoundedSet::new(MAX_ACTIVE_TURN_FLAGS),
            claude_turn_streamed_deltas: BoundedSet::new(MAX_ACTIVE_TURN_FLAGS),
            claude_pending_requests: BoundedMap::new(MAX_ACTIVE_TURN_FLAGS),
            claude_sessions: HashMap::new(),
            antigravity_binary: crate::collectors::discover_antigravity_binary(),
            antigravity_active_turns: HashMap::new(),
            antigravity_cancels: HashMap::new(),
            antigravity_buffered_events: Arc::new(Mutex::new(VecDeque::new())),
            antigravity_cached_cwds: BoundedMap::new(MAX_TRANSPORT_SESSION_CACHE),
            antigravity_cached_models: BoundedMap::new(MAX_TRANSPORT_SESSION_CACHE),
            antigravity_turn_emitted_text: BoundedSet::new(MAX_ACTIVE_TURN_FLAGS),
            antigravity_turn_streamed_deltas: BoundedSet::new(MAX_ACTIVE_TURN_FLAGS),
            antigravity_pending_requests: BoundedMap::new(MAX_ACTIVE_TURN_FLAGS),
            antigravity_known_real_ids: BoundedSet::new(MAX_TRANSPORT_SESSION_CACHE),
            antigravity_sessions: HashMap::new(),
            acp_servers: HashMap::new(),
            acp_session_plugins: HashMap::new(),
            acp_active_turns: HashMap::new(),
            acp_pending_requests: BoundedMap::new(MAX_ACTIVE_TURN_FLAGS),
            acp_buffered_events: VecDeque::new(),
            acp_cached_cwds: BoundedMap::new(MAX_TRANSPORT_SESSION_CACHE),
            #[cfg(test)]
            skip_antigravity_daemon: false,
        }
    }

    /// Constructs a transport immediately while starting the interactive Codex
    /// app-server on a background thread so remote WS registration never waits
    /// on a cold Node/Codex child process launch.
    pub fn with_async_codex(binary: PathBuf, timeout: Duration) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("agenthub-codex-boot".into())
            .spawn(move || {
                let codex = CodexAppServer::start_interactive(&binary, timeout).ok();
                let _ = tx.send(codex);
            });
        let mut transport = Self::new(None);
        if spawned.is_ok() {
            transport.codex_boot = Some(rx);
        }
        transport
    }

    pub(crate) fn poll_codex_boot(&mut self) {
        let Some(ref rx) = self.codex_boot else {
            return;
        };
        match rx.try_recv() {
            Ok(codex) => {
                if self.codex.is_none() {
                    self.codex = codex;
                }
                self.codex_boot = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.codex_boot = None;
            }
        }
    }

    pub(crate) fn wait_for_codex_boot(&mut self) {
        if self.codex.is_some() {
            self.codex_boot = None;
            return;
        }
        if let Some(rx) = self.codex_boot.take() {
            if let Ok(codex) = rx.recv() {
                self.codex = codex;
            }
        }
    }

    pub(crate) fn allow_antigravity_daemon_fallback(&self) -> bool {
        #[cfg(test)]
        {
            !self.skip_antigravity_daemon
        }
        #[cfg(not(test))]
        {
            true
        }
    }

    pub fn add_acp_server(
        &mut self,
        session_id: impl Into<String>,
        server: AcpAppServer,
        cwd: PathBuf,
    ) {
        let sid = session_id.into();
        self.acp_cached_cwds.insert(sid.clone(), cwd);
        self.acp_servers.insert(sid, server);
    }

    pub(crate) fn is_claude_desktop_session(&mut self, session_id: &str) -> bool {
        let raw_id = session_id.trim_start_matches("claude_");
        if self.claude_desktop_sessions.contains(session_id)
            || self.claude_desktop_sessions.contains(raw_id)
        {
            return true;
        }
        for item in cached_local_store_sessions().iter() {
            if item.get("id").and_then(Value::as_str) == Some(session_id)
                && item.get("provider").and_then(Value::as_str) == Some("claude_desktop")
            {
                self.claude_desktop_sessions.insert(session_id.to_string());
                self.claude_desktop_sessions.insert(raw_id.to_string());
                return true;
            }
        }
        false
    }

    pub(crate) fn get_or_spawn_claude_session(
        &mut self,
        session_id: &str,
        cwd: &Path,
        model: Option<&str>,
        effort: Option<&str>,
        permission_mode: Option<&str>,
        claude_desktop: bool,
    ) -> Result<&mut ClaudeProcessSession, NativeError> {
        if let Some(existing) = self.claude_sessions.get(session_id) {
            if existing.is_dead() {
                self.claude_sessions.remove(session_id);
            } else if !existing.is_turn_in_progress() {
                let model_changed = model.is_some() && existing.requested_model() != model;
                let effort_changed = effort.is_some() && existing.reasoning_effort() != effort;
                let mode_changed =
                    permission_mode.is_some() && existing.permission_mode() != permission_mode;
                let client_changed = existing.is_claude_desktop() != claude_desktop;
                if model_changed || effort_changed || mode_changed || client_changed {
                    self.claude_sessions.remove(session_id);
                }
            }
        }
        if !self.claude_sessions.contains_key(session_id) {
            let raw_id = session_id.trim_start_matches("claude_");
            let resume_id = if raw_id.is_empty() {
                None
            } else if crate::collectors::find_claude_session_info(raw_id)
                .ok()
                .flatten()
                .is_some()
            {
                Some(raw_id)
            } else {
                None
            };

            let options = crate::collectors::TurnOptions {
                model,
                reasoning_effort: effort,
                permission_mode,
                claude_desktop,
                ..Default::default()
            };

            let session = ClaudeProcessSession::start(&self.claude_binary, cwd, resume_id, options)
                .map_err(|e| NativeError::rejected(format!("claude_start_failed: {}", e)))?;
            self.claude_sessions.insert(session_id.to_string(), session);
        }

        self.claude_sessions
            .get_mut(session_id)
            .ok_or_else(|| NativeError::rejected("claude_session_not_found"))
    }

    pub(crate) fn get_or_spawn_antigravity_session(
        &mut self,
        session_id: &str,
        cwd: &Path,
        model: Option<&str>,
        effort: Option<&str>,
        permission_mode: Option<&str>,
        session_exists: Option<bool>,
    ) -> Result<&mut AntigravityProcessSession, NativeError> {
        let mut remembered_id: Option<String> = None;
        if let Some(existing) = self.antigravity_sessions.get(session_id) {
            if existing.is_dead() || existing.is_interrupted() {
                let sid = existing.session_id().trim();
                if !sid.is_empty() {
                    remembered_id = Some(sid.to_string());
                }
                self.antigravity_sessions.remove(session_id);
            } else if !existing.is_turn_in_progress() {
                let model_changed = model.is_some() && existing.requested_model() != model;
                let effort_changed = effort.is_some() && existing.reasoning_effort() != effort;
                let mode_changed =
                    permission_mode.is_some() && existing.permission_mode() != permission_mode;
                if model_changed || effort_changed || mode_changed {
                    let sid = existing.session_id().trim();
                    if !sid.is_empty() {
                        remembered_id = Some(sid.to_string());
                    }
                    self.antigravity_sessions.remove(session_id);
                }
            }
        }
        if !self.antigravity_sessions.contains_key(session_id) {
            let raw_id = session_id
                .strip_prefix("antigravity_")
                .unwrap_or(session_id);
            let known_real = self.antigravity_known_real_ids.contains(session_id)
                || self.antigravity_known_real_ids.contains(raw_id);
            let resume_id = if let Some(ref sid) = remembered_id {
                Some(sid.as_str())
            } else if session_exists == Some(false) || raw_id.is_empty() {
                None
            } else if session_exists == Some(true)
                || known_real
                || crate::collectors::find_antigravity_session_info(raw_id)
                    .ok()
                    .flatten()
                    .is_some()
            {
                Some(raw_id)
            } else {
                None
            };
            let options = crate::collectors::TurnOptions {
                model,
                reasoning_effort: effort,
                permission_mode,
                session_exists,
                claude_desktop: false,
            };
            let session =
                AntigravityProcessSession::start(&self.antigravity_binary, cwd, resume_id, options)
                    .map_err(|e| NativeError::rejected(format!("antigravity_start_failed: {e}")))?;
            self.antigravity_sessions
                .insert(session_id.to_string(), session);
        }
        self.antigravity_sessions
            .get_mut(session_id)
            .ok_or_else(|| NativeError::rejected("antigravity_session_not_found"))
    }
}

impl NativeTransport for MultiProviderTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
        self.handle_request(method, params)
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        self.handle_respond(id, result)
    }

    fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
        self.handle_drain()
    }
}

pub(crate) fn resolve_antigravity_control_decision(result: &Value) -> &str {
    result
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            if result.get("answers").is_some() || result.get("response").is_some() {
                "answer"
            } else {
                "decline"
            }
        })
}

impl MultiProviderTransport {
    pub(crate) fn handle_codex_transport_loss(&mut self) {
        self.codex = None;
        self.codex_models_cache = None;
        for (_, (thread_id, request_id)) in self.codex_pending_requests.drain() {
            self.codex_buffered_events.push_back(json!({
                "method": "serverRequest/resolved",
                "params": {
                    "threadId": thread_id,
                    "requestId": request_id
                }
            }));
        }
        let err_msg = "Codex 进程连接已断开";
        for (thread_id, turn_id) in self.codex_active_turns.drain() {
            self.codex_buffered_events.push_back(json!({
                "method": "error",
                "params": {
                    "threadId": &thread_id,
                    "turnId": &turn_id,
                    "willRetry": false,
                    "error": { "message": err_msg }
                }
            }));
            self.codex_buffered_events.push_back(json!({
                "method": "turn/completed",
                "params": {
                    "threadId": thread_id,
                    "turn": {
                        "id": turn_id,
                        "status": "failed",
                        "error": err_msg
                    }
                }
            }));
        }
    }

    pub(crate) fn handle_respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        // Route "claude_req_<id>" strictly to its owning Claude session.
        if let Some(id_str) = id.as_str()
            && let Some(actual_id) = id_str.strip_prefix("claude_req_")
        {
            let response_payload =
                if let Some(decision) = result.get("decision").and_then(Value::as_str) {
                    let behavior = if decision == "accept" {
                        "allow"
                    } else {
                        "deny"
                    };
                    json!({
                        "decision": decision,
                        "behavior": behavior
                    })
                } else {
                    result
                };
            let Some(owner_sid) = self.claude_pending_requests.get(actual_id).cloned() else {
                return Err(NativeError::rejected("pending_request_not_found"));
            };
            let Some(session) = self.claude_sessions.get_mut(&owner_sid) else {
                return Err(NativeError::rejected("pending_request_not_found"));
            };
            session
                .send_control_response(actual_id, "success", response_payload)
                .map_err(|_| NativeError::rejected("control_response_failed"))?;
            self.claude_pending_requests.remove(actual_id);
            return Ok(());
        }
        // Antigravity 2.0 Desktop daemon interactive permission / command / ask_question response
        if let Some(id_str) = id.as_str()
            && let Some(spec) = id_str.strip_prefix("antigravity_daemon_req_")
        {
            let mut parts = spec.splitn(3, ':');
            let (Some(cascade_id), Some(step_str), Some(kind)) =
                (parts.next(), parts.next(), parts.next())
            else {
                return Err(NativeError::rejected("invalid_daemon_request_id"));
            };
            let Ok(step_idx) = step_str.parse::<u64>() else {
                return Err(NativeError::rejected("invalid_daemon_request_id"));
            };
            if cascade_id.trim().is_empty() || kind.trim().is_empty() {
                return Err(NativeError::rejected("invalid_daemon_request_id"));
            }
            crate::collectors::antigravity::respond_daemon_interaction(
                cascade_id, step_idx, kind, &result,
            )
            .map_err(|_| NativeError::rejected("control_response_failed"))?;
            return Ok(());
        }
        // If request ID is string and starts with "antigravity_req_", route strictly to owning session
        if let Some(id_str) = id.as_str()
            && let Some(actual_id) = id_str.strip_prefix("antigravity_req_")
        {
            let decision = resolve_antigravity_control_decision(&result).to_string();
            let Some(owner_sid) = self.antigravity_pending_requests.get(actual_id).cloned() else {
                return Err(NativeError::rejected("pending_request_not_found"));
            };
            let Some(session) = self.antigravity_sessions.get_mut(&owner_sid) else {
                return Err(NativeError::rejected("pending_request_not_found"));
            };
            session
                .send_control_response(actual_id, &decision, result)
                .map_err(|_| NativeError::rejected("control_response_failed"))?;
            self.antigravity_pending_requests.remove(actual_id);
            return Ok(());
        }
        // Route "acp_req_<id>" strictly to its owning ACP server.
        if let Some(actual_id) = id
            .as_str()
            .and_then(|id_str| id_str.strip_prefix("acp_req_"))
            .map(str::to_owned)
        {
            let decision = result
                .get("decision")
                .and_then(Value::as_str)
                .unwrap_or("decline");
            let Some(owner_sid) = self.acp_pending_requests.get(&actual_id).cloned() else {
                return Err(NativeError::rejected("pending_request_not_found"));
            };
            let Some(server) = self.acp_servers.get_mut(&owner_sid) else {
                return Err(NativeError::rejected("pending_request_not_found"));
            };
            server
                .respond_permission(id, decision)
                .map_err(|_| NativeError::rejected("control_response_failed"))?;
            self.acp_pending_requests.remove(&actual_id);
            return Ok(());
        }
        self.poll_codex_boot();
        if let Some(ref mut codex) = self.codex {
            let req_key = id.to_string();
            match codex.respond(id, result) {
                Ok(()) => {
                    self.codex_pending_requests.remove(&req_key);
                    Ok(())
                }
                Err(_) => {
                    self.handle_codex_transport_loss();
                    Err(NativeError::rejected("codex_provider_lost"))
                }
            }
        } else {
            Err(NativeError::rejected("codex_provider_lost"))
        }
    }
}
