use super::*;

#[test]
fn session_id_validation_rejects_paths() {
    assert!(validate_session_id("019abc-session_1").is_ok());
    assert!(validate_session_id("../auth.json").is_err());
    assert!(validate_session_id("bad\nvalue").is_err());
}

#[test]
fn tool_call_label_extracts_key_arguments_and_questions() {
    let bash_input = json!({"command": "cargo test -p runner"});
    assert_eq!(
        tool_call_label("Bash", Some(&bash_input)),
        "Bash: cargo test -p runner"
    );

    let question_input = json!({
        "questions": [
            {
                "question": "Which framework to use?",
                "header": "Framework"
            }
        ]
    });
    assert_eq!(
        tool_call_label("AskUserQuestion", Some(&question_input)),
        "AskUserQuestion: Which framework to use?"
    );

    let todo_input = json!({
        "todos": ["task 1", "task 2", "task 3"]
    });
    assert_eq!(
        tool_call_label("TodoWrite", Some(&todo_input)),
        "TodoWrite: 3 项待办"
    );
}

#[test]
fn local_conversation_parser_supports_provider_native_message_shapes() {
    let claude = json!({"type":"user","message":{"content":[{"type":"text","text":"你好"}]}});
    let codex = json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"已完成"}]}});
    let antigravity =
        json!({"type":"USER_INPUT","content":"<USER_REQUEST>修复乱码</USER_REQUEST>"});

    assert_eq!(
        local_conversation_message(&claude),
        Some(("user", "你好".into()))
    );
    assert_eq!(
        local_conversation_message(&codex),
        Some(("assistant", "已完成".into()))
    );
    assert_eq!(
        local_conversation_message(&antigravity),
        Some(("user", "<USER_REQUEST>修复乱码</USER_REQUEST>".into()))
    );
}

#[test]
fn compact_summary_never_reaches_the_conversation() {
    // Exactly what Claude Code writes when it auto-compacts: a `user`
    // record holding the condensed history of everything said so far.
    let summary = json!({
        "type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,
        "turnOrigin":"sdk","message":{"role":"user","content":
        "This session is being continued from a previous conversation that ran out of context.\n\nSummary:\n1. Primary Request and Intent:\n   - SECRET_CONDENSED_HISTORY"}
    });
    let boundary = json!({
        "type":"system","subtype":"compact_boundary","level":"info",
        "content":"Conversation compacted",
        "compactMetadata":{"trigger":"auto","preTokens":177701,"postTokens":14111,
            "cumulativeDroppedTokens":163590,"durationMs":104470}
    });

    assert!(normalize_local_conversation_items(&summary).is_empty());

    // The boundary itself is a status row, not leaked content.
    let items = normalize_local_conversation_items(&boundary);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "status");
    assert_eq!(items[0]["role"], "system");
    assert_eq!(items[0]["label"], "上下文压缩");
    assert!(items[0]["content"].as_str().unwrap().contains("163k"));
    assert!(
        !items[0]["content"]
            .as_str()
            .unwrap()
            .contains("SECRET_CONDENSED_HISTORY")
    );

    // A boundary without a reported token count still renders.
    let bare =
        json!({"type":"system","subtype":"compact_boundary","content":"Conversation compacted"});
    assert_eq!(normalize_local_conversation_items(&bare).len(), 1);

    // An ordinary user turn is unaffected by the filter.
    let prompt = json!({"type":"user","message":{"content":"你好"}});
    assert_eq!(normalize_local_conversation_items(&prompt).len(), 1);
}

#[test]
fn token_counts_format_like_the_official_client() {
    assert_eq!(format_token_count(999), "999");
    assert_eq!(format_token_count(1_000), "1k");
    assert_eq!(format_token_count(163_590), "163k");
    assert_eq!(format_token_count(1_500_000), "1.5M");
}

#[test]
fn local_conversation_normalizer_filters_control_markup_and_keeps_tools() {
    let caveat = json!({"type":"user","message":{"content":"<local-command-caveat>internal only</local-command-caveat>"}});
    let command = json!({"type":"user","message":{"content":"<command-name>/compact</command-name><command-message>compact</command-message>"}});
    let claude = json!({"type":"assistant","message":{"content":[
        {"type":"text","text":"- 完成一项\n- 完成二项"},
        {"type":"tool_use","name":"Read","input":{"file_path":"README.md"}}
    ]}});
    let codex = json!({"type":"response_item","payload":{"type":"command_execution","command":"cargo test","status":"completed","aggregated_output":"ok"}});

    assert!(normalize_local_conversation_items(&caveat).is_empty());
    assert_eq!(
        normalize_local_conversation_items(&command)[0]["kind"],
        "command"
    );
    let claude_items = normalize_local_conversation_items(&claude);
    assert_eq!(claude_items[0]["kind"], "message");
    assert_eq!(claude_items[1]["kind"], "tool");
    assert_eq!(
        normalize_local_conversation_items(&codex)[0]["label"],
        "cargo test"
    );
}

#[test]
fn local_text_decoder_handles_utf8_and_utf16_without_mojibake() {
    assert_eq!(decode_local_text("会话内容".as_bytes()), "会话内容");
    let mut utf16 = vec![0xff, 0xfe];
    for unit in "会话内容".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    assert_eq!(decode_local_text(&utf16), "会话内容");
    let utf16_without_bom = "session 内容"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(decode_local_text(&utf16_without_bom), "session 内容");
}

#[test]
fn load_session_conversation_caps_messages_and_returns_empty_without_transcript() {
    let home = tempfile::tempdir().unwrap();
    let codex_dir = home.path().join(".codex").join("sessions");
    fs::create_dir_all(&codex_dir).unwrap();
    let mut rollout = String::new();
    for index in 0..400 {
        rollout.push_str(&format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"msg-{index}\"}}]}}}}\n"
        ));
    }
    fs::write(codex_dir.join("thread-abc.jsonl"), rollout).unwrap();

    let messages = load_session_conversation(home.path(), AgentType::Codex, "thread-abc");
    assert_eq!(messages.len(), 300);
    // Head-and-tail budgeting retains the initial head (msg-0) and latest tail (msg-399)
    assert_eq!(messages[0]["content"], "msg-0");
    assert_eq!(messages[299]["content"], "msg-399");
    assert!(load_session_conversation(home.path(), AgentType::Codex, "missing").is_empty());
    assert!(load_session_conversation(home.path(), AgentType::Codex, "../escape").is_empty());
}

#[test]
fn load_session_conversation_stays_inside_one_ipc_frame() {
    let home = tempfile::tempdir().unwrap();
    let codex_dir = home.path().join(".codex").join("sessions");
    fs::create_dir_all(&codex_dir).unwrap();
    let mut rollout = String::new();
    for _ in 0..40 {
        rollout.push_str(&format!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"{}\"}}]}}}}\n",
            "x".repeat(60_000)
        ));
    }
    fs::write(codex_dir.join("thread-big.jsonl"), rollout).unwrap();

    let messages = load_session_conversation(home.path(), AgentType::Codex, "thread-big");
    let approx: usize = messages
        .iter()
        .map(|item| item["content"].as_str().map(str::len).unwrap_or_default() + 128)
        .sum();
    assert!(messages.len() < 40);
    assert!(approx < 600 * 1024);
    // Head item is preserved even when the transcript is huge
    assert!(!messages.is_empty());
    assert_eq!(messages[0]["role"], "assistant");
}

#[test]
fn load_session_conversation_claude_preserves_opening_prompt() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home.path().join(".claude").join("projects").join("proj-1");
    fs::create_dir_all(&claude_dir).unwrap();

    let mut lines = Vec::new();
    // Opening prompt
    lines.push(
        r#"{"type":"user","message":{"role":"user","content":"请开始重构代码架构"}}"#.to_string(),
    );
    // Lots of tool calls and outputs in between
    for i in 0..100 {
        lines.push(format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"call_{i}","name":"Read","input":{{"file_path":"src/lib.rs"}}}}]}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"call_{i}","content":"{}"}}]}}}}"#,
            "output_data_".repeat(200)
        ));
    }
    // Latest assistant turn
    lines.push(r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"重构已顺利完成！"}]}}"#.to_string());

    fs::write(claude_dir.join("sess-test.jsonl"), lines.join("\n")).unwrap();

    let messages = load_session_conversation(home.path(), AgentType::ClaudeCode, "sess-test");
    assert!(!messages.is_empty());
    // Opening prompt is ALWAYS preserved at the head!
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "请开始重构代码架构");
    // Latest turn is at the tail
    assert_eq!(messages.last().unwrap()["role"], "assistant");
    assert_eq!(messages.last().unwrap()["content"], "重构已顺利完成！");
}

#[test]
fn clean_message_text_filters_codex_and_zcode_system_prompts() {
    let plugins = "<recommended_plugins>\n- Airtable\n- Granola\n</recommended_plugins>";
    assert!(clean_message_text(plugins).is_empty());

    let agents_md = "# AGENTS.md instructions for D:\\Claudespace\n\n<INSTRUCTIONS>\n遵循 AGENT.md\n</INSTRUCTIONS>";
    assert!(clean_message_text(agents_md).is_empty());

    let env_ctx = "<environment_context>\n  <cwd>D:\\Claudespace</cwd>\n  <shell>powershell</shell>\n  <timezone>Asia/Shanghai</timezone>\n</environment_context>";
    assert!(clean_message_text(env_ctx).is_empty());

    let mixed = format!("{plugins}\n\n{agents_md}\n\n{env_ctx}\n\n真实用户提问：请修复Bug");
    assert_eq!(clean_message_text(&mixed), "真实用户提问：请修复Bug");

    let zcode_reminder = "<system-reminder>\nContinue working toward the active session goal.\n<untrusted_objective>\n任务\n</untrusted_objective>\nBudget:\n- Time spent pursuing goal: 0 seconds\n</system-reminder>";
    assert!(clean_message_text(zcode_reminder).is_empty());

    let zcode_tool_inject = "Called the Read tool with the following input: {\"file_path\":\"test.rs\"}\nResult of calling the Read tool:\n1\tfn main() {}";
    assert!(clean_message_text(zcode_tool_inject).is_empty());
}

#[test]
fn find_provider_transcript_prefers_latest_modified_continuation_file() {
    let home = tempfile::tempdir().unwrap();
    let codex_dir = home.path().join(".codex").join("sessions");
    let day1 = codex_dir.join("2026").join("09").join("15");
    let day2 = codex_dir.join("2026").join("09").join("17");
    fs::create_dir_all(&day1).unwrap();
    fs::create_dir_all(&day2).unwrap();

    let session_id = "01a0a555-5821-71b2-9ce6-efffaffccf8d";
    let file1 = day1.join(format!("rollout-2026-09-15T21-50-28-{session_id}.jsonl"));
    let file2 = day2.join(format!(
        "rollout-2026-09-17T10-18-25-{session_id}_continuation.jsonl"
    ));

    fs::write(&file1, "{\"type\":\"first\"}\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&file2, "{\"type\":\"latest\"}\n").unwrap();

    let matched = find_provider_transcript(&home.path().join(".codex"), session_id, 0);
    assert_eq!(matched, Some(file2));
}

#[test]
fn synthetic_error_and_placeholder_messages_are_not_rendered_as_assistant_bubbles() {
    let api_error = json!({
        "type": "assistant",
        "error": "rate_limit",
        "isApiErrorMessage": true,
        "apiErrorStatus": 429,
        "message": {
            "model": "<synthetic>",
            "role": "assistant",
            "content": [{"type": "text", "text": "API Error: Request rejected (429) · Resource has been exhausted."}]
        }
    });

    let placeholder = json!({
        "type": "assistant",
        "isApiErrorMessage": false,
        "message": {
            "model": "<synthetic>",
            "role": "assistant",
            "content": [{"type": "text", "text": "No response requested."}]
        }
    });

    let err_items = normalize_local_conversation_items(&api_error);
    assert_eq!(err_items.len(), 1);
    assert_eq!(err_items[0]["role"], "system");
    assert_eq!(err_items[0]["kind"], "status");
    assert_eq!(err_items[0]["status"], "failed");
    assert_eq!(err_items[0]["label"], "API 异常记录");
    assert!(err_items[0]["content"].as_str().unwrap().contains("429"));

    let placeholder_items = normalize_local_conversation_items(&placeholder);
    assert!(placeholder_items.is_empty());
}

#[test]
fn test_real_claude_sessions_loading() {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return;
    };
    let entries = crate::collectors::collect_claude_sessions_for_remote().unwrap();
    if entries.is_empty() {
        println!("No claude sessions found in environment, skipping test");
        return;
    }
    println!("Collected {} claude sessions for remote", entries.len());
    let mut empty_count = 0;
    let mut ok_count = 0;
    for entry in entries.iter().take(15) {
        let prefixed = format!("claude_{}", entry.session_id);
        let messages = load_session_conversation(&home, AgentType::ClaudeCode, &prefixed);
        if messages.is_empty() {
            empty_count += 1;
        } else {
            ok_count += 1;
        }
    }
    println!("WITH PREFIXED ID: ok={}, empty={}", ok_count, empty_count);
    assert!(
        ok_count > 0,
        "Prefixed session IDs must resolve successfully when sessions exist!"
    );
}

#[test]
fn load_session_conversation_deduplicates_resume_replay_by_uuid() {
    let home = tempfile::tempdir().unwrap();
    let claude_dir = home
        .path()
        .join(".claude")
        .join("projects")
        .join("proj-dedup");
    fs::create_dir_all(&claude_dir).unwrap();

    let lines = [
        // Original prompt
        r#"{"uuid":"msg-u1","type":"user","message":{"role":"user","content":"第一次提问"}}"#,
        // Original reply
        r#"{"uuid":"msg-a1","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"第一次回复"}]}}"#,
        // Claude resume replay (same lines appended again with identical uuids)
        r#"{"uuid":"msg-u1","type":"user","message":{"role":"user","content":"第一次提问"}}"#,
        r#"{"uuid":"msg-a1","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"第一次回复"}]}}"#,
        // Second turn
        r#"{"uuid":"msg-u2","type":"user","message":{"role":"user","content":"第二次提问"}}"#,
        r#"{"uuid":"msg-a2","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"第二次回复"}]}}"#,
    ];

    fs::write(claude_dir.join("sess-dedup.jsonl"), lines.join("\n")).unwrap();

    let messages = load_session_conversation(home.path(), AgentType::ClaudeCode, "sess-dedup");
    // Duplicates should be filtered out; exactly 4 unique messages should remain.
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0]["content"], "第一次提问");
    assert_eq!(messages[1]["content"], "第一次回复");
    assert_eq!(messages[2]["content"], "第二次提问");
    assert_eq!(messages[3]["content"], "第二次回复");
}

#[test]
fn test_sanitize_claude_project_dir_and_workspace_lookup() {
    let win_path = Path::new(r"D:\Claudespace\Project\agenthub");
    let unix_path = Path::new("/home/user/code/agenthub");

    assert_eq!(
        sanitize_claude_project_dir(win_path),
        "D--Claudespace-Project-agenthub"
    );
    assert_eq!(
        sanitize_claude_project_dir(unix_path),
        "-home-user-code-agenthub"
    );

    let home = tempfile::tempdir().unwrap();
    let proj_dir = home
        .path()
        .join(".claude")
        .join("projects")
        .join("D--Claudespace-Project-agenthub");
    fs::create_dir_all(&proj_dir).unwrap();
    let target_file = proj_dir.join("test-sess-123.jsonl");
    fs::write(&target_file, "{\"type\":\"user\"}\n").unwrap();

    let resolved = find_claude_workspace_transcript(home.path(), win_path, "test-sess-123");
    assert_eq!(resolved, Some(target_file));
}
#[test]
fn antigravity_native_steps_render_thinking_tools_clean_user_and_suppress_system() {
    let user = json!({
        "step_index": 0,
        "source": "USER_EXPLICIT",
        "type": "USER_INPUT",
        "content": "<ADDITIONAL_METADATA>ignored</ADDITIONAL_METADATA><USER_REQUEST>优化 Antigravity 支持</USER_REQUEST>"
    });
    let user_items = normalize_local_conversation_items(&user);
    assert_eq!(user_items.len(), 1);
    assert_eq!(user_items[0]["role"], "user");
    assert_eq!(user_items[0]["content"], "优化 Antigravity 支持");

    let planner = json!({
        "step_index": 1,
        "source": "MODEL",
        "type": "PLANNER_RESPONSE",
        "thinking": "Inspecting codebase...",
        "tool_calls": [
            { "name": "run_command", "args": { "CommandLine": "cargo test" } },
            { "name": "view_file", "args": { "AbsolutePath": "src/main.rs" } }
        ]
    });
    let planner_items = normalize_local_conversation_items(&planner);
    assert_eq!(planner_items.len(), 3);
    assert_eq!(planner_items[0]["kind"], "thinking");
    assert_eq!(planner_items[1]["kind"], "tool");
    assert_eq!(planner_items[1]["label"], "run_command: cargo test");
    assert_eq!(planner_items[2]["label"], "view_file: src/main.rs");

    let generic_reply = json!({
        "step_index": 2,
        "source": "MODEL",
        "type": "GENERIC",
        "content": "已完成第一轮检查。"
    });
    let reply_items = normalize_local_conversation_items(&generic_reply);
    assert_eq!(reply_items.len(), 1);
    assert_eq!(reply_items[0]["role"], "assistant");
    assert_eq!(reply_items[0]["content"], "已完成第一轮检查。");

    let sys_msg = json!({
        "step_index": 3,
        "source": "SYSTEM",
        "type": "SYSTEM_MESSAGE",
        "content": "<SYSTEM_MESSAGE>internal</SYSTEM_MESSAGE>"
    });
    assert!(normalize_local_conversation_items(&sys_msg).is_empty());

    let checkpoint = json!({
        "step_index": 4,
        "source": "SYSTEM",
        "type": "CHECKPOINT"
    });
    let cp_items = normalize_local_conversation_items(&checkpoint);
    assert_eq!(cp_items.len(), 1);
    assert_eq!(cp_items[0]["kind"], "status");
}
#[test]
fn codex_rollout_renders_reasoning_apply_patch_diffs_and_folds_call_outputs() {
    let home = tempfile::tempdir().unwrap();
    let sess_dir = home
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("11")
        .join("18");
    fs::create_dir_all(&sess_dir).unwrap();
    let lines = [
        r#"{"type":"session_meta","payload":{"id":"cx-sess-1"}}"#,
        r#"{"type":"event_msg","payload":{"type":"user_message","message":"Fix parser"}}"#,
        r#"{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"Analyzing parser edge cases"}]}}"#,
        r#"{"type":"response_item","payload":{"type":"custom_tool_call","call_id":"call_patch_1","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-pub fn old() {}\n+pub fn fixed() {}\n*** End Patch"}}"#,
        r#"{"type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call_patch_1","output":"Success. Updated the following files:\nM src/lib.rs"}}"#,
        r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Updated `src/lib.rs`."}]}}"#,
    ];
    fs::write(
        sess_dir.join("rollout-2026-11-18-cx-sess-1.jsonl"),
        lines.join("\n"),
    )
    .unwrap();

    let items = load_session_conversation(home.path(), AgentType::Codex, "cx-sess-1");
    assert_eq!(items.len(), 4);
    assert_eq!(items[0]["role"], "user");
    assert_eq!(items[0]["content"], "Fix parser");
    assert_eq!(items[1]["kind"], "thinking");
    assert_eq!(items[1]["content"], "Analyzing parser edge cases");
    assert_eq!(items[2]["kind"], "tool");
    assert_eq!(items[2]["label"], "apply_patch: src/lib.rs");
    let tool_text = items[2]["content"].as_str().unwrap();
    assert!(tool_text.contains("--- a/src/lib.rs"));
    assert!(tool_text.contains("+++ b/src/lib.rs"));
    assert!(tool_text.contains("-pub fn old() {}"));
    assert!(tool_text.contains("+pub fn fixed() {}"));
    assert!(tool_text.contains("〔结果〕\nSuccess. Updated the following files:"));
    assert_eq!(items[3]["role"], "assistant");
    assert_eq!(items[3]["content"], "Updated `src/lib.rs`.");
}
