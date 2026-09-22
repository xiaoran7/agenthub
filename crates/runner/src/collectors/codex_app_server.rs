use std::{
    collections::VecDeque,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    resolve_codex_app_server_binary,
    BoundedLine, ChildJob, CollectorError, DEFAULT_COLLECTOR_TIMEOUT,
    MAX_PROTOCOL_LINE_BYTES, ProtocolRead, configure_process_group,
    read_bounded_line, sanitize_protocol_message,
};

pub struct CodexAppServer {
    program: String,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    receiver: Receiver<ProtocolRead>,
    reader: Option<JoinHandle<()>>,
    pending_messages: VecDeque<Value>,
    next_request_id: u64,
    response_timeout: Duration,
    job: Option<ChildJob>,
}

impl CodexAppServer {
    pub fn start(binary: impl AsRef<Path>) -> Result<Self, CollectorError> {
        Self::start_with_timeout(binary, DEFAULT_COLLECTOR_TIMEOUT)
    }

    pub fn start_with_timeout(
        binary: impl AsRef<Path>,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        Self::start_mode(binary, response_timeout, false)
    }

    /// A long-lived control connection must receive content and tool events.
    /// Metadata collectors retain their original opt-out list.
    pub fn start_interactive(
        binary: impl AsRef<Path>,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        Self::start_mode(binary, response_timeout, true)
    }

    fn start_mode(
        binary: impl AsRef<Path>,
        response_timeout: Duration,
        interactive: bool,
    ) -> Result<Self, CollectorError> {
        let configured_binary = binary.as_ref();
        let binary = resolve_codex_app_server_binary(configured_binary);
        let program = binary.display().to_string();
        let mut command = Command::new(&binary);
        command
            .args(["app-server", "--listen", "stdio://"])
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
        let reader = thread::spawn(move || read_protocol(stdout, sender));
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
        };
        server.initialize(interactive)?;
        Ok(server)
    }

    fn initialize(&mut self, interactive: bool) -> Result<(), CollectorError> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "agent_platform",
                    "title": "AgentPulse Runner",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": false,
                    "optOutNotificationMethods": if interactive { json!([]) } else { json!([
                        "thread/started",
                        "thread/name/updated",
                        "thread/tokenUsage/updated",
                        "turn/diff/updated",
                        "turn/plan/updated",
                        "item/started",
                        "item/completed",
                        "item/agentMessage/delta",
                        "item/plan/delta",
                        "item/reasoning/textDelta",
                        "item/reasoning/summaryTextDelta",
                        "item/reasoning/summaryPartAdded",
                        "item/commandExecution/outputDelta",
                        "item/fileChange/outputDelta",
                        "warning",
                        "configWarning",
                        "error"
                    ]) }
                }
            }),
        )?;
        self.send(&json!({"method": "initialized", "params": {}}))?;
        Ok(())
    }

    pub fn list_threads(&mut self, limit: u32) -> Result<Value, CollectorError> {
        self.request(
            "thread/list",
            json!({
                "cursor": null,
                "limit": limit,
                "sortKey": "recency_at",
                "sortDirection": "desc",
                "archived": false,
                "useStateDbOnly": false,
                "sourceKinds": [
                    "cli",
                    "vscode",
                    "exec",
                    "appServer",
                    "subAgent",
                    "subAgentReview",
                    "subAgentCompact",
                    "subAgentThreadSpawn",
                    "subAgentOther",
                    "unknown"
                ]
            }),
        )
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, CollectorError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.send(&json!({
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
            // Server-to-client requests and notifications carry a `method` and
            // use an id namespace independent from ours; a colliding numeric id
            // must not be mistaken for the response to this request.
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

    /// Wait for a server notification while preserving unrelated messages for
    /// the normal collector loop. Auth login uses this to keep the app-server
    /// alive until the browser/device-code flow reports completion.
    pub fn wait_for_notification(
        &mut self,
        method: &str,
        timeout: Duration,
    ) -> Result<Option<Value>, CollectorError> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CollectorError::Timeout {
                    program: self.program.clone(),
                    timeout_ms: timeout.as_millis(),
                });
            }
            let message = self.read_message(0, remaining)?;
            if message.get("method").and_then(Value::as_str) == Some(method) {
                return Ok(message.get("params").cloned());
            }
            self.pending_messages.push_back(message);
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), CollectorError> {
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

    /// Answer a server request in its own ID namespace; never allocate a new ID.
    pub fn respond(&mut self, request_id: Value, result: Value) -> Result<(), CollectorError> {
        self.send(&json!({"id": request_id, "result": result}))
    }

    pub fn drain_messages(&mut self) -> Result<Vec<Value>, CollectorError> {
        let mut messages: Vec<Value> = self.pending_messages.drain(..).collect();
        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(message)) => messages.push(message),
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
                Ok(ProtocolRead::Eof) => {
                    return Err(CollectorError::UnexpectedEof {
                        program: self.program.clone(),
                        request_id: self.next_request_id,
                    });
                }
                Err(TryRecvError::Empty) => return Ok(messages),
                Err(TryRecvError::Disconnected) => {
                    return Err(CollectorError::ReaderStopped {
                        program: self.program.clone(),
                    });
                }
            }
        }
    }

    fn read_message(
        &mut self,
        request_id: u64,
        timeout: Duration,
    ) -> Result<Value, CollectorError> {
        match self.receiver.recv_timeout(timeout) {
            Ok(ProtocolRead::Message(message)) => Ok(message),
            Ok(ProtocolRead::InvalidJson(source)) => Err(CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }),
            Ok(ProtocolRead::LineTooLarge) => Err(CollectorError::ProtocolLineTooLarge {
                program: self.program.clone(),
                limit: MAX_PROTOCOL_LINE_BYTES,
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

impl Drop for CodexAppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.job.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(crate) fn read_protocol(stdout: impl Read, sender: mpsc::Sender<ProtocolRead>) {
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
            BoundedLine::Line(line) => match serde_json::from_str(&line) {
                Ok(message) => {
                    if sender.send(ProtocolRead::Message(message)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(ProtocolRead::InvalidJson(error));
                    return;
                }
            },
            BoundedLine::Io(error) => {
                let _ = sender.send(ProtocolRead::Io(error));
                return;
            }
        }
    }
}



