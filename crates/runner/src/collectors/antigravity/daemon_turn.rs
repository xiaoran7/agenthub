use std::{
    collections::{HashSet, VecDeque},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{
    AntigravityDaemonEndpoint, call_daemon_rpc, cancel_daemon_cascade, discover_antigravity_homes,
    discover_live_antigravity_daemon, read_antigravity_transcript_metrics,
    resolve_antigravity_context_window, resolve_brain_session_transcript,
    send_daemon_cascade_message, start_daemon_cascade,
};
use crate::{
    collectors::CollectorError,
    conversation::antigravity::{clean_antigravity_tool_output, format_antigravity_tool_details},
};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const STARTUP_IDLE_GRACE: Duration = Duration::from_secs(4);

pub struct DaemonTurnSpec<'a> {
    pub session_id: &'a str,
    pub raw_id: &'a str,
    pub turn_id: &'a str,
    pub cwd: Option<&'a Path>,
    pub text: &'a str,
    pub image_blocks: &'a [Value],
    pub model: Option<&'a str>,
    pub session_exists: Option<bool>,
    pub cancel: &'a AtomicBool,
    pub events_queue: &'a Arc<Mutex<VecDeque<Value>>>,
}

fn set_tool_item_input(item_json: &mut Value, item_type: &str, text: &str) {
    match item_type {
        "commandExecution" => item_json["command"] = json!(text),
        "fileChange" => {
            item_json["diff"] = json!(text);
            item_json["input"] = json!(text);
        }
        _ => item_json["input"] = json!(text),
    }
}

fn push_event(events_queue: &Arc<Mutex<VecDeque<Value>>>, evt: Value) {
    if let Ok(mut q) = events_queue.lock() {
        q.push_back(evt);
    }
}

fn query_step_count_and_status(
    ep: &AntigravityDaemonEndpoint,
    cascade_id: &str,
) -> Option<(u64, String)> {
    let val = call_daemon_rpc(
        ep,
        "GetCascadeTrajectory",
        &json!({
            "cascadeId": cascade_id,
            "disableRehydration": false
        }),
        Duration::from_secs(4),
    )
    .ok()?;
    let total = val
        .get("numTotalSteps")
        .or_else(|| val.get("num_total_steps"))
        .and_then(Value::as_u64)
        .or_else(|| {
            val.pointer("/trajectory/steps")
                .and_then(Value::as_array)
                .map(|a| a.len() as u64)
        })
        .unwrap_or(0);
    let status = val
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Some((total, status))
}

fn fetch_steps_from(
    ep: &AntigravityDaemonEndpoint,
    cascade_id: &str,
    step_offset: u64,
) -> Vec<Value> {
    call_daemon_rpc(
        ep,
        "GetCascadeTrajectorySteps",
        &json!({
            "cascadeId": cascade_id,
            "stepOffset": step_offset,
        }),
        Duration::from_secs(4),
    )
    .ok()
    .and_then(|v| v.get("steps").and_then(Value::as_array).cloned())
    .unwrap_or_default()
}

fn emit_step_events(
    session_id: &str,
    cascade_id: &str,
    turn_id: &str,
    step_idx: u64,
    step: &Value,
    pending_tool_ids: &mut VecDeque<(String, String, Option<String>)>,
    emitted_waiting: &mut HashSet<u64>,
    events_queue: &Arc<Mutex<VecDeque<Value>>>,
) {
    let step_type = step.get("type").and_then(Value::as_str).unwrap_or_default();
    let status = step
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // Surface waiting approvals / user questions to Remote Console & Mobile
    if status.contains("WAITING") && emitted_waiting.insert(step_idx) {
        if let Some(req) = step
            .get("requestedInteraction")
            .or_else(|| step.pointer("/metadata/requestedInteraction"))
        {
            if req.get("runCommand").is_some() {
                let cmd = req
                    .pointer("/runCommand/commandLine")
                    .or_else(|| req.pointer("/runCommand/proposedCommandLine"))
                    .and_then(Value::as_str)
                    .unwrap_or("执行终端命令");
                push_event(
                    events_queue,
                    json!({
                        "id": format!("antigravity_daemon_req_{cascade_id}:{step_idx}:run_command"),
                        "method": "item/commandExecution/requestApproval",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "toolName": "run_command",
                            "command": cmd,
                            "availableDecisions": ["accept", "decline"]
                        }
                    }),
                );
            } else if let Some(ask) = req.get("askQuestion") {
                let prompt = ask
                    .pointer("/questions/0/question")
                    .or_else(|| ask.get("question"))
                    .and_then(Value::as_str)
                    .unwrap_or("Antigravity 请求确认选项");
                let options: Vec<Value> = ask
                    .pointer("/questions/0/options")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|o| {
                                o.as_str()
                                    .or_else(|| o.get("text").and_then(Value::as_str))
                                    .map(|label| json!({ "label": label }))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                push_event(
                    events_queue,
                    json!({
                        "id": format!("antigravity_daemon_req_{cascade_id}:{step_idx}:ask_question"),
                        "method": "item/askUserQuestion",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "prompt": prompt,
                            "options": options
                        }
                    }),
                );
            } else if req.get("permission").is_some() {
                let desc = req
                    .pointer("/permission/message")
                    .or_else(|| req.pointer("/permission/absolutePathUri"))
                    .and_then(Value::as_str)
                    .unwrap_or("需要授权访问受保护路径或工具");
                push_event(
                    events_queue,
                    json!({
                        "id": format!("antigravity_daemon_req_{cascade_id}:{step_idx}:permission"),
                        "method": "item/fileChange/requestApproval",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "toolName": "Permission",
                            "reason": desc,
                            "availableDecisions": ["accept", "decline"]
                        }
                    }),
                );
            }
        }
    }

    if !status.contains("DONE") && !status.contains("ERROR") && !status.contains("FAILED") {
        return;
    }

    if step_type.ends_with("PLANNER_RESPONSE") {
        let pr = step.get("plannerResponse").unwrap_or(step);
        if let Some(thinking) = pr
            .get("thinking")
            .or_else(|| step.get("thinking"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let think_id = format!("agy_think_{step_idx}");
            push_event(
                events_queue,
                json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": session_id,
                        "turnId": turn_id,
                        "itemId": think_id,
                        "item": {
                            "id": think_id,
                            "type": "reasoning",
                            "text": thinking
                        }
                    }
                }),
            );
        }

        if let Some(calls) = pr
            .get("toolCalls")
            .or_else(|| pr.get("tool_calls"))
            .or_else(|| step.get("tool_calls"))
            .and_then(Value::as_array)
        {
            for (c_idx, tc) in calls.iter().enumerate() {
                let name = tc
                    .get("name")
                    .or_else(|| tc.get("toolName"))
                    .or_else(|| tc.pointer("/function/name"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let args = tc
                    .get("args")
                    .or_else(|| tc.get("parameters"))
                    .or_else(|| tc.get("arguments"))
                    .or_else(|| tc.pointer("/function/arguments"));
                let formatted = format_antigravity_tool_details(name, args);
                let tool_id = format!("agy_tool_{step_idx}_{c_idx}");
                let item_type = if matches!(name, "edit_file" | "write_file") {
                    "fileChange"
                } else if name == "run_command" {
                    "commandExecution"
                } else {
                    "toolCall"
                };
                let mut item_json = json!({
                    "id": tool_id,
                    "type": item_type,
                    "name": name,
                    "tool": name,
                    "status": "running",
                });
                set_tool_item_input(&mut item_json, item_type, &formatted);
                push_event(
                    events_queue,
                    json!({
                        "method": "item/started",
                        "params": {
                            "threadId": session_id,
                            "turnId": turn_id,
                            "itemId": tool_id,
                            "item": item_json
                        }
                    }),
                );
                pending_tool_ids.push_back((
                    tool_id,
                    item_type.to_owned(),
                    (!formatted.is_empty()).then_some(formatted),
                ));
            }
        }

        if let Some(reply) = pr
            .get("response")
            .or_else(|| pr.get("modifiedResponse"))
            .or_else(|| pr.get("content"))
            .or_else(|| step.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let msg_id = format!("agy_msg_{step_idx}");
            push_event(
                events_queue,
                json!({
                    "method": "item/agentMessage/delta",
                    "params": {
                        "threadId": session_id,
                        "turnId": turn_id,
                        "itemId": msg_id,
                        "delta": reply
                    }
                }),
            );
            push_event(
                events_queue,
                json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": session_id,
                        "turnId": turn_id,
                        "itemId": msg_id,
                        "item": {
                            "id": msg_id,
                            "type": "agentMessage",
                            "text": reply,
                            "status": "completed"
                        }
                    }
                }),
            );
        }
    } else if step_type.ends_with("GENERIC") {
        let raw_out = step
            .pointer("/generic/content")
            .or_else(|| step.pointer("/generic/output"))
            .or_else(|| step.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (clean_out, failed) = clean_antigravity_tool_output(raw_out);
        let st_str = if failed || status.contains("ERROR") || status.contains("FAILED") {
            "failed"
        } else {
            "completed"
        };
        if let Some((tool_id, item_type, prev_input)) = pending_tool_ids.pop_front() {
            let mut item_json = json!({
                "id": tool_id,
                "type": item_type,
                "status": st_str,
                "output": clean_out,
                "aggregatedOutput": clean_out,
            });
            if let Some(inp) = prev_input {
                set_tool_item_input(&mut item_json, &item_type, &inp);
            }
            push_event(
                events_queue,
                json!({
                    "method": "item/completed",
                    "params": {
                        "threadId": session_id,
                        "turnId": turn_id,
                        "itemId": tool_id,
                        "item": item_json
                    }
                }),
            );
        }
    }
}

/// Runs a full Antigravity turn through the local Antigravity 2.0 `language_server` ConnectRPC
/// daemon, streaming live thoughts, tool calls, diffs, approvals, and replies into `events_queue`.
pub fn run_antigravity_daemon_turn(spec: DaemonTurnSpec<'_>) -> Result<String, CollectorError> {
    let ep = discover_live_antigravity_daemon().ok_or_else(|| CollectorError::Start {
        program: "antigravity-daemon".to_owned(),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Antigravity 2.0 桌面语言服务守护进程未运行",
        ),
    })?;

    let existing_step_count = if spec.session_exists == Some(false) {
        None
    } else {
        query_step_count_and_status(&ep, spec.raw_id).map(|(count, _)| count)
    };

    let (real_raw_id, mut next_step) = match existing_step_count {
        Some(count) => (spec.raw_id.to_owned(), count),
        None => {
            let created = start_daemon_cascade(&ep, spec.cwd, spec.model)?;
            (created, 0_u64)
        }
    };

    send_daemon_cascade_message(&ep, &real_raw_id, spec.text, spec.image_blocks, spec.model)?;

    let started_at = Instant::now();
    let deadline = started_at + Duration::from_secs(600);
    let mut saw_running_or_new_step = false;
    let mut pending_tool_ids: VecDeque<(String, String, Option<String>)> = VecDeque::new();
    let mut emitted_waiting: HashSet<u64> = HashSet::new();

    while Instant::now() < deadline {
        if spec.cancel.load(Ordering::Relaxed) {
            let _ = cancel_daemon_cascade(&real_raw_id);
            return Err(CollectorError::Cancelled {
                program: "antigravity-daemon".to_owned(),
            });
        }

        let steps = fetch_steps_from(&ep, &real_raw_id, next_step);
        let mut progressed_in_pass = false;
        for (offset, step) in steps.iter().enumerate() {
            let idx = next_step + offset as u64;
            let st = step
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            emit_step_events(
                spec.session_id,
                &real_raw_id,
                spec.turn_id,
                idx,
                step,
                &mut pending_tool_ids,
                &mut emitted_waiting,
                spec.events_queue,
            );
            if st.contains("DONE") || st.contains("ERROR") || st.contains("FAILED") {
                if idx == next_step {
                    next_step += 1;
                    saw_running_or_new_step = true;
                    progressed_in_pass = true;
                }
            } else {
                saw_running_or_new_step = true;
                break;
            }
        }

        if let Some((total_steps, status)) = query_step_count_and_status(&ep, &real_raw_id) {
            let status_upper = status.to_ascii_uppercase();
            if status_upper.contains("RUNNING") || status_upper.contains("WAITING") {
                saw_running_or_new_step = true;
            } else if total_steps <= next_step
                && !progressed_in_pass
                && (saw_running_or_new_step || started_at.elapsed() >= STARTUP_IDLE_GRACE)
            {
                break;
            }
        }

        thread::sleep(POLL_INTERVAL);
    }

    // Flush updated token/context usage from on-disk transcript if present
    for ag_home in discover_antigravity_homes() {
        if let Some(tp) = resolve_brain_session_transcript(&ag_home, &real_raw_id)
            && let Ok(metrics) = read_antigravity_transcript_metrics(&tp)
            && metrics.context_used_tokens > 0
        {
            let window = metrics
                .model
                .as_deref()
                .or(spec.model)
                .map(resolve_antigravity_context_window)
                .unwrap_or(1_000_000);
            push_event(
                spec.events_queue,
                json!({
                    "method": "thread/tokenUsage/updated",
                    "params": {
                        "threadId": spec.session_id,
                        "turnId": spec.turn_id,
                        "tokenUsage": {
                            "used": metrics.context_used_tokens,
                            "last": { "inputTokens": metrics.context_used_tokens },
                            "window": window,
                            "modelContextWindow": window
                        }
                    }
                }),
            );
            break;
        }
    }

    Ok(real_raw_id)
}
