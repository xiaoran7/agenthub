//! Native session control core. This is local IPC, not the legacy v2 wire format.
//! A Host owns one long-lived app-server and never resumes arbitrary live processes.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::collectors::CodexAppServer;

/// Protocol errors are safe codes; provider payloads belong to the content stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub code: String,
    pub outcome_unknown: bool,
}

impl NativeError {
    fn rejected(code: &str) -> Self {
        Self {
            code: code.into(),
            outcome_unknown: false,
        }
    }

    fn uncertain(code: &str) -> Self {
        Self {
            code: code.into(),
            outcome_unknown: true,
        }
    }
}

pub trait NativeTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError>;
    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError>;
    fn drain(&mut self) -> Result<Vec<Value>, NativeError>;
}

impl NativeTransport for CodexAppServer {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
        self.request(method, params).map_err(|error| match error {
            crate::collectors::CollectorError::Protocol { .. } => {
                NativeError::rejected("provider_rejected")
            }
            _ => NativeError::uncertain("provider_transport_lost"),
        })
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        self.respond(id, result)
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }

    fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
        self.drain_messages()
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectMode {
    Create,
    Resume,
    Attach,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexSandbox {
    #[default]
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexApprovalPolicy {
    #[default]
    Untrusted,
    OnRequest,
    Never,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteAction {
    Connect {
        mode: ConnectMode,
        workspace_path: PathBuf,
        #[serde(default)]
        provider_session_id: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        sandbox: CodexSandbox,
        #[serde(default)]
        approval_policy: CodexApprovalPolicy,
    },
    SendPrompt {
        text: String,
    },
    ResolveApproval {
        approval_id: String,
        approved: bool,
    },
    Interrupt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteCommand {
    pub client_cmd_id: String,
    pub hub_session_id: String,
    /// Required for all controls of an existing binding. Fences stale clients.
    pub host_generation: Option<String>,
    pub command: RemoteAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Unknown,
    Idle,
    Running,
    AwaitingPermission,
    AwaitingInput,
    Error,
    Completed,
    Terminated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionBinding {
    pub hub_session_id: String,
    pub provider_session_id: String,
    pub workspace_path: PathBuf,
    pub host_generation: String,
    pub control_connected: bool,
    pub execution_state: ExecutionState,
    pub current_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub client_cmd_id: String,
    pub hub_session_id: String,
    /// accepted means protocol acceptance, never execution completion.
    pub status: String,
    pub code: Option<String>,
    pub result: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEvent {
    pub event_id: String,
    pub host_generation: String,
    pub hub_session_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub kind: String,
    /// Unmodified provider message; this is content, never a diagnostic log.
    pub raw: Value,
    pub binding: Option<SessionBinding>,
    pub approval_id: Option<String>,
}

struct PendingApproval {
    hub_session_id: String,
    turn_id: String,
    request_id: Value,
    supported: bool,
    available_decisions: Option<Vec<Value>>,
}

pub struct NativeSessionHost<T: NativeTransport> {
    transport: T,
    generation: String,
    workspaces: Vec<PathBuf>,
    sessions: HashMap<String, SessionBinding>,
    native_to_hub: HashMap<String, String>,
    approvals: HashMap<String, PendingApproval>,
    answered_requests: HashSet<String>,
    receipts: HashMap<String, (RemoteCommand, CommandReceipt)>,
    alive: bool,
}

impl<T: NativeTransport> NativeSessionHost<T> {
    pub fn new(transport: T, workspaces: Vec<PathBuf>) -> Result<Self, NativeError> {
        let workspaces = workspaces
            .into_iter()
            .map(|path| {
                path.canonicalize()
                    .ok()
                    .filter(|path| path.is_dir())
                    .ok_or_else(|| NativeError::rejected("workspace_unavailable"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if workspaces.is_empty() {
            return Err(NativeError::rejected("workspace_required"));
        }
        Ok(Self {
            transport,
            generation: uuid::Uuid::new_v4().to_string(),
            workspaces,
            sessions: HashMap::new(),
            native_to_hub: HashMap::new(),
            approvals: HashMap::new(),
            answered_requests: HashSet::new(),
            receipts: HashMap::new(),
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

    pub(crate) fn restore_detached(&mut self, bindings: Vec<SessionBinding>) {
        for mut binding in bindings {
            binding.control_connected = false;
            binding.execution_state = ExecutionState::Unknown;
            self.native_to_hub.insert(
                binding.provider_session_id.clone(),
                binding.hub_session_id.clone(),
            );
            self.sessions
                .insert(binding.hub_session_id.clone(), binding);
        }
    }

    /// Actual provider model/list response, with no invented availability or fallback catalog.
    pub fn models(&mut self) -> Result<Value, NativeError> {
        self.transport.request("model/list", json!({"limit": 100}))
    }

    pub fn discover(&mut self) -> Result<Value, NativeError> {
        let response = self
            .transport
            .request("thread/list", json!({"limit":100}))?;
        let candidates = response["data"].as_array().into_iter().flatten().filter_map(|thread| {
            let id = thread["id"].as_str()?;
            let cwd = PathBuf::from(thread["cwd"].as_str()?).canonicalize().ok()?;
            if !self.workspaces.contains(&cwd) { return None; }
            Some(json!({"provider_session_id":id,"workspace_path":cwd,
                "title":thread["name"].as_str().map(|name|name.chars().take(160).collect::<String>()),
                "execution_state":execution_state(&thread["status"]),"control_connected":false}))
        }).collect::<Vec<_>>();
        Ok(json!({"candidates":candidates,"has_more":!response["nextCursor"].is_null()}))
    }

    pub fn handle_command(&mut self, command: RemoteCommand) -> CommandReceipt {
        let rejected = |code: &str| CommandReceipt {
            client_cmd_id: command.client_cmd_id.clone(),
            hub_session_id: command.hub_session_id.clone(),
            status: "rejected".into(),
            code: Some(code.into()),
            result: None,
        };
        if command.client_cmd_id.is_empty()
            || command.client_cmd_id.len() > 128
            || command.hub_session_id.is_empty()
            || command.hub_session_id.len() > 128
        {
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
        if self.receipts.len() >= 10_000 {
            return rejected("command_ledger_full");
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
        receipt
    }

    fn execute(&mut self, command: &RemoteCommand) -> Result<Value, NativeError> {
        let hub_id = &command.hub_session_id;
        if let RemoteAction::Connect {
            mode,
            workspace_path,
            provider_session_id,
            model,
            sandbox,
            approval_policy,
        } = &command.command
        {
            if *mode == ConnectMode::Attach {
                return Err(NativeError::rejected("attach_not_supported"));
            }
            if let Some(existing) = self.sessions.get(hub_id)
                && (existing.control_connected
                    || *mode != ConnectMode::Resume
                    || provider_session_id.as_ref() != Some(&existing.provider_session_id))
            {
                return Err(NativeError::rejected("session_already_bound"));
            }
            let cwd = workspace_path
                .canonicalize()
                .map_err(|_| NativeError::rejected("workspace_unavailable"))?;
            if !self.workspaces.contains(&cwd) {
                return Err(NativeError::rejected("workspace_not_authorized"));
            }
            if model.as_ref().is_some_and(|model| model.trim().is_empty()) {
                return Err(NativeError::rejected("invalid_model"));
            }
            let mut params = json!({"cwd": cwd, "sandbox": sandbox, "approvalPolicy": approval_policy,
                "approvalsReviewer": "user"});
            if let Some(model) = model {
                params["model"] = json!(model);
            }
            let method = match mode {
                ConnectMode::Create => {
                    if provider_session_id.is_some() {
                        return Err(NativeError::rejected("create_cannot_set_native_id"));
                    }
                    params["experimentalRawEvents"] = json!(false);
                    "thread/start"
                }
                ConnectMode::Resume => {
                    let id = provider_session_id
                        .as_ref()
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| NativeError::rejected("native_id_required"))?;
                    if self
                        .native_to_hub
                        .get(id)
                        .is_some_and(|bound| bound != hub_id)
                    {
                        return Err(NativeError::rejected("native_session_already_bound"));
                    }
                    // Read does not take control. Never resume a known active thread.
                    let history = self.transport.request(
                        "thread/read",
                        json!({"threadId": id, "includeTurns": false}),
                    )?;
                    let thread = &history["thread"];
                    if thread["id"].as_str() != Some(id) {
                        return Err(NativeError::rejected("native_id_mismatch"));
                    }
                    if !matches!(
                        thread["status"]["type"].as_str(),
                        Some("notLoaded" | "idle")
                    ) {
                        return Err(NativeError::rejected("external_session_not_resumable"));
                    }
                    let original_cwd = thread["cwd"]
                        .as_str()
                        .and_then(|path| PathBuf::from(path).canonicalize().ok());
                    if original_cwd.as_ref() != Some(&cwd) {
                        return Err(NativeError::rejected("resume_workspace_mismatch"));
                    }
                    params["threadId"] = json!(id);
                    "thread/resume"
                }
                ConnectMode::Attach => unreachable!(),
            };
            let response = self.transport.request(method, params)?;
            let id = response["thread"]["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| NativeError::uncertain("provider_missing_native_id"))?
                .to_string();
            if provider_session_id
                .as_ref()
                .is_some_and(|expected| expected != &id)
                || self
                    .native_to_hub
                    .get(&id)
                    .is_some_and(|bound| bound != hub_id)
            {
                return Err(NativeError::uncertain("provider_identity_conflict"));
            }
            let binding = SessionBinding {
                hub_session_id: hub_id.clone(),
                provider_session_id: id.clone(),
                workspace_path: cwd,
                host_generation: self.generation.clone(),
                control_connected: true,
                execution_state: execution_state(&response["thread"]["status"]),
                current_turn_id: None,
            };
            self.native_to_hub.insert(id, hub_id.clone());
            self.sessions.insert(hub_id.clone(), binding.clone());
            return Ok(json!({"binding": binding, "provider_response": response}));
        }

        if command.host_generation.as_deref() != Some(&self.generation) {
            return Err(NativeError::rejected("host_generation_mismatch"));
        }
        let session = self
            .sessions
            .get(hub_id)
            .cloned()
            .ok_or_else(|| NativeError::rejected("session_not_bound"))?;
        if !session.control_connected {
            return Err(NativeError::rejected("session_control_lost"));
        }
        match &command.command {
            RemoteAction::SendPrompt { text } => {
                if text.trim().is_empty() || text.len() > 512 * 1024 {
                    return Err(NativeError::rejected("invalid_prompt"));
                }
                if text.trim_start().starts_with('/') {
                    return Err(NativeError::rejected("slash_not_supported"));
                }
                if session.current_turn_id.is_some()
                    || !matches!(
                        session.execution_state,
                        ExecutionState::Idle | ExecutionState::Completed
                    )
                {
                    return Err(NativeError::rejected("session_busy_or_unknown"));
                }
                let result = self.transport.request(
                    "turn/start",
                    json!({"threadId": session.provider_session_id,
                    "input": [{"type": "text", "text": text, "text_elements": []}]}),
                )?;
                let turn_id = result["turn"]["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| NativeError::uncertain("provider_missing_turn_id"))?;
                let binding = self.sessions.get_mut(hub_id).expect("binding checked");
                binding.current_turn_id = Some(turn_id.to_string());
                binding.execution_state = turn_state(&result["turn"]["status"]);
                Ok(result)
            }
            RemoteAction::ResolveApproval {
                approval_id,
                approved,
            } => {
                let approval = self
                    .approvals
                    .get(approval_id)
                    .ok_or_else(|| NativeError::rejected("approval_not_pending"))?;
                if approval.hub_session_id != *hub_id
                    || session.current_turn_id.as_ref() != Some(&approval.turn_id)
                {
                    return Err(NativeError::rejected("approval_session_mismatch"));
                }
                if !approval.supported {
                    return Err(NativeError::rejected("approval_type_not_supported"));
                }
                let decision = if *approved { "accept" } else { "decline" };
                if approval
                    .available_decisions
                    .as_ref()
                    .is_some_and(|allowed| !allowed.contains(&json!(decision)))
                {
                    return Err(NativeError::rejected("approval_decision_not_available"));
                }
                self.transport
                    .respond(approval.request_id.clone(), json!({"decision": decision}))?;
                self.approvals.remove(approval_id);
                self.answered_requests.insert(approval_id.clone());
                // Native notifications, not the act of sending a response, update execution state.
                Ok(json!({"approval_id": approval_id, "response_sent": true}))
            }
            RemoteAction::Interrupt => {
                let turn_id = session
                    .current_turn_id
                    .ok_or_else(|| NativeError::rejected("no_active_turn"))?;
                self.transport.request(
                    "turn/interrupt",
                    json!({"threadId": session.provider_session_id, "turnId": turn_id}),
                )
            }
            RemoteAction::Connect { .. } => unreachable!(),
        }
    }

    pub fn poll(&mut self) -> Result<Vec<HostEvent>, NativeError> {
        if !self.alive {
            return Ok(Vec::new());
        }
        let messages = match self.transport.drain() {
            Ok(messages) => messages,
            Err(error) => {
                self.mark_lost();
                return Err(error);
            }
        };
        Ok(messages.into_iter().map(|raw| self.consume(raw)).collect())
    }

    fn mark_lost(&mut self) {
        self.alive = false;
        self.approvals.clear();
        for binding in self.sessions.values_mut() {
            binding.control_connected = false;
            binding.execution_state = ExecutionState::Unknown;
        }
    }

    fn consume(&mut self, raw: Value) -> HostEvent {
        let method = raw["method"].as_str().unwrap_or("provider_response");
        let params = &raw["params"];
        let native_id = params["threadId"]
            .as_str()
            .or_else(|| params["thread"]["id"].as_str())
            .map(str::to_owned);
        let hub_id = native_id
            .as_ref()
            .and_then(|id| self.native_to_hub.get(id))
            .cloned();
        let mut approval_id = None;
        let mut kind = method.to_string();
        if let Some(binding) = hub_id.as_ref().and_then(|id| self.sessions.get_mut(id)) {
            match method {
                "thread/status/changed" => {
                    binding.execution_state = execution_state(&params["status"])
                }
                "turn/started" => {
                    binding.current_turn_id = params["turn"]["id"].as_str().map(str::to_owned);
                    binding.execution_state = turn_state(&params["turn"]["status"]);
                }
                // Delayed events for an older turn must not end a newer turn.
                "turn/completed"
                    if params["turn"]["id"].as_str() == binding.current_turn_id.as_deref() =>
                {
                    binding.execution_state = turn_state(&params["turn"]["status"]);
                    binding.current_turn_id = None;
                    self.approvals
                        .retain(|_, approval| approval.hub_session_id != binding.hub_session_id);
                }
                "serverRequest/resolved" => {
                    self.approvals.retain(|_, approval| {
                        approval.hub_session_id != binding.hub_session_id
                            || approval.request_id != params["requestId"]
                    });
                }
                _ if raw.get("id").is_some() && raw.get("method").is_some() => {
                    let request_id = raw["id"].clone();
                    let turn_id = params["turnId"].as_str().unwrap_or("");
                    if binding.current_turn_id.as_deref() == Some(turn_id) {
                        let id = format!(
                            "{}:{}:{}",
                            self.generation, binding.provider_session_id, request_id
                        );
                        let supported = matches!(
                            method,
                            "item/commandExecution/requestApproval"
                                | "item/fileChange/requestApproval"
                        );
                        if !self.answered_requests.contains(&id) {
                            binding.execution_state = if supported {
                                ExecutionState::AwaitingPermission
                            } else {
                                ExecutionState::AwaitingInput
                            };
                            self.approvals
                                .entry(id.clone())
                                .or_insert_with(|| PendingApproval {
                                    hub_session_id: binding.hub_session_id.clone(),
                                    turn_id: turn_id.into(),
                                    request_id,
                                    supported,
                                    available_decisions: params["availableDecisions"]
                                        .as_array()
                                        .cloned(),
                                });
                            approval_id = Some(id);
                            kind = if supported {
                                "approval_requested"
                            } else {
                                "unsupported_provider_request"
                            }
                            .into();
                        }
                    }
                }
                _ => {}
            }
        }
        HostEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            host_generation: self.generation.clone(),
            binding: hub_id
                .as_ref()
                .and_then(|id| self.sessions.get(id))
                .cloned(),
            hub_session_id: hub_id,
            provider_session_id: native_id,
            kind,
            raw,
            approval_id,
        }
    }
}

fn execution_state(status: &Value) -> ExecutionState {
    match status["type"].as_str() {
        Some("idle") => ExecutionState::Idle,
        Some("systemError") => ExecutionState::Error,
        Some("active") => {
            let flags = status["activeFlags"].as_array();
            if flags.is_some_and(|flags| flags.contains(&json!("waitingOnApproval"))) {
                ExecutionState::AwaitingPermission
            } else if flags.is_some_and(|flags| flags.contains(&json!("waitingOnUserInput"))) {
                ExecutionState::AwaitingInput
            } else {
                ExecutionState::Running
            }
        }
        _ => ExecutionState::Unknown,
    }
}

fn turn_state(status: &Value) -> ExecutionState {
    match status.as_str() {
        Some("inProgress") => ExecutionState::Running,
        Some("completed" | "interrupted") => ExecutionState::Idle,
        Some("failed") => ExecutionState::Error,
        _ => ExecutionState::Unknown,
    }
}
