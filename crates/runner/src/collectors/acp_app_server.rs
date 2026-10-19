use std::{
    collections::{HashMap, VecDeque},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    BoundedLine, ChildJob, CollectorError, ProtocolRead, configure_process_group,
    read_bounded_line, sanitize_protocol_message,
};

#[derive(Debug, Clone)]
pub struct AcpPermissionOption {
    pub option_id: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct PendingPromptInfo {
    pub turn_id: String,
    pub session_id: String,
}

pub struct AcpAppServer {
    program: String,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    receiver: Receiver<ProtocolRead>,
    reader: Option<JoinHandle<()>>,
    pending_messages: VecDeque<Value>,
    next_request_id: u64,
    response_timeout: Duration,
    job: Option<ChildJob>,
    pending_prompts: HashMap<u64, PendingPromptInfo>,
    pending_permissions: HashMap<String, Vec<AcpPermissionOption>>,
    active_turns: HashMap<String, String>,
}

impl AcpAppServer {
    pub fn start(
        program_path: impl AsRef<Path>,
        args: &[String],
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        let program = program_path.as_ref().display().to_string();
        let mut command = Command::new(program_path.as_ref());
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
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
        let job = ChildJob::attach(&child).map_err(|source| {
            let _ = child.kill();
            let _ = child.wait();
            CollectorError::Io {
                program: program.clone(),
                source,
            }
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
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
            job: Some(job),
            pending_prompts: HashMap::new(),
            pending_permissions: HashMap::new(),
            active_turns: HashMap::new(),
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
        let cwd_str = cwd.as_ref().display().to_string();
        let result = self.request(
            "session/new",
            json!({
                "cwd": cwd_str,
                "mcpServers": []
            }),
        )?;
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
        self.request(
            "session/load",
            json!({
                "sessionId": session_id
            }),
        )
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
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.pending_prompts.insert(
            request_id,
            PendingPromptInfo {
                turn_id: turn_id.to_string(),
                session_id: session_id.to_string(),
            },
        );
        self.active_turns
            .insert(session_id.to_string(), turn_id.to_string());
        self.send_json(&json!({
            "jsonrpc": "2.0",
            "method": "session/prompt",
            "id": request_id,
            "params": {
                "sessionId": session_id,
                "prompt": [
                    {
                        "type": "text",
                        "text": text
                    }
                ]
            }
        }))?;
        Ok(request_id)
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
        let raw_id = id
            .as_str()
            .map(|s| s.strip_prefix("acp_req_").unwrap_or(s).to_string())
            .or_else(|| id.as_i64().map(|n| n.to_string()))
            .or_else(|| id.as_u64().map(|n| n.to_string()))
            .unwrap_or_else(|| id.to_string());

        let target_id = if let Ok(num) = raw_id.parse::<i64>() {
            json!(num)
        } else {
            json!(raw_id)
        };

        let options = self.pending_permissions.remove(&raw_id);
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
        let mut incoming = Vec::new();
        while let Some(msg) = self.pending_messages.pop_front() {
            incoming.push(msg);
        }
        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(msg)) => incoming.push(msg),
                Ok(ProtocolRead::LineTooLarge) => {
                    return Err(CollectorError::ProtocolLineTooLarge {
                        program: self.program.clone(),
                        limit: super::MAX_PROTOCOL_LINE_BYTES,
                    });
                }
                Ok(ProtocolRead::InvalidJson(source)) => {
                    return Err(CollectorError::InvalidJson {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::Io(source)) => {
                    return Err(CollectorError::Io {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::Eof) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => break,
            }
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
        // Every arm below resolves the read one way or another, so this is a
        // single bounded wait rather than a retry loop.
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
                limit: super::MAX_PROTOCOL_LINE_BYTES,
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
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(job) = self.job.take() {
            drop(job);
        }
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
                    // Skip banners or log noise
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

pub fn discover_acp_binary() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("ACP_SERVER_PATH").filter(|v| !v.is_empty()) {
        let candidate = std::path::PathBuf::from(path);
        if candidate.is_file() {
            return dunce::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }
    None
}

pub fn build_permission_outcome(
    options: Option<&Vec<AcpPermissionOption>>,
    decision: &str,
) -> Value {
    if decision == "accept" {
        let chosen_opt = options.and_then(|opts| {
            opts.iter()
                .find(|o| o.kind == "allow_once" || o.kind == "allow_always")
                .map(|o| o.option_id.clone())
        });
        if let Some(option_id) = chosen_opt {
            json!({ "outcome": "selected", "optionId": option_id })
        } else {
            json!({ "outcome": "selected" })
        }
    } else {
        let chosen_opt = options.and_then(|opts| {
            opts.iter()
                .find(|o| o.kind == "reject_once" || o.kind == "reject_always")
                .map(|o| o.option_id.clone())
        });
        if let Some(option_id) = chosen_opt {
            json!({ "outcome": "selected", "optionId": option_id })
        } else {
            json!({ "outcome": "cancelled" })
        }
    }
}

pub fn normalize_acp_message(
    msg: Value,
    pending_prompts: &mut HashMap<u64, PendingPromptInfo>,
    pending_permissions: &mut HashMap<String, Vec<AcpPermissionOption>>,
    active_turns: &mut HashMap<String, String>,
) -> Vec<Value> {
    let mut normalized = Vec::new();
    if msg.get("method").is_none() {
        if let Some(id) = msg.get("id").and_then(Value::as_u64) {
            if let Some(prompt_info) = pending_prompts.remove(&id) {
                let is_err = msg.get("error").is_some();
                let stop_reason = msg
                    .get("result")
                    .and_then(|r| r.get("stopReason"))
                    .and_then(Value::as_str)
                    .unwrap_or("end_turn");

                let status_type = if is_err {
                    "failed"
                } else if stop_reason == "cancelled" {
                    "interrupted"
                } else {
                    "completed"
                };

                normalized.push(json!({
                    "method": "turn/completed",
                    "params": {
                        "threadId": prompt_info.session_id,
                        "turn": {
                            "id": prompt_info.turn_id,
                            "status": {
                                "type": status_type
                            }
                        }
                    }
                }));
                active_turns.remove(&prompt_info.session_id);
                return normalized;
            }
        }
    }

    if let Some(method) = msg.get("method").and_then(Value::as_str) {
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let session_id = params
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let active_turn_id = active_turns
            .get(&session_id)
            .cloned()
            .unwrap_or_else(|| "turn_unknown".to_string());

        match method {
            "session/update" => {
                let update = &params["update"];
                let update_type = update
                    .get("sessionUpdate")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match update_type {
                    "agent_message_chunk" => {
                        let text = update
                            .get("content")
                            .and_then(|c| c.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        normalized.push(json!({
                            "method": "item/agentMessage/delta",
                            "params": {
                                "threadId": session_id,
                                "turnId": active_turn_id,
                                "itemId": "msg-1",
                                "delta": text
                            }
                        }));
                    }
                    "tool_call" => {
                        let tool_call_id = update
                            .get("toolCallId")
                            .and_then(Value::as_str)
                            .unwrap_or("tool-1");
                        let kind = update
                            .get("kind")
                            .and_then(Value::as_str)
                            .unwrap_or("other");
                        let item_type = match kind {
                            "edit" | "write" | "delete" | "move" => "fileChange",
                            "execute" => "commandExecution",
                            _ => "commandExecution",
                        };
                        normalized.push(json!({
                            "method": "item/started",
                            "params": {
                                "threadId": session_id,
                                "turnId": active_turn_id,
                                "item": {
                                    "id": tool_call_id,
                                    "type": item_type
                                }
                            }
                        }));
                    }
                    "tool_call_update" => {
                        let tool_call_id = update
                            .get("toolCallId")
                            .and_then(Value::as_str)
                            .unwrap_or("tool-1");
                        let status = update
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("completed");
                        normalized.push(json!({
                            "method": "item/completed",
                            "params": {
                                "threadId": session_id,
                                "turnId": active_turn_id,
                                "item": {
                                    "id": tool_call_id,
                                    "status": status
                                }
                            }
                        }));
                    }
                    _ => {}
                }
            }
            "session/request_permission" => {
                let req_id_raw = msg.get("id").cloned().unwrap_or(json!("1"));
                let req_id_str = req_id_raw
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| req_id_raw.as_i64().map(|n| n.to_string()))
                    .or_else(|| req_id_raw.as_u64().map(|n| n.to_string()))
                    .unwrap_or_else(|| "1".to_string());

                let tool_call = &params["toolCall"];
                let kind = tool_call
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("execute");
                let title = tool_call
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Execute tool");
                let raw_input = &tool_call["rawInput"];

                let is_file = matches!(kind, "edit" | "write" | "fileChange");

                if let Some(options_array) = params.get("options").and_then(Value::as_array) {
                    let mut opts = Vec::new();
                    for opt in options_array {
                        if let (Some(opt_id), Some(opt_kind)) = (
                            opt.get("optionId").and_then(Value::as_str),
                            opt.get("kind").and_then(Value::as_str),
                        ) {
                            opts.push(AcpPermissionOption {
                                option_id: opt_id.to_string(),
                                kind: opt_kind.to_string(),
                            });
                        }
                    }
                    pending_permissions.insert(req_id_str.clone(), opts);
                }

                if is_file {
                    let file_path = raw_input
                        .get("file_path")
                        .or_else(|| raw_input.get("path"))
                        .or_else(|| raw_input.get("filePath"))
                        .and_then(Value::as_str);

                    let diff = if let (Some(old_str), Some(new_str)) = (
                        raw_input.get("old_string").and_then(Value::as_str),
                        raw_input.get("new_string").and_then(Value::as_str),
                    ) {
                        let mut payload = json!({
                            "old_string": crate::privacy::redact_paths_inside_text(old_str),
                            "new_string": crate::privacy::redact_paths_inside_text(new_str),
                        })
                        .to_string();
                        if payload.len() > 65536 {
                            payload.truncate(65536);
                        }
                        Some(payload)
                    } else if let Some(content) = raw_input.get("content").and_then(Value::as_str) {
                        let mut payload = json!({
                            "new_string": crate::privacy::redact_paths_inside_text(content),
                        })
                        .to_string();
                        if payload.len() > 65536 {
                            payload.truncate(65536);
                        }
                        Some(payload)
                    } else if let Some(raw_diff) = raw_input
                        .get("diff")
                        .or_else(|| raw_input.get("patch"))
                        .and_then(Value::as_str)
                    {
                        let mut sanitized = crate::privacy::redact_paths_inside_text(raw_diff);
                        if sanitized.len() > 65536 {
                            sanitized.truncate(65536);
                        }
                        Some(sanitized)
                    } else {
                        None
                    };

                    normalized.push(json!({
                        "id": format!("acp_req_{}", req_id_str),
                        "method": "item/fileChange/requestApproval",
                        "params": {
                            "threadId": session_id,
                            "turnId": active_turn_id,
                            "requestId": req_id_raw,
                            "toolName": "FileChange",
                            "filePath": file_path,
                            "diff": diff,
                            "reason": title,
                            "availableDecisions": ["accept", "decline"]
                        }
                    }));
                } else {
                    let command = raw_input
                        .get("command")
                        .or_else(|| raw_input.get("cmd"))
                        .and_then(Value::as_str)
                        .unwrap_or(title);

                    normalized.push(json!({
                        "id": format!("acp_req_{}", req_id_str),
                        "method": "item/commandExecution/requestApproval",
                        "params": {
                            "threadId": session_id,
                            "turnId": active_turn_id,
                            "requestId": req_id_raw,
                            "toolName": "Bash",
                            "command": command,
                            "reason": title,
                            "availableDecisions": ["accept", "decline"]
                        }
                    }));
                }
            }
            _ => {}
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_agent_message_chunk_to_delta() {
        let mut prompts = HashMap::new();
        let mut permissions = HashMap::new();
        let mut turns = HashMap::new();
        turns.insert("s-1".to_string(), "t-100".to_string());

        let chunk = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "s-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {
                        "type": "text",
                        "text": "Hello ACP"
                    }
                }
            }
        });

        let events = normalize_acp_message(chunk, &mut prompts, &mut permissions, &mut turns);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/agentMessage/delta");
        assert_eq!(events[0]["params"]["threadId"], "s-1");
        assert_eq!(events[0]["params"]["turnId"], "t-100");
        assert_eq!(events[0]["params"]["delta"], "Hello ACP");
    }

    #[test]
    fn normalizes_tool_call_lifecycle() {
        let mut prompts = HashMap::new();
        let mut permissions = HashMap::new();
        let mut turns = HashMap::new();
        turns.insert("s-1".to_string(), "t-1".to_string());

        let started = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "s-1",
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-42",
                    "kind": "edit"
                }
            }
        });

        let events = normalize_acp_message(started, &mut prompts, &mut permissions, &mut turns);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/started");
        assert_eq!(events[0]["params"]["item"]["id"], "call-42");
        assert_eq!(events[0]["params"]["item"]["type"], "fileChange");

        let completed = json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "s-1",
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-42",
                    "status": "completed"
                }
            }
        });

        let events = normalize_acp_message(completed, &mut prompts, &mut permissions, &mut turns);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "item/completed");
        assert_eq!(events[0]["params"]["item"]["id"], "call-42");
        assert_eq!(events[0]["params"]["item"]["status"], "completed");
    }

    #[test]
    fn normalizes_request_permission_with_edit_diff_and_redaction() {
        let mut prompts = HashMap::new();
        let mut permissions = HashMap::new();
        let mut turns = HashMap::new();
        turns.insert("s-1".to_string(), "t-1".to_string());

        let req = json!({
            "jsonrpc": "2.0",
            "id": 105,
            "method": "session/request_permission",
            "params": {
                "sessionId": "s-1",
                "toolCall": {
                    "kind": "edit",
                    "title": "Edit src/main.rs",
                    "rawInput": {
                        "filePath": "src/main.rs",
                        "old_string": "const SECRET: &str = \"/Users/alice/token.txt\";",
                        "new_string": "const SECRET: &str = \"/Users/alice/new_token.txt\";"
                    }
                },
                "options": [
                    { "optionId": "opt-allow-1", "kind": "allow_once" },
                    { "optionId": "opt-reject-1", "kind": "reject_once" }
                ]
            }
        });

        let events = normalize_acp_message(req, &mut prompts, &mut permissions, &mut turns);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], "acp_req_105");
        assert_eq!(events[0]["method"], "item/fileChange/requestApproval");

        let diff = events[0]["params"]["diff"].as_str().expect("diff present");
        assert!(diff.contains("[local_path_redacted]"));
        assert!(!diff.contains("/Users/alice/token.txt"));

        // Options cached in pending_permissions
        let opts = permissions.get("105").expect("options saved");
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].option_id, "opt-allow-1");

        // Test decision mapping
        let accept_outcome = build_permission_outcome(Some(opts), "accept");
        assert_eq!(accept_outcome["outcome"], "selected");
        assert_eq!(accept_outcome["optionId"], "opt-allow-1");

        let decline_outcome = build_permission_outcome(Some(opts), "decline");
        assert_eq!(decline_outcome["outcome"], "selected");
        assert_eq!(decline_outcome["optionId"], "opt-reject-1");
    }

    #[test]
    fn prompt_response_resolves_turn_completion() {
        let mut prompts = HashMap::new();
        prompts.insert(
            77,
            PendingPromptInfo {
                turn_id: "turn-prompt-1".into(),
                session_id: "session-acp-1".into(),
            },
        );
        let mut permissions = HashMap::new();
        let mut turns = HashMap::new();
        turns.insert("session-acp-1".into(), "turn-prompt-1".into());

        let res = json!({
            "jsonrpc": "2.0",
            "id": 77,
            "result": {
                "stopReason": "end_turn"
            }
        });

        let events = normalize_acp_message(res, &mut prompts, &mut permissions, &mut turns);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["method"], "turn/completed");
        assert_eq!(events[0]["params"]["threadId"], "session-acp-1");
        assert_eq!(events[0]["params"]["turn"]["id"], "turn-prompt-1");
        assert_eq!(events[0]["params"]["turn"]["status"]["type"], "completed");
        assert!(turns.is_empty());
        assert!(prompts.is_empty());
    }
}
