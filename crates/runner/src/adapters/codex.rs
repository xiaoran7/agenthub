use std::collections::HashMap;

use chrono::{DateTime, Utc};
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
pub enum CodexAdapterError {
    #[error("invalid Codex JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Codex hook has no session_id")]
    MissingHookSessionId,
    #[error("Codex thread list result has no data array")]
    MissingThreadListData,
}

#[derive(Debug, Clone, Default)]
pub struct CodexThreadContext {
    threads: HashMap<String, ThreadMetadata>,
}

#[derive(Debug, Clone)]
struct ThreadMetadata {
    cwd: String,
}

impl CodexThreadContext {
    pub fn remember(&mut self, thread_id: impl Into<String>, cwd: impl Into<String>) {
        self.threads
            .insert(thread_id.into(), ThreadMetadata { cwd: cwd.into() });
    }

    fn get(&self, thread_id: &str) -> Option<&ThreadMetadata> {
        self.threads.get(thread_id)
    }
}

pub fn normalize_app_server_message(
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
    context: &mut CodexThreadContext,
) -> Result<Option<SessionObservation>, CodexAdapterError> {
    let value: Value = serde_json::from_str(input)?;
    let Some(method) = string_field(&value, &["method"]) else {
        return Ok(None);
    };
    let Some(thread_id) = extract_thread_id(&value) else {
        return Ok(None);
    };

    let cwd = string_field(&value, &["params", "cwd"])
        .or_else(|| string_field(&value, &["params", "thread", "cwd"]))
        .map(str::to_owned)
        .or_else(|| context.get(thread_id).map(|entry| entry.cwd.clone()))
        .unwrap_or_default();
    if !cwd.is_empty() {
        context.remember(thread_id, &cwd);
    }

    let (presence, activity_status, waiting_reason, raw_status) =
        map_app_server_event(method, &value);
    let observation = SessionObservation::new(
        meta,
        AgentType::Codex,
        thread_id,
        redactor.redact(&cwd),
        SessionState {
            presence,
            activity_status,
            raw_status: clamp_raw_status(&raw_status),
            raw_status_detail: detail([
                ("method", Some(sanitize_detail_value(method))),
                (
                    "tool_category",
                    string_field(&value, &["params", "toolName"])
                        .and_then(|name| safe_tool_category(Some(name))),
                ),
            ]),
            waiting_reason,
        },
        ObservationSource::CodexAppServer,
        // A method or status we cannot map is not an authoritative reading.
        if activity_status == ActivityStatus::Unknown {
            Confidence::Inferred
        } else {
            Confidence::Authoritative
        },
    );
    Ok(Some(observation))
}

fn extract_thread_id(value: &Value) -> Option<&str> {
    string_field(value, &["params", "threadId"])
        .or_else(|| string_field(value, &["params", "thread", "id"]))
        .or_else(|| string_field(value, &["params", "turn", "threadId"]))
}

fn map_app_server_event(
    method: &str,
    value: &Value,
) -> (Presence, ActivityStatus, Option<WaitingReason>, String) {
    if method.ends_with("/requestApproval") {
        let waiting_reason = if method.contains("permissions") {
            WaitingReason::Sandbox
        } else {
            WaitingReason::Permission
        };
        return (
            Presence::Live,
            ActivityStatus::WaitingPermission,
            Some(waiting_reason),
            method.to_owned(),
        );
    }

    match method {
        "thread/status/changed" => {
            let status_value = &value["params"]["status"];
            let status = status_value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let (presence, activity, waiting_reason) = map_thread_status(status_value);
            (presence, activity, waiting_reason, status.to_owned())
        }
        "thread/started" => {
            let status_value = &value["params"]["thread"]["status"];
            let status = status_value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("idle");
            let (presence, activity, waiting_reason) = map_thread_status(status_value);
            (presence, activity, waiting_reason, status.to_owned())
        }
        "thread/closed" => (
            Presence::Unknown,
            ActivityStatus::Unknown,
            None,
            "closed".into(),
        ),
        "turn/started" => (
            Presence::Live,
            ActivityStatus::Working,
            None,
            "inProgress".into(),
        ),
        "turn/completed" => {
            let status = string_field(value, &["params", "turn", "status"]).unwrap_or("unknown");
            let activity = match status {
                "completed" => ActivityStatus::Completed,
                "interrupted" => ActivityStatus::Stopped,
                "failed" => ActivityStatus::Failed,
                _ => ActivityStatus::Unknown,
            };
            (Presence::Live, activity, None, status.to_owned())
        }
        "item/tool/requestUserInput" => (
            Presence::Live,
            ActivityStatus::WaitingInput,
            Some(WaitingReason::UserInput),
            method.to_owned(),
        ),
        "mcpServer/elicitation/request" => (
            Presence::Live,
            ActivityStatus::WaitingInput,
            Some(WaitingReason::McpElicitation),
            method.to_owned(),
        ),
        "serverRequest/resolved" | "item/started" | "item/completed" => (
            Presence::Live,
            ActivityStatus::Working,
            None,
            method.to_owned(),
        ),
        _ => (
            Presence::Live,
            ActivityStatus::Unknown,
            None,
            method.to_owned(),
        ),
    }
}

fn map_thread_status(status: &Value) -> (Presence, ActivityStatus, Option<WaitingReason>) {
    let status_type = status
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let waiting_on_approval = status
        .get("activeFlags")
        .and_then(Value::as_array)
        .is_some_and(|flags| {
            flags
                .iter()
                .any(|flag| flag.as_str() == Some("waitingOnApproval"))
        });
    if status_type == "active" && waiting_on_approval {
        return (
            Presence::Live,
            ActivityStatus::WaitingPermission,
            Some(WaitingReason::Permission),
        );
    }

    match status_type {
        "active" => (Presence::Live, ActivityStatus::Working, None),
        "idle" => (Presence::Live, ActivityStatus::Idle, None),
        "systemError" => (Presence::Live, ActivityStatus::Failed, None),
        "notLoaded" => (Presence::Unknown, ActivityStatus::Unknown, None),
        _ => (Presence::Unknown, ActivityStatus::Unknown, None),
    }
}

pub fn normalize_thread_list_result(
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
    context: &mut CodexThreadContext,
) -> Result<Vec<SessionObservation>, CodexAdapterError> {
    let value: Value = serde_json::from_str(input)?;
    let result = value.get("result").unwrap_or(&value);
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or(CodexAdapterError::MissingThreadListData)?;

    let observations = data
        .iter()
        .filter_map(|thread| {
            let thread_id = thread.get("id")?.as_str()?.to_owned();
            // A listing without a cwd (common for notLoaded threads) must not
            // erase a previously learned project; fall back to the remembered
            // one and only remember non-empty values.
            let cwd = thread
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .or_else(|| context.get(&thread_id).map(|entry| entry.cwd.clone()))
                .unwrap_or_default();
            let status_value = thread.get("status").unwrap_or(&Value::Null);
            let status = status_value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("notLoaded");
            let (presence, activity_status, waiting_reason) = map_thread_status(status_value);
            if !cwd.is_empty() {
                context.remember(&thread_id, &cwd);
            }

            let mut observation = SessionObservation::new(
                meta,
                AgentType::Codex,
                thread_id,
                redactor.redact(&cwd),
                SessionState {
                    presence,
                    activity_status,
                    raw_status: clamp_raw_status(status),
                    raw_status_detail: detail([(
                        "source",
                        thread
                            .get("source")
                            .and_then(Value::as_str)
                            .map(sanitize_detail_value),
                    )]),
                    waiting_reason,
                },
                ObservationSource::CodexAppServer,
                // Unmapped status types are readings we cannot vouch for, the
                // same as notLoaded.
                if activity_status == ActivityStatus::Unknown {
                    Confidence::Inferred
                } else {
                    Confidence::Authoritative
                },
            );
            observation.started_at = thread
                .get("createdAt")
                .and_then(Value::as_i64)
                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
            observation.last_activity_at = thread
                .get("recencyAt")
                .or_else(|| thread.get("updatedAt"))
                .and_then(Value::as_i64)
                .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));
            Some(observation)
        })
        .collect();
    Ok(observations)
}

pub fn normalize_hook(
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<SessionObservation, CodexAdapterError> {
    let value: Value = serde_json::from_str(input)?;
    let session_id = string_field(&value, &["session_id"])
        .filter(|id| !id.trim().is_empty())
        .ok_or(CodexAdapterError::MissingHookSessionId)?;
    let cwd = string_field(&value, &["cwd"]).unwrap_or("");
    let event = string_field(&value, &["hook_event_name"]).unwrap_or("Unknown");
    let tool_name = string_field(&value, &["tool_name"]);

    let (presence, activity_status, waiting_reason) = match event {
        "SessionStart" => (Presence::Live, ActivityStatus::Idle, None),
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => {
            (Presence::Live, ActivityStatus::Working, None)
        }
        "PermissionRequest" => (
            Presence::Live,
            ActivityStatus::WaitingPermission,
            Some(WaitingReason::Permission),
        ),
        "Stop" => (Presence::Live, ActivityStatus::Completed, None),
        "SessionEnd" => (Presence::Persisted, ActivityStatus::Unknown, None),
        _ => (Presence::Live, ActivityStatus::Unknown, None),
    };

    Ok(SessionObservation::new(
        meta,
        AgentType::Codex,
        session_id,
        redactor.redact(cwd),
        SessionState {
            presence,
            activity_status,
            raw_status: clamp_raw_status(event),
            raw_status_detail: detail([("tool_category", safe_tool_category(tool_name))]),
            waiting_reason,
        },
        ObservationSource::CodexHook,
        Confidence::Observed,
    ))
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
    fn maps_command_approval_without_copying_command() {
        let input = r#"{
          "method": "item/commandExecution/requestApproval",
          "id": 42,
          "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "cwd": "/work/private-project",
            "command": "echo TOP_SECRET",
            "reason": "Needs network"
          }
        }"#;
        let mut context = CodexThreadContext::default();

        let observation = normalize_app_server_message(input, &meta(), &redactor(), &mut context)
            .unwrap()
            .unwrap();
        let serialized = serde_json::to_string(&observation).unwrap();
        assert_eq!(
            observation.state.activity_status,
            ActivityStatus::WaitingPermission
        );
        assert!(!serialized.contains("TOP_SECRET"));
        assert!(!serialized.contains("/work/private-project"));
    }

    #[test]
    fn not_loaded_is_unknown_not_historical() {
        let input = r#"{
          "data": [{
            "id": "thread-1",
            "cwd": "/work/demo",
            "createdAt": 1700000000,
            "updatedAt": 1700000010,
            "status": {"type": "notLoaded"}
          }]
        }"#;
        let mut context = CodexThreadContext::default();

        let observations =
            normalize_thread_list_result(input, &meta(), &redactor(), &mut context).unwrap();
        assert_eq!(observations[0].state.presence, Presence::Unknown);
        assert_eq!(
            observations[0].state.activity_status,
            ActivityStatus::Unknown
        );
    }

    #[test]
    fn thread_list_without_cwd_keeps_the_remembered_project() {
        let mut context = CodexThreadContext::default();
        context.remember("thread-1", "/work/known-project");
        let input = r#"{
          "data": [{
            "id": "thread-1",
            "status": {"type": "notLoaded"}
          }]
        }"#;

        let observations =
            normalize_thread_list_result(input, &meta(), &redactor(), &mut context).unwrap();
        assert_eq!(observations[0].project.display_name, "known-project");
        assert_eq!(
            context.get("thread-1").map(|entry| entry.cwd.as_str()),
            Some("/work/known-project")
        );
    }

    #[test]
    fn unmapped_thread_status_is_inferred_not_authoritative() {
        let input = r#"{
          "data": [{
            "id": "thread-1",
            "cwd": "/work/demo",
            "status": {"type": "archived"}
          }]
        }"#;
        let mut context = CodexThreadContext::default();

        let observations =
            normalize_thread_list_result(input, &meta(), &redactor(), &mut context).unwrap();
        assert_eq!(observations[0].state.presence, Presence::Unknown);
        assert_eq!(
            observations[0].state.activity_status,
            ActivityStatus::Unknown
        );
        assert_eq!(
            observations[0].confidence,
            crate::domain::Confidence::Inferred
        );
    }

    #[test]
    fn maps_failed_turn() {
        let input = r#"{
          "method": "turn/completed",
          "params": {
            "threadId": "thread-1",
            "turn": {"id": "turn-1", "status": "failed",
              "error": {"message": "sensitive failure output"}}
          }
        }"#;
        let mut context = CodexThreadContext::default();

        let observation = normalize_app_server_message(input, &meta(), &redactor(), &mut context)
            .unwrap()
            .unwrap();
        assert_eq!(observation.state.activity_status, ActivityStatus::Failed);
        assert!(
            !serde_json::to_string(&observation)
                .unwrap()
                .contains("sensitive failure output")
        );
    }

    #[test]
    fn active_waiting_on_approval_is_not_reported_as_working() {
        let input = r#"{
          "data": [{
            "id": "thread-1",
            "cwd": "/work/demo",
            "status": {
              "type": "active",
              "activeFlags": ["waitingOnApproval"]
            }
          }]
        }"#;
        let mut context = CodexThreadContext::default();

        let observations =
            normalize_thread_list_result(input, &meta(), &redactor(), &mut context).unwrap();
        assert_eq!(
            observations[0].state.activity_status,
            ActivityStatus::WaitingPermission
        );
        assert_eq!(
            observations[0].state.waiting_reason,
            Some(WaitingReason::Permission)
        );
    }
}
