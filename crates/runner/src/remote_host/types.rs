use super::*;

/// In-memory replay window for command receipts.
///
/// The daemon's durable ledger (`remote_journal::RemoteJournal`) is the
/// authority for "has this command already run", so this bound only needs to
/// cover repeats seen while the Host is alive. Oldest entries are evicted
/// first; a command whose id is gone from here is answered from the journal
/// rather than re-executed.
pub const MAX_COMMAND_LEDGER_ENTRIES: usize = 10_000;
/// Retries for a first `thread/list` page that came back without a `data`
/// array before the discovery call reports `thread_list_malformed`.
pub(crate) const LIST_ATTEMPTS: u32 = 3;

/// In-memory replay window for answered approval/question request ids.
///
/// Bounds re-delivery suppression so the set cannot grow for the life of a
/// long-lived daemon. Comfortably above the number of requests a single
/// provider re-delivers after a response.
pub const MAX_ANSWERED_REQUESTS: usize = 10_000;

/// Protocol errors are safe codes; provider payloads belong to the content stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub code: String,
    pub outcome_unknown: bool,
}

impl NativeError {
    pub fn rejected(code: impl AsRef<str>) -> Self {
        Self {
            code: code.as_ref().to_owned(),
            outcome_unknown: false,
        }
    }

    pub fn uncertain(code: impl AsRef<str>) -> Self {
        Self {
            code: code.as_ref().to_owned(),
            outcome_unknown: true,
        }
    }
}

pub trait NativeTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError>;
    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError>;
    fn drain(&mut self) -> Result<Vec<Value>, NativeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectMode {
    Create,
    Resume,
    Preview,
    TakeControl,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptAttachment {
    pub name: String,
    pub media_type: String,
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

pub(crate) fn decode_base64_bytes(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte.is_ascii_whitespace() || byte == b'=' {
            continue;
        }
        let val = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
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
        #[serde(default)]
        agent_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_mode: Option<String>,
    },
    SendPrompt {
        text: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        acquire_control: bool,
        /// Per-turn reasoning effort. Providers that do not accept one ignore it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        /// Per-turn model override.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// Per-turn permission mode override (`default`, `acceptEdits`, `plan`, `bypassPermissions`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_mode: Option<String>,
        /// Optional user-selected file/image attachments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<PromptAttachment>,
    },
    ResolveApproval {
        approval_id: String,
        approved: bool,
    },
    AnswerQuestion {
        question_id: String,
        answer: Value,
    },
    SetArchived {
        archived: bool,
    },
    ReleaseControl,
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
    ReleasingControl,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<PendingApprovalSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<PendingQuestionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApprovalSnapshot {
    pub approval_id: String,
    pub hub_session_id: String,
    pub host_generation: String,
    pub turn_id: String,
    pub can_approve: bool,
    pub can_decline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOptionSnapshot {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingQuestionSnapshot {
    pub question_id: String,
    pub hub_session_id: String,
    pub host_generation: String,
    pub turn_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOptionSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
}

pub(crate) struct PendingApproval {
    pub(crate) hub_session_id: String,
    pub(crate) turn_id: String,
    pub(crate) request_id: Value,
    pub(crate) supported: bool,
    pub(crate) available_decisions: Option<Vec<Value>>,
    pub(crate) tool_name: Option<String>,
    pub(crate) command_preview: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) diff: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) risk_level: Option<String>,
}

pub(crate) struct PendingQuestion {
    pub(crate) hub_session_id: String,
    pub(crate) turn_id: String,
    pub(crate) request_id: Value,
    pub(crate) prompt: String,
    pub(crate) header: Option<String>,
    pub(crate) codex_question_id: Option<String>,
    pub(crate) options: Option<Vec<QuestionOptionSnapshot>>,
    pub(crate) multi_select: Option<bool>,
}

pub(crate) fn execution_state(status: &Value) -> ExecutionState {
    let type_str = status["type"].as_str().or_else(|| status.as_str());
    match type_str {
        Some("idle" | "notLoaded" | "persisted") => ExecutionState::Idle,
        Some("systemError" | "error" | "failed") => ExecutionState::Error,
        Some("completed") => ExecutionState::Completed,
        Some("active" | "running" | "inProgress") => {
            let flags = status["activeFlags"]
                .as_array()
                .or_else(|| status["flags"].as_array());
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

pub(crate) fn turn_state(status: &Value) -> ExecutionState {
    let status_str = status.as_str().or_else(|| status["type"].as_str());
    match status_str {
        Some("inProgress" | "running") => ExecutionState::Running,
        Some("completed") => ExecutionState::Completed,
        Some("interrupted") => ExecutionState::Idle,
        Some("failed") => ExecutionState::Error,
        _ => ExecutionState::Unknown,
    }
}
