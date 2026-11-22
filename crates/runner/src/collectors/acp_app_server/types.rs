use super::*;

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
    pub(crate) program: String,
    pub(crate) child: Child,
    pub(crate) stdin: BufWriter<ChildStdin>,
    pub(crate) receiver: Receiver<ProtocolRead>,
    pub(crate) reader: Option<JoinHandle<()>>,
    pub(crate) pending_messages: VecDeque<Value>,
    pub(crate) next_request_id: u64,
    pub(crate) response_timeout: Duration,
    pub(crate) job: Option<ChildJob>,
    pub(crate) pending_prompts: HashMap<u64, PendingPromptInfo>,
    pub(crate) pending_permissions: HashMap<String, Vec<AcpPermissionOption>>,
    pub(crate) active_turns: HashMap<String, String>,
}
