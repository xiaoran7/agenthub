use super::*;

pub(crate) const DEFAULT_MAX_LOCAL_EVENTS: i64 = 10_000;
pub(crate) const MAX_CONFIGURED_LOCAL_EVENTS: i64 = 100_000;
/// Bound on a projected provider error message. A stack-trace-sized payload would
/// otherwise travel to every subscribed client on every reconnect.
pub(crate) const MAX_ERROR_MESSAGE_CHARS: usize = 2_048;

type Result<T> = std::result::Result<T, JournalError>;

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("remote_journal_io")]
    Sqlite(#[from] rusqlite::Error),
    #[error("remote_journal_invalid_data")]
    Json(#[from] serde_json::Error),
    #[error("remote_journal_invalid_ack")]
    InvalidAck,
    #[error("remote_journal_identity_mismatch")]
    IdentityMismatch,
    #[error("{0}")]
    HostLost(String),
}

pub enum BeginCommand {
    New,
    Existing(CommandReceipt),
}

pub enum BeginInboundCommand {
    New,
    CompleteExisting(CommandReceipt),
    AlreadyCompleted(CommandReceipt),
}

/// One host event queued for a batched journal write.
///
/// A poll tick drains many events at once. Committing each one separately means
/// one `synchronous=FULL` fsync per event, so a burst costs a fsync storm.
pub struct BatchedAppend<'a> {
    pub local: &'a Value,
    /// Remote projection; `None` keeps this event local-only.
    pub remote: Option<&'a Value>,
    pub bindings: &'a [SessionBinding],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalSnapshot {
    pub sessions: Vec<SessionBinding>,
    pub pending_approvals: Vec<PendingApprovalSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_questions: Vec<crate::remote_host::PendingQuestionSnapshot>,
    pub cursor: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEvent {
    pub cursor: i64,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EventsAfter {
    Events {
        events: Vec<JournalEvent>,
        cursor: i64,
    },
    SnapshotRequired {
        snapshot: JournalSnapshot,
    },
}
