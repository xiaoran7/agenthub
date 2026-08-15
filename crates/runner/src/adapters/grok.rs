use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::{
    adapters::{clamp_raw_status, detail, safe_tool_category, sanitize_detail_value, string_field},
    domain::{
        ActivityStatus, AgentType, Confidence, ObservationMeta, ObservationSource, Presence,
        SessionObservation, SessionState, WaitingReason,
    },
    privacy::ProjectRedactor,
};

#[derive(Debug, Error)]
pub enum GrokAdapterError {
    #[error("invalid Grok sessions JSON: {0}")]
    InvalidSessionsJson(#[from] serde_json::Error),
    #[error("Grok hook has no session_id")]
    MissingHookSessionId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrokSessionEntry {
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    started_at: Option<i64>,
    #[serde(default)]
    last_activity_at: Option<i64>,
    #[serde(default)]
    live: Option<bool>,
    #[serde(default)]
    source: Option<String>,
}

pub fn normalize_sessions_json(
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<Vec<SessionObservation>, GrokAdapterError> {
    let entries: Vec<GrokSessionEntry> = serde_json::from_str(input)?;
    Ok(entries
        .into_iter()
        .filter_map(|entry| normalize_session_entry(entry, meta, redactor))
        .collect())
}

fn normalize_session_entry(
    entry: GrokSessionEntry,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Option<SessionObservation> {
    let session_id = entry
        .session_id
        .clone()
        .or_else(|| entry.id.clone())
        .filter(|id| !id.trim().is_empty())?;

    let raw_status = entry.state.as_deref().unwrap_or("unknown").to_owned();
    let process_alive = entry.pid.is_some() || entry.live == Some(true);
    let activity_status = map_activity(&raw_status);
    let presence = map_presence(&raw_status, process_alive, activity_status);
    let source = map_source(entry.source.as_deref(), process_alive);
    let confidence = if activity_status == ActivityStatus::Unknown {
        Confidence::Inferred
    } else if source == ObservationSource::GrokActiveSessions {
        Confidence::Observed
    } else {
        Confidence::Authoritative
    };

    let mut observation = SessionObservation::new(
        meta,
        AgentType::GrokBuild,
        session_id,
        redactor.redact(&entry.cwd),
        SessionState {
            presence,
            activity_status,
            raw_status: clamp_raw_status(&raw_status),
            raw_status_detail: detail([
                ("process", entry.pid.map(|_| "alive".to_owned())),
                (
                    "live",
                    entry.live.map(|value| {
                        if value {
                            "true".to_owned()
                        } else {
                            "false".to_owned()
                        }
                    }),
                ),
                ("source", entry.source.as_deref().map(sanitize_detail_value)),
            ]),
            waiting_reason: None,
        },
        source,
        confidence,
    );
    observation.started_at = entry
        .started_at
        .and_then(DateTime::<Utc>::from_timestamp_millis);
    observation.last_activity_at = entry
        .last_activity_at
        .and_then(DateTime::<Utc>::from_timestamp_millis);
    Some(observation)
}

fn map_activity(raw_status: &str) -> ActivityStatus {
    match raw_status.to_ascii_lowercase().as_str() {
        "working" | "running" | "in_progress" | "inprogress" => ActivityStatus::Working,
        "idle" => ActivityStatus::Idle,
        "done" | "completed" => ActivityStatus::Completed,
        "failed" | "error" => ActivityStatus::Failed,
        "stopped" | "terminated" | "interrupted" => ActivityStatus::Stopped,
        "persisted" => ActivityStatus::Unknown,
        _ => ActivityStatus::Unknown,
    }
}

fn map_presence(
    raw_status: &str,
    process_alive: bool,
    activity_status: ActivityStatus,
) -> Presence {
    if raw_status.eq_ignore_ascii_case("persisted") && !process_alive {
        return Presence::Persisted;
    }
    if process_alive {
        return Presence::Live;
    }
    match activity_status {
        ActivityStatus::Working
        | ActivityStatus::WaitingInput
        | ActivityStatus::WaitingPermission
        | ActivityStatus::Idle => Presence::Live,
        ActivityStatus::Completed | ActivityStatus::Failed | ActivityStatus::Stopped => {
            Presence::Persisted
        }
        ActivityStatus::Unknown => Presence::Unknown,
    }
}

fn map_source(source: Option<&str>, process_alive: bool) -> ObservationSource {
    match source.map(str::to_ascii_lowercase).as_deref() {
        Some("active") | Some("active_sessions") => ObservationSource::GrokActiveSessions,
        Some("hook") => ObservationSource::GrokHook,
        _ if process_alive => ObservationSource::GrokActiveSessions,
        _ => ObservationSource::GrokSessions,
    }
}

pub fn normalize_hook(
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<SessionObservation, GrokAdapterError> {
    let value: Value = serde_json::from_str(input)?;
    let session_id = string_field(&value, &["session_id"])
        .filter(|id| !id.trim().is_empty())
        .ok_or(GrokAdapterError::MissingHookSessionId)?;
    let cwd = string_field(&value, &["cwd"]).unwrap_or("");
    let event = string_field(&value, &["hook_event_name"]).unwrap_or("Unknown");
    let notification_type = string_field(&value, &["notification_type"]);
    let tool_name = string_field(&value, &["tool_name"]);

    let (presence, activity_status, waiting_reason) =
        map_hook_event(event, notification_type, tool_name);
    let confidence = if activity_status == ActivityStatus::Unknown && event != "SessionEnd" {
        Confidence::Inferred
    } else {
        Confidence::Observed
    };

    Ok(SessionObservation::new(
        meta,
        AgentType::GrokBuild,
        session_id,
        redactor.redact(cwd),
        SessionState {
            presence,
            activity_status,
            raw_status: clamp_raw_status(event),
            raw_status_detail: detail([
                (
                    "notification_type",
                    notification_type.map(sanitize_detail_value),
                ),
                ("tool_category", safe_tool_category(tool_name)),
                (
                    "error_type",
                    string_field(&value, &["error"]).map(sanitize_detail_value),
                ),
                (
                    "end_reason",
                    string_field(&value, &["reason"]).map(sanitize_detail_value),
                ),
            ]),
            waiting_reason,
        },
        ObservationSource::GrokHook,
        confidence,
    ))
}

fn map_hook_event(
    event: &str,
    notification_type: Option<&str>,
    tool_name: Option<&str>,
) -> (Presence, ActivityStatus, Option<WaitingReason>) {
    match event {
        "SessionStart" => (Presence::Live, ActivityStatus::Idle, None),
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" => {
            (Presence::Live, ActivityStatus::Working, None)
        }
        "PreToolUse" if matches!(tool_name, Some("AskUserQuestion" | "ask_user_question")) => (
            Presence::Live,
            ActivityStatus::WaitingInput,
            Some(WaitingReason::UserInput),
        ),
        "PreToolUse" => (Presence::Live, ActivityStatus::Working, None),
        "PermissionRequest" => (
            Presence::Live,
            ActivityStatus::WaitingPermission,
            Some(WaitingReason::Permission),
        ),
        "Elicitation" => (
            Presence::Live,
            ActivityStatus::WaitingInput,
            Some(WaitingReason::McpElicitation),
        ),
        "Notification" => match notification_type {
            Some("permission_prompt") => (
                Presence::Live,
                ActivityStatus::WaitingPermission,
                Some(WaitingReason::Permission),
            ),
            Some("agent_needs_input") => (
                Presence::Live,
                ActivityStatus::WaitingInput,
                Some(WaitingReason::UserInput),
            ),
            Some("idle_prompt") => (Presence::Live, ActivityStatus::Idle, None),
            Some("agent_completed") => (Presence::Live, ActivityStatus::Completed, None),
            _ => (Presence::Live, ActivityStatus::Unknown, None),
        },
        "Stop" => (Presence::Live, ActivityStatus::Completed, None),
        "StopFailure" => (Presence::Live, ActivityStatus::Failed, None),
        "SessionEnd" => (Presence::Persisted, ActivityStatus::Unknown, None),
        _ => (Presence::Live, ActivityStatus::Unknown, None),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn meta() -> ObservationMeta {
        ObservationMeta {
            device_id: "device-1".into(),
            observed_at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        }
    }

    fn redactor() -> ProjectRedactor {
        ProjectRedactor::new(b"secret").unwrap()
    }

    #[test]
    fn maps_live_working_session_without_copying_paths() {
        let input = r#"[{
          "sessionId": "01a005b5-5ee8-7013-947b-0140a7b267fa",
          "cwd": "C:\\work\\private-project",
          "state": "working",
          "pid": 4888,
          "live": true,
          "source": "active",
          "startedAt": 1700000000000,
          "lastActivityAt": 1700000060000
        }]"#;

        let observations = normalize_sessions_json(input, &meta(), &redactor()).unwrap();
        let observation = &observations[0];
        assert_eq!(observation.state.activity_status, ActivityStatus::Working);
        assert_eq!(observation.state.presence, Presence::Live);
        assert_eq!(observation.source, ObservationSource::GrokActiveSessions);
        assert_eq!(observation.project.display_name, "private-project");
        assert!(
            !serde_json::to_string(observation)
                .unwrap()
                .contains(r"C:\work")
        );
    }

    #[test]
    fn persisted_history_is_not_reported_as_live() {
        let input = r#"[{
          "sessionId": "session-history",
          "cwd": "/work/demo",
          "state": "persisted"
        }]"#;

        let observations = normalize_sessions_json(input, &meta(), &redactor()).unwrap();
        assert_eq!(observations[0].state.presence, Presence::Persisted);
        assert_eq!(
            observations[0].state.activity_status,
            ActivityStatus::Unknown
        );
        assert_eq!(observations[0].source, ObservationSource::GrokSessions);
        assert_eq!(observations[0].confidence, Confidence::Inferred);
    }

    #[test]
    fn hook_does_not_copy_prompt_or_command() {
        let input = r#"{
          "session_id": "session-123",
          "cwd": "/work/private-project",
          "hook_event_name": "PermissionRequest",
          "tool_name": "run_terminal_cmd",
          "tool_input": {"command": "echo TOP_SECRET"},
          "prompt": "my secret prompt"
        }"#;

        let observation = normalize_hook(input, &meta(), &redactor()).unwrap();
        let serialized = serde_json::to_string(&observation).unwrap();
        assert_eq!(
            observation.state.activity_status,
            ActivityStatus::WaitingPermission
        );
        assert_eq!(
            observation.state.raw_status_detail.get("tool_category"),
            Some(&"shell".to_owned())
        );
        assert!(!serialized.contains("TOP_SECRET"));
        assert!(!serialized.contains("my secret prompt"));
        assert!(!serialized.contains("/work/private-project"));
    }

    #[test]
    fn unknown_hook_is_inferred_not_authoritative() {
        let observation = normalize_hook(
            r#"{"session_id":"session-1","hook_event_name":"FutureWorkflow"}"#,
            &meta(),
            &redactor(),
        )
        .unwrap();
        assert_eq!(observation.state.activity_status, ActivityStatus::Unknown);
        assert_eq!(observation.confidence, Confidence::Inferred);
    }
}
