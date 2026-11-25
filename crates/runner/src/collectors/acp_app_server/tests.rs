use super::*;

#[test]
fn normalizes_agent_message_chunk_to_delta() {
    let mut prompts = HashMap::new();
    let mut permissions = HashMap::new();
    let mut turns = HashMap::new();
    turns.insert("s-1".to_string(), "t-100".to_string());

    let chunk = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "s-1",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": "Hello ACP"
                }
            }
        }
    });

    let events = normalize_acp_message(chunk, &mut prompts, &mut permissions, &mut turns);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["method"], "item/agentMessage/delta");
    assert_eq!(events[0]["params"]["threadId"], "s-1");
    assert_eq!(events[0]["params"]["turnId"], "t-100");
    assert_eq!(events[0]["params"]["delta"], "Hello ACP");
}

#[test]
fn normalizes_tool_call_lifecycle() {
    let mut prompts = HashMap::new();
    let mut permissions = HashMap::new();
    let mut turns = HashMap::new();
    turns.insert("s-1".to_string(), "t-1".to_string());

    let started = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "s-1",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "call-42",
                "kind": "edit"
            }
        }
    });

    let events = normalize_acp_message(started, &mut prompts, &mut permissions, &mut turns);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["method"], "item/started");
    assert_eq!(events[0]["params"]["item"]["id"], "call-42");
    assert_eq!(events[0]["params"]["item"]["type"], "fileChange");

    let completed = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "s-1",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-42",
                "status": "completed"
            }
        }
    });

    let events = normalize_acp_message(completed, &mut prompts, &mut permissions, &mut turns);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["method"], "item/completed");
    assert_eq!(events[0]["params"]["item"]["id"], "call-42");
    assert_eq!(events[0]["params"]["item"]["status"], "completed");
}

#[test]
fn normalizes_request_permission_with_edit_diff_and_redaction() {
    let mut prompts = HashMap::new();
    let mut permissions = HashMap::new();
    let mut turns = HashMap::new();
    turns.insert("s-1".to_string(), "t-1".to_string());

    let req = json!({
        "jsonrpc": "2.0",
        "id": 105,
        "method": "session/request_permission",
        "params": {
            "sessionId": "s-1",
            "toolCall": {
                "kind": "edit",
                "title": "Edit src/main.rs",
                "rawInput": {
                    "filePath": "src/main.rs",
                    "old_string": "const SECRET: &str = \"/Users/alice/token.txt\";",
                    "new_string": "const SECRET: &str = \"/Users/alice/new_token.txt\";"
                }
            },
            "options": [
                { "optionId": "opt-allow-1", "kind": "allow_once" },
                { "optionId": "opt-reject-1", "kind": "reject_once" }
            ]
        }
    });

    let events = normalize_acp_message(req, &mut prompts, &mut permissions, &mut turns);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["id"], "acp_req_105");
    assert_eq!(events[0]["method"], "item/fileChange/requestApproval");

    let diff = events[0]["params"]["diff"].as_str().expect("diff present");
    assert!(diff.contains("[local_path_redacted]"));
    assert!(!diff.contains("/Users/alice/token.txt"));

    // Options and verbatim JSON-RPC id cached in pending_permissions
    let pending = permissions.get("105").expect("options saved");
    assert_eq!(pending.raw_id, json!(105));
    let opts = &pending.options;
    assert_eq!(opts.len(), 2);
    assert_eq!(opts[0].option_id, "opt-allow-1");

    // Test decision mapping
    let accept_outcome = build_permission_outcome(Some(opts), "accept");
    assert_eq!(accept_outcome["outcome"], "selected");
    assert_eq!(accept_outcome["optionId"], "opt-allow-1");

    let decline_outcome = build_permission_outcome(Some(opts), "decline");
    assert_eq!(decline_outcome["outcome"], "selected");
    assert_eq!(decline_outcome["optionId"], "opt-reject-1");
}

#[test]
fn prompt_response_resolves_turn_completion() {
    let mut prompts = HashMap::new();
    prompts.insert(
        77,
        PendingPromptInfo {
            turn_id: "turn-prompt-1".into(),
            session_id: "session-acp-1".into(),
        },
    );
    let mut permissions = HashMap::new();
    let mut turns = HashMap::new();
    turns.insert("session-acp-1".into(), "turn-prompt-1".into());

    let res = json!({
        "jsonrpc": "2.0",
        "id": 77,
        "result": {
            "stopReason": "end_turn"
        }
    });

    let events = normalize_acp_message(res, &mut prompts, &mut permissions, &mut turns);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["method"], "turn/completed");
    assert_eq!(events[0]["params"]["threadId"], "session-acp-1");
    assert_eq!(events[0]["params"]["turn"]["id"], "turn-prompt-1");
    assert_eq!(events[0]["params"]["turn"]["status"]["type"], "completed");
    assert!(turns.is_empty());
    assert!(prompts.is_empty());
}

#[test]
fn normalizes_usage_update_with_valid_tokens_and_ignores_empty() {
    let mut prompts = HashMap::new();
    let mut permissions = HashMap::new();
    let mut turns = HashMap::new();
    turns.insert("s-1".to_string(), "t-1".to_string());

    let valid_usage = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "s-1",
            "update": {
                "sessionUpdate": "usage_update",
                "used": 15000,
                "size": 200000
            }
        }
    });

    let events = normalize_acp_message(valid_usage, &mut prompts, &mut permissions, &mut turns);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["method"], "thread/tokenUsage/updated");
    assert_eq!(
        events[0]["params"]["tokenUsage"]["last"]["inputTokens"],
        15000
    );
    assert_eq!(
        events[0]["params"]["tokenUsage"]["modelContextWindow"],
        200000
    );

    // Size only without used tokens must not emit an invalid event with null last
    let size_only = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "s-1",
            "update": {
                "sessionUpdate": "usage_update",
                "size": 200000
            }
        }
    });
    let empty_events = normalize_acp_message(size_only, &mut prompts, &mut permissions, &mut turns);
    assert_eq!(empty_events.len(), 0);
}

#[test]
fn detect_acp_gateway_kind_identifies_codex_antigravity_and_claude() {
    assert_eq!(
        detect_acp_gateway_kind(Path::new("/usr/local/bin/codex-acp"), &[]),
        Some(AcpGatewayKind::Codex)
    );
    assert_eq!(
        detect_acp_gateway_kind(Path::new("/usr/local/bin/gemini-acp"), &[]),
        Some(AcpGatewayKind::Antigravity)
    );
    assert_eq!(
        detect_acp_gateway_kind(Path::new("C:/tools/claude-code-acp.exe"), &[]),
        Some(AcpGatewayKind::ClaudeCode)
    );
    assert_eq!(
        detect_acp_gateway_kind(
            Path::new("/usr/local/bin/acp-server"),
            &["--cwd".into(), "/home/user/claude-project".into()]
        ),
        None
    );
    assert_eq!(
        detect_acp_gateway_kind(Path::new("/usr/local/bin/generic-acp"), &["--stdio".into()]),
        None
    );
}

#[test]
fn configure_acp_gateway_env_with_injects_only_matching_profile() {
    let envs_of = |cmd: &Command| -> HashMap<String, String> {
        cmd.get_envs()
            .filter_map(|(k, v)| {
                v.map(|val| {
                    (
                        k.to_string_lossy().into_owned(),
                        val.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect()
    };

    let codex = || {
        Some(crate::claude_gateway::CodexGatewayProfile::new(
            "http://127.0.0.1:8316/v1",
            "sk-codex",
            "test",
            vec![],
        ))
    };
    let anti = || {
        Some(crate::claude_gateway::AntigravityGatewayProfile::new(
            "http://127.0.0.1:8316",
            "sk-anti",
            "test",
            vec![],
        ))
    };
    let claude = || {
        Some(crate::claude_gateway::ClaudeGatewayProfile::new(
            "http://127.0.0.1:8316",
            "sk-claude",
            "test",
            vec![],
        ))
    };

    let mut codex_cmd = Command::new("codex-acp");
    configure_acp_gateway_env_with(
        &mut codex_cmd,
        Path::new("codex-acp"),
        &[],
        codex,
        anti,
        claude,
    );
    let c_env = envs_of(&codex_cmd);
    assert_eq!(
        c_env.get("OPENAI_API_KEY").map(String::as_str),
        Some("sk-codex")
    );
    assert!(!c_env.contains_key("ANTHROPIC_AUTH_TOKEN"));
    assert!(!c_env.contains_key("GEMINI_API_KEY"));

    let mut anti_cmd = Command::new("antigravity-acp");
    configure_acp_gateway_env_with(
        &mut anti_cmd,
        Path::new("antigravity-acp"),
        &[],
        codex,
        anti,
        claude,
    );
    let a_env = envs_of(&anti_cmd);
    assert_eq!(
        a_env.get("GEMINI_API_KEY").map(String::as_str),
        Some("sk-anti")
    );
    assert!(!a_env.contains_key("OPENAI_API_KEY"));

    let mut claude_cmd = Command::new("claude-acp");
    configure_acp_gateway_env_with(
        &mut claude_cmd,
        Path::new("claude-acp"),
        &[],
        codex,
        anti,
        claude,
    );
    let cl_env = envs_of(&claude_cmd);
    assert_eq!(
        cl_env.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
        Some("sk-claude")
    );
    assert!(!cl_env.contains_key("OPENAI_API_KEY"));

    let mut unknown_cmd = Command::new("other-acp");
    configure_acp_gateway_env_with(
        &mut unknown_cmd,
        Path::new("other-acp"),
        &[],
        codex,
        anti,
        claude,
    );
    assert!(envs_of(&unknown_cmd).is_empty());
}
#[test]
fn pending_permissions_are_bounded_and_cleaned_up_on_turn_completed() {
    let mut pending_prompts = HashMap::new();
    let mut pending_permissions = HashMap::new();
    let mut active_turns = HashMap::new();
    active_turns.insert("sess-a".to_owned(), "turn-a".to_owned());
    active_turns.insert("sess-b".to_owned(), "turn-b".to_owned());

    for i in 0..(protocol::MAX_PENDING_PERMISSIONS + 20) {
        let sess = if i.is_multiple_of(2) {
            "sess-a"
        } else {
            "sess-b"
        };
        let req = json!({
            "jsonrpc": "2.0",
            "id": i as u64,
            "method": "session/request_permission",
            "params": {
                "sessionId": sess,
                "toolCall": { "kind": "execute", "title": "run", "rawInput": { "command": "echo hi" } },
                "options": [{ "optionId": "allow", "kind": "allow_once" }]
            }
        });
        let _ = normalize_acp_message(
            req,
            &mut pending_prompts,
            &mut pending_permissions,
            &mut active_turns,
        );
    }
    assert_eq!(pending_permissions.len(), protocol::MAX_PENDING_PERMISSIONS);

    pending_prompts.insert(
        999,
        PendingPromptInfo {
            turn_id: "turn-a".to_owned(),
            session_id: "sess-a".to_owned(),
        },
    );
    let done = json!({
        "jsonrpc": "2.0",
        "id": 999,
        "result": { "stopReason": "end_turn" }
    });
    let out = normalize_acp_message(
        done,
        &mut pending_prompts,
        &mut pending_permissions,
        &mut active_turns,
    );
    assert!(out.iter().any(|v| v["method"] == "turn/completed"));
    assert!(
        pending_permissions
            .values()
            .all(|req| req.session_id != "sess-a"),
        "completed session permissions must be cleaned up"
    );
    assert!(
        pending_permissions
            .values()
            .any(|req| req.session_id == "sess-b"),
        "other active session permissions must remain intact"
    );
}
