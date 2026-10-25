use agenthub_runner::remote_host::*;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct State {
    expected: VecDeque<(&'static str, Result<Value, NativeError>)>,
    calls: Vec<(String, Value)>,
    responses: Vec<(Value, Value)>,
    messages: Vec<Value>,
    lost: bool,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);
impl NativeTransport for Fake {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
        let mut state = self.0.lock().unwrap();
        state.calls.push((method.into(), params));
        let (expected, result) = state
            .expected
            .pop_front()
            .expect("unexpected native request");
        assert_eq!(method, expected);
        result
    }
    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        self.0.lock().unwrap().responses.push((id, result));
        Ok(())
    }
    fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
        let mut state = self.0.lock().unwrap();
        if state.lost {
            return Err(NativeError {
                code: "provider_transport_lost".into(),
                outcome_unknown: true,
            });
        }
        Ok(std::mem::take(&mut state.messages))
    }
}
impl Fake {
    fn expect(&self, method: &'static str, result: Value) {
        self.0
            .lock()
            .unwrap()
            .expected
            .push_back((method, Ok(result)));
    }
    fn event(&self, value: Value) {
        self.0.lock().unwrap().messages.push(value);
    }
}

fn setup() -> (tempfile::TempDir, Fake, NativeSessionHost<Fake>) {
    let dir = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let host = NativeSessionHost::new(fake.clone(), vec![dir.path().into()]).unwrap();
    (dir, fake, host)
}

#[test]
fn discovery_only_exports_authorized_metadata_and_never_grants_control() {
    let (dir, fake, mut host) = setup();
    let other = tempfile::tempdir().unwrap();
    fake.expect("thread/list", json!({"data":[
        {"id":"allowed","cwd":dir.path(),"name":"Example","preview":"private preview","turns":["private body"],"status":{"type":"idle"}},
        {"id":"other","cwd":other.path(),"name":"Outside","status":{"type":"idle"}}
    ],"nextCursor":null}));
    let result = host.discover().unwrap();
    assert_eq!(result["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(result["candidates"][0]["control_connected"], false);
    assert_eq!(result["candidates"][1]["control_connected"], false);
    assert!(!result.to_string().contains("private"));
    assert!(host.sessions().is_empty());
}

#[test]
fn discovery_retries_a_malformed_first_page_instead_of_reporting_an_empty_history() {
    let (dir, fake, mut host) = setup();
    fake.expect("thread/list", json!({"error": "temporarily overloaded"}));
    fake.expect("thread/list", json!({"error": "temporarily overloaded"}));
    fake.expect(
        "thread/list",
        json!({"data":[{"id":"allowed","cwd":dir.path(),"name":"Example","status":{"type":"idle"}}],"nextCursor":null}),
    );
    let result = host.discover().unwrap();
    assert_eq!(result["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(result["candidates"][0]["provider_session_id"], "allowed");
}
fn connect(dir: &tempfile::TempDir, hub: &str, generation: &str) -> RemoteCommand {
    let ws_id = agenthub_runner::privacy::workspace_id_for_path(&dir.path().to_string_lossy());
    RemoteCommand {
        client_cmd_id: format!("create-{hub}"),
        hub_session_id: hub.into(),
        host_generation: Some(generation.into()),
        command: RemoteAction::Connect {
            mode: ConnectMode::Create,
            workspace_path: ws_id.into(),
            provider_session_id: None,
            model: None,
            sandbox: CodexSandbox::ReadOnly,
            approval_policy: CodexApprovalPolicy::Untrusted,
            agent_type: None,
            permission_mode: None,
        },
    }
}
fn control(host: &NativeSessionHost<Fake>, id: &str, action: RemoteAction) -> RemoteCommand {
    RemoteCommand {
        client_cmd_id: id.into(),
        hub_session_id: "hub".into(),
        host_generation: Some(host.generation().into()),
        command: action,
    }
}
fn bind(dir: &tempfile::TempDir, fake: &Fake, host: &mut NativeSessionHost<Fake>) {
    fake.expect(
        "thread/start",
        json!({"thread": {"id": "native-real", "status": {"type":"idle"}}}),
    );
    let generation = host.generation().to_owned();
    assert_eq!(
        host.handle_command(connect(dir, "hub", &generation)).status,
        "accepted"
    );
}
fn start_turn(fake: &Fake, host: &mut NativeSessionHost<Fake>, id: &str) {
    fake.expect(
        "turn/start",
        json!({"turn": {"id": id, "status": "inProgress"}}),
    );
    let cmd = control(
        host,
        id,
        RemoteAction::SendPrompt {
            text: "hello".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    assert_eq!(host.handle_command(cmd).status, "accepted");
}

#[test]
fn create_uses_native_identity_and_never_reexecutes_duplicate_commands() {
    let (dir, fake, mut host) = setup();
    let command = connect(&dir, "hub", host.generation());
    fake.expect(
        "thread/start",
        json!({"thread": {"id":"native-real", "status":{"type":"idle"}}}),
    );
    let first = host.handle_command(command.clone());
    assert_eq!(first, host.handle_command(command.clone()));
    assert_eq!(
        first.result.unwrap()["binding"]["provider_session_id"],
        "native-real"
    );
    let mut conflict = command;
    conflict.hub_session_id = "another".into();
    assert_eq!(
        host.handle_command(conflict).code.as_deref(),
        Some("command_id_conflict")
    );
    let state = fake.0.lock().unwrap();
    assert_eq!(state.calls.len(), 1);
    assert_eq!(state.calls[0].1["sandbox"], "read-only");
    assert_eq!(state.calls[0].1["approvalPolicy"], "untrusted");
    assert!(state.calls[0].1.get("model").is_none());
}

#[test]
fn two_turns_share_native_id_and_only_native_completion_releases_busy_state() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-1");
    let busy = control(
        &host,
        "busy",
        RemoteAction::SendPrompt {
            text: "second".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    assert_eq!(
        host.handle_command(busy).code.as_deref(),
        Some("session_busy_or_unknown")
    );
    fake.event(json!({"method":"turn/completed", "params":{"threadId":"native-real", "turn":{"id":"turn-1", "status":"completed"}}}));
    host.poll().unwrap();
    start_turn(&fake, &mut host, "turn-2");
    fake.event(json!({"method":"turn/completed", "params":{"threadId":"native-real", "turn":{"id":"turn-1", "status":"completed"}}}));
    host.poll().unwrap();
    assert_eq!(
        host.sessions()[0].current_turn_id.as_deref(),
        Some("turn-2")
    );
    let state = fake.0.lock().unwrap();
    let turns: Vec<_> = state
        .calls
        .iter()
        .filter(|(method, _)| method == "turn/start")
        .collect();
    assert_eq!(turns.len(), 2);
    assert!(
        turns
            .iter()
            .all(|(_, params)| params["threadId"] == "native-real")
    );
}

#[test]
fn failed_turn_does_not_block_subsequent_turns_and_object_status_transitions_running() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);

    // 1. Start a turn with object status { "type": "inProgress" } (as MultiProviderTransport returns)
    fake.expect(
        "turn/start",
        json!({"turn": {"id": "turn-obj", "status": {"type": "inProgress"}}}),
    );
    let cmd = control(
        &host,
        "turn-obj",
        RemoteAction::SendPrompt {
            text: "hello object status".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    assert_eq!(host.handle_command(cmd).status, "accepted");
    assert_eq!(host.sessions()[0].execution_state, ExecutionState::Running);
    assert_eq!(
        host.sessions()[0].current_turn_id.as_deref(),
        Some("turn-obj")
    );

    // 2. Complete turn with "failed" status
    fake.event(json!({
        "method": "turn/completed",
        "params": {
            "threadId": "native-real",
            "turn": {
                "id": "turn-obj",
                "status": "failed"
            }
        }
    }));
    host.poll().unwrap();
    assert_eq!(host.sessions()[0].execution_state, ExecutionState::Error);
    assert_eq!(host.sessions()[0].current_turn_id, None);

    // 3. Ensure a second turn can be started immediately despite previous Error state
    fake.expect(
        "turn/start",
        json!({"turn": {"id": "turn-retry", "status": {"type": "inProgress"}}}),
    );
    let retry_cmd = control(
        &host,
        "turn-retry",
        RemoteAction::SendPrompt {
            text: "retry after failure".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    assert_eq!(host.handle_command(retry_cmd).status, "accepted");
    assert_eq!(host.sessions()[0].execution_state, ExecutionState::Running);
    assert_eq!(
        host.sessions()[0].current_turn_id.as_deref(),
        Some("turn-retry")
    );

    // 4. Complete with object status { "type": "completed" }
    fake.event(json!({
        "method": "turn/completed",
        "params": {
            "threadId": "native-real",
            "turn": {
                "id": "turn-retry",
                "status": {"type": "completed"}
            }
        }
    }));
    host.poll().unwrap();
    assert_eq!(
        host.sessions()[0].execution_state,
        ExecutionState::Completed
    );
    assert_eq!(host.sessions()[0].current_turn_id, None);
}

#[test]
fn archive_uses_the_bound_native_session_and_rejects_busy_sessions() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    fake.expect("thread/archive", json!({"thread": {"id": "native-real"}}));
    let archive = control(
        &host,
        "archive",
        RemoteAction::SetArchived { archived: true },
    );
    assert_eq!(host.handle_command(archive).status, "accepted");
    assert_eq!(
        fake.0.lock().unwrap().calls.last().unwrap().1["threadId"],
        "native-real"
    );

    start_turn(&fake, &mut host, "turn-active");
    let busy = control(
        &host,
        "unarchive",
        RemoteAction::SetArchived { archived: false },
    );
    assert_eq!(
        host.handle_command(busy).code.as_deref(),
        Some("session_busy_or_unknown")
    );
}

#[test]
fn pending_approval_projection_is_actionable_bounded_and_ephemeral() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-1");
    fake.event(json!({
        "id": 7,
        "method": "item/commandExecution/requestApproval",
        "params": {
            "threadId": "native-real",
            "turnId": "turn-1",
            "command": "private command",
            "availableDecisions": ["decline"]
        }
    }));
    let approval_id = host.poll().unwrap()[0].approval_id.clone().unwrap();
    let digest = approval_id
        .strip_prefix("approval-")
        .expect("opaque approval prefix");
    assert_eq!(digest.len(), 64);
    assert!(
        digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert!(!approval_id.contains("native-real"));
    assert!(!approval_id.ends_with(":7"));
    let approvals = host.pending_approvals();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].approval_id, approval_id);
    assert_eq!(approvals[0].hub_session_id, "hub");
    assert_eq!(approvals[0].host_generation, host.generation());
    assert_eq!(approvals[0].turn_id, "turn-1");
    assert!(!approvals[0].can_approve);
    assert!(approvals[0].can_decline);
    let encoded = serde_json::to_value(&approvals[0]).unwrap();
    for forbidden in ["request_id", "raw", "command", "diff", "path"] {
        assert!(encoded.get(forbidden).is_none());
    }

    let action = RemoteAction::ResolveApproval {
        approval_id,
        approved: false,
    };
    assert_eq!(
        host.handle_command(control(&host, "decline", action))
            .status,
        "accepted"
    );
    assert!(host.pending_approvals().is_empty());

    fake.event(json!({
        "id": 8,
        "method": "item/fileChange/requestApproval",
        "params": {"threadId":"native-real","turnId":"turn-1"}
    }));
    host.poll().unwrap();
    assert_eq!(host.pending_approvals().len(), 1);
    fake.event(json!({"method":"turn/completed","params":{"threadId":"native-real","turn":{"id":"turn-1","status":"completed"}}}));
    host.poll().unwrap();
    assert!(host.pending_approvals().is_empty());

    start_turn(&fake, &mut host, "turn-2");
    fake.event(json!({
        "id": 9,
        "method": "item/fileChange/requestApproval",
        "params": {"threadId":"native-real","turnId":"turn-2"}
    }));
    let approval_id = host.poll().unwrap()[0].approval_id.clone().unwrap();
    assert_eq!(host.pending_approvals().len(), 1);
    fake.event(json!({"method":"serverRequest/resolved","params":{"threadId":"native-real","requestId":9}}));
    let resolved = host.poll().unwrap();
    assert_eq!(resolved[0].kind, "approval_resolved");
    assert_eq!(
        resolved[0].approval_id.as_deref(),
        Some(approval_id.as_str())
    );
    assert!(host.pending_approvals().is_empty());

    fake.event(json!({
        "id": 10,
        "method": "item/fileChange/requestApproval",
        "params": {"threadId":"native-real","turnId":"turn-2"}
    }));
    host.poll().unwrap();
    assert_eq!(host.pending_approvals().len(), 1);
    fake.0.lock().unwrap().lost = true;
    assert!(host.poll().is_err());
    assert!(host.pending_approvals().is_empty());
}

#[test]
fn approval_preserves_request_id_fences_generation_and_rejects_replays() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-1");
    let raw = json!({"id":7,"method":"item/commandExecution/requestApproval", "params":{"threadId":"native-real","turnId":"turn-1","command":"test","availableDecisions":["accept","decline"]}});
    fake.event(raw.clone());
    let events = host.poll().unwrap();
    assert_eq!(events[0].raw, raw);
    let approval_id = events[0].approval_id.clone().unwrap();
    assert_eq!(
        events[0].approval.as_ref().unwrap().approval_id,
        approval_id
    );
    let action = RemoteAction::ResolveApproval {
        approval_id: approval_id.clone(),
        approved: true,
    };
    let mut stale = control(&host, "stale", action.clone());
    stale.host_generation = Some("old".into());
    assert_eq!(
        host.handle_command(stale).code.as_deref(),
        Some("host_generation_mismatch")
    );
    let good = control(&host, "approve", action.clone());
    let receipt = host.handle_command(good.clone());
    assert_eq!(receipt.status, "accepted");
    assert_eq!(receipt, host.handle_command(good));
    assert_eq!(
        host.sessions()[0].execution_state,
        ExecutionState::AwaitingPermission
    );
    fake.event(raw);
    assert!(host.poll().unwrap()[0].approval_id.is_none());
    let repeat = control(&host, "repeat", action);
    assert_eq!(
        host.handle_command(repeat).code.as_deref(),
        Some("approval_not_pending")
    );
    assert_eq!(
        fake.0.lock().unwrap().responses,
        vec![(json!(7), json!({"decision":"accept"}))]
    );
}

#[test]
fn acp_question_and_approval_metadata_flow() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-1");

    // 1. Verify rich approval metadata extraction
    fake.event(json!({
        "id": 101,
        "method": "item/commandExecution/requestApproval",
        "params": {
            "threadId": "native-real",
            "turnId": "turn-1",
            "command": "git push origin main",
            "reason": "Pushing release v2.4.27",
            "availableDecisions": ["accept", "decline"]
        }
    }));
    let events = host.poll().unwrap();
    assert_eq!(events.len(), 1);
    let app = events[0].approval.as_ref().expect("approval snapshot");
    assert_eq!(app.tool_name.as_deref(), Some("Bash"));
    assert_eq!(app.command_preview.as_deref(), Some("git push origin main"));
    assert_eq!(app.description.as_deref(), Some("Pushing release v2.4.27"));
    assert_eq!(app.risk_level.as_deref(), Some("high"));
    assert!(app.can_approve);
    assert!(app.can_decline);

    // Resolve approval
    let resolve_cmd = control(
        &host,
        "res-1",
        RemoteAction::ResolveApproval {
            approval_id: app.approval_id.clone(),
            approved: true,
        },
    );
    assert_eq!(host.handle_command(resolve_cmd).status, "accepted");
    assert_eq!(
        fake.0.lock().unwrap().responses,
        vec![(json!(101), json!({"decision":"accept"}))]
    );
    assert!(host.pending_approvals().is_empty());

    // 2. Verify interactive question flow (ACP ask_question)
    fake.event(json!({
        "id": 102,
        "method": "client/askQuestion",
        "params": {
            "threadId": "native-real",
            "turnId": "turn-1",
            "header": "Approach",
            "question": "Which database adapter to use?",
            "options": [
                {"label": "PostgreSQL (Recommended)", "description": "Production scale"},
                {"label": "SQLite", "description": "Local embedded"}
            ]
        }
    }));
    let events = host.poll().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "question_requested");
    let q = events[0].question.as_ref().expect("question snapshot");
    assert_eq!(q.header.as_deref(), Some("Approach"));
    assert_eq!(q.prompt, "Which database adapter to use?");
    let opts = q.options.as_ref().expect("options");
    assert_eq!(opts.len(), 2);
    assert_eq!(opts[0].label, "PostgreSQL (Recommended)");
    assert_eq!(
        host.sessions()[0].execution_state,
        ExecutionState::AwaitingInput
    );

    // Answer the question
    let answer_cmd = control(
        &host,
        "ans-1",
        RemoteAction::AnswerQuestion {
            question_id: q.question_id.clone(),
            answer: json!("PostgreSQL (Recommended)"),
        },
    );
    assert_eq!(host.handle_command(answer_cmd).status, "accepted");
    assert_eq!(
        fake.0.lock().unwrap().responses.last(),
        Some(&(
            json!(102),
            json!({
                "answers": {
                    "Which database adapter to use?": "PostgreSQL (Recommended)"
                },
                "response": "PostgreSQL (Recommended)"
            })
        ))
    );
    assert!(host.pending_questions().is_empty());

    // 3. Verify tool approval with custom tool name and implicit/omitted turnId
    fake.event(json!({
        "id": 103,
        "method": "item/commandExecution/requestApproval",
        "params": {
            "threadId": "native-real",
            "toolName": "WebFetch",
            "command": "https://api.github.com",
            "reason": "Fetch release notes",
            "availableDecisions": ["accept", "decline"]
        }
    }));
    let events2 = host.poll().unwrap();
    assert_eq!(events2.len(), 1);
    let app2 = events2[0].approval.as_ref().expect("approval snapshot");
    assert_eq!(app2.tool_name.as_deref(), Some("WebFetch"));
    assert_eq!(
        app2.command_preview.as_deref(),
        Some("https://api.github.com")
    );
    assert_eq!(app2.turn_id, "turn-1");
}

#[test]
fn interrupt_keeps_running_until_provider_confirmation() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-1");
    fake.expect("turn/interrupt", json!({}));
    let command = control(&host, "stop", RemoteAction::Interrupt);
    assert_eq!(host.handle_command(command).status, "accepted");
    assert_eq!(host.sessions()[0].execution_state, ExecutionState::Running);
    fake.event(json!({"method":"turn/completed","params":{"threadId":"native-real","turn":{"id":"turn-1","status":"interrupted"}}}));
    host.poll().unwrap();
    assert_eq!(host.sessions()[0].execution_state, ExecutionState::Idle);
    assert!(host.sessions()[0].current_turn_id.is_none());
}

#[test]
fn transport_loss_is_unknown_and_never_automatically_reexecutes_a_prompt() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    fake.0.lock().unwrap().expected.push_back((
        "turn/start",
        Err(NativeError {
            code: "timeout".into(),
            outcome_unknown: true,
        }),
    ));
    let command = control(
        &host,
        "uncertain",
        RemoteAction::SendPrompt {
            text: "change something".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    let receipt = host.handle_command(command.clone());
    assert_eq!(receipt.status, "indeterminate");
    assert_eq!(receipt, host.handle_command(command));
    assert!(!host.sessions()[0].control_connected);
    assert_eq!(host.sessions()[0].execution_state, ExecutionState::Unknown);
    assert!(!host.is_alive());
}

#[test]
fn resume_reads_before_loading_and_preserves_provider_identity() {
    let (dir, fake, mut host) = setup();
    let mut command = connect(&dir, "hub", host.generation());
    if let RemoteAction::Connect {
        mode,
        provider_session_id,
        ..
    } = &mut command.command
    {
        *mode = ConnectMode::Resume;
        *provider_session_id = Some("existing".into());
    }
    fake.expect(
        "thread/read",
        json!({"thread":{"id":"existing","cwd":dir.path(),"status":{"type":"notLoaded"}}}),
    );
    fake.expect(
        "thread/resume",
        json!({"thread":{"id":"existing","status":{"type":"idle"}}}),
    );
    assert_eq!(host.handle_command(command).status, "accepted");
    assert_eq!(host.sessions()[0].provider_session_id, "existing");
    assert_eq!(fake.0.lock().unwrap().calls[1].1["threadId"], "existing");
}

#[test]
fn zcode_multi_provider_transport_routes_sess_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let codex_fake = Fake::default();
    let mut host = NativeSessionHost::new(codex_fake.clone(), vec![dir.path().into()]).unwrap();
    let mut command = connect(&dir, "hub-zcode", host.generation());
    if let RemoteAction::Connect {
        mode,
        provider_session_id,
        ..
    } = &mut command.command
    {
        *mode = ConnectMode::Resume;
        *provider_session_id = Some("sess_mock_test_123".into());
    }
    codex_fake.expect(
        "thread/read",
        json!({"thread":{"id":"sess_mock_test_123","cwd":dir.path(),"status":{"type":"idle"}}}),
    );
    codex_fake.expect(
        "thread/resume",
        json!({"thread":{"id":"sess_mock_test_123","status":{"type":"idle"}}}),
    );
    assert_eq!(host.handle_command(command).status, "accepted");
    assert_eq!(host.sessions()[0].provider_session_id, "sess_mock_test_123");
}

#[test]
fn zcode_interaction_permission_and_user_input_conversion_flow() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-z1");

    // Simulate ZCode interaction/requestPermission translated to approval
    fake.event(json!({
        "id": "zcode_req_42",
        "method": "item/commandExecution/requestApproval",
        "params": {
            "threadId": "native-real",
            "turnId": "turn-z1",
            "command": "Write pelican.svg",
            "filePath": "D:/ClaudeSpace/scratch_zcode_test/pelican.svg",
            "reason": "Draw pelican graphic",
            "availableDecisions": ["accept", "decline"]
        }
    }));
    let events = host.poll().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "approval_requested");
    let approvals = host.pending_approvals();
    assert_eq!(approvals.len(), 1);
    assert_eq!(
        approvals[0].command_preview.as_deref(),
        Some("Write pelican.svg")
    );
    assert_eq!(
        approvals[0].path.as_deref(),
        Some("D:/ClaudeSpace/scratch_zcode_test/pelican.svg")
    );
    assert_eq!(approvals[0].tool_name.as_deref(), Some("Bash"));

    let approval_id = approvals[0].approval_id.clone();
    let resolve_cmd = control(
        &host,
        "accept-zcode-approval",
        RemoteAction::ResolveApproval {
            approval_id: approval_id.clone(),
            approved: true,
        },
    );
    assert_eq!(host.handle_command(resolve_cmd).status, "accepted");
    assert!(host.pending_approvals().is_empty());

    // Simulate ZCode interaction/requestUserInput translated to askQuestion
    fake.event(json!({
        "id": "zcode_req_99",
        "method": "client/askQuestion",
        "params": {
            "threadId": "native-real",
            "turnId": "turn-z1",
            "prompt": "Which framework would you like for the mini website?",
            "header": "Framework",
            "options": [
                {"label": "React", "description": "Interactive SPA"},
                {"label": "Vanilla", "description": "Lightweight HTML"}
            ]
        }
    }));
    let q_events = host.poll().unwrap();
    assert_eq!(q_events.len(), 1);
    assert_eq!(q_events[0].kind, "question_requested");
    let questions = host.pending_questions();
    assert_eq!(questions.len(), 1);
    assert_eq!(
        questions[0].prompt,
        "Which framework would you like for the mini website?"
    );
    assert_eq!(questions[0].options.as_ref().unwrap().len(), 2);
    assert_eq!(questions[0].options.as_ref().unwrap()[0].label, "React");

    let q_id = questions[0].question_id.clone();
    let answer_cmd = control(
        &host,
        "answer-zcode-q",
        RemoteAction::AnswerQuestion {
            question_id: q_id,
            answer: json!({"label": "React"}),
        },
    );
    assert_eq!(host.handle_command(answer_cmd).status, "accepted");
    assert!(host.pending_questions().is_empty());
}

#[test]
fn claude_multi_provider_transport_routes_claude_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let codex_fake = Fake::default();
    let mut host = NativeSessionHost::new(codex_fake.clone(), vec![dir.path().into()]).unwrap();
    let mut command = connect(&dir, "hub-claude", host.generation());
    if let RemoteAction::Connect {
        mode,
        provider_session_id,
        ..
    } = &mut command.command
    {
        *mode = ConnectMode::Resume;
        *provider_session_id = Some("claude_mock_test_123".into());
    }
    codex_fake.expect(
        "thread/read",
        json!({"thread":{"id":"claude_mock_test_123","cwd":dir.path(),"status":{"type":"idle"}}}),
    );
    codex_fake.expect(
        "thread/resume",
        json!({"thread":{"id":"claude_mock_test_123","status":{"type":"idle"}}}),
    );
    assert_eq!(host.handle_command(command).status, "accepted");
    assert_eq!(
        host.sessions()[0].provider_session_id,
        "claude_mock_test_123"
    );
}

#[test]
fn known_active_external_session_is_never_resumed_or_attached() {
    let (dir, fake, mut host) = setup();
    let mut command = connect(&dir, "hub", host.generation());
    if let RemoteAction::Connect {
        mode,
        provider_session_id,
        ..
    } = &mut command.command
    {
        *mode = ConnectMode::Resume;
        *provider_session_id = Some("active".into());
    }
    fake.expect("thread/read",json!({"thread":{"id":"active","cwd":dir.path(),"status":{"type":"active","activeFlags":[]}}}));
    assert_eq!(
        host.handle_command(command.clone()).code.as_deref(),
        Some("external_session_not_resumable")
    );
    command.client_cmd_id = "attach".into();
    if let RemoteAction::Connect { mode, .. } = &mut command.command {
        *mode = ConnectMode::Attach;
    }
    assert_eq!(
        host.handle_command(command).code.as_deref(),
        Some("attach_not_supported")
    );
    assert_eq!(fake.0.lock().unwrap().calls.len(), 1);
}

#[test]
fn missing_native_id_fails_instead_of_fabricating_a_session() {
    let (dir, fake, mut host) = setup();
    fake.expect("thread/start", json!({"thread":{}}));
    let generation = host.generation().to_owned();
    let receipt = host.handle_command(connect(&dir, "hub", &generation));
    assert_eq!(receipt.status, "indeterminate");
    assert!(host.sessions().is_empty());
}

#[test]
fn unauthorized_workspace_is_rejected_before_any_provider_call() {
    let (_, fake, mut host) = setup();
    let other = tempfile::tempdir().unwrap();
    let generation = host.generation().to_owned();
    assert_eq!(
        host.handle_command(connect(&other, "hub", &generation))
            .code
            .as_deref(),
        Some("workspace_not_authorized")
    );
    assert!(fake.0.lock().unwrap().calls.is_empty());
}

#[test]
fn raw_filesystem_path_is_rejected_as_unauthorized_workspace() {
    let (dir, fake, mut host) = setup();
    let mut command = connect(&dir, "hub", host.generation());
    // Submitting raw filesystem path instead of workspace_id must be strictly rejected
    if let RemoteAction::Connect { workspace_path, .. } = &mut command.command {
        *workspace_path = dir.path().to_path_buf();
    }
    assert_eq!(
        host.handle_command(command).code.as_deref(),
        Some("workspace_not_authorized")
    );
    assert!(fake.0.lock().unwrap().calls.is_empty());
}

#[test]
fn file_change_approval_preserves_sanitized_diff() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-file-1");

    let raw_diff = "@@ -1,2 +1,2 @@\n-import secret from \"/Users/alice/config.json\";\n+import secret from \"/Users/alice/new_config.json\";\n";
    fake.event(json!({
        "id": 88,
        "method": "item/fileChange/requestApproval",
        "params": {
            "threadId": "native-real",
            "turnId": "turn-file-1",
            "filePath": "src/config.ts",
            "diff": raw_diff,
            "reason": "Update config import",
            "availableDecisions": ["accept", "decline"]
        }
    }));

    let events = host.poll().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "approval_requested");
    let approval_id = events[0].approval_id.clone().unwrap();

    let approvals = host.pending_approvals();
    assert_eq!(approvals.len(), 1);
    assert_eq!(approvals[0].approval_id, approval_id);
    assert_eq!(approvals[0].tool_name.as_deref(), Some("FileChange"));
    assert_eq!(approvals[0].path.as_deref(), Some("src/config.ts"));
    assert_eq!(approvals[0].risk_level.as_deref(), Some("medium"));
    assert!(approvals[0].can_approve);
    assert!(approvals[0].can_decline);

    let diff = approvals[0].diff.as_ref().expect("diff should be present");
    // Local filesystem paths must be redacted
    assert!(!diff.contains("/Users/alice/config.json"));
    assert!(!diff.contains("/Users/alice/new_config.json"));
    assert!(diff.contains("[local_path_redacted]"));

    let encoded = serde_json::to_value(&approvals[0]).unwrap();
    assert!(encoded.get("diff").is_some());
    for forbidden in ["request_id", "raw"] {
        assert!(encoded.get(forbidden).is_none());
    }

    let action = RemoteAction::ResolveApproval {
        approval_id,
        approved: true,
    };
    assert_eq!(
        host.handle_command(control(&host, "approve-file", action))
            .status,
        "accepted"
    );
    assert!(host.pending_approvals().is_empty());
}

#[test]
fn unknown_notifications_are_retained_without_inventing_execution_state() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    let raw =
        json!({"method":"future/event","params":{"threadId":"native-real","new_field":"payload"}});
    fake.event(raw.clone());
    let events = host.poll().unwrap();
    assert_eq!(events[0].raw, raw);
    assert_eq!(
        events[0].binding.as_ref().unwrap().execution_state,
        ExecutionState::Idle
    );
}

#[test]
fn stale_approval_after_turn_completion_cannot_be_answered() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-1");
    fake.event(json!({"id":"request-1","method":"item/fileChange/requestApproval","params":{"threadId":"native-real","turnId":"turn-1"}}));
    let approval_id = host.poll().unwrap()[0].approval_id.clone().unwrap();
    fake.event(json!({"method":"turn/completed","params":{"threadId":"native-real","turn":{"id":"turn-1","status":"completed"}}}));
    host.poll().unwrap();
    let command = control(
        &host,
        "approve-late",
        RemoteAction::ResolveApproval {
            approval_id,
            approved: true,
        },
    );
    assert_eq!(
        host.handle_command(command).code.as_deref(),
        Some("approval_not_pending")
    );
    assert!(fake.0.lock().unwrap().responses.is_empty());
}

#[test]
fn model_catalog_is_returned_from_native_protocol_without_fallback() {
    let (_, fake, mut host) = setup();
    let result = json!({"data":[{"id":"native-dynamic-model"}],"nextCursor":null});
    fake.expect("model/list", result.clone());
    let models = host.models().unwrap();
    // `data` stays the flat list the console has always read, and the
    // app-server's answer is Codex's own catalog. The `agents` map also
    // carries whatever engines this machine's config files describe, so the
    // assertions are on the one entry the fake supplied rather than on the
    // whole map, which is host-dependent by design.
    assert_eq!(
        models["agents"]["codex"],
        json!([{"id":"native-dynamic-model"}])
    );
    assert_eq!(models["data"][0], json!({"id":"native-dynamic-model"}));
    // Nothing invented: every id in `data` also appears in one of the
    // per-engine lists, and an engine with no catalog is simply absent.
    let data = models["data"].as_array().unwrap();
    let listed: Vec<&Value> = models["agents"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|list| list.as_array().unwrap())
        .collect();
    for model in data {
        assert!(listed.contains(&model), "unattributed model {model}");
    }
}

#[test]
fn unknown_slash_does_not_fall_through_to_prompt() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    let command = control(
        &host,
        "slash",
        RemoteAction::SendPrompt {
            text: "/compact".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    assert_eq!(
        host.handle_command(command).code.as_deref(),
        Some("slash_not_supported")
    );
    assert_eq!(fake.0.lock().unwrap().calls.len(), 1);
}

#[test]
fn a_full_receipt_ledger_evicts_oldest_entries_instead_of_rejecting_new_commands() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);

    // Push the ledger past its bound with commands the Host answers itself, so
    // a long-lived daemon never reaches a state where it rejects every new
    // command with `command_ledger_full`.
    let generation: String = host.generation().into();
    let archive = |id: &str| RemoteCommand {
        client_cmd_id: id.into(),
        hub_session_id: "hub".into(),
        host_generation: Some(generation.clone()),
        command: RemoteAction::SetArchived { archived: true },
    };
    for index in 0..(MAX_COMMAND_LEDGER_ENTRIES + 8) {
        fake.expect("thread/archive", json!({"ok": true}));
        let receipt = host.handle_command(archive(&format!("cmd-{index}")));
        assert_ne!(
            receipt.code.as_deref(),
            Some("command_ledger_full"),
            "command {index} was rejected by a full ledger"
        );
        assert_eq!(receipt.status, "accepted");
    }

    // The oldest id is gone from the in-memory window, so its repeat is no
    // longer short-circuited here. `remote_journal` is what keeps that from
    // re-executing in production; this asserts only that eviction happened.
    fake.expect("thread/archive", json!({"ok": true}));
    host.handle_command(archive("cmd-0"));

    // A repeat inside the window is still deduplicated without re-executing.
    fake.expect("thread/archive", json!({"ok": true}));
    let first = host.handle_command(archive("cmd-3"));
    assert_eq!(first.status, "accepted");
    let calls_before = fake.0.lock().unwrap().calls.len();
    let repeat = host.handle_command(archive("cmd-3"));
    assert_eq!(repeat, first);
    assert_eq!(fake.0.lock().unwrap().calls.len(), calls_before);
}

fn question(id: usize, prompt: &str) -> Value {
    json!({
        "id": id,
        "method": "item/askUserQuestion",
        "params": {"threadId":"native-real","turnId":"turn-1","prompt":prompt}
    })
}

#[test]
fn answered_request_suppression_is_bounded_and_still_deduplicates_within_the_window() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);
    start_turn(&fake, &mut host, "turn-1");
    let generation: String = host.generation().into();
    let answer = |id: &str, question_id: &str| RemoteCommand {
        client_cmd_id: id.into(),
        hub_session_id: "hub".into(),
        host_generation: Some(generation.clone()),
        command: RemoteAction::AnswerQuestion {
            question_id: question_id.into(),
            answer: "yes".into(),
        },
    };

    // Answer more requests than the suppression window holds, so the oldest ids
    // are evicted instead of the set growing for the life of the daemon.
    for index in 0..(MAX_ANSWERED_REQUESTS + 4) {
        fake.event(question(1000 + index, &format!("q-{index}")));
        let question_id = host.poll().unwrap()[0].question_id.clone().unwrap();
        assert_eq!(
            host.handle_command(answer(&format!("answer-{index}"), &question_id))
                .status,
            "accepted"
        );
    }

    // A re-delivery of a request answered inside the window is still
    // suppressed, so the operator is not asked the same question twice.
    fake.event(question(50_000, "recent"));
    let question_id = host.poll().unwrap()[0].question_id.clone().unwrap();
    assert_eq!(
        host.handle_command(answer("answer-recent", &question_id))
            .status,
        "accepted"
    );
    fake.event(question(50_000, "recent"));
    assert!(
        host.poll().unwrap()[0].question_id.is_none(),
        "a re-delivered answered request raised a second question"
    );

    // The id from the very first request fell out of the window, so it is
    // offered again rather than silently dropped.
    fake.event(question(1000, "q-0"));
    assert!(
        host.poll().unwrap()[0].question_id.is_some(),
        "an evicted request id was still suppressed"
    );
}

#[test]
fn first_turn_of_newly_created_session_passes_session_exists_false_and_subsequent_turns_do_not() {
    let (dir, fake, mut host) = setup();
    bind(&dir, &fake, &mut host);

    // Turn 1: should include "sessionExists": false because it was created fresh
    fake.expect(
        "turn/start",
        json!({"turn": {"id": "turn-1", "status": "inProgress"}}),
    );
    let cmd1 = control(
        &host,
        "turn-1",
        RemoteAction::SendPrompt {
            text: "hello 1".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    assert_eq!(host.handle_command(cmd1).status, "accepted");
    {
        let calls = &fake.0.lock().unwrap().calls;
        let turn_start_1 = calls.iter().find(|(m, _)| m == "turn/start").unwrap();
        assert_eq!(turn_start_1.1["sessionExists"], json!(false));
    }

    // Complete turn 1
    fake.event(json!({
        "method": "turn/completed",
        "params": {
            "threadId": "native-real",
            "turn": { "id": "turn-1", "status": "completed" }
        }
    }));
    let _ = host.poll();

    // Turn 2: should NOT include "sessionExists" flag (it is now an existing session)
    fake.expect(
        "turn/start",
        json!({"turn": {"id": "turn-2", "status": "inProgress"}}),
    );
    let cmd2 = control(
        &host,
        "turn-2",
        RemoteAction::SendPrompt {
            text: "hello 2".into(),
            effort: None,
            model: None,
            permission_mode: None,
            attachments: Vec::new(),
        },
    );
    assert_eq!(host.handle_command(cmd2).status, "accepted");
    {
        let calls = &fake.0.lock().unwrap().calls;
        let turn_starts: Vec<_> = calls.iter().filter(|(m, _)| m == "turn/start").collect();
        assert_eq!(turn_starts.len(), 2);
        assert!(turn_starts[1].1.get("sessionExists").is_none());
    }
}

#[test]
fn acp_session_routing_in_multiprovider_transport() {
    let mut transport = MultiProviderTransport::new(None, None);

    // 1. thread/read for an ACP session returns an idle thread with its session id
    let read_res = transport
        .request("thread/read", json!({ "threadId": "acp_session_test_1" }))
        .expect("thread/read should succeed");
    assert_eq!(read_res["thread"]["id"], "acp_session_test_1");
    assert_eq!(read_res["thread"]["status"]["type"], "idle");

    // 2. thread/resume for an ACP session returns the session
    let resume_res = transport
        .request("thread/resume", json!({ "threadId": "acp_session_test_1" }))
        .expect("thread/resume should succeed");
    assert_eq!(resume_res["thread"]["id"], "acp_session_test_1");
    assert_eq!(resume_res["thread"]["status"]["type"], "idle");

    // 3. thread/archive and thread/unarchive succeed for an ACP session
    assert!(
        transport
            .request(
                "thread/archive",
                json!({ "threadId": "acp_session_test_1" })
            )
            .is_ok()
    );
    assert!(
        transport
            .request(
                "thread/unarchive",
                json!({ "threadId": "acp_session_test_1" })
            )
            .is_ok()
    );

    // 4. turn/interrupt for an ACP session succeeds
    assert!(
        transport
            .request(
                "turn/interrupt",
                json!({ "threadId": "acp_session_test_1" })
            )
            .is_ok()
    );
}
