use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use thiserror::Error;

const MAX_PROTOCOL_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(15);
const CHILD_WAIT_INTERVAL: Duration = Duration::from_millis(10);

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("failed to start {program}: {source}")]
    Start {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} exited unsuccessfully with status {status}")]
    Exit { program: String, status: String },
    #[error("failed to communicate with {program}: {source}")]
    Io {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} did not expose the expected stdio handles")]
    MissingStdio { program: String },
    #[error("invalid JSON from {program}: {source}")]
    InvalidJson {
        program: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{program} returned output that was not UTF-8: {source}")]
    InvalidUtf8 {
        program: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("{program} protocol line exceeded {limit} bytes")]
    ProtocolLineTooLarge { program: String, limit: usize },
    #[error("{program} output exceeded {limit} bytes")]
    OutputTooLarge { program: String, limit: usize },
    #[error("{program} did not respond within {timeout_ms} ms")]
    Timeout { program: String, timeout_ms: u128 },
    #[error("{program} output reader stopped unexpectedly")]
    ReaderStopped { program: String },
    #[error("{program} closed stdout before responding to request {request_id}")]
    UnexpectedEof { program: String, request_id: u64 },
    #[error("{program} returned protocol error {code}: {message}")]
    Protocol {
        program: String,
        code: String,
        message: String,
    },
}

impl CollectorError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Start { source, .. } => match source.kind() {
                std::io::ErrorKind::NotFound => "executable_not_found",
                std::io::ErrorKind::PermissionDenied => "permission_denied",
                _ => "process_start_failed",
            },
            Self::Exit { .. } => "process_exit_failed",
            Self::Io { .. } => "protocol_io_failed",
            Self::MissingStdio { .. } => "protocol_stdio_missing",
            Self::InvalidJson { .. } => "protocol_invalid_json",
            Self::InvalidUtf8 { .. } => "process_invalid_utf8",
            Self::ProtocolLineTooLarge { .. } => "protocol_line_too_large",
            Self::OutputTooLarge { .. } => "process_output_too_large",
            Self::Timeout { .. } => "collector_timeout",
            Self::ReaderStopped { .. } => "collector_reader_stopped",
            Self::UnexpectedEof { .. } => "protocol_unexpected_eof",
            Self::Protocol { .. } => "protocol_error",
        }
    }
}

pub fn collect_claude_agents(
    binary: impl AsRef<Path>,
    include_completed: bool,
) -> Result<String, CollectorError> {
    collect_claude_agents_with_timeout(binary, include_completed, DEFAULT_COLLECTOR_TIMEOUT)
}

pub fn collect_claude_agents_with_timeout(
    binary: impl AsRef<Path>,
    include_completed: bool,
    timeout: Duration,
) -> Result<String, CollectorError> {
    let binary = binary.as_ref();
    let program = binary.display().to_string();
    let mut command = Command::new(binary);
    command.args(["agents", "--json"]);
    if include_completed {
        command.arg("--all");
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
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
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CollectorError::MissingStdio {
            program: program.clone(),
        })?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take((MAX_PROTOCOL_LINE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let status = match wait_for_child(&mut child, &program, timeout) {
        Ok(status) => status,
        Err(error) => {
            drop(job);
            let _ = reader.join();
            return Err(error);
        }
    };
    // The direct child has exited, but descendants it spawned can still hold
    // the stdout write end. Close the job before joining the reader so the
    // remaining process tree is terminated and the reader always sees EOF.
    drop(job);
    let output = reader
        .join()
        .map_err(|_| CollectorError::ReaderStopped {
            program: program.clone(),
        })?
        .map_err(|source| CollectorError::Io {
            program: program.clone(),
            source,
        })?;
    if output.len() > MAX_PROTOCOL_LINE_BYTES {
        return Err(CollectorError::OutputTooLarge {
            program,
            limit: MAX_PROTOCOL_LINE_BYTES,
        });
    }
    if !status.success() {
        return Err(CollectorError::Exit {
            program,
            status: status.to_string(),
        });
    }
    String::from_utf8(output).map_err(|source| CollectorError::InvalidUtf8 { program, source })
}

pub fn run_agent_task(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: &Path,
    prompt: &str,
    timeout: Duration,
) -> Result<(), CollectorError> {
    let binary = binary.as_ref();
    let program = binary.display().to_string();
    let mut command = Command::new(binary);
    match agent {
        crate::domain::AgentType::ClaudeCode => {
            command.args(["-p", "--output-format", "text"]);
        }
        crate::domain::AgentType::Codex => {
            command.args(["exec", "-"]);
        }
    }
    command
        .current_dir(project_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
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
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| CollectorError::MissingStdio {
            program: program.clone(),
        })?;
    stdin
        .write_all(prompt.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .map_err(|source| CollectorError::Io {
            program: program.clone(),
            source,
        })?;
    drop(stdin);
    let status = wait_for_child(&mut child, &program, timeout)?;
    drop(job);
    if !status.success() {
        return Err(CollectorError::Exit {
            program,
            status: status.to_string(),
        });
    }
    Ok(())
}

fn wait_for_child(
    child: &mut Child,
    program: &str,
    timeout: Duration,
) -> Result<std::process::ExitStatus, CollectorError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CollectorError::Timeout {
                program: program.to_owned(),
                timeout_ms: timeout.as_millis(),
            });
        }
        thread::sleep(CHILD_WAIT_INTERVAL);
    }
}

enum ProtocolRead {
    Message(Value),
    InvalidJson(serde_json::Error),
    LineTooLarge,
    Io(std::io::Error),
    Eof,
}

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
        let binary = binary.as_ref();
        let program = binary.display().to_string();
        let mut command = Command::new(binary);
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
        server.initialize()?;
        Ok(server)
    }

    fn initialize(&mut self) -> Result<(), CollectorError> {
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
                    "optOutNotificationMethods": [
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
                    ]
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

fn read_protocol(stdout: impl Read, sender: mpsc::Sender<ProtocolRead>) {
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

enum BoundedLine {
    Line(String),
    TooLarge,
    Eof,
    Io(std::io::Error),
}

/// Read one newline-terminated line while enforcing the size limit during the
/// read. `read_line` would buffer an arbitrarily long line before any length
/// check could run, letting a misbehaving child exhaust memory.
fn read_bounded_line(reader: &mut impl BufRead) -> BoundedLine {
    let mut line = Vec::new();
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return BoundedLine::Io(error),
        };
        if buffer.is_empty() {
            if line.is_empty() {
                return BoundedLine::Eof;
            }
            return finish_line(line);
        }
        let (take, found_newline) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (buffer.len(), false),
        };
        if line.len() + take > MAX_PROTOCOL_LINE_BYTES {
            return BoundedLine::TooLarge;
        }
        line.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if found_newline {
            return finish_line(line);
        }
    }
}

fn finish_line(line: Vec<u8>) -> BoundedLine {
    match String::from_utf8(line) {
        Ok(line) => BoundedLine::Line(line),
        Err(error) => BoundedLine::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.utf8_error(),
        )),
    }
}

#[cfg(windows)]
struct ChildJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl ChildJob {
    fn attach(child: &Child) -> std::io::Result<Self> {
        use std::{
            ffi::c_void,
            mem::{size_of, zeroed},
            os::windows::io::AsRawHandle,
            ptr,
        };
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
        };

        // SAFETY: All pointers refer to initialized local data for the duration of each
        // call. Every successful job handle is owned by ChildJob and closed once.
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const information).cast::<c_void>(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .expect("job information size fits u32"),
            ) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            if AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            Ok(Self(handle))
        }
    }
}

#[cfg(windows)]
impl Drop for ChildJob {
    fn drop(&mut self) {
        // SAFETY: The handle is created and exclusively owned by this ChildJob.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Place the child in its own process group on Unix so the whole tree can be
/// signalled at teardown; the Windows equivalent is the Job Object.
fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

#[cfg(not(windows))]
struct ChildJob {
    process_group: i32,
}

#[cfg(not(windows))]
impl ChildJob {
    fn attach(child: &Child) -> std::io::Result<Self> {
        // `configure_process_group` made the child a process-group leader, so
        // its PID doubles as the process-group ID.
        Ok(Self {
            process_group: i32::try_from(child.id())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?,
        })
    }
}

#[cfg(not(windows))]
impl Drop for ChildJob {
    fn drop(&mut self) {
        // SAFETY: kill(2) with a negative PID signals the process group the
        // runner created for this collector child; it never signals our own
        // group because the child was spawned as a group leader.
        unsafe {
            libc::kill(-self.process_group, libc::SIGKILL);
        }
    }
}

fn sanitize_protocol_message(message: &str) -> String {
    let sanitized: String = message
        .chars()
        .take(160)
        .filter(|ch| !ch.is_control())
        .collect();
    if sanitized.is_empty() {
        "protocol error".into()
    } else {
        sanitized
    }
}

#[derive(Debug, Clone)]
pub struct CollectorBinaries {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

impl Default for CollectorBinaries {
    fn default() -> Self {
        Self {
            claude: PathBuf::from("claude"),
            codex: PathBuf::from("codex"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use super::*;

    fn sleeping_script() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();

        #[cfg(windows)]
        let script = {
            let path = directory.path().join("claude.cmd");
            fs::write(
                &path,
                "@echo off\r\npowershell -NoProfile -Command \"Start-Sleep -Seconds 5\"\r\n",
            )
            .unwrap();
            path
        };

        #[cfg(unix)]
        let script = {
            use std::os::unix::fs::PermissionsExt;

            let path = directory.path().join("claude");
            fs::write(&path, "#!/bin/sh\nsleep 5\n").unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        };
        (directory, script)
    }

    fn recording_script(
        directory: &tempfile::TempDir,
        arguments_path: &Path,
        input_path: &Path,
    ) -> PathBuf {
        #[cfg(windows)]
        {
            let path = directory.path().join("agent.cmd");
            fs::write(
                &path,
                format!(
                    "@echo off\r\necho %* > \"{}\"\r\nmore > \"{}\"\r\n",
                    arguments_path.display(),
                    input_path.display(),
                ),
            )
            .unwrap();
            path
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let path = directory.path().join("agent");
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s' \"$*\" > '{}'\ncat > '{}'\n",
                    arguments_path.display(),
                    input_path.display(),
                ),
            )
            .unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }
    }

    #[test]
    fn sanitizes_protocol_error_messages() {
        let input = "bad\0message\nwith control";
        assert_eq!(sanitize_protocol_message(input), "badmessagewith control");
    }

    #[test]
    fn protocol_reader_delivers_messages_without_blocking_the_request_thread() {
        let input = Cursor::new(
            br#"{"method":"thread/status/changed","params":{"threadId":"thr-1"}}
{"id":7,"result":{"data":[]}}
"#,
        );
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || read_protocol(input, sender));

        let first = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        let second = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        let eof = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        reader.join().unwrap();

        assert!(matches!(
            first,
            ProtocolRead::Message(value)
                if value.get("method").and_then(Value::as_str)
                    == Some("thread/status/changed")
        ));
        assert!(matches!(
            second,
            ProtocolRead::Message(value)
                if value.get("id").and_then(Value::as_u64) == Some(7)
        ));
        assert!(matches!(eof, ProtocolRead::Eof));
    }

    #[test]
    fn bounded_line_reader_stops_oversized_lines_without_buffering_them() {
        struct EndlessBytes;

        impl std::io::Read for EndlessBytes {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                buffer.fill(b'a');
                Ok(buffer.len())
            }
        }

        let mut reader = BufReader::new(EndlessBytes);
        assert!(matches!(
            read_bounded_line(&mut reader),
            BoundedLine::TooLarge
        ));
    }

    #[test]
    fn bounded_line_reader_returns_lines_and_eof() {
        let mut reader = BufReader::new(Cursor::new(b"{\"id\":1}\ntrailing".to_vec()));
        assert!(matches!(
            read_bounded_line(&mut reader),
            BoundedLine::Line(line) if line == "{\"id\":1}\n"
        ));
        assert!(matches!(
            read_bounded_line(&mut reader),
            BoundedLine::Line(line) if line == "trailing"
        ));
        assert!(matches!(read_bounded_line(&mut reader), BoundedLine::Eof));
    }

    #[test]
    fn task_prompt_uses_stdin_instead_of_process_arguments() {
        let directory = tempfile::tempdir().unwrap();
        let arguments_path = directory.path().join("arguments.txt");
        let input_path = directory.path().join("input.txt");
        let script = recording_script(&directory, &arguments_path, &input_path);
        let prompt = "private prompt marker";

        run_agent_task(
            crate::domain::AgentType::Codex,
            script,
            directory.path(),
            prompt,
            Duration::from_secs(2),
        )
        .unwrap();

        let arguments = fs::read_to_string(arguments_path).unwrap();
        let input = fs::read_to_string(input_path).unwrap();
        assert!(arguments.contains("exec"));
        assert!(!arguments.contains(prompt));
        assert_eq!(input.trim_end(), prompt);
    }

    #[test]
    fn claude_collector_kills_a_hung_process_at_the_deadline() {
        let (_directory, script) = sleeping_script();

        let started = Instant::now();
        let error = collect_claude_agents_with_timeout(script, false, Duration::from_millis(100))
            .unwrap_err();

        assert_eq!(error.code(), "collector_timeout");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn codex_handshake_kills_a_silent_process_at_the_deadline() {
        let (_directory, script) = sleeping_script();
        let started = Instant::now();
        let result = CodexAppServer::start_with_timeout(script, Duration::from_millis(100));
        let error = match result {
            Ok(_) => panic!("silent fake app-server unexpectedly initialized"),
            Err(error) => error,
        };

        assert_eq!(error.code(), "collector_timeout");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
