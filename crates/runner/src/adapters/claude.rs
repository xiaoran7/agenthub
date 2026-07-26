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
pub enum ClaudeAdapterError {
    #[error("invalid Claude agents JSON: {0}")]
    InvalidAgentsJson(#[from] serde_json::Error),
    #[error("Claude hook has no session_id")]
    MissingHookSessionId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAgentEntry {
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    started_at: Option<i64>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    waiting_for: Option<String>,
}

pub fn normalize_agents_json(
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<Vec<SessionObservation>, ClaudeAdapterError> {
    let entries: Vec<ClaudeAgentEntry> = serde_json::from_str(input)?;
    // One malformed entry must not blind monitoring for every other session;
    // entries without a usable session id are skipped, matching the Codex
    // thread-list behaviour.
    Ok(entries
        .into_iter()
        .filter_map(|entry| normalize_agent_entry(entry, meta, redactor))
        .collect())
}

fn normalize_agent_entry(
    entry: ClaudeAgentEntry,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Option<SessionObservation> {
    let session_id = entry
        .session_id
        .clone()
        .or_else(|| entry.id.clone())
        .filter(|id| !id.trim().is_empty())?;

    let status_value = entry.status.as_ref().and_then(value_as_short_status);
    let raw_status = entry
        .state
        .as_deref()
        .or(status_value.as_deref())
        .unwrap_or("unknown")
        .to_owned();
    let waiting_reason = entry.waiting_for.as_deref().map(map_waiting_reason);
    let activity_status = map_activity(&raw_status, waiting_reason);
    let presence = map_presence(activity_status, entry.pid.is_some());
    let confidence = if activity_status == ActivityStatus::Unknown {
        Confidence::Inferred
    } else {
        Confidence::Authoritative
    };

    let mut observation = SessionObservation::new(
        meta,
        AgentType::ClaudeCode,
        session_id,
        redactor.redact(&entry.cwd),
        SessionState {
            presence,
            activity_status,
            raw_status: clamp_raw_status(&raw_status),
            raw_status_detail: detail([
                ("kind", entry.kind.as_deref().map(sanitize_detail_value)),
                ("process", entry.pid.map(|_| "alive".to_owned())),
                (
                    "waiting_for",
                    entry.waiting_for.as_deref().map(sanitize_detail_value),
                ),
                (
                    "cli_status",
                    status_value.as_deref().map(sanitize_detail_value),
                ),
            ]),
            waiting_reason,
        },
        ObservationSource::ClaudeAgentsJson,
        confidence,
    );
    observation.started_at = entry
        .started_at
        .and_then(DateTime::<Utc>::from_timestamp_millis);
    observation.last_activity_at = None;
    Some(observation)
}

fn value_as_short_status(value: &Value) -> Option<String> {
    match value {
        Value::String(value) if value.len() <= 64 => Some(value.clone()),
        Value::Object(map) => map
            .get("type")
            .or_else(|| map.get("status"))
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 64)
            .map(str::to_owned),
        _ => None,
    }
}

fn map_activity(raw_status: &str, waiting_reason: Option<WaitingReason>) -> ActivityStatus {
    match raw_status.to_ascii_lowercase().as_str() {
        "working" | "running" | "in_progress" | "inprogress" => ActivityStatus::Working,
        "blocked" | "waiting" => match waiting_reason {
            Some(WaitingReason::Permission | WaitingReason::Sandbox) => {
                ActivityStatus::WaitingPermission
            }
            // An unrecognized waitingFor phrase could mean input or a new
            // approval flow; claiming WAITING_INPUT would fabricate certainty.
            Some(WaitingReason::Unknown) | None => ActivityStatus::Unknown,
            Some(_) => ActivityStatus::WaitingInput,
        },
        "idle" => ActivityStatus::Idle,
        "done" | "completed" => ActivityStatus::Completed,
        "failed" | "error" => ActivityStatus::Failed,
        "stopped" | "terminated" | "interrupted" => ActivityStatus::Stopped,
        _ => ActivityStatus::Unknown,
    }
}

fn map_presence(activity_status: ActivityStatus, process_alive: bool) -> Presence {
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

fn map_waiting_reason(waiting_for: &str) -> WaitingReason {
    match waiting_for.to_ascii_lowercase().as_str() {
        "permission prompt" | "permission" => WaitingReason::Permission,
        "input needed" | "user input" => WaitingReason::UserInput,
        "sandbox request" | "sandbox" => WaitingReason::Sandbox,
        "worker request" | "worker" => WaitingReason::Worker,
        "dialog open" | "dialog" => WaitingReason::Dialog,
        "mcp elicitation" => WaitingReason::McpElicitation,
        _ => WaitingReason::Unknown,
    }
}

pub fn normalize_hook(
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<SessionObservation, ClaudeAdapterError> {
    let value: Value = serde_json::from_str(input)?;
    let session_id = string_field(&value, &["session_id"])
        .filter(|id| !id.trim().is_empty())
        .ok_or(ClaudeAdapterError::MissingHookSessionId)?;
    let cwd = string_field(&value, &["cwd"]).unwrap_or("");
    let event = string_field(&value, &["hook_event_name"]).unwrap_or("Unknown");
    let notification_type = string_field(&value, &["notification_type"]);
    let tool_name = string_field(&value, &["tool_name"]);

    let (presence, activity_status, waiting_reason) =
        map_hook_event(event, notification_type, tool_name);
    let state = SessionState {
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
    };

    Ok(SessionObservation::new(
        meta,
        AgentType::ClaudeCode,
        session_id,
        redactor.redact(cwd),
        state,
        ObservationSource::ClaudeHook,
        Confidence::Observed,
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
        "PreToolUse" if tool_name == Some("AskUserQuestion") => (
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
    fn maps_blocked_permission_session() {
        let input = r#"[{
          "cwd": "C:\\work\\demo",
          "kind": "background",
          "startedAt": 1700000000000,
          "id": "short-id",
          "sessionId": "session-123",
          "name": "demo-agent",
          "state": "blocked",
          "pid": 1234,
          "status": "waiting",
          "waitingFor": "permission prompt"
        }]"#;

        let observations = normalize_agents_json(input, &meta(), &redactor()).unwrap();
        let observation = &observations[0];
        assert_eq!(
            observation.state.activity_status,
            ActivityStatus::WaitingPermission
        );
        assert_eq!(observation.state.presence, Presence::Live);
        assert_eq!(
            observation.state.waiting_reason,
            Some(WaitingReason::Permission)
        );
        assert_eq!(observation.project.display_name, "demo");
        assert!(
            !serde_json::to_string(observation)
                .unwrap()
                .contains(r"C:\work")
        );
    }

    #[test]
    fn blocked_with_unrecognized_waiting_reason_is_not_reported_as_input() {
        let input = r#"[{
          "cwd": "C:\\work\\demo",
          "sessionId": "session-999",
          "state": "blocked",
          "pid": 4300,
          "waitingFor": "permission: Bash(rm -rf /tmp/secret)"
        }]"#;

        let observations = normalize_agents_json(input, &meta(), &redactor()).unwrap();
        let observation = &observations[0];
        assert_eq!(observation.state.activity_status, ActivityStatus::Unknown);
        assert_eq!(
            observation.state.waiting_reason,
            Some(WaitingReason::Unknown)
        );
        assert_eq!(observation.confidence, crate::domain::Confidence::Inferred);
        // The unrecognized phrase is stored sanitized, never verbatim.
        let stored = observation
            .state
            .raw_status_detail
            .get("waiting_for")
            .unwrap();
        assert!(!stored.contains('('));
        assert!(!stored.contains(':'));
    }

    #[test]
    fn agents_json_detail_values_are_sanitized_and_bounded() {
        let long_kind = "k".repeat(500);
        let input = format!(
            r#"[{{
              "cwd": "C:\\work\\demo",
              "sessionId": "session-1",
              "state": "working",
              "kind": "{long_kind}\u0007"
            }}]"#
        );

        let observations = normalize_agents_json(&input, &meta(), &redactor()).unwrap();
        let kind = observations[0].state.raw_status_detail.get("kind").unwrap();
        assert!(kind.len() <= 64);
        assert!(kind.chars().all(|ch| !ch.is_control()));
    }

    #[test]
    fn hook_does_not_copy_prompt_or_command() {
        let input = r#"{
          "session_id": "session-123",
          "cwd": "/work/private-project",
          "hook_event_name": "PermissionRequest",
          "tool_name": "Bash",
          "tool_input": {"command": "echo TOP_SECRET"},
          "prompt": "my secret prompt"
        }"#;

        let observation = normalize_hook(input, &meta(), &redactor()).unwrap();
        let serialized = serde_json::to_string(&observation).unwrap();
        assert_eq!(
            observation.state.activity_status,
            ActivityStatus::WaitingPermission
        );
        assert!(!serialized.contains("TOP_SECRET"));
        assert!(!serialized.contains("my secret prompt"));
        assert!(!serialized.contains("/work/private-project"));
    }
}
