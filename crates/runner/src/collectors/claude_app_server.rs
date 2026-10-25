use std::{
    collections::VecDeque,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    BoundedLine, ChildJob, CollectorError, MAX_PROTOCOL_LINE_BYTES, ProtocolRead, TurnOptions,
    configure_process_group, read_bounded_line,
};

const MAX_STDERR_CAPTURE_BYTES: usize = 8192;

/// A persistent, bidirectional Claude Code session running via:
/// `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages`
///
/// Holding stdin open allows multi-turn prompt execution, live delta streaming,
/// and bi-directional control requests (e.g. approvals and permission updates).
pub struct ClaudeProcessSession {
    program: String,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    receiver: Receiver<ProtocolRead>,
    _reader: Option<JoinHandle<()>>,
    _stderr_reader: Option<JoinHandle<()>>,
    stderr_buf: Arc<Mutex<String>>,
    job: Option<ChildJob>,
    session_id: String,
    cwd: PathBuf,
    model: Option<String>,
    requested_model: Option<String>,
    reasoning_effort: Option<String>,
    permission_mode: Option<String>,
    claude_desktop: bool,
    pending_messages: VecDeque<Value>,
    is_turn_in_progress: bool,
    current_turn_id: Option<String>,
    eof_reached: bool,
}

impl ClaudeProcessSession {
    /// Launches a long-lived Claude Code process for a workspace directory.
    pub fn start(
        binary: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        resume_session_id: Option<&str>,
        options: TurnOptions<'_>,
    ) -> Result<Self, CollectorError> {
        let binary_path = binary.as_ref();
        let program = binary_path.display().to_string();
        let cwd_path = cwd.as_ref().to_path_buf();

        let mut command = Command::new(binary_path);
        command.current_dir(&cwd_path);

        let mut base_args = vec![
            "-p".to_string(),
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
            "--include-partial-messages".to_string(),
        ];

        if let Some(id) = resume_session_id
            && !id.is_empty()
        {
            base_args.push("--resume".to_string());
            base_args.push(id.to_string());
        }

        command.args(&base_args);

        // Apply client-appropriate Gateway configuration if present
        let mut effective_model = options
            .model
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(gateway) =
            crate::claude_gateway::discover_claude_gateway_for_client(options.claude_desktop)
        {
            gateway.inject_into_command_with_session(
                &mut command,
                &mut effective_model,
                resume_session_id,
            );
        }

        // Apply CLI flags
        if let Some(ref model) = effective_model {
            command.args(["--model", model]);
        }
        if let Some(mode) = options
            .permission_mode
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            command.args(["--permission-mode", mode]);
        }
        if let Some(effort) = options
            .reasoning_effort
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            command.args(["--effort", effort]);
        }

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        configure_process_group(&mut command);

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
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;

        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let stderr_clone = stderr_buf.clone();
        let stderr_reader = thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut chunk = [0_u8; 1024];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let piece = String::from_utf8_lossy(&chunk[..n]);
                if let Ok(mut guard) = stderr_clone.lock() {
                    if guard.len() < MAX_STDERR_CAPTURE_BYTES {
                        let remaining = MAX_STDERR_CAPTURE_BYTES - guard.len();
                        let clipped: String = piece.chars().take(remaining).collect();
                        guard.push_str(&clipped);
                    }
                }
            }
        });

        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_bounded_line(&mut reader) {
                    BoundedLine::Line(line) => {
                        let parsed: Result<Value, _> = serde_json::from_str(&line);
                        match parsed {
                            Ok(msg) => {
                                if sender.send(ProtocolRead::Message(msg)).is_err() {
                                    break;
                                }
                            }
                            Err(source) => {
                                let _ = sender.send(ProtocolRead::InvalidJson(source));
                                break;
                            }
                        }
                    }
                    BoundedLine::TooLarge => {
                        let _ = sender.send(ProtocolRead::LineTooLarge);
                        break;
                    }
                    BoundedLine::Io(source) => {
                        let _ = sender.send(ProtocolRead::Io(source));
                        break;
                    }
                    BoundedLine::Eof => {
                        let _ = sender.send(ProtocolRead::Eof);
                        break;
                    }
                }
            }
        });

        let mut session = Self {
            program: program.clone(),
            child,
            stdin: BufWriter::new(stdin),
            receiver,
            _reader: Some(reader),
            _stderr_reader: Some(stderr_reader),
            stderr_buf,
            job: Some(job),
            session_id: resume_session_id.unwrap_or_default().to_string(),
            cwd: cwd_path,
            model: effective_model,
            requested_model: options.model.map(str::to_string),
            reasoning_effort: options.reasoning_effort.map(str::to_string),
            permission_mode: options.permission_mode.map(str::to_string),
            claude_desktop: options.claude_desktop,
            pending_messages: VecDeque::new(),
            is_turn_in_progress: false,
            current_turn_id: None,
            eof_reached: false,
        };

        // `claude -p --input-format stream-json` waits for the first user JSONL frame
        // on stdin before emitting `system/init`. Probe briefly (60ms) so immediate
        // CLI flag / resume failures surface right away without adding a 10s delay.
        let init_timeout = Duration::from_millis(60);
        let deadline = Instant::now() + init_timeout;
        let mut discovered_session_id = None;

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match session.receiver.recv_timeout(remaining) {
                Ok(ProtocolRead::Message(msg)) => {
                    if msg.get("type").and_then(Value::as_str) == Some("system")
                        && msg.get("subtype").and_then(Value::as_str) == Some("init")
                    {
                        if let Some(id) = msg.get("session_id").and_then(Value::as_str) {
                            discovered_session_id = Some(id.to_string());
                        }
                        session.pending_messages.push_back(msg);
                        break;
                    } else {
                        session.pending_messages.push_back(msg);
                    }
                }
                Ok(ProtocolRead::InvalidJson(source)) => {
                    return Err(CollectorError::InvalidJson { program, source });
                }
                Ok(ProtocolRead::LineTooLarge) => {
                    return Err(CollectorError::ProtocolLineTooLarge {
                        program,
                        limit: MAX_PROTOCOL_LINE_BYTES,
                    });
                }
                Ok(ProtocolRead::Io(source)) => {
                    return Err(CollectorError::Io { program, source });
                }
                Ok(ProtocolRead::Eof) => {
                    let detail = session.last_stderr();
                    let status = match session.child.try_wait() {
                        Ok(Some(code)) if !detail.is_empty() => format!("{code}: {detail}"),
                        Ok(Some(code)) => code.to_string(),
                        _ if !detail.is_empty() => detail,
                        _ => "unexpected EOF during startup".to_string(),
                    };
                    return Err(CollectorError::Exit { program, status });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CollectorError::ReaderStopped { program });
                }
            }
        }

        if let Some(id) = discovered_session_id {
            session.session_id = id;
        }

        Ok(session)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn requested_model(&self) -> Option<&str> {
        self.requested_model.as_deref()
    }

    pub fn reasoning_effort(&self) -> Option<&str> {
        self.reasoning_effort.as_deref()
    }

    pub fn permission_mode(&self) -> Option<&str> {
        self.permission_mode.as_deref()
    }

    pub fn is_claude_desktop(&self) -> bool {
        self.claude_desktop
    }

    pub fn is_dead(&self) -> bool {
        self.eof_reached
    }

    pub fn last_stderr(&self) -> String {
        let raw = self
            .stderr_buf
            .lock()
            .ok()
            .map(|g| g.trim().to_string())
            .unwrap_or_default();
        if raw.is_empty() {
            return String::new();
        }
        let scrubbed = crate::privacy::redact_paths_inside_text(&raw);
        scrubbed.chars().take(512).collect()
    }

    pub fn is_turn_in_progress(&self) -> bool {
        self.is_turn_in_progress
    }

    /// Sends a new user turn prompt to the long-running Claude process over stdin as JSONL.
    pub fn send_prompt(&mut self, turn_id: &str, prompt: &str) -> Result<(), CollectorError> {
        self.send_prompt_with_blocks(turn_id, prompt, &[])
    }

    /// Sends a user turn with optional extra content blocks (such as base64 image blocks).
    pub fn send_prompt_with_blocks(
        &mut self,
        turn_id: &str,
        prompt: &str,
        extra_blocks: &[Value],
    ) -> Result<(), CollectorError> {
        let mut content = Vec::with_capacity(extra_blocks.len() + 1);
        content.extend_from_slice(extra_blocks);
        content.push(json!({
            "type": "text",
            "text": prompt
        }));
        let msg = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": content
            }
        });

        self.send_json(&msg)?;
        self.is_turn_in_progress = true;
        self.current_turn_id = Some(turn_id.to_string());
        Ok(())
    }

    /// Sends a control response or request over stdin (e.g. for approvals / permissions).
    pub fn send_control_response(
        &mut self,
        request_id: &str,
        subtype: &str,
        payload: Value,
    ) -> Result<(), CollectorError> {
        let msg = json!({
            "type": "control_response",
            "response": {
                "subtype": subtype,
                "request_id": request_id,
                "response": payload
            }
        });
        self.send_json(&msg)
    }

    fn send_json(&mut self, message: &Value) -> Result<(), CollectorError> {
        serde_json::to_writer(&mut self.stdin, message).map_err(|source| {
            CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }
        })?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|source| CollectorError::Io {
                program: self.program.clone(),
                source,
            })
    }

    /// Drains raw stdout messages collected so far.
    pub fn drain_messages(&mut self) -> Result<Vec<Value>, CollectorError> {
        let mut messages = Vec::new();
        while let Some(msg) = self.pending_messages.pop_front() {
            self.track_turn_state(&msg);
            messages.push(msg);
        }

        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(msg)) => {
                    self.track_turn_state(&msg);
                    messages.push(msg);
                }
                Ok(ProtocolRead::InvalidJson(source)) => {
                    return Err(CollectorError::InvalidJson {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::LineTooLarge) => {
                    return Err(CollectorError::ProtocolLineTooLarge {
                        program: self.program.clone(),
                        limit: MAX_PROTOCOL_LINE_BYTES,
                    });
                }
                Ok(ProtocolRead::Io(source)) => {
                    return Err(CollectorError::Io {
                        program: self.program.clone(),
                        source,
                    });
                }
                Ok(ProtocolRead::Eof) | Err(TryRecvError::Disconnected) => {
                    self.eof_reached = true;
                    if self.is_turn_in_progress {
                        self.is_turn_in_progress = false;
                        let stderr_detail = self.last_stderr();
                        let exit_detail = match self.child.try_wait() {
                            Ok(Some(status)) if !stderr_detail.is_empty() => {
                                format!("Claude 进程异常退出 ({status}): {stderr_detail}")
                            }
                            Ok(Some(status)) => format!("Claude 进程异常退出 ({status})"),
                            _ if !stderr_detail.is_empty() => {
                                format!("Claude 会话连接中断: {stderr_detail}")
                            }
                            _ => "Claude 会话连接中断，未返回执行结果".to_string(),
                        };
                        messages.push(json!({
                            "type": "result",
                            "subtype": "error_process_exit",
                            "is_error": true,
                            "result": exit_detail,
                        }));
                    }
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        Ok(messages)
    }

    fn track_turn_state(&mut self, msg: &Value) {
        if let Some(msg_type) = msg.get("type").and_then(Value::as_str) {
            if msg_type == "system"
                && msg.get("subtype").and_then(Value::as_str) == Some("init")
                && let Some(sid) = msg.get("session_id").and_then(Value::as_str)
                && !sid.is_empty()
            {
                self.session_id = sid.to_string();
            } else if msg_type == "result" {
                if let Some(sid) = msg.get("session_id").and_then(Value::as_str)
                    && !sid.is_empty()
                {
                    self.session_id = sid.to_string();
                }
                self.is_turn_in_progress = false;
            }
        }
    }

    /// Gracefully interrupts or kills the process.
    pub fn interrupt(&mut self) -> Result<(), CollectorError> {
        let _ = self.child.kill();
        self.is_turn_in_progress = false;
        Ok(())
    }
}

impl Drop for ClaudeProcessSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.job.take();
    }
}
