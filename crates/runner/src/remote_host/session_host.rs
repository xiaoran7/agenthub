use super::*;

pub struct NativeSessionHost<T: NativeTransport> {
    pub(crate) transport: T,
    pub(crate) generation: String,
    pub(crate) workspaces: Vec<PathBuf>,
    pub(crate) discovered_workspaces: HashMap<String, PathBuf>,
    pub(crate) cwd_canon_cache: HashMap<String, PathBuf>,
    pub(crate) sessions: HashMap<String, SessionBinding>,
    pub(crate) native_to_hub: HashMap<String, String>,
    pub(crate) newly_created: HashSet<String>,
    pub(crate) control_idle_since: HashMap<String, Instant>,
    pub(crate) control_release_checks: HashMap<String, (Instant, u8, Instant)>,
    pub(crate) approvals: HashMap<String, PendingApproval>,
    pub(crate) questions: HashMap<String, PendingQuestion>,
    /// Requests this Host already answered, keyed by their derived id. Some
    /// providers re-deliver a request the client already responded to; replaying
    /// it would raise a second approval the operator has to dismiss.
    pub(crate) answered_requests: HashSet<String>,
    /// Insertion order of `answered_requests`, for the same bounded-lifetime
    /// reason as `receipt_order`. Without it a long-lived daemon would keep every
    /// id it ever answered for as long as it ran.
    pub(crate) answered_order: VecDeque<String>,
    pub(crate) receipts: HashMap<String, (RemoteCommand, CommandReceipt)>,
    /// Insertion order of `receipts` keys. Lets the ledger drop its oldest
    /// entries instead of growing until `command_ledger_full` permanently
    /// rejects every new command on a long-lived daemon.
    pub(crate) receipt_order: VecDeque<String>,
    pub(crate) pending_historical_events: VecDeque<HostEvent>,
    pub(crate) live_mirror: LiveTranscriptMirror,
    pub(crate) alive: bool,
}

impl<T: NativeTransport> NativeSessionHost<T> {
    pub fn new(transport: T, workspaces: Vec<PathBuf>) -> Result<Self, NativeError> {
        let mut workspaces = workspaces
            .into_iter()
            .map(|path| {
                dunce::canonicalize(&path)
                    .ok()
                    .filter(|path| path.is_dir())
                    .ok_or_else(|| NativeError::rejected("workspace_unavailable"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        workspaces.sort();
        workspaces.dedup();
        if workspaces.is_empty() {
            return Err(NativeError::rejected("workspace_required"));
        }
        Ok(Self {
            transport,
            generation: uuid::Uuid::new_v4().to_string(),
            workspaces,
            discovered_workspaces: HashMap::new(),
            cwd_canon_cache: HashMap::new(),
            sessions: HashMap::new(),
            native_to_hub: HashMap::new(),
            newly_created: HashSet::new(),
            control_idle_since: HashMap::new(),
            control_release_checks: HashMap::new(),
            approvals: HashMap::new(),
            questions: HashMap::new(),
            answered_requests: HashSet::new(),
            answered_order: VecDeque::new(),
            receipts: HashMap::new(),
            receipt_order: VecDeque::new(),
            pending_historical_events: VecDeque::new(),
            live_mirror: LiveTranscriptMirror::default(),
            alive: true,
        })
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn is_alive(&self) -> bool {
        self.alive
    }
    pub fn sessions(&self) -> Vec<SessionBinding> {
        self.sessions.values().cloned().collect()
    }
    pub fn pending_approvals(&self) -> Vec<PendingApprovalSnapshot> {
        let mut approvals = self
            .approvals
            .iter()
            .filter(|(_, approval)| approval.supported)
            .map(|(approval_id, approval)| {
                let decisions = approval.available_decisions.as_ref();
                PendingApprovalSnapshot {
                    approval_id: approval_id.clone(),
                    hub_session_id: approval.hub_session_id.clone(),
                    host_generation: self.generation.clone(),
                    turn_id: approval.turn_id.clone(),
                    can_approve: decisions.is_none_or(|values| values.contains(&json!("accept"))),
                    can_decline: decisions.is_none_or(|values| values.contains(&json!("decline"))),
                    tool_name: approval.tool_name.clone(),
                    command_preview: approval.command_preview.clone(),
                    path: approval.path.clone(),
                    diff: approval.diff.clone(),
                    description: approval.description.clone(),
                    risk_level: approval.risk_level.clone(),
                }
            })
            .collect::<Vec<_>>();
        approvals.sort_by(|left, right| left.approval_id.cmp(&right.approval_id));
        approvals
    }
    pub fn pending_questions(&self) -> Vec<PendingQuestionSnapshot> {
        let mut questions = self
            .questions
            .iter()
            .map(|(question_id, q)| PendingQuestionSnapshot {
                question_id: question_id.clone(),
                hub_session_id: q.hub_session_id.clone(),
                host_generation: self.generation.clone(),
                turn_id: q.turn_id.clone(),
                prompt: q.prompt.clone(),
                header: q.header.clone(),
                options: q.options.clone(),
                multi_select: q.multi_select,
            })
            .collect::<Vec<_>>();
        questions.sort_by(|left, right| left.question_id.cmp(&right.question_id));
        questions
    }
    pub fn workspaces(&self) -> &[PathBuf] {
        &self.workspaces
    }
    pub fn discovered_workspaces(&self) -> &HashMap<String, PathBuf> {
        &self.discovered_workspaces
    }
    pub fn authorize_workspace(&mut self, path: PathBuf) -> Result<PathBuf, NativeError> {
        let canonical = dunce::canonicalize(&path)
            .ok()
            .filter(|p| p.is_dir())
            .ok_or_else(|| NativeError::rejected("workspace_unavailable"))?;
        if !self.workspaces.contains(&canonical) {
            if self.workspaces.len() >= 32 {
                return Err(NativeError::rejected("workspace_limit_exceeded"));
            }
            self.workspaces.push(canonical.clone());
        }
        Ok(canonical)
    }

    pub(crate) fn restore_detached(&mut self, bindings: Vec<SessionBinding>) {
        for mut binding in bindings {
            binding.control_connected = false;
            binding.execution_state = ExecutionState::Unknown;
            binding.current_turn_id = None;
            binding.host_generation = self.generation.clone();
            self.native_to_hub.insert(
                binding.provider_session_id.clone(),
                binding.hub_session_id.clone(),
            );
            self.sessions
                .insert(binding.hub_session_id.clone(), binding);
        }
    }

    pub(crate) fn enqueue_session_history_and_usage(
        &mut self,
        binding: &SessionBinding,
        turns_source: Option<&Vec<Value>>,
    ) {
        let id = &binding.provider_session_id;
        let hub_id = &binding.hub_session_id;
        let cwd = &binding.workspace_path;

        let mut replayed_turn_items = false;
        if let Some(turns) = turns_source.filter(|turns| !turns.is_empty()) {
            for turn in turns {
                let turn_id = turn["id"].as_str().unwrap_or_default();
                if let Some(items) = turn["items"].as_array() {
                    for item in items {
                        let item_type = item["type"].as_str().unwrap_or_default();
                        if matches!(
                            item_type,
                            "userMessage"
                                | "agentMessage"
                                | "reasoning"
                                | "thinking"
                                | "commandExecution"
                                | "fileChange"
                                | "mcpToolCall"
                                | "toolCall"
                                | "functionCall"
                                | "customToolCall"
                        ) {
                            replayed_turn_items = true;
                            let synthetic_raw = json!({
                                "method": "item/completed",
                                "params": {
                                    "threadId": id,
                                    "turnId": turn_id,
                                    "item": item
                                }
                            });
                            self.pending_historical_events.push_back(HostEvent {
                                event_id: uuid::Uuid::new_v4().to_string(),
                                host_generation: self.generation.clone(),
                                binding: Some(binding.clone()),
                                hub_session_id: Some(hub_id.clone()),
                                provider_session_id: Some(id.clone()),
                                kind: "item/completed".to_string(),
                                raw: synthetic_raw,
                                approval_id: None,
                                approval: None,
                                question_id: None,
                                question: None,
                            });
                        }
                    }
                }
            }
        }

        let Some(home) = crate::collectors::home_dir() else {
            return;
        };
        let provider = provider_for_session_id(id);
        let usage =
            crate::collectors::lookup_session_context_usage(&home, provider.clone(), id, Some(cwd));
        let usage_pair = usage.as_ref().map(|u| (u.used, u.window));

        if !replayed_turn_items {
            let items = crate::conversation::load_session_conversation_with_workspace(
                &home,
                provider,
                id,
                Some(cwd),
            );
            self.live_mirror
                .seed_from_items(hub_id, &items, usage_pair, Instant::now());
            if !items.is_empty() {
                self.pending_historical_events.push_back(HostEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    host_generation: self.generation.clone(),
                    binding: Some(binding.clone()),
                    hub_session_id: Some(hub_id.clone()),
                    provider_session_id: Some(id.clone()),
                    kind: "history/snapshot".to_string(),
                    raw: json!({"method": "history/snapshot", "params": {"items": items}}),
                    approval_id: None,
                    approval: None,
                    question_id: None,
                    question: None,
                });
            }
        } else {
            self.live_mirror
                .defer_after_local_turn(hub_id, Instant::now());
        }

        if let Some(usage) = usage {
            self.pending_historical_events.push_back(HostEvent {
                event_id: uuid::Uuid::new_v4().to_string(),
                host_generation: self.generation.clone(),
                binding: Some(binding.clone()),
                hub_session_id: Some(hub_id.clone()),
                provider_session_id: Some(id.clone()),
                kind: "thread/tokenUsage/updated".to_string(),
                raw: json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": id,
                        "tokenUsage": {
                            "used": usage.used,
                            "last": {"inputTokens": usage.used},
                            "window": usage.window,
                            "modelContextWindow": usage.window
                        }
                    }
                }),
                approval_id: None,
                approval: None,
                question_id: None,
                question: None,
            });
        }
    }

    /// Actual provider model/list response, with no invented availability or fallback catalog.
    ///
    /// `data` stays the flat list the console has always consumed. `agents`
    /// carries the same answer split by the engine that can run each model, so
    /// the launch dialog can offer only the ids the chosen engine accepts: an
    /// id read from one engine's config is a provider rejection on another.
    pub fn models(&mut self) -> Result<Value, NativeError> {
        let mut data = Vec::new();
        if let Ok(resp) = self.transport.request("model/list", json!({"limit": 100})) {
            if let Some(list) = resp.get("data").and_then(Value::as_array) {
                data.extend(list.iter().cloned());
            }
        }
        let mut agents = serde_json::Map::new();
        if !data.is_empty() {
            // The app-server's own catalog is Codex's: it is the only engine
            // whose list arrives over RPC rather than from a config file.
            agents.insert("codex".into(), json!(data));
        }
        let mut seen: std::collections::HashSet<String> = data
            .iter()
            .filter_map(|item| item.get("id").and_then(Value::as_str))
            .map(str::to_ascii_lowercase)
            .collect();
        for (agent, models) in crate::model_catalog::agent_model_catalogs() {
            for model in &models {
                let id = model.get("id").and_then(Value::as_str).unwrap_or("");
                if !id.is_empty() && seen.insert(id.to_ascii_lowercase()) {
                    data.push(model.clone());
                }
            }
            if let Some(existing) = agents.get_mut(&agent).and_then(Value::as_array_mut) {
                let mut existing_ids: HashSet<String> = existing
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_str))
                    .map(str::to_ascii_lowercase)
                    .collect();
                for model in models {
                    let id = model.get("id").and_then(Value::as_str).unwrap_or("");
                    if !id.is_empty() && existing_ids.insert(id.to_ascii_lowercase()) {
                        existing.push(model);
                    }
                }
            } else {
                agents.insert(agent, json!(models));
            }
        }
        if data.is_empty() {
            let catalog = crate::model_catalog::fetch_catalog_models(false);
            data = crate::model_catalog::catalog_models_to_json(&catalog)["data"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
        Ok(json!({ "data": data, "agents": agents }))
    }

    pub fn discover(&mut self) -> Result<Value, NativeError> {
        let mut candidates = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        let mut cursor: Option<String> = None;
        let mut attempts = 0u32;
        loop {
            let mut params = json!({"limit": 500});
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }
            let response = self.transport.request("thread/list", params)?;
            // A page without a `data` array is a failed page, not an empty
            // history. Read as zero sessions it flattens the console, so the
            // first page is retried before giving up.
            let Some(items) = response.get("data").and_then(Value::as_array) else {
                if cursor.is_none() && attempts < LIST_ATTEMPTS {
                    attempts += 1;
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                return Err(NativeError::rejected("thread_list_malformed"));
            };
            for thread in items {
                let Some(id) = thread["id"].as_str() else {
                    continue;
                };
                if !seen_ids.insert(id.to_string()) {
                    continue;
                }
                let raw_cwd = thread["cwd"].as_str().unwrap_or(".");
                let cwd = if let Some(cached) = self.cwd_canon_cache.get(raw_cwd) {
                    cached.clone()
                } else {
                    const MAX_CWD_CANON_CACHE: usize = 512;
                    const MAX_DISCOVERED_WORKSPACE_KEYS: usize = 4096;
                    let resolved = dunce::canonicalize(PathBuf::from(raw_cwd))
                        .unwrap_or_else(|_| PathBuf::from(raw_cwd));
                    let ws_id = crate::privacy::workspace_id_for_path(&resolved.to_string_lossy());
                    if !self.discovered_workspaces.contains_key(&ws_id)
                        && self.discovered_workspaces.len() >= MAX_DISCOVERED_WORKSPACE_KEYS
                    {
                        self.discovered_workspaces
                            .retain(|_, p| self.workspaces.iter().any(|w| w == p));
                    }
                    if self.discovered_workspaces.contains_key(&ws_id)
                        || self.discovered_workspaces.len() < MAX_DISCOVERED_WORKSPACE_KEYS
                    {
                        self.discovered_workspaces.insert(ws_id, resolved.clone());
                        self.discovered_workspaces
                            .insert(resolved.to_string_lossy().to_string(), resolved.clone());
                        let clean = resolved
                            .to_string_lossy()
                            .trim_start_matches(r"\\?\")
                            .to_string();
                        self.discovered_workspaces
                            .insert(clean.clone(), resolved.clone());
                        let forward = clean.replace('\\', "/");
                        self.discovered_workspaces
                            .insert(forward.clone(), resolved.clone());
                        self.discovered_workspaces
                            .insert(clean.to_lowercase(), resolved.clone());
                        self.discovered_workspaces
                            .insert(forward.to_lowercase(), resolved.clone());
                    }
                    if self.cwd_canon_cache.len() >= MAX_CWD_CANON_CACHE {
                        self.cwd_canon_cache.clear();
                    }
                    self.cwd_canon_cache
                        .insert(raw_cwd.to_string(), resolved.clone());
                    resolved
                };
                let title = thread["name"]
                    .as_str()
                    .filter(|name| !name.trim().is_empty())
                    .or_else(|| {
                        thread["preview"]
                            .as_str()
                            .and_then(|p| p.lines().find(|l| !l.trim().is_empty()))
                    })
                    .map(|name| name.chars().take(160).collect::<String>());
                let pt = thread
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let ct = thread
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let tt = thread
                    .get("total_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(|| pt.saturating_add(ct));
                let cr = thread
                    .get("cache_read_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let ctx_used = thread
                    .get("context_used_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let ctx_win = thread
                    .get("context_window_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let last_ms = thread
                    .get("last_activity_at_ms")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let model = thread.get("model").cloned().unwrap_or(Value::Null);
                let req_count = thread
                    .get("request_count")
                    .and_then(Value::as_u64)
                    .or_else(|| thread.get("calls").and_then(Value::as_u64))
                    .unwrap_or(1);

                candidates.push(json!({
                    "provider_session_id": id,
                    "provider": thread["provider"],
                    "resumable": thread["resumable"],
                    "workspace_path": cwd,
                    "title": title,
                    "execution_state": execution_state(&thread["status"]),
                    "control_connected": false,
                    "model": model,
                    "prompt_tokens": pt,
                    "completion_tokens": ct,
                    "total_tokens": tt,
                    "cache_read_tokens": cr,
                    "context_used_tokens": ctx_used,
                    "context_window_tokens": ctx_win,
                    "request_count": req_count,
                    "calls": req_count,
                    "last_activity_at_ms": last_ms,
                    "subagents": thread
                        .get("subagents")
                        .cloned()
                        .unwrap_or_else(|| json!([]))
                }));
            }
            if let Some(next) = response["nextCursor"].as_str() {
                cursor = Some(next.to_string());
            } else {
                break;
            }
        }
        Ok(json!({"candidates": candidates, "has_more": false}))
    }

    pub fn handle_command(&mut self, command: RemoteCommand) -> CommandReceipt {
        let rejected = |code: &str| CommandReceipt {
            client_cmd_id: command.client_cmd_id.clone(),
            hub_session_id: command.hub_session_id.clone(),
            status: "rejected".into(),
            code: Some(code.into()),
            result: None,
        };
        let valid_id = |id: &str| {
            !id.is_empty()
                && id.len() <= 128
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        };
        if !valid_id(&command.client_cmd_id) || !valid_id(&command.hub_session_id) {
            return rejected("invalid_command_identity");
        }
        if let Some((previous, receipt)) = self.receipts.get(&command.client_cmd_id) {
            return if previous == &command {
                receipt.clone()
            } else {
                rejected("command_id_conflict")
            };
        }
        // Do not evict IDs and accidentally execute them again. R2 adds a durable ledger.
        if self.receipts.len() >= MAX_COMMAND_LEDGER_ENTRIES {
            // The daemon answers every command id the Gateway still holds in
            // `remote_inbound_commands`, so it stays replay-safe after a
            // restart. This in-memory copy only deduplicates repeats inside one
            // Host lifetime; without eviction it would reach the cap and then
            // reject every later command for the life of the process.
            self.evict_oldest_receipts(self.receipts.len() + 1 - MAX_COMMAND_LEDGER_ENTRIES);
        }
        let result = if self.alive {
            self.execute(&command)
        } else {
            Err(NativeError::rejected("host_lost"))
        };
        let receipt = match result {
            Ok(result) => CommandReceipt {
                client_cmd_id: command.client_cmd_id.clone(),
                hub_session_id: command.hub_session_id.clone(),
                status: "accepted".into(),
                code: None,
                result: Some(result),
            },
            Err(error) => {
                if error.outcome_unknown {
                    self.mark_lost();
                }
                CommandReceipt {
                    client_cmd_id: command.client_cmd_id.clone(),
                    hub_session_id: command.hub_session_id.clone(),
                    status: if error.outcome_unknown {
                        "indeterminate"
                    } else {
                        "rejected"
                    }
                    .into(),
                    code: Some(error.code),
                    result: None,
                }
            }
        };
        self.receipts
            .insert(command.client_cmd_id.clone(), (command, receipt.clone()));
        self.receipt_order.push_back(receipt.client_cmd_id.clone());
        receipt
    }

    /// Records a request id as answered and evicts the oldest once the
    /// suppression window is full. The window only has to outlive a provider's
    /// re-delivery of a request the client has already seen, so bounding it by
    /// count rather than by age is enough to stop a long-lived daemon from
    /// retaining every id it ever answered.
    pub(crate) fn mark_answered(&mut self, id: &str) {
        if self.answered_requests.insert(id.to_string()) {
            self.answered_order.push_back(id.to_string());
        }
        while self.answered_requests.len() > MAX_ANSWERED_REQUESTS {
            let Some(oldest) = self.answered_order.pop_front() else {
                break;
            };
            self.answered_requests.remove(&oldest);
        }
    }

    /// Drops the `count` oldest ledger entries. Callers only reach this once the
    /// ledger is full, so it is expected to run on a long-lived daemon.
    pub(crate) fn evict_oldest_receipts(&mut self, count: usize) {
        for _ in 0..count {
            let Some(oldest) = self.receipt_order.pop_front() else {
                break;
            };
            self.receipts.remove(&oldest);
        }
    }
}
