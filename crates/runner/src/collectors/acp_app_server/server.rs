use std::collections::BTreeMap;

use super::*;

impl AcpAppServer {
    pub fn start(
        program_path: impl AsRef<Path>,
        args: &[String],
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        Self::start_with_options(
            program_path,
            args,
            None,
            &BTreeMap::new(),
            true,
            response_timeout,
        )
    }

    pub fn start_with_options(
        program_path: impl AsRef<Path>,
        args: &[String],
        working_dir: Option<&Path>,
        extra_env: &BTreeMap<String, String>,
        inject_gateway_env: bool,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        let path = program_path.as_ref();
        let program = path.display().to_string();
        let mut command = super::super::command_for_binary(path);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(dir) = working_dir {
            command.current_dir(dir);
        }
        for (k, v) in extra_env {
            if !k.trim().is_empty() {
                command.env(k, v);
            }
        }
        configure_process_group(&mut command);
        if inject_gateway_env {
            configure_acp_gateway_env(&mut command, path, args);
        }
        Self::start_with_command(command, program, response_timeout)
    }

    pub fn start_with_command(
        mut command: Command,
        program: String,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        let mut child = command.spawn().map_err(|source| CollectorError::Start {
            program: program.clone(),
            source,
        })?;
        let mut job = Some(ChildJob::attach(&child).map_err(|source| {
            let _ = child.kill();
            let _ = child.wait();
            CollectorError::Io {
                program: program.clone(),
                source,
            }
        })?);
        let (stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
            (Some(stdin), Some(stdout)) => (stdin, stdout),
            _ => {
                drop(job.take());
                let _ = child.kill();
                let _ = child.wait();
                return Err(CollectorError::MissingStdio {
                    program: program.clone(),
                });
            }
        };
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || read_acp_protocol(stdout, sender));
        let mut server = Self {
            program,
            child,
            stdin: BufWriter::new(stdin),
            receiver,
            reader: Some(reader),
            pending_messages: VecDeque::new(),
            next_request_id: 1,
            response_timeout,
            job,
            pending_prompts: HashMap::new(),
            pending_permissions: HashMap::new(),
            active_turns: HashMap::new(),
            eof_reached: false,
            deferred_read_error: None,
        };
        server.initialize()?;
        Ok(server)
    }

    fn initialize(&mut self) -> Result<Value, CollectorError> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": "1.0",
                "clientInfo": {
                    "name": "AgentHub",
                    "title": "AgentHub Runner",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "clientCapabilities": {}
            }),
        )
    }

    pub fn session_new(&mut self, cwd: impl AsRef<Path>) -> Result<String, CollectorError> {
        self.session_new_with_options(cwd, None, None)
    }

    pub fn session_new_with_options(
        &mut self,
        cwd: impl AsRef<Path>,
        model: Option<&str>,
        permission_mode: Option<&str>,
    ) -> Result<String, CollectorError> {
        let cwd_str = cwd.as_ref().display().to_string();
        let mut params = json!({
            "cwd": cwd_str,
            "mcpServers": []
        });
        if model.is_some() || permission_mode.is_some() {
            params["_meta"] = json!({
                "model": model,
                "permissionMode": permission_mode,
            });
        }
        let result = self.request("session/new", params)?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| CollectorError::Protocol {
                program: self.program.clone(),
                code: "missing_session_id".into(),
                message: "session/new response did not contain sessionId".into(),
            })?;
        Ok(session_id.to_string())
    }

    pub fn session_load(&mut self, session_id: &str) -> Result<Value, CollectorError> {
        self.session_load_with_cwd(session_id, None)
    }

    pub fn session_load_with_cwd(
        &mut self,
        session_id: &str,
        cwd: Option<&Path>,
    ) -> Result<Value, CollectorError> {
        let mut params = json!({
            "sessionId": session_id,
            "mcpServers": []
        });
        if let Some(c) = cwd {
            params["cwd"] = json!(c.to_string_lossy());
        }
        self.request("session/load", params)
    }

    pub fn session_cancel(&mut self, session_id: &str) -> Result<(), CollectorError> {
        self.send_notification(
            "session/cancel",
            json!({
                "sessionId": session_id
            }),
        )
    }

    pub fn send_tracked_prompt(
        &mut self,
        session_id: &str,
        text: &str,
        turn_id: &str,
    ) -> Result<u64, CollectorError> {
        self.send_tracked_prompt_with_options(session_id, text, turn_id, None, None)
    }

    pub fn send_tracked_prompt_with_options(
        &mut self,
        session_id: &str,
        text: &str,
        turn_id: &str,
        model: Option<&str>,
        permission_mode: Option<&str>,
    ) -> Result<u64, CollectorError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let mut params = json!({
            "sessionId": session_id,
            "prompt": [
                {
                    "type": "text",
                    "text": text
                }
            ]
        });
        if model.is_some() || permission_mode.is_some() {
            params["_meta"] = json!({
                "model": model,
                "permissionMode": permission_mode,
            });
        }
        self.send_json(&json!({
            "jsonrpc": "2.0",
            "method": "session/prompt",
            "id": request_id,
            "params": params
        }))?;
        self.pending_prompts.insert(
            request_id,
            PendingPromptInfo {
                turn_id: turn_id.to_string(),
                session_id: session_id.to_string(),
            },
        );
        self.active_turns
            .insert(session_id.to_string(), turn_id.to_string());
        Ok(request_id)
    }

    pub fn request_raw(&mut self, method: &str, params: Value) -> Result<Value, CollectorError> {
        self.request(method, params)
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, CollectorError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.send_json(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "id": request_id,
            "params": params
        }))?;

        let deadline = Instant::now() + self.response_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CollectorError::Timeout {
                    program: self.program.clone(),
                    timeout_ms: self.response_timeout.as_millis(),
                });
            }
            let message = self.read_message(request_id, remaining)?;
            if message.get("method").is_some()
                || message.get("id").and_then(Value::as_u64) != Some(request_id)
            {
                self.pending_messages.push_back(message);
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(CollectorError::Protocol {
                    program: self.program.clone(),
                    code: error
                        .get("code")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "unknown".into()),
                    message: sanitize_protocol_message(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error"),
                    ),
                });
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    pub fn respond_permission(&mut self, id: Value, decision: &str) -> Result<(), CollectorError> {
        let lookup_key = id
            .as_str()
            .map(|s| s.strip_prefix("acp_req_").unwrap_or(s).to_string())
            .or_else(|| id.as_i64().map(|n| n.to_string()))
            .or_else(|| id.as_u64().map(|n| n.to_string()))
            .unwrap_or_else(|| id.to_string());

        let pending = self.pending_permissions.remove(&lookup_key);
        let (target_id, options) = match pending {
            Some(req) => (req.raw_id, Some(req.options)),
            None => {
                let fallback_id = match &id {
                    Value::String(s) => {
                        Value::String(s.strip_prefix("acp_req_").unwrap_or(s).to_string())
                    }
                    other => other.clone(),
                };
                (fallback_id, None)
            }
        };
        let outcome = build_permission_outcome(options.as_ref(), decision);

        self.send_json(&json!({
            "jsonrpc": "2.0",
            "id": target_id,
            "result": {
                "outcome": outcome
            }
        }))
    }

    pub fn respond(&mut self, id: Value, result: Value) -> Result<(), CollectorError> {
        self.send_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    pub fn send_notification(&mut self, method: &str, params: Value) -> Result<(), CollectorError> {
        self.send_json(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        }))
    }

    fn send_json(&mut self, payload: &Value) -> Result<(), CollectorError> {
        serde_json::to_writer(&mut self.stdin, payload).map_err(|source| {
            CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }
        })?;
        self.stdin
            .write_all(b"\n")
            .map_err(|source| CollectorError::Io {
                program: self.program.clone(),
                source,
            })?;
        self.stdin.flush().map_err(|source| CollectorError::Io {
            program: self.program.clone(),
            source,
        })
    }

    pub fn drain_messages(&mut self) -> Result<Vec<Value>, CollectorError> {
        if self.pending_messages.is_empty()
            && let Some(err) = self.deferred_read_error.take()
        {
            return Err(err);
        }
        if self.eof_reached && self.pending_messages.is_empty() {
            return Err(CollectorError::UnexpectedEof {
                program: self.program.clone(),
                request_id: self.next_request_id,
            });
        }
        let mut incoming = Vec::new();
        while let Some(msg) = self.pending_messages.pop_front() {
            incoming.push(msg);
        }
        let mut terminal_err: Option<CollectorError> = None;
        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(msg)) => incoming.push(msg),
                Ok(ProtocolRead::LineTooLarge) => {
                    terminal_err = Some(CollectorError::ProtocolLineTooLarge {
                        program: self.program.clone(),
                        limit: crate::collectors::MAX_PROTOCOL_LINE_BYTES,
                    });
                    break;
                }
                Ok(ProtocolRead::InvalidJson(source)) => {
                    terminal_err = Some(CollectorError::InvalidJson {
                        program: self.program.clone(),
                        source,
                    });
                    break;
                }
                Ok(ProtocolRead::Io(source)) => {
                    terminal_err = Some(CollectorError::Io {
                        program: self.program.clone(),
                        source,
                    });
                    break;
                }
                Ok(ProtocolRead::Eof) => {
                    self.eof_reached = true;
                    terminal_err = Some(CollectorError::UnexpectedEof {
                        program: self.program.clone(),
                        request_id: self.next_request_id,
                    });
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    self.eof_reached = true;
                    terminal_err = Some(CollectorError::ReaderStopped {
                        program: self.program.clone(),
                    });
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        if let Some(err) = terminal_err {
            if incoming.is_empty() {
                return Err(err);
            }
            // Preserve already-dequeued messages and surface the error on next drain.
            self.deferred_read_error = Some(err);
        }

        let mut normalized = Vec::new();
        for msg in incoming {
            normalized.extend(normalize_acp_message(
                msg,
                &mut self.pending_prompts,
                &mut self.pending_permissions,
                &mut self.active_turns,
            ));
        }

        Ok(normalized)
    }

    fn read_message(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<Value, CollectorError> {
        let deadline = Instant::now() + timeout;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CollectorError::Timeout {
                program: self.program.clone(),
                timeout_ms: timeout.as_millis(),
            });
        }
        match self.receiver.recv_timeout(remaining) {
            Ok(ProtocolRead::Message(message)) => Ok(message),
            Ok(ProtocolRead::LineTooLarge) => Err(CollectorError::ProtocolLineTooLarge {
                program: self.program.clone(),
                limit: crate::collectors::MAX_PROTOCOL_LINE_BYTES,
            }),
            Ok(ProtocolRead::InvalidJson(source)) => Err(CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }),
            Ok(ProtocolRead::Io(source)) => Err(CollectorError::Io {
                program: self.program.clone(),
                source,
            }),
            Ok(ProtocolRead::Eof) => Err(CollectorError::UnexpectedEof {
                program: self.program.clone(),
                request_id,
            }),
            Err(RecvTimeoutError::Timeout) => Err(CollectorError::Timeout {
                program: self.program.clone(),
                timeout_ms: timeout.as_millis(),
            }),
            Err(RecvTimeoutError::Disconnected) => Err(CollectorError::ReaderStopped {
                program: self.program.clone(),
            }),
        }
    }
}

impl Drop for AcpAppServer {
    fn drop(&mut self) {
        if let Some(job) = self.job.take() {
            drop(job);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_acp_protocol(stdout: impl Read, sender: mpsc::Sender<ProtocolRead>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_bounded_line(&mut reader) {
            BoundedLine::Eof => {
                let _ = sender.send(ProtocolRead::Eof);
                return;
            }
            BoundedLine::TooLarge => {
                let _ = sender.send(ProtocolRead::LineTooLarge);
                return;
            }
            BoundedLine::Line(line) => {
                let trimmed = line.trim();
                if !trimmed.starts_with('{') {
                    continue;
                }
                match serde_json::from_str(trimmed) {
                    Ok(message) => {
                        if sender.send(ProtocolRead::Message(message)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(ProtocolRead::InvalidJson(error));
                        return;
                    }
                }
            }
            BoundedLine::Io(error) => {
                let _ = sender.send(ProtocolRead::Io(error));
                return;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpGatewayKind {
    Codex,
    Antigravity,
    ClaudeCode,
}

pub fn detect_acp_gateway_kind(program_path: &Path, _args: &[String]) -> Option<AcpGatewayKind> {
    let name = program_path
        .file_name()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.contains("codex") {
        Some(AcpGatewayKind::Codex)
    } else if name.contains("gemini") || name.contains("antigravity") {
        Some(AcpGatewayKind::Antigravity)
    } else if name.contains("claude") {
        Some(AcpGatewayKind::ClaudeCode)
    } else {
        None
    }
}

fn configure_acp_gateway_env(command: &mut Command, program_path: &Path, args: &[String]) {
    configure_acp_gateway_env_with(
        command,
        program_path,
        args,
        crate::claude_gateway::discover_codex_gateway,
        crate::claude_gateway::discover_antigravity_gateway,
        crate::claude_gateway::discover_claude_code_gateway,
    );
}

pub fn configure_acp_gateway_env_with<FCodex, FAnti, FClaude>(
    command: &mut Command,
    program_path: &Path,
    args: &[String],
    codex_fn: FCodex,
    antigravity_fn: FAnti,
    claude_fn: FClaude,
) where
    FCodex: FnOnce() -> Option<crate::claude_gateway::CodexGatewayProfile>,
    FAnti: FnOnce() -> Option<crate::claude_gateway::AntigravityGatewayProfile>,
    FClaude: FnOnce() -> Option<crate::claude_gateway::ClaudeGatewayProfile>,
{
    let mut ignored_model = None;
    match detect_acp_gateway_kind(program_path, args) {
        Some(AcpGatewayKind::Codex) => {
            if let Some(profile) = codex_fn() {
                profile.inject_into_command(command, &mut ignored_model);
            }
        }
        Some(AcpGatewayKind::Antigravity) => {
            if let Some(profile) = antigravity_fn() {
                profile.inject_into_command(command, &mut ignored_model);
            }
        }
        Some(AcpGatewayKind::ClaudeCode) => {
            if let Some(profile) = claude_fn() {
                profile.inject_into_command(command, &mut ignored_model);
            }
        }
        None => {}
    }
}
