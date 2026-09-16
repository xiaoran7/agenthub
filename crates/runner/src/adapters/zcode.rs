use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    adapters::{clamp_raw_status, safe_tool_category, sanitize_detail_value},
    domain::{
        ActivityStatus, AgentType, Confidence, ObservationMeta, ObservationSource, Presence,
        SessionObservation, SessionState, WaitingReason,
    },
    privacy::ProjectRedactor,
};

#[derive(Debug, Error)]
pub enum ZCodeAdapterError {
    #[error("failed to parse ZCode JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid ZCode session data: {0}")]
    InvalidSession(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZCodeSessionEntry {
    pub session_id: String,
    pub title: Option<String>,
    pub cwd: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub latest_role: Option<String>,
    pub latest_part_type: Option<String>,
    pub latest_tool_name: Option<String>,
    pub latest_tool_status: Option<String>,
    pub is_running: bool,
}

pub fn normalize_sessions_json(
    payload: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<Vec<SessionObservation>, ZCodeAdapterError> {
    let entries: Vec<ZCodeSessionEntry> = serde_json::from_str(payload)?;
    Ok(entries
        .iter()
        .filter_map(|entry| normalize_zcode_session(entry, meta, redactor))
        .collect())
}

pub fn normalize_zcode_session(
    entry: &ZCodeSessionEntry,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Option<SessionObservation> {
    let session_id = entry.session_id.trim();
    if session_id.is_empty() {
        return None;
    }

    let presence = if entry.is_running {
        Presence::Live
    } else {
        Presence::Persisted
    };

    let (activity_status, waiting_reason, raw_status) = if entry.is_running {
        if let Some(status) = entry.latest_tool_status.as_deref() {
            match status {
                "running" => {
                    let tool = entry.latest_tool_name.as_deref().unwrap_or("tool");
                    (ActivityStatus::Working, None, format!("running_{tool}"))
                }
                "pending" | "waiting_approval" => (
                    ActivityStatus::WaitingPermission,
                    Some(WaitingReason::Permission),
                    "waiting_permission".to_string(),
                ),
                "waiting_input" | "ask_user" => (
                    ActivityStatus::WaitingInput,
                    Some(WaitingReason::UserInput),
                    "waiting_user_input".to_string(),
                ),
                _ => (ActivityStatus::Working, None, "active".to_string()),
            }
        } else if entry.latest_role.as_deref() == Some("user") {
            (
                ActivityStatus::Working,
                None,
                "processing_user_request".to_string(),
            )
        } else {
            (ActivityStatus::Working, None, "working".to_string())
        }
    } else {
        (ActivityStatus::Completed, None, "completed".to_string())
    };

    let tool_category = safe_tool_category(entry.latest_tool_name.as_deref());

    let mut status_detail = BTreeMap::new();
    if let Some(title) = &entry.title {
        status_detail.insert("title".to_string(), sanitize_detail_value(title));
    }
    if let Some(tool) = &entry.latest_tool_name {
        status_detail.insert("tool".to_string(), sanitize_detail_value(tool));
    }
    if let Some(cat) = tool_category {
        status_detail.insert("tool_category".to_string(), cat);
    }

    let mut observation = SessionObservation::new(
        meta,
        AgentType::ZCode,
        session_id,
        redactor.redact(&entry.cwd),
        SessionState {
            presence,
            activity_status,
            raw_status: clamp_raw_status(&raw_status),
            raw_status_detail: status_detail,
            waiting_reason,
        },
        ObservationSource::ZCodeDb,
        if entry.is_running {
            Confidence::Observed
        } else {
            Confidence::Authoritative
        },
    );

    observation.started_at = DateTime::<Utc>::from_timestamp_millis(entry.time_created);
    observation.last_activity_at = DateTime::<Utc>::from_timestamp_millis(entry.time_updated);

    Some(observation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_live_working_zcode_session() {
        let entry = ZCodeSessionEntry {
            session_id: "sess_123456".to_string(),
            title: Some("Fix auth bug".to_string()),
            cwd: "D:\\ClaudeSpace".to_string(),
            time_created: 1000,
            time_updated: 2000,
            latest_role: Some("assistant".to_string()),
            latest_part_type: Some("tool".to_string()),
            latest_tool_name: Some("Bash".to_string()),
            latest_tool_status: Some("running".to_string()),
            is_running: true,
        };

        let meta = ObservationMeta::now("device-test");
        let redactor = ProjectRedactor::new(b"01234567890123456789012345678901").unwrap();

        let obs = normalize_zcode_session(&entry, &meta, &redactor).unwrap();
        assert_eq!(obs.agent_type, AgentType::ZCode);
        assert_eq!(obs.state.presence, Presence::Live);
        assert_eq!(obs.state.activity_status, ActivityStatus::Working);
        assert_eq!(obs.state.raw_status, "running_Bash");
        assert_eq!(
            obs.state.raw_status_detail.get("tool_category"),
            Some(&"shell".to_string())
        );
    }

    #[test]
    fn maps_persisted_completed_zcode_session() {
        let entry = ZCodeSessionEntry {
            session_id: "sess_offline".to_string(),
            title: Some("Completed task".to_string()),
            cwd: "D:\\ClaudeSpace".to_string(),
            time_created: 1000,
            time_updated: 2000,
            latest_role: Some("assistant".to_string()),
            latest_part_type: Some("text".to_string()),
            latest_tool_name: None,
            latest_tool_status: None,
            is_running: false,
        };

        let meta = ObservationMeta::now("device-test");
        let redactor = ProjectRedactor::new(b"01234567890123456789012345678901").unwrap();

        let obs = normalize_zcode_session(&entry, &meta, &redactor).unwrap();
        assert_eq!(obs.agent_type, AgentType::ZCode);
        assert_eq!(obs.state.presence, Presence::Persisted);
        assert_eq!(obs.state.activity_status, ActivityStatus::Completed);
    }
}
