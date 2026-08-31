use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

use crate::{
    domain::{
        ActivityStatus, AgentType, Confidence, ObservationMeta, ObservationSource, Presence,
        SessionObservation, SessionState, WaitingReason,
    },
    privacy::ProjectRedactor,
};

#[derive(Debug, Error)]
pub enum AntigravityAdapterError {
    #[error("failed to parse Antigravity JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid Antigravity step format: {0}")]
    InvalidStep(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityStep {
    pub step_index: Option<i64>,
    pub source: Option<String>,
    #[serde(rename = "type")]
    pub step_type: Option<String>,
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub content: Option<String>,
    pub thinking: Option<String>,
    pub tool_calls: Option<Vec<AntigravityToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityToolCall {
    pub tool_name: Option<String>,
    pub tool_action: Option<String>,
    pub tool_summary: Option<String>,
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct AntigravitySessionMetadata {
    pub session_id: String,
    pub cwd: String,
    pub is_process_alive: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub last_activity_at: Option<DateTime<Utc>>,
}

pub fn parse_transcript_lines(
    contents: &str,
) -> Result<Vec<AntigravityStep>, AntigravityAdapterError> {
    let mut steps = Vec::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(step) = serde_json::from_str::<AntigravityStep>(line) {
            steps.push(step);
        }
    }
    Ok(steps)
}

pub fn normalize_transcript_steps(
    steps: &[AntigravityStep],
    session: &AntigravitySessionMetadata,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Option<SessionObservation> {
    let session_id = session.session_id.trim();
    if session_id.is_empty() {
        return None;
    }

    let presence = if session.is_process_alive {
        Presence::Live
    } else {
        Presence::Persisted
    };

    let mut activity_status = ActivityStatus::Idle;
    let mut waiting_reason = None;
    let mut last_observed_time = session.last_activity_at;

    if let Some(last_step) = steps.last() {
        if let Some(dt) = last_step
            .created_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        {
            last_observed_time = Some(dt.with_timezone(&Utc));
        }

        let source = last_step.source.as_deref().unwrap_or("");
        let step_type = last_step.step_type.as_deref().unwrap_or("");
        let step_status = last_step.status.as_deref().unwrap_or("");

        if source == "USER_EXPLICIT" || step_type == "USER_INPUT" {
            activity_status = ActivityStatus::Working;
        } else if step_type == "PLANNER_RESPONSE" {
            if step_status == "RUNNING" {
                activity_status = ActivityStatus::Working;
            } else if let Some(tool_calls) = &last_step.tool_calls {
                let has_user_prompt = tool_calls.iter().any(|tc| {
                    let name = tc.tool_name.as_deref().unwrap_or("");
                    name == "ask_question" || name == "ask_user_input"
                });
                if has_user_prompt {
                    activity_status = ActivityStatus::WaitingInput;
                    waiting_reason = Some(WaitingReason::UserInput);
                } else if !session.is_process_alive {
                    activity_status = ActivityStatus::Completed;
                } else {
                    activity_status = ActivityStatus::Idle;
                }
            } else if !session.is_process_alive {
                activity_status = ActivityStatus::Completed;
            } else {
                activity_status = ActivityStatus::Idle;
            }
        }
    }

    let mut observation = SessionObservation::new(
        meta,
        AgentType::Antigravity,
        session_id,
        redactor.redact(&session.cwd),
        SessionState {
            presence,
            activity_status,
            raw_status: "antigravity".to_owned(),
            raw_status_detail: BTreeMap::new(),
            waiting_reason,
        },
        ObservationSource::AntigravityBrain,
        Confidence::Authoritative,
    );

    observation.started_at = session.started_at;
    observation.last_activity_at = last_observed_time;

    Some(observation)
}

pub fn normalize_hook(
    payload: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<SessionObservation, AntigravityAdapterError> {
    let val: Value = serde_json::from_str(payload)?;
    let session_id = val
        .get("sessionId")
        .or_else(|| val.get("session_id"))
        .or_else(|| val.get("conversation_id"))
        .and_then(Value::as_str)
        .unwrap_or("antigravity-session")
        .to_owned();

    let cwd = val
        .get("cwd")
        .or_else(|| val.get("workspace"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    let activity_status = match val.get("status").and_then(Value::as_str) {
        Some("working") | Some("running") => ActivityStatus::Working,
        Some("waiting_input") => ActivityStatus::WaitingInput,
        Some("waiting_permission") => ActivityStatus::WaitingPermission,
        Some("completed") | Some("done") => ActivityStatus::Completed,
        Some("failed") | Some("error") => ActivityStatus::Failed,
        _ => ActivityStatus::Idle,
    };

    let obs = SessionObservation::new(
        meta,
        AgentType::Antigravity,
        session_id,
        redactor.redact(&cwd),
        SessionState {
            presence: Presence::Live,
            activity_status,
            raw_status: "hook".to_owned(),
            raw_status_detail: BTreeMap::new(),
            waiting_reason: None,
        },
        ObservationSource::AntigravityHook,
        Confidence::Authoritative,
    );

    Ok(obs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_antigravity_steps_and_normalizes() {
        let sample = r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-08-31T03:00:00Z","content":"hello"}
{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"RUNNING","created_at":"2026-08-31T03:00:05Z","content":"thinking..."}"#;

        let steps = parse_transcript_lines(sample).expect("steps parsed");
        assert_eq!(steps.len(), 2);

        let redactor = ProjectRedactor::new(b"01234567890123456789012345678901").unwrap();
        let meta = ObservationMeta::now("device-test");
        let session = AntigravitySessionMetadata {
            session_id: "test-uuid-1234".into(),
            cwd: "/home/user/project".into(),
            is_process_alive: true,
            started_at: None,
            last_activity_at: None,
        };

        let obs =
            normalize_transcript_steps(&steps, &session, &meta, &redactor).expect("normalized");
        assert_eq!(obs.agent_type, AgentType::Antigravity);
        assert_eq!(obs.agent_session_id, "test-uuid-1234");
        assert_eq!(obs.state.activity_status, ActivityStatus::Working);
    }
}
