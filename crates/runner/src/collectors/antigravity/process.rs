use std::{
    collections::VecDeque,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::super::{
    BoundedLine, ChildJob, CollectorError, MAX_PROTOCOL_LINE_BYTES, ProtocolRead, TurnOptions,
    configure_process_group, read_bounded_line,
};
use super::config::is_model_placeholder;

const MAX_STDERR_CAPTURE_BYTES: usize = 8192;

fn append_bounded_utf8(buf: &mut String, piece: &str, max_bytes: usize) {
    let remaining = max_bytes.saturating_sub(buf.len());
    if remaining == 0 {
        return;
    }
    if piece.len() <= remaining {
        buf.push_str(piece);
        return;
    }
    let mut end = remaining;
    while end > 0 && !piece.is_char_boundary(end) {
        end -= 1;
    }
    buf.push_str(&piece[..end]);
}

fn take_child_stdio(
    child: &mut Child,
    program: &str,
) -> Result<
    (
        std::process::ChildStdin,
        std::process::ChildStdout,
        std::process::ChildStderr,
    ),
    CollectorError,
> {
    let missing = || CollectorError::MissingStdio {
        program: program.to_string(),
    };
    let stdin = child.stdin.take().ok_or_else(missing)?;
    let stdout = child.stdout.take().ok_or_else(missing)?;
    let stderr = child.stderr.take().ok_or_else(missing)?;
    Ok((stdin, stdout, stderr))
}

/// Maps AgentHub `permission_mode` values to documented Antigravity CLI (`agy`) flags.
pub(crate) fn map_antigravity_permission_args(mode: &str) -> Vec<&'static str> {
    match mode.trim() {
        "bypassPermissions" | "danger-full-access" | "yolo" => {
            vec!["--dangerously-skip-permissions"]
        }
        "acceptEdits" | "accept-edits" => vec!["--mode", "accept-edits"],
        "plan" | "read-only" => vec!["--mode", "plan"],
        "auto" => vec!["--mode", "auto"],
        _ => Vec::new(),
    }
}

/// Builds headless bidirectional CLI arguments for `agy -p --input-format stream-json --output-format stream-json`.
pub(crate) fn build_antigravity_stream_args(
    resume_session_id: Option<&str>,
    effective_model: Option<&str>,
    permission_mode: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
    ];
    if let Some(id) = resume_session_id
        .map(str::trim)
        .filter(|s| super::super::validate_safe_session_id(s))
    {
        args.push("--conversation".to_string());
        args.push(id.to_string());
    }
    if let Some(model) = effective_model
        .map(str::trim)
        .filter(|m| !is_model_placeholder(m) && super::super::is_safe_model_id(m))
    {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    if let Some(mode) = permission_mode.and_then(super::super::sanitize_permission_mode) {
        let mapped = map_antigravity_permission_args(mode);
        for flag in mapped {
            args.push(flag.to_string());
        }
    }
    args
}

/// Persistent bidirectional Antigravity session driven via NDJSON `stdin`/`stdout`.
pub struct AntigravityProcessSession {
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
    pending_messages: VecDeque<Value>,
    is_turn_in_progress: bool,
    interrupted: bool,
    current_turn_id: Option<String>,
    eof_reached: bool,
}

impl AntigravityProcessSession {
    /// Starts a long-lived `agy` / `antigravity` process in `cwd`.
    pub fn start(
        binary: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        resume_session_id: Option<&str>,
        options: TurnOptions<'_>,
    ) -> Result<Self, CollectorError> {
        let binary_path = binary.as_ref();
        let program = binary_path.display().to_string();
        let cwd_path = cwd.as_ref().to_path_buf();

        let validated_resume_id = match resume_session_id.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => {
                if !super::super::validate_safe_session_id(id) {
                    return Err(CollectorError::Protocol {
                        program,
                        code: "invalid_session_id".to_owned(),
                        message: "resume session id contains disallowed characters".to_owned(),
                    });
                }
                Some(id)
            }
            None => None,
        };

        let mut command = super::super::command_for_binary(binary_path);
        command.current_dir(&cwd_path);

        let mut effective_model = options
            .model
            .map(str::trim)
            .filter(|s| !s.is_empty() && !is_model_placeholder(s))
            .map(str::to_string);
        if let Some(gateway) = crate::claude_gateway::discover_antigravity_gateway() {
            gateway.inject_into_command_with_session(
                &mut command,
                &mut effective_model,
                validated_resume_id,
            );
        }

        let base_args = build_antigravity_stream_args(
            validated_resume_id,
            effective_model.as_deref(),
            options.permission_mode,
        );
        command.args(&base_args);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_process_group(&mut command);

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

        let (stdin, stdout, stderr) = take_child_stdio(&mut child, &program).inspect_err(|_| {
            drop(job.take());
            let _ = child.kill();
            let _ = child.wait();
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
                    append_bounded_utf8(&mut guard, &piece, MAX_STDERR_CAPTURE_BYTES);
                }
            }
        });

        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_bounded_line(&mut reader) {
                    BoundedLine::Line(line) => match serde_json::from_str::<Value>(&line) {
                        Ok(msg) => {
                            if sender.send(ProtocolRead::Message(msg)).is_err() {
                                break;
                            }
                        }
                        Err(source) => {
                            let _ = sender.send(ProtocolRead::InvalidJson(source));
                            break;
                        }
                    },
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
            job,
            session_id: validated_resume_id.unwrap_or_default().to_string(),
            cwd: cwd_path,
            model: effective_model,
            requested_model: options.model.map(str::to_string),
            reasoning_effort: options.reasoning_effort.map(str::to_string),
            permission_mode: options.permission_mode.map(str::to_string),
            pending_messages: VecDeque::new(),
            is_turn_in_progress: false,
            interrupted: false,
            current_turn_id: None,
            eof_reached: false,
        };

        // Briefly probe startup so unsupported `--input-format stream-json` or
        // bad CLI flags surface quickly without stalling fallback.
        let deadline = Instant::now() + Duration::from_millis(60);
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match session.receiver.recv_timeout(remaining) {
                Ok(ProtocolRead::Message(msg)) => {
                    session.track_state(&msg);
                    session.pending_messages.push_back(msg);
                    if !session.session_id.is_empty() {
                        break;
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
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CollectorError::ReaderStopped { program });
                }
            }
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

    pub fn is_dead(&self) -> bool {
        self.eof_reached
    }

    pub fn is_turn_in_progress(&self) -> bool {
        self.is_turn_in_progress
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted
    }

    pub fn current_turn_id(&self) -> Option<&str> {
        self.current_turn_id.as_deref()
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
        crate::privacy::redact_paths_inside_text(&raw)
            .chars()
            .take(512)
            .collect()
    }

    pub fn send_prompt(&mut self, turn_id: &str, prompt: &str) -> Result<(), CollectorError> {
        self.send_prompt_with_blocks(turn_id, prompt, &[])
    }

    pub fn send_prompt_with_blocks(
        &mut self,
        turn_id: &str,
        prompt: &str,
        extra_blocks: &[Value],
    ) -> Result<(), CollectorError> {
        let mut msg = json!({
            "event": "user",
            "message": {
                "content": prompt
            }
        });
        if !extra_blocks.is_empty() {
            msg["message"]["blocks"] = Value::Array(extra_blocks.to_vec());
        }
        self.send_json(&msg)?;
        self.interrupted = false;
        self.is_turn_in_progress = true;
        self.current_turn_id = Some(turn_id.to_string());
        Ok(())
    }

    pub fn send_control_response(
        &mut self,
        request_id: &str,
        decision: &str,
        payload: Value,
    ) -> Result<(), CollectorError> {
        let msg = json!({
            "event": "control_response",
            "request_id": request_id,
            "decision": decision,
            "response": payload
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

    pub fn drain_messages(&mut self) -> Result<Vec<Value>, CollectorError> {
        let mut messages = Vec::new();
        while let Some(msg) = self.pending_messages.pop_front() {
            self.track_state(&msg);
            messages.push(msg);
        }

        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(msg)) => {
                    self.track_state(&msg);
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
                        if self.interrupted {
                            messages.push(json!({
                                "event": "result",
                                "interrupted": true,
                                "status": "INTERRUPTED"
                            }));
                        } else {
                            let stderr_detail = self.last_stderr();
                            let detail = match self.child.try_wait() {
                                Ok(Some(status)) if !stderr_detail.is_empty() => {
                                    format!("Antigravity 进程异常退出 ({status}): {stderr_detail}")
                                }
                                Ok(Some(status)) => {
                                    format!("Antigravity 进程异常退出 ({status})")
                                }
                                _ if !stderr_detail.is_empty() => {
                                    format!("Antigravity 会话连接中断: {stderr_detail}")
                                }
                                _ => "Antigravity 会话连接中断，未返回执行结果".to_string(),
                            };
                            messages.push(json!({
                                "event": "result",
                                "is_error": true,
                                "status": "ERROR",
                                "response": detail
                            }));
                        }
                    }
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        Ok(messages)
    }

    fn track_state(&mut self, msg: &Value) {
        if let Some(sid) = super::stream::extract_antigravity_session_id(msg) {
            self.session_id = sid;
        }
        let kind = msg
            .get("event")
            .or_else(|| msg.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if kind == "result" {
            self.is_turn_in_progress = false;
        }
    }

    pub fn interrupt(&mut self) -> Result<(), CollectorError> {
        self.interrupted = true;
        drop(self.job.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for AntigravityProcessSession {
    fn drop(&mut self) {
        drop(self.job.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
