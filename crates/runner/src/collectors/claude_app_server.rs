use std::{
    collections::VecDeque,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    BoundedLine, ChildJob, CollectorError, MAX_PROTOCOL_LINE_BYTES, ProtocolRead, TurnOptions,
    configure_process_group, read_bounded_line,
};

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
    job: Option<ChildJob>,
    session_id: String,
    cwd: PathBuf,
    model: Option<String>,
    pending_messages: VecDeque<Value>,
    is_turn_in_progress: bool,
    current_turn_id: Option<String>,
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

        // Apply Gateway configuration if present
        let mut effective_model = options.model.map(str::to_string);
        if let Some(gateway) = crate::claude_gateway::discover_claude_gateway() {
            gateway.inject_into_command(&mut command, &mut effective_model);
        }

        // Apply CLI flags
        if let Some(ref model) = effective_model {
            command.args(["--model", model]);
        }
        if let Some(mode) = options.permission_mode {
            command.args(["--permission-mode", mode]);
        }
        if let Some(effort) = options.reasoning_effort {
            command.args(["--effort", effort]);
        }

        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

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
            job: Some(job),
            session_id: resume_session_id.unwrap_or_default().to_string(),
            cwd: cwd_path,
            model: effective_model,
            pending_messages: VecDeque::new(),
            is_turn_in_progress: false,
            current_turn_id: None,
        };

        // Wait up to 10 seconds for the system/init handshake line from Claude Code
        let init_timeout = Duration::from_secs(10);
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
                    return Err(CollectorError::UnexpectedEof {
                        program,
                        request_id: 0,
                    });
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

    pub fn is_turn_in_progress(&self) -> bool {
        self.is_turn_in_progress
    }

    /// Sends a new user turn prompt to the long-running Claude process over stdin as JSONL.
    pub fn send_prompt(&mut self, turn_id: &str, prompt: &str) -> Result<(), CollectorError> {
        let msg = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": prompt
                    }
                ]
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
                Ok(ProtocolRead::Eof)
                | Err(TryRecvError::Empty)
                | Err(TryRecvError::Disconnected) => break,
            }
        }

        Ok(messages)
    }

    fn track_turn_state(&mut self, msg: &Value) {
        if let Some(msg_type) = msg.get("type").and_then(Value::as_str)
            && msg_type == "result"
        {
            self.is_turn_in_progress = false;
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
