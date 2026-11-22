use super::*;
#[cfg(unix)]
use std::fs;
use tempfile::tempdir;

#[test]
fn missing_antigravity_binary_reports_actionable_error_without_panicking() {
    let dir = tempdir().unwrap();
    let mut transport = MultiProviderTransport::new(None);
    transport.antigravity_binary = dir.path().join("nonexistent-agy-bin");
    transport.skip_antigravity_daemon = true;

    let created = transport
        .request(
            "thread/start",
            json!({
                "agentType": "antigravity",
                "cwd": dir.path(),
                "model": "gemini-3.8-flash"
            }),
        )
        .expect("thread/start creates binding metadata");
    let sid = created["thread"]["id"].as_str().unwrap().to_string();
    assert!(sid.starts_with("antigravity_"));
    assert_eq!(
        transport
            .antigravity_cached_models
            .get(&sid)
            .map(String::as_str),
        Some("gemini-3.8-flash")
    );

    let started = transport
        .request(
            "turn/start",
            json!({
                "threadId": sid,
                "cwd": dir.path(),
                "sessionExists": false,
                "input": [{ "type": "text", "text": "hello" }]
            }),
        )
        .expect("turn/start queues async attempt");
    assert!(started["turn"]["id"].as_str().is_some());

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut collected = Vec::new();
    while Instant::now() < deadline {
        collected.extend(transport.drain().unwrap());
        if collected
            .iter()
            .any(|m| m.get("method").and_then(Value::as_str) == Some("turn/completed"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let err_msg = collected
        .iter()
        .find(|m| m.get("method").and_then(Value::as_str) == Some("error"))
        .and_then(|m| m["params"]["error"]["message"].as_str())
        .unwrap_or("");
    assert!(
        err_msg.contains("agy") && err_msg.contains("ANTIGRAVITY_BINARY"),
        "expected actionable missing-CLI hint, got: {err_msg}"
    );
    let done = collected
        .iter()
        .find(|m| m.get("method").and_then(Value::as_str) == Some("turn/completed"))
        .expect("turn/completed emitted");
    assert_eq!(done["params"]["turn"]["status"], "failed");
}

#[cfg(unix)]
#[test]
fn persistent_antigravity_session_supports_multi_turn_approvals_usage_and_interrupt() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let pid_file = dir.path().join("spawn_count.txt");
    let script_path = dir.path().join("mock-agy.sh");
    let script = format!(
        r#"#!/usr/bin/env python3
import json, sys, time

with open({pid_file:?}, "a", encoding="utf-8") as f:
    f.write("spawn\n")

args = sys.argv[1:]
if "--version" in args:
    print("2.35.0 (Antigravity CLI)")
    sys.exit(0)

if "--input-format" not in args:
    sys.exit(2)

print(json.dumps({{"event": "init", "conversation_id": "real-ag-conv-1"}}), flush=True)
turn = 0
for raw in sys.stdin:
    line = raw.strip()
    if not line:
        continue
    msg = json.loads(line)
    if msg.get("event") == "user":
        turn += 1
        content = msg["message"]["content"]
        if "SLEEP" in content:
            print(json.dumps({{"event": "step_update", "step_update": {{"thinking": "waiting before interrupt"}}}}), flush=True)
            time.sleep(10)
        elif turn == 1:
            print(json.dumps({{"event": "step_update", "step_update": {{"thinking": "planning patch"}}}}), flush=True)
            print(json.dumps({{"event": "step_update", "step_update": {{"tool_name": "edit_file", "state": "ACTIVE"}}}}), flush=True)
            print(json.dumps({{
                "event": "control_request",
                "request_id": "perm-1",
                "request": {{
                    "tool_name": "Edit",
                    "input": {{"file_path": "src/app.rs", "diff": "--- /Users/secret/src/app.rs\n+++ b/src/app.rs"}}
                }}
            }}), flush=True)
            resp = json.loads(sys.stdin.readline())
            assert resp["event"] == "control_response" and resp["decision"] == "accept"
            print(json.dumps({{"event": "step_update", "step_update": {{"text_delta": "Turn1: " + content}}}}), flush=True)
            print(json.dumps({{
                "event": "result",
                "result": {{
                    "conversation_id": "real-ag-conv-1",
                    "status": "SUCCESS",
                    "response": "Turn1: " + content,
                    "usage": {{"input_tokens": 300, "cache_read_tokens": 700, "output_tokens": 40}}
                }}
            }}), flush=True)
        else:
            print(json.dumps({{"event": "step_update", "step_update": {{"text_delta": "Turn2: " + content}}}}), flush=True)
            print(json.dumps({{
                "event": "result",
                "result": {{
                    "conversation_id": "real-ag-conv-1",
                    "status": "SUCCESS",
                    "response": "Turn2: " + content,
                    "usage": {{"input_tokens": 500, "cache_read_tokens": 1000, "output_tokens": 50}}
                }}
            }}), flush=True)
"#,
        pid_file = pid_file.to_string_lossy().to_string()
    );
    fs::write(&script_path, script).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let mut transport = MultiProviderTransport::new(None);
    transport.antigravity_binary = script_path.clone();

    let created = transport
        .request(
            "thread/start",
            json!({ "agentType": "antigravity", "cwd": dir.path(), "model": "gemini-3.8-flash" }),
        )
        .unwrap();
    let temp_sid = created["thread"]["id"].as_str().unwrap().to_string();

    transport
        .request(
            "turn/start",
            json!({
                "threadId": temp_sid,
                "cwd": dir.path(),
                "sessionExists": false,
                "permissionMode": "acceptEdits",
                "input": [{ "type": "text", "text": "hello-1" }]
            }),
        )
        .unwrap();

    let drain_until = |tr: &mut MultiProviderTransport, pred: &dyn Fn(&Value) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut all = Vec::new();
        while Instant::now() < deadline {
            let batch = tr.drain().unwrap();
            let hit = batch.iter().any(pred);
            all.extend(batch);
            if hit {
                break;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        all
    };

    let before_approve = drain_until(&mut transport, &|m| {
        m.get("method").and_then(Value::as_str) == Some("item/fileChange/requestApproval")
    });
    assert!(
        before_approve
            .iter()
            .any(|m| m["method"] == "item/reasoning/delta"),
        "missing reasoning delta: {before_approve:?}"
    );
    let req_msg = before_approve
        .iter()
        .find(|m| m["method"] == "item/fileChange/requestApproval")
        .unwrap();
    assert_eq!(req_msg["id"], "antigravity_req_perm-1");
    assert!(
        req_msg["params"]["diff"]
            .as_str()
            .unwrap()
            .contains("[local_path_redacted]")
    );

    transport
        .respond(
            json!("antigravity_req_perm-1"),
            json!({ "decision": "accept" }),
        )
        .unwrap();

    let after_approve = drain_until(&mut transport, &|m| {
        m.get("method").and_then(Value::as_str) == Some("turn/completed")
    });
    assert!(
        after_approve
            .iter()
            .any(|m| m["method"] == "thread/tokenUsage/updated"
                && m["params"]["tokenUsage"]["used"] == 1000),
        "missing token usage update: {after_approve:?}"
    );
    let done1 = after_approve
        .iter()
        .find(|m| m["method"] == "turn/completed")
        .unwrap();
    assert_eq!(done1["params"]["turn"]["status"], "completed");
    assert_eq!(
        done1["params"]["realSessionId"],
        "antigravity_real-ag-conv-1"
    );

    transport
        .request(
            "turn/start",
            json!({
                "threadId": "antigravity_real-ag-conv-1",
                "cwd": dir.path(),
                "permissionMode": "acceptEdits",
                "input": [{ "type": "text", "text": "hello-2" }]
            }),
        )
        .unwrap();
    let turn2_events = drain_until(&mut transport, &|m| {
        m.get("method").and_then(Value::as_str) == Some("turn/completed")
    });
    let done2 = turn2_events
        .iter()
        .find(|m| m["method"] == "turn/completed")
        .unwrap();
    assert_eq!(done2["params"]["turn"]["output"], "Turn2: hello-2");
    let spawn_count = fs::read_to_string(&pid_file).unwrap().lines().count();
    assert_eq!(
        spawn_count, 1,
        "Turn 2 must reuse the live persistent process without respawning"
    );

    let turn3 = transport
        .request(
            "turn/start",
            json!({
                "threadId": "antigravity_real-ag-conv-1",
                "cwd": dir.path(),
                "permissionMode": "acceptEdits",
                "input": [{ "type": "text", "text": "SLEEP" }]
            }),
        )
        .unwrap();
    let turn3_id = turn3["turn"]["id"].as_str().unwrap().to_string();
    let _ = drain_until(&mut transport, &|m| {
        m.get("method").and_then(Value::as_str) == Some("item/reasoning/delta")
    });
    transport
        .request(
            "turn/interrupt",
            json!({ "threadId": "antigravity_real-ag-conv-1" }),
        )
        .unwrap();
    let turn3_events = drain_until(&mut transport, &|m| {
        m.get("method").and_then(Value::as_str) == Some("turn/completed")
    });
    let done3 = turn3_events
        .iter()
        .find(|m| m["method"] == "turn/completed")
        .unwrap();
    assert_eq!(done3["params"]["turn"]["id"], turn3_id);
    assert_eq!(done3["params"]["turn"]["status"], "interrupted");
}

#[test]
fn antigravity_multibyte_utf8_diff_truncates_safely_and_deduplicates_full_message() {
    let mut transport = MultiProviderTransport::new(None);
    let sid = "antigravity_utf8_test";
    let mut messages = Vec::new();

    // Non-ASCII diff exceeding 65536 bytes must never panic at a char boundary.
    let big_cjk_diff = "中".repeat(30_000);
    let approval_ev = json!({
        "event": "control_request",
        "request_id": "req-utf8",
        "request": {
            "tool_name": "edit_file",
            "input": { "file_path": "src/lib.rs", "diff": big_cjk_diff }
        }
    });
    transport.normalize_antigravity_stream_msg(sid, "turn-1", &approval_ev, "", &mut messages);
    let req = messages
        .iter()
        .find(|m| m["method"] == "item/fileChange/requestApproval")
        .expect("approval emitted");
    assert_eq!(
        transport
            .antigravity_pending_requests
            .get("req-utf8")
            .map(String::as_str),
        Some(sid)
    );
    let truncated_diff = req["params"]["diff"].as_str().unwrap();
    assert!(truncated_diff.len() <= 65536);

    // Once text_delta streams in this turn, a summary assistant full-text frame is skipped.
    messages.clear();
    transport.normalize_antigravity_stream_msg(
        sid,
        "turn-1",
        &json!({ "event": "step_update", "step_update": { "text_delta": "hello " } }),
        "",
        &mut messages,
    );
    transport.normalize_antigravity_stream_msg(
        sid,
        "turn-1",
        &json!({ "event": "assistant", "content": "hello world" }),
        "",
        &mut messages,
    );
    let deltas: Vec<_> = messages
        .iter()
        .filter(|m| m["method"] == "item/agentMessage/delta")
        .collect();
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0]["params"]["delta"], "hello ");
}

#[test]
fn antigravity_approval_routing_fails_closed_and_does_not_broadcast_unknown_requests() {
    assert_eq!(
        crate::remote_host::multi_transport::resolve_antigravity_control_decision(&json!({})),
        "decline"
    );
    assert_eq!(
        crate::remote_host::multi_transport::resolve_antigravity_control_decision(&json!({
            "decision": 123
        })),
        "decline"
    );
    assert_eq!(
        crate::remote_host::multi_transport::resolve_antigravity_control_decision(&json!({
            "decision": "accept"
        })),
        "accept"
    );
    assert_eq!(
        crate::remote_host::multi_transport::resolve_antigravity_control_decision(&json!({
            "answers": { "q": "yes" }
        })),
        "answer"
    );

    let mut transport = MultiProviderTransport::new(None);
    transport
        .antigravity_pending_requests
        .insert("known-1".to_string(), "antigravity_sess_a".to_string());

    assert!(
        transport
            .respond(json!("antigravity_req_known-1"), json!({}))
            .is_ok()
    );
    assert!(
        !transport
            .antigravity_pending_requests
            .contains_key("known-1"),
        "handled request id must be removed"
    );
    assert!(
        transport
            .respond(
                json!("antigravity_req_unknown-99"),
                json!({ "decision": "accept" })
            )
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn antigravity_create_turn_interrupt_after_init_still_emits_real_session_id_and_evicts_dead_session()
 {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let script_path = dir.path().join("mock-agy-int.sh");
    let script = r#"#!/usr/bin/env python3
import json, sys, time
if "--version" in sys.argv[1:]:
    print("2.35.0")
    sys.exit(0)
print(json.dumps({"event": "init", "conversation_id": "init-only-conv-7"}), flush=True)
for raw in sys.stdin:
    if raw.strip():
        print(json.dumps({"event": "step_update", "step_update": {"thinking": "working"}}), flush=True)
        time.sleep(10)
"#;
    fs::write(&script_path, script).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let mut transport = MultiProviderTransport::new(None);
    transport.antigravity_binary = script_path;

    let created = transport
        .request(
            "thread/start",
            json!({ "agentType": "antigravity", "cwd": dir.path(), "model": "gemini-3.8-flash" }),
        )
        .unwrap();
    let temp_sid = created["thread"]["id"].as_str().unwrap().to_string();

    transport
        .request(
            "turn/start",
            json!({
                "threadId": temp_sid,
                "cwd": dir.path(),
                "sessionExists": false,
                "input": [{ "type": "text", "text": "hello" }]
            }),
        )
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_reasoning = false;
    while Instant::now() < deadline {
        let msgs = transport.drain().unwrap();
        if msgs.iter().any(|m| m["method"] == "item/reasoning/delta") {
            saw_reasoning = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    assert!(saw_reasoning);

    transport
        .request("turn/interrupt", json!({ "threadId": temp_sid }))
        .unwrap();

    let mut events = Vec::new();
    let deadline2 = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline2 {
        let batch = transport.drain().unwrap();
        let done = batch.iter().any(|m| m["method"] == "turn/completed");
        events.extend(batch);
        if done {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    let done = events
        .iter()
        .find(|m| m["method"] == "turn/completed")
        .expect("turn/completed emitted");
    assert_eq!(done["params"]["turn"]["status"], "interrupted");
    assert_eq!(
        done["params"]["realSessionId"],
        "antigravity_init-only-conv-7"
    );
    assert!(
        !transport
            .antigravity_sessions
            .contains_key("antigravity_init-only-conv-7"),
        "dead interrupted session must be evicted even after key promotion"
    );
    assert!(
        transport
            .antigravity_known_real_ids
            .contains("antigravity_init-only-conv-7")
    );
}
