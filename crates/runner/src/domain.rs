use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    ClaudeCode,
    Codex,
    Antigravity,
    GrokBuild,
    #[serde(rename = "zcode", alias = "z_code")]
    ZCode,
}

impl AgentType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::GrokBuild => "grok_build",
            Self::ZCode => "zcode",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Presence {
    Live,
    Persisted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityStatus {
    Working,
    WaitingInput,
    WaitingPermission,
    Idle,
    Completed,
    Failed,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Unknown,
    Inferred,
    Observed,
    Authoritative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    ClaudeAgentsJson,
    ClaudeHook,
    CodexAppServer,
    CodexHook,
    AntigravityBrain,
    AntigravityHook,
    AntigravityCli,
    GrokSessions,
    GrokHook,
    GrokActiveSessions,
    ProcessProbe,
    LocalHistory,
    ZCodeDb,
    ZCodeHook,
    ZCodeRollout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitingReason {
    UserInput,
    Permission,
    Sandbox,
    Worker,
    Dialog,
    McpElicitation,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectRef {
    pub display_name: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub presence: Presence,
    pub activity_status: ActivityStatus,
    pub raw_status: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub raw_status_detail: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_reason: Option<WaitingReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionObservation {
    pub schema_version: u16,
    pub event_id: Uuid,
    pub device_id: String,
    pub agent_type: AgentType,
    pub agent_session_id: String,
    pub project: ProjectRef,
    pub state: SessionState,
    pub source: ObservationSource,
    pub confidence: Confidence,
    pub observed_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ObservationMeta {
    pub device_id: String,
    pub observed_at: DateTime<Utc>,
}

impl ObservationMeta {
    pub fn now(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            observed_at: Utc::now(),
        }
    }
}

impl SessionObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        meta: &ObservationMeta,
        agent_type: AgentType,
        agent_session_id: impl Into<String>,
        project: ProjectRef,
        state: SessionState,
        source: ObservationSource,
        confidence: Confidence,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            event_id: Uuid::new_v4(),
            device_id: meta.device_id.clone(),
            agent_type,
            agent_session_id: agent_session_id.into(),
            project,
            state,
            source,
            confidence,
            observed_at: meta.observed_at,
            started_at: None,
            last_activity_at: Some(meta.observed_at),
        }
    }

    pub fn session_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.agent_type.as_str(),
            self.device_id,
            self.agent_session_id
        )
    }

    pub fn has_same_snapshot_payload(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version
            && self.device_id == other.device_id
            && self.agent_type == other.agent_type
            && self.agent_session_id == other.agent_session_id
            && self.project == other.project
            && self.state == other.state
            && self.source == other.source
            && self.confidence == other.confidence
            && self.started_at == other.started_at
            && self.last_activity_at == other.last_activity_at
    }
}

pub fn merge_observations(
    previous: &SessionObservation,
    incoming: &SessionObservation,
) -> SessionObservation {
    if incoming.observed_at < previous.observed_at {
        return previous.clone();
    }

    if incoming.observed_at == previous.observed_at && incoming.confidence < previous.confidence {
        return previous.clone();
    }

    let mut merged = incoming.clone();
    if incoming.state.activity_status == ActivityStatus::Unknown {
        merged.state.activity_status = previous.state.activity_status;
        if incoming.state.waiting_reason.is_none() {
            merged.state.waiting_reason = previous.state.waiting_reason;
        }
    }
    if incoming.started_at.is_none() {
        merged.started_at = previous.started_at;
    }
    if merged.last_activity_at < previous.last_activity_at {
        merged.last_activity_at = previous.last_activity_at;
    }
    merged
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn observation(status: ActivityStatus, confidence: Confidence) -> SessionObservation {
        let meta = ObservationMeta {
            device_id: "device-1".into(),
            observed_at: Utc.timestamp_opt(100, 0).unwrap(),
        };
        SessionObservation::new(
            &meta,
            AgentType::Codex,
            "session-1",
            ProjectRef {
                display_name: "project".into(),
                fingerprint: "fingerprint".into(),
            },
            SessionState {
                presence: Presence::Live,
                activity_status: status,
                raw_status: "raw".into(),
                raw_status_detail: BTreeMap::new(),
                waiting_reason: None,
            },
            ObservationSource::CodexAppServer,
            confidence,
        )
    }

    #[test]
    fn unknown_activity_preserves_previous_activity() {
        let previous = observation(ActivityStatus::Working, Confidence::Authoritative);
        let mut incoming = observation(ActivityStatus::Unknown, Confidence::Observed);
        incoming.observed_at += chrono::Duration::seconds(1);

        let merged = merge_observations(&previous, &incoming);
        assert_eq!(merged.state.activity_status, ActivityStatus::Working);
        assert_eq!(merged.state.presence, incoming.state.presence);
    }

    #[test]
    fn unknown_activity_preserves_previous_waiting_reason() {
        let mut previous = observation(ActivityStatus::WaitingPermission, Confidence::Observed);
        previous.state.waiting_reason = Some(crate::domain::WaitingReason::Permission);
        let mut incoming = observation(ActivityStatus::Unknown, Confidence::Inferred);
        incoming.observed_at += chrono::Duration::seconds(1);

        let merged = merge_observations(&previous, &incoming);
        assert_eq!(
            merged.state.activity_status,
            ActivityStatus::WaitingPermission
        );
        assert_eq!(
            merged.state.waiting_reason,
            Some(crate::domain::WaitingReason::Permission)
        );
    }

    #[test]
    fn last_activity_never_moves_backwards() {
        let mut previous = observation(ActivityStatus::Working, Confidence::Observed);
        previous.last_activity_at = Some(previous.observed_at + chrono::Duration::seconds(5));
        let mut incoming = observation(ActivityStatus::Idle, Confidence::Authoritative);
        incoming.observed_at += chrono::Duration::seconds(1);
        incoming.last_activity_at = Some(previous.observed_at);

        let merged = merge_observations(&previous, &incoming);
        assert_eq!(merged.last_activity_at, previous.last_activity_at);
    }

    #[test]
    fn older_observation_cannot_replace_newer_state() {
        let mut previous = observation(ActivityStatus::Completed, Confidence::Observed);
        previous.observed_at += chrono::Duration::seconds(10);
        let incoming = observation(ActivityStatus::Working, Confidence::Authoritative);

        let merged = merge_observations(&previous, &incoming);
        assert_eq!(merged.state.activity_status, ActivityStatus::Completed);
    }
}
