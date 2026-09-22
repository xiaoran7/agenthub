use std::{
    collections::VecDeque,
    env, fs,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    home_dir, MAX_PROTOCOL_LINE_BYTES,
    BoundedLine, ChildJob, CollectorError, ProtocolRead,
    configure_process_group, read_bounded_line, sanitize_protocol_message,
};

pub struct ZCodeAppServer {
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

pub fn resolve_zcode_app_server_script() -> Option<PathBuf> {
    if let Some(path) = env::var_os("ZCODE_APP_SERVER_PATH").filter(|v| !v.is_empty()) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return dunce::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }
    let candidates = [
        PathBuf::from(r"D:\zcode\resources\glm\zcode.cjs"),
        PathBuf::from(r"C:\Program Files\zcode\resources\glm\zcode.cjs"),
        PathBuf::from(r"C:\Program Files (x86)\zcode\resources\glm\zcode.cjs"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return dunce::canonicalize(&candidate).ok().or(Some(candidate));
        }
    }
    if let Some(home) = home_dir() {
        let local_app = home.join(r"AppData\Local\Programs\zcode\resources\glm\zcode.cjs");
        if local_app.is_file() {
            return dunce::canonicalize(&local_app).ok().or(Some(local_app));
        }
    }
    None
}

pub fn resolve_node_executable() -> PathBuf {
    if let Some(path) = env::var_os("NODE_BINARY").filter(|v| !v.is_empty()) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return dunce::canonicalize(&candidate).unwrap_or(candidate);
        }
    }
    let candidates = [
        PathBuf::from(r"D:\nodejs\node.exe"),
        PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
    ];
    for candidate in candidates {
        if candidate.is_file() {
            return dunce::canonicalize(&candidate).unwrap_or(candidate);
        }
    }
    PathBuf::from("node")
}

pub fn load_zcode_v2_provider_registry() -> Option<Vec<Value>> {
    let home = home_dir()?;
    let config_path = home.join(r".zcode\v2\config.json");
    if !config_path.is_file() {
        return None;
    }
    let data = fs::read_to_string(&config_path).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    let providers_obj = json.get("provider")?.as_object()?;
    let mut providers = Vec::new();
    for (pid, pdata) in providers_obj {
        if pdata.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let options = pdata.get("options").unwrap_or(&Value::Null);
        let base_url = options.get("baseURL").and_then(Value::as_str)?;
        let mut models = Vec::new();
        if let Some(models_obj) = pdata.get("models").and_then(Value::as_object) {
            for (mid, mdata) in models_obj {
                let context = mdata
                    .pointer("/limit/context")
                    .and_then(Value::as_u64)
                    .unwrap_or(128_000);
                let supports_images = mdata
                    .pointer("/modalities/input")
                    .and_then(Value::as_array)
                    .map(|arr| arr.iter().any(|v| v.as_str() == Some("image")))
                    .unwrap_or(false);
                models.push(json!({
                    "modelId": mid,
                    "label": mid,
                    "contextWindow": context,
                    "supportsImages": supports_images
                }));
            }
        }
        if models.is_empty() {
            continue;
        }
        let raw_kind = pdata
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("openai-compatible");
        let kind = match raw_kind {
            "anthropic" | "openai" | "openai-compatible" => raw_kind,
            _ => "openai-compatible",
        };
        let mut entry = json!({
            "providerId": pid,
            "kind": kind,
            "label": pdata.get("name").and_then(Value::as_str).unwrap_or(pid),
            "baseURL": base_url,
            "models": models
        });
        if let Some(api_key) = options.get("apiKey").and_then(Value::as_str)
            && !api_key.trim().is_empty()
        {
            entry["apiKey"] = json!({
                "source": "inline",
                "value": api_key
            });
        }
        providers.push(entry);
    }
    if providers.is_empty() {
        None
    } else {
        Some(providers)
    }
}

impl ZCodeAppServer {
    pub fn start_if_available(response_timeout: Duration) -> Result<Self, CollectorError> {
        let script = resolve_zcode_app_server_script().ok_or_else(|| CollectorError::Start {
            program: "zcode".to_owned(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "zcode_script_not_found"),
        })?;
        Self::start_with_script(script, response_timeout)
    }

    pub fn start_with_script(
        script: impl AsRef<Path>,
        response_timeout: Duration,
    ) -> Result<Self, CollectorError> {
        let script_path = script.as_ref();
        let node_bin = resolve_node_executable();
        let program = format!("node {}", script_path.display());
        let mut command = Command::new(&node_bin);
        command
            .arg(script_path)
            .arg("app-server")
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
        let reader = thread::spawn(move || read_zcode_protocol(stdout, sender));
        let server = Self {
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
        Ok(server)
    }

    fn handle_internal_query(&mut self, message: &Value) -> Result<bool, CollectorError> {
        if let (Some(req_id), Some(method)) = (
            message.get("id"),
            message.get("method").and_then(Value::as_str),
        ) {
            if method == "session/requestRuntimePreferences" {
                self.respond(
                    req_id.clone(),
                    json!({
                        "nativeSearchEnhancementsEnabled": false,
                        "memoryEnabled": false,
                        "askUserQuestionAutoResolutionEnabled": true,
                        "modelContextBudgetStrategy": "legacy"
                    }),
                )?;
                return Ok(true);
            } else if method == "interaction/requestOfficialMcpAuthHeaders" {
                self.respond(
                    req_id.clone(),
                    json!({"ok": false, "reason": "official_auth_unavailable"}),
                )?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, CollectorError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let request_id_str = request_id.to_string();
        self.send(&json!({
            "method": method,
            "id": request_id_str,
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
            if self.handle_internal_query(&message)? {
                continue;
            }
            let is_match = message
                .get("id")
                .and_then(Value::as_str)
                .map(|id| id == request_id_str)
                .unwrap_or(false)
                || message.get("id").and_then(Value::as_u64) == Some(request_id);
            if message.get("method").is_some() || !is_match {
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

    pub fn respond(&mut self, request_id: Value, result: Value) -> Result<(), CollectorError> {
        self.send(&json!({"id": request_id, "result": result}))
    }

    pub fn drain_messages(&mut self) -> Result<Vec<Value>, CollectorError> {
        let mut messages: Vec<Value> = Vec::new();
        while let Some(msg) = self.pending_messages.pop_front() {
            if !self.handle_internal_query(&msg)? {
                messages.push(msg);
            }
        }
        loop {
            match self.receiver.try_recv() {
                Ok(ProtocolRead::Message(message)) => {
                    if !self.handle_internal_query(&message)? {
                        messages.push(message);
                    }
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

    pub fn session_create(&mut self, workspace_path: &str) -> Result<Value, CollectorError> {
        let clean_path = workspace_path.trim_start_matches(r"\\?\");
        let res = self.request(
            "session/create",
            json!({
                "workspace": {
                    "workspacePath": clean_path,
                    "workspaceKey": clean_path
                }
            }),
        )?;
        let session_id = res
            .pointer("/session/sessionId")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !session_id.is_empty() {
            let _ = self.request(
                "session/subscribe",
                json!({"sessionId": session_id, "deliveryKind": "desktop-continuous"}),
            );
            let _ = self.sync_provider_registry_with_key(clean_path, clean_path);
        }
        Ok(res)
    }

    pub fn session_resume(&mut self, session_id: &str) -> Result<Value, CollectorError> {
        let resume_res = self.request("session/resume", json!({"sessionId": session_id}))?;
        let _ = self.request(
            "session/subscribe",
            json!({"sessionId": session_id, "deliveryKind": "desktop-continuous"}),
        );
        let ws_key = resume_res
            .pointer("/session/workspace/workspaceKey")
            .and_then(Value::as_str);
        let ws_path = resume_res
            .pointer("/session/workspace/workspacePath")
            .and_then(Value::as_str);
        if let Some(path) = ws_path.or(ws_key) {
            let key = ws_key.unwrap_or(path);
            let _ = self.sync_provider_registry_with_key(key, path);
        }
        Ok(resume_res)
    }

    pub fn sync_provider_registry(&mut self, workspace_path: &str) -> Result<(), CollectorError> {
        self.sync_provider_registry_with_key(workspace_path, workspace_path)
    }

    pub fn sync_provider_registry_with_key(
        &mut self,
        workspace_key: &str,
        workspace_path: &str,
    ) -> Result<(), CollectorError> {
        if let Some(providers) = load_zcode_v2_provider_registry() {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let clean_key = workspace_key.trim_start_matches(r"\\?\");
            let clean_path = workspace_path.trim_start_matches(r"\\?\");
            let _ = self.request(
                "workspace/updateProviderRegistry",
                json!({
                    "workspace": {
                        "workspaceKey": clean_key,
                        "workspacePath": clean_path
                    },
                    "registry": {
                        "revision": format!("rev-v2-{}", now_ms),
                        "generatedAt": now_ms,
                        "providers": providers
                    }
                }),
            )?;
        }
        Ok(())
    }

    pub fn session_send(
        &mut self,
        session_id: &str,
        content: &str,
    ) -> Result<Value, CollectorError> {
        self.request(
            "session/send",
            json!({"sessionId": session_id, "content": content}),
        )
    }

    pub fn session_stop(&mut self, session_id: &str) -> Result<Value, CollectorError> {
        self.request("session/stop", json!({"sessionId": session_id}))
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

impl Drop for ZCodeAppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.job.take();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_zcode_protocol(stdout: impl Read, sender: mpsc::Sender<ProtocolRead>) {
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

