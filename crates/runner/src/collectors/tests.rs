use std::{
    fs,
    io::{BufReader, Cursor},
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, mpsc},
    thread,
    time::{Duration, Instant, SystemTime},
};

use rusqlite::Connection;
use serde_json::json;

use super::*;

fn sleeping_script() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();

    #[cfg(windows)]
    let script = {
        let path = directory.path().join("claude.cmd");
        fs::write(
            &path,
            "@echo off\r\npowershell -NoProfile -Command \"Start-Sleep -Seconds 5\"\r\n",
        )
        .unwrap();
        path
    };

    #[cfg(unix)]
    let script = {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.path().join("claude");
        fs::write(&path, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).unwrap();
        path
    };
    (directory, script)
}

#[test]
#[cfg(windows)]
fn resolves_only_known_windows_codex_npm_shim_layouts() {
    let directory = tempfile::tempdir().unwrap();
    let npm_root = directory.path().join("npm");
    let shim = npm_root.join("codex.cmd");
    let native = npm_root
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("node_modules")
        .join("@openai")
        .join("codex-win32-x64")
        .join("vendor")
        .join("x86_64-pc-windows-msvc")
        .join("bin")
        .join("codex.exe");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    fs::write(&shim, "@echo off\r\n").unwrap();
    fs::write(&native, b"test").unwrap();

    assert_eq!(
        resolve_windows_codex_candidate(&shim),
        dunce::canonicalize(&native).ok()
    );
    let unrelated = directory.path().join("other").join("codex.cmd");
    fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
    fs::write(&unrelated, "@echo off\r\n").unwrap();
    assert!(resolve_windows_codex_candidate(&unrelated).is_none());
}

#[test]
fn sanitizes_protocol_error_messages() {
    let input = "bad\0message\nwith control";
    assert_eq!(sanitize_protocol_message(input), "badmessagewith control");
}

#[test]
fn protocol_reader_delivers_messages_without_blocking_the_request_thread() {
    let input = Cursor::new(
        br#"{"method":"thread/status/changed","params":{"threadId":"thr-1"}}
{"id":7,"result":{"data":[]}}
"#,
    );
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || read_protocol(input, sender));

    let first = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    let second = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    let eof = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    reader.join().unwrap();

    assert!(matches!(
        first,
        ProtocolRead::Message(value)
            if value.get("method").and_then(Value::as_str)
                == Some("thread/status/changed")
    ));
    assert!(matches!(
        second,
        ProtocolRead::Message(value)
            if value.get("id").and_then(Value::as_u64) == Some(7)
    ));
    assert!(matches!(eof, ProtocolRead::Eof));
}

#[test]
fn bounded_line_reader_stops_oversized_lines_without_buffering_them() {
    struct EndlessBytes;

    impl std::io::Read for EndlessBytes {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            buffer.fill(b'a');
            Ok(buffer.len())
        }
    }

    let mut reader = BufReader::new(EndlessBytes);
    assert!(matches!(
        read_bounded_line(&mut reader),
        BoundedLine::TooLarge
    ));
}

#[test]
fn bounded_line_reader_returns_lines_and_eof() {
    let mut reader = BufReader::new(Cursor::new(b"{\"id\":1}\ntrailing".to_vec()));
    assert!(matches!(
        read_bounded_line(&mut reader),
        BoundedLine::Line(line) if line == "{\"id\":1}\n"
    ));
    assert!(matches!(
        read_bounded_line(&mut reader),
        BoundedLine::Line(line) if line == "trailing"
    ));
    assert!(matches!(read_bounded_line(&mut reader), BoundedLine::Eof));
}

#[test]
fn child_output_reader_drains_but_retains_only_the_bounded_prefix() {
    let input = vec![b'x'; MAX_AGENT_TURN_OUTPUT_BYTES + 32 * 1024];
    let output = read_child_output(Cursor::new(input))
        .join()
        .unwrap()
        .unwrap();

    assert_eq!(output.len(), MAX_AGENT_TURN_OUTPUT_BYTES + 1);
    assert!(output.iter().all(|byte| *byte == b'x'));
}

#[test]
fn claude_stream_events_emit_text_and_thinking_activity() {
    let text = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hello"}}}"#;
    let thinking = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"private"}}}"#;
    let init = r#"{"type":"system","subtype":"init"}"#;
    let compact = r#"{"type":"system","subtype":"compact_boundary"}"#;

    assert_eq!(
        parse_agent_turn_update(&crate::domain::AgentType::ClaudeCode, text),
        Some(AgentTurnUpdate::AssistantDelta("hello".to_owned()))
    );
    assert_eq!(
        parse_agent_turn_update(&crate::domain::AgentType::ClaudeCode, thinking),
        Some(AgentTurnUpdate::Activity("Claude 正在深度思考..."))
    );
    assert_eq!(
        parse_agent_turn_update(&crate::domain::AgentType::ClaudeCode, init),
        Some(AgentTurnUpdate::Activity("Claude 正在初始化上下文..."))
    );
    assert_eq!(
        parse_agent_turn_update(&crate::domain::AgentType::ClaudeCode, compact),
        Some(AgentTurnUpdate::Activity(
            "Claude 正在压缩整理历史上下文..."
        ))
    );
}

#[test]
fn codex_stream_events_emit_safe_activity_and_final_messages() {
    let command =
        r#"{"type":"item.started","item":{"type":"command_execution","command":"secret"}}"#;
    let message = r#"{"type":"item.completed","item":{"type":"agent_message","text":" done "}}"#;

    assert_eq!(
        parse_agent_turn_update(&crate::domain::AgentType::Codex, command),
        Some(AgentTurnUpdate::Activity("Codex 正在执行命令"))
    );
    assert_eq!(
        parse_agent_turn_update(&crate::domain::AgentType::Codex, message),
        Some(AgentTurnUpdate::AssistantMessage("done".to_owned()))
    );
    assert_eq!(
        final_agent_message(&crate::domain::AgentType::Codex, message).as_deref(),
        Some("done")
    );
}

#[test]
fn final_claude_result_uses_the_last_non_empty_message() {
    let output = concat!(
        "{\"type\":\"result\",\"result\":\"first\"}\n",
        "{\"type\":\"result\",\"result\":\" final \"}\n",
    );
    assert_eq!(
        final_agent_message(&crate::domain::AgentType::ClaudeCode, output).as_deref(),
        Some("final")
    );
}

#[test]
fn session_turn_cancellation_stops_the_child_process() {
    let (_directory, script) = sleeping_script();
    let cancelled = AtomicBool::new(true);
    let started = Instant::now();

    let error = run_agent_session_turn_streaming(
        crate::domain::AgentType::ClaudeCode,
        script,
        None,
        "019abc-session",
        "private prompt",
        TurnOptions::default(),
        Duration::from_secs(10),
        &cancelled,
        |_| {},
    )
    .unwrap_err();

    assert!(matches!(error, CollectorError::Cancelled { .. }));
    assert!(started.elapsed() < Duration::from_secs(6));
}

#[test]
fn claude_transcript_collector_reads_metadata_without_retaining_content() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects").join("encoded-project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("session-123.jsonl"),
        concat!(
            "{\"type\":\"user\",\"sessionId\":\"session-123\",",
            "\"cwd\":\"/work/private-project\",",
            "\"timestamp\":\"2026-08-14T01:02:03Z\",",
            "\"message\":{\"content\":\"TOP_SECRET_TRANSCRIPT_CONTENT\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"session-123\",",
            "\"message\":{\"role\":\"assistant\",\"content\":",
            "[{\"type\":\"text\",\"text\":\"SECOND_SECRET_REPLY\"}]}}\n"
        ),
    )
    .unwrap();

    let payload = collect_claude_transcripts_from(directory.path(), false).unwrap();
    let entries: Value = serde_json::from_str(&payload).unwrap();
    let entry = &entries.as_array().unwrap()[0];

    assert_eq!(entry["sessionId"], "session-123");
    assert_eq!(entry["cwd"], "/work/private-project");
    assert_eq!(entry["state"], "persisted");
    assert_eq!(entry["startedAt"], 1_786_669_323_000_i64);
    assert!(!payload.contains("TOP_SECRET_TRANSCRIPT_CONTENT"));
    assert!(!payload.contains("SECOND_SECRET_REPLY"));
    assert!(!payload.contains("message"));
}

#[test]
fn claude_transcript_collector_hides_sessions_with_no_assistant_output() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects").join("encoded-project");
    fs::create_dir_all(&project).unwrap();
    // A session the user abandoned before the first reply. It still has a
    // prompt, a custom title and a full transcript on disk - only the
    // absence of assistant output marks it as empty.
    fs::write(
        project.join("abandoned.jsonl"),
        concat!(
            "{\"type\":\"queue-operation\",\"sessionId\":\"abandoned\"}\n",
            "{\"type\":\"user\",\"sessionId\":\"abandoned\",",
            "\"cwd\":\"/work/project\",\"timestamp\":\"2026-08-14T01:02:03Z\",",
            "\"message\":{\"content\":\"现在的项目甚至没有git？\"}}\n",
            "{\"type\":\"user\",\"sessionId\":\"abandoned\",",
            "\"message\":{\"content\":[{\"type\":\"text\",",
            "\"text\":\"[Request interrupted by user]\"}]}}\n",
            "{\"type\":\"custom-title\",\"sessionId\":\"abandoned\",",
            "\"customTitle\":\"项目缺少Git配置\"}\n"
        ),
    )
    .unwrap();
    fs::write(
        project.join("real.jsonl"),
        concat!(
            "{\"type\":\"user\",\"sessionId\":\"real\",\"cwd\":\"/work/project\",",
            "\"timestamp\":\"2026-08-14T02:00:00Z\",",
            "\"message\":{\"content\":\"同样的首问\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"real\",",
            "\"message\":{\"role\":\"assistant\",\"content\":",
            "[{\"type\":\"text\",\"text\":\"有 git\"}]}}\n"
        ),
    )
    .unwrap();

    let payload = collect_claude_transcripts_from(directory.path(), false).unwrap();
    let entries: Value = serde_json::from_str(&payload).unwrap();
    let sessions: Vec<&str> = entries
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["sessionId"].as_str())
        .collect();

    assert_eq!(sessions, ["real"]);
}

#[test]
fn claude_output_detection_accepts_shapes_and_rejects_lookalikes() {
    let output = |value: Value| is_assistant_output(&value);

    // Text array form and plain string form both count as output.
    assert!(output(serde_json::json!({
        "type":"assistant","message":{"role":"assistant",
        "content":[{"type":"text","text":"hi"}]}
    })));
    assert!(output(serde_json::json!({
        "type":"assistant","message":{"role":"assistant","content":"hi"}
    })));
    // Assistant turns that are pure tool calls produced no visible reply.
    assert!(!output(serde_json::json!({
        "type":"assistant","message":{"role":"assistant",
        "content":[{"type":"tool_use","name":"Bash"}]}
    })));
    assert!(!output(serde_json::json!({
        "type":"assistant","message":{"role":"assistant",
        "content":[{"type":"text","text":"   "}]}
    })));
    // A subagent transcript is not output the user ever saw.
    assert!(!output(serde_json::json!({
        "type":"assistant","isSidechain":true,
        "message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}
    })));
    // Prompts and tool results are not assistant output.
    assert!(!output(serde_json::json!({
        "type":"user","message":{"role":"user","content":"hi"}
    })));
    assert!(!output(serde_json::json!({
        "type":"user","message":{"role":"user",
        "content":[{"type":"tool_result","content":"hi"}]}
    })));
}

#[test]
fn claude_context_collector_keeps_explicit_runtime_metadata_only() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("projects").join("encoded-project");
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("session-ctx.jsonl"),
        concat!(
            "{\"type\":\"user\",\"sessionId\":\"session-ctx\",",
            "\"message\":{\"content\":\"TOP_SECRET_PROMPT\"}}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\" Auth   refactor \"}\n",
            "{\"type\":\"permission-mode\",\"mode\":\"acceptEdits\"}\n",
            "{\"type\":\"assistant\",\"message\":{",
            "\"model\":\"claude-opus-4-7\",",
            "\"content\":[{\"type\":\"text\",\"text\":\"TOP_SECRET_REPLY\"}],",
            "\"usage\":{\"input_tokens\":100,",
            "\"cache_creation_input_tokens\":20,\"cache_read_input_tokens\":30}},",
            "\"context_window\":{\"context_window_size\":200000}}\n"
        ),
    )
    .unwrap();

    let contexts = collect_claude_session_contexts_from(directory.path(), 10).unwrap();
    let context = &contexts[0];
    let serialized = serde_json::to_string(context).unwrap();

    assert_eq!(context.agent_session_id, "session-ctx");
    assert_eq!(context.title.as_deref(), Some("Auth refactor"));
    assert_eq!(context.model.as_deref(), Some("claude-opus-4-7"));
    assert_eq!(context.permission_mode.as_deref(), Some("acceptEdits"));
    assert_eq!(context.context_used_tokens, Some(150));
    assert_eq!(context.context_window_tokens, Some(200_000));
    assert!(!serialized.contains("TOP_SECRET"));
}

#[test]
fn claude_context_supports_current_title_and_mode_events() {
    let mut context = LocalSessionContext::new(
        crate::domain::AgentType::ClaudeCode,
        "session-current".to_owned(),
    );

    apply_claude_context_line(
        r#"{"type":"ai-title","aiTitle":" Provider generated title "}"#,
        &mut context,
    );
    apply_claude_context_line(r#"{"type":"mode","mode":"plan"}"#, &mut context);
    assert_eq!(context.title.as_deref(), Some("Provider generated title"));
    assert_eq!(context.permission_mode.as_deref(), Some("plan"));

    apply_claude_context_line(
        r#"{"type":"custom-title","customTitle":" User title "}"#,
        &mut context,
    );
    apply_claude_context_line(
        r#"{"type":"ai-title","aiTitle":" Later generated title "}"#,
        &mut context,
    );
    assert_eq!(context.title.as_deref(), Some("User title"));
}

#[test]
fn local_prompt_titles_ignore_provider_control_messages() {
    assert_eq!(
        local_prompt_title("<USER_REQUEST>优化侧边栏与会话页</USER_REQUEST>").as_deref(),
        Some("优化侧边栏与会话页")
    );
    assert!(local_prompt_title("<local-command-caveat>internal</local-command-caveat>").is_none());
    assert!(local_prompt_title("<command-name>/compact</command-name>").is_none());
    assert!(local_prompt_title("Untitled").is_none());
    assert!(local_prompt_title("未命名会话").is_none());
    assert_eq!(
            local_prompt_title(
                "# AGENTS.md instructions for /repo\n\n<INSTRUCTIONS>\ninternal\n</INSTRUCTIONS>\n\n实现重命名功能"
            )
            .as_deref(),
            Some("实现重命名功能")
        );
}

#[test]
fn test_claude_prompt_extraction_filters_tool_results_and_synthetics() {
    use serde_json::json;

    // 1. Tool result message in role=user should NEVER be extracted as prompt
    let tool_result_line = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [
                {
                    "type": "tool_result",
                    "tool_use_id": "toolu_123",
                    "content": "cargo test --workspace succeeded"
                }
            ]
        }
    });
    assert_eq!(extract_claude_prompt_text(&tool_result_line), None);

    // 2. Synthetic injected commands should be ignored
    let synthetic_line = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": "<command-name>/doctor</command-name>"
        }
    });
    assert_eq!(extract_claude_prompt_text(&synthetic_line), None);

    // 3. Genuine human prompt should be cleanly extracted
    let real_prompt_line = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": "请帮我重构网络连接池逻辑"
        }
    });
    assert_eq!(
        extract_claude_prompt_text(&real_prompt_line).as_deref(),
        Some("请帮我重构网络连接池逻辑")
    );

    // 4. Image-only prompt gets [image] label
    let image_line = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [
                {
                    "type": "image",
                    "source": { "type": "base64", "data": "abc" }
                }
            ]
        }
    });
    assert_eq!(
        extract_claude_prompt_text(&image_line).as_deref(),
        Some("[image]")
    );
}

#[test]
fn test_claude_context_window_inference() {
    assert_eq!(claude_context_window("claude-opus-5-20261001"), 1_000_000);
    assert_eq!(claude_context_window("claude-sonnet-5"), 1_000_000);
    assert_eq!(claude_context_window("claude-fable-5-1"), 1_000_000);
    assert_eq!(claude_context_window("custom-model[1m]"), 1_000_000);
    assert_eq!(claude_context_window("claude-3-7-sonnet"), 200_000);
}

#[test]
fn test_claude_result_reports_context_usage() {
    // Cache reads and writes occupy the window like fresh input does.
    let result = json!({
        "type":"result",
        "result":"done",
        "usage":{"input_tokens":1000,"output_tokens":200,
                 "cache_read_input_tokens":30000,"cache_creation_input_tokens":5000}
    });
    assert_eq!(
        claude_result_context_usage(&result),
        Some(ContextUsage {
            used: 36_000,
            window: None
        })
    );

    let with_window = json!({
        "type":"result","usage":{"input_tokens":40_000},"model_context_window":200_000
    });
    assert_eq!(
        claude_result_context_usage(&with_window),
        Some(ContextUsage {
            used: 40_000,
            window: Some(200_000)
        })
    );

    // Nothing usable must stay absent rather than become a zero ring.
    assert_eq!(
        claude_result_context_usage(&json!({"type":"result","usage":{}})),
        None
    );
    assert_eq!(
        claude_result_context_usage(&json!({"type":"assistant","usage":{"input_tokens":5}})),
        None
    );
}

#[test]
fn test_claude_stop_reason_and_turn_lifecycle() {
    let mut context = LocalSessionContext::new(
        crate::domain::AgentType::ClaudeCode,
        "session-lifecycle".to_owned(),
    );

    // 1. Tool use stop_reason -> mid-turn, turn_ended is false
    let tool_use_line = serde_json::json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "model": "claude-sonnet-5",
            "stop_reason": "tool_use",
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 150,
                "cache_read_input_tokens": 500,
                "cache_creation_input_tokens": 200
            }
        }
    })
    .to_string();
    apply_claude_context_line(&tool_use_line, &mut context);
    assert_eq!(context.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(context.turn_ended, Some(false));
    assert_eq!(context.context_used_tokens, Some(1700));

    // 2. End turn stop_reason -> completed turn, turn_ended is true
    let end_turn_line = serde_json::json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 1200,
                "output_tokens": 300,
                "cache_read_input_tokens": 500,
                "cache_creation_input_tokens": 200
            }
        }
    })
    .to_string();
    apply_claude_context_line(&end_turn_line, &mut context);
    assert_eq!(context.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(context.turn_ended, Some(true));

    // 3. New genuine user prompt accepted -> starts next turn, turn_ended resets to false
    let user_prompt = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": "请接着处理"
        }
    })
    .to_string();
    apply_claude_context_line(&user_prompt, &mut context);
    assert_eq!(context.turn_ended, Some(false));
}

#[test]
fn test_apply_oversized_claude_record_slicing() {
    let mut context = LocalSessionContext::new(
        crate::domain::AgentType::ClaudeCode,
        "session-oversized".to_owned(),
    );

    // Construct head slice and tail slice representing an oversized 2MB assistant line
    let head = r#"{"type":"assistant","message":{"role":"assistant","model":"claude-opus-5-20261001","content":[{"type":"text","text":"long content..."#;
    let tail = r#"...more content"}],"stop_reason":"end_turn","usage":{"input_tokens":5000,"output_tokens":1200,"cache_read_input_tokens":10000,"cache_creation_input_tokens":2000}}}"#;

    apply_oversized_claude_record(head, tail, &mut context);

    assert_eq!(context.model.as_deref(), Some("claude-opus-5-20261001"));
    assert_eq!(context.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(context.turn_ended, Some(true));
    assert_eq!(context.context_used_tokens, Some(17000));
    assert_eq!(context.context_window_tokens, Some(1_000_000));
}

#[test]
fn test_scan_claude_context_region_handles_oversized_line() {
    let mut context = LocalSessionContext::new(
        crate::domain::AgentType::ClaudeCode,
        "session-streaming-oversized".to_owned(),
    );

    // Build a single line exceeding 128 KiB
    let huge_padding = "x".repeat(150_000);
    let oversized_line = format!(
        r#"{{"type":"assistant","sessionId":"sess-over","message":{{"role":"assistant","model":"claude-fable-5-1","content":[{{"type":"text","text":"{huge_padding}"}}],"stop_reason":"end_turn","usage":{{"input_tokens":4000,"output_tokens":800,"cache_read_input_tokens":2000,"cache_creation_input_tokens":1000}}}}}}{}"#,
        "\n"
    );

    let mut cursor = std::io::Cursor::new(oversized_line.as_bytes());
    scan_claude_context_region(
        &mut cursor,
        oversized_line.len() as u64 + 1024,
        &mut context,
        "test",
    )
    .unwrap();

    assert_eq!(context.model.as_deref(), Some("claude-fable-5-1"));
    assert_eq!(context.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(context.turn_ended, Some(true));
    assert_eq!(context.context_used_tokens, Some(7000));
    assert_eq!(context.context_window_tokens, Some(1_000_000));
}

#[test]
fn codex_rollout_extracts_title_from_response_item() {
    let mut context = LocalSessionContext::new(
        crate::domain::AgentType::Codex,
        "session-codex-title".to_owned(),
    );
    let line = serde_json::json!({
            "type": "response_item",
            "payload": {
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "# AGENTS.md instructions for /test\n<INSTRUCTIONS>\nsecret\n</INSTRUCTIONS>"
                    },
                    {
                        "type": "input_text",
                        "text": "分析内存泄漏问题"
                    }
                ]
            }
        })
        .to_string();
    apply_codex_context_line(&line, &mut context);
    assert_eq!(context.title.as_deref(), Some("分析内存泄漏问题"));
}

#[test]
fn codex_local_store_collector_selects_metadata_only() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("state_5.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    cwd TEXT NOT NULL,
                    title TEXT NOT NULL,
                    preview TEXT NOT NULL,
                    source TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    recency_at INTEGER NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO threads VALUES (
                    'thread-123', '/work/private-project',
                    'TOP_SECRET_TITLE', 'TOP_SECRET_PREVIEW', 'cli',
                    100, 200, 190, 0
                );",
        )
        .unwrap();
    drop(connection);

    let payload = collect_codex_local_threads_from(&database, 10).unwrap();
    let serialized = serde_json::to_string(&payload).unwrap();
    let thread = &payload["data"][0];

    assert_eq!(thread["id"], "thread-123");
    assert_eq!(thread["cwd"], "/work/private-project");
    assert_eq!(thread["status"]["type"], "persisted");
    assert!(!serialized.contains("TOP_SECRET_TITLE"));
    assert!(!serialized.contains("TOP_SECRET_PREVIEW"));
}

#[test]
fn codex_context_collector_reads_configuration_without_message_content() {
    let directory = tempfile::tempdir().unwrap();
    let rollout = directory.path().join("rollout-thread-ctx.jsonl");
    fs::write(
        &rollout,
        concat!(
            "{\"type\":\"response_item\",\"payload\":{",
            "\"content\":\"TOP_SECRET_TRANSCRIPT\"}}\n",
            "{\"type\":\"turn_context\",\"payload\":{",
            "\"model\":\"gpt-5.5-codex\",\"effort\":\"high\",",
            "\"approval_policy\":\"on-request\",",
            "\"sandbox_policy\":{\"type\":\"workspace-write\"}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",",
            "\"info\":{\"model_context_window\":258400,",
            "\"last_token_usage\":{\"input_tokens\":42000,\"total_tokens\":42100},",
            "\"total_token_usage\":{\"total_tokens\":88000}}}}\n"
        ),
    )
    .unwrap();
    let database = directory.path().join("state_5.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE threads (
                    id TEXT PRIMARY KEY, title TEXT, model TEXT,
                    reasoning_effort TEXT, approval_mode TEXT,
                    sandbox_policy TEXT, tokens_used INTEGER,
                    rollout_path TEXT, updated_at INTEGER, archived INTEGER
                );
                INSERT INTO threads VALUES (
                    'thread-ctx', ' Review checkout ', 'gpt-old', 'medium',
                    'unless-trusted', '{{\"type\":\"read-only\"}}',
                    70000, '{}', 200, 0
                );",
            rollout.display().to_string().replace('\\', "\\\\")
        ))
        .unwrap();
    drop(connection);

    let contexts = collect_codex_session_contexts_from(&database, 10).unwrap();
    let context = &contexts[0];
    let serialized = serde_json::to_string(context).unwrap();

    assert_eq!(context.title.as_deref(), Some("Review checkout"));
    assert_eq!(context.model.as_deref(), Some("gpt-5.5-codex"));
    assert_eq!(context.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(context.approval_policy.as_deref(), Some("on-request"));
    assert_eq!(context.sandbox_mode.as_deref(), Some("workspace-write"));
    assert_eq!(context.context_window_tokens, Some(258_400));
    assert_eq!(context.context_used_tokens, Some(42_100));
    assert_eq!(context.session_total_tokens, Some(88_000));
    assert!(!serialized.contains("TOP_SECRET"));
}

#[test]
fn codex_context_collector_rejects_rollouts_outside_state_root() {
    let directory = tempfile::tempdir().unwrap();
    let state_root = directory.path().join("codex-home");
    fs::create_dir(&state_root).unwrap();
    let escaped_rollout = directory.path().join("outside-rollout.jsonl");
    fs::write(
        &escaped_rollout,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{",
            "\"model\":\"TOP_SECRET_MODEL\",",
            "\"approval_policy\":\"bypassPermissions\"}}\n"
        ),
    )
    .unwrap();
    let database = state_root.join("state_5.sqlite");
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE threads (
                    id TEXT PRIMARY KEY, title TEXT, model TEXT,
                    approval_mode TEXT, sandbox_policy TEXT,
                    rollout_path TEXT, updated_at INTEGER
                );
                INSERT INTO threads VALUES (
                    'thread-contained', 'Safe title', 'safe-model',
                    'on-request', '{{\"type\":\"workspace-write\"}}', '{}', 200
                );",
            escaped_rollout.display().to_string().replace('\\', "\\\\")
        ))
        .unwrap();
    drop(connection);

    let contexts = collect_codex_session_contexts_from(&database, 10).unwrap();
    let context = &contexts[0];
    let serialized = serde_json::to_string(context).unwrap();

    assert_eq!(context.model.as_deref(), Some("safe-model"));
    assert_eq!(context.approval_policy.as_deref(), Some("on-request"));
    assert_eq!(context.sandbox_mode.as_deref(), Some("workspace-write"));
    assert!(!serialized.contains("TOP_SECRET"));
}

#[test]
fn codex_handshake_kills_a_silent_process_at_the_deadline() {
    let (_directory, script) = sleeping_script();
    let started = Instant::now();
    let result = CodexAppServer::start_with_timeout(script, Duration::from_millis(100));
    let error = match result {
        Ok(_) => panic!("silent fake app-server unexpectedly initialized"),
        Err(error) => error,
    };

    assert_eq!(error.code(), "collector_timeout");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn turn_options_pass_reasoning_effort_per_provider() {
    let args_for = |agent, options: TurnOptions<'_>| {
        let mut command = Command::new("placeholder");
        apply_turn_options(&mut command, &agent, &options);
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };

    // Claude Code takes a --effort flag and supports the deep levels.
    assert_eq!(
        args_for(
            crate::domain::AgentType::ClaudeCode,
            TurnOptions {
                reasoning_effort: Some("xhigh"),
                ..Default::default()
            },
        ),
        vec!["--effort", "xhigh"]
    );

    // Codex has no --effort flag: effort is passed as a config override.
    assert_eq!(
        args_for(
            crate::domain::AgentType::Codex,
            TurnOptions {
                reasoning_effort: Some("high"),
                ..Default::default()
            },
        ),
        vec!["-c", "model_reasoning_effort=\"high\""]
    );

    // Providers without a per-turn effort ignore it rather than passing a
    // flag the CLI would reject.
    assert!(
        args_for(
            crate::domain::AgentType::DeepseekHarness,
            TurnOptions {
                reasoning_effort: Some("high"),
                ..Default::default()
            },
        )
        .is_empty()
    );

    // Effort still applies when a model override is absent, and blank
    // values never reach the command line.
    assert_eq!(
        args_for(
            crate::domain::AgentType::ClaudeCode,
            TurnOptions {
                reasoning_effort: Some("  "),
                ..Default::default()
            },
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        args_for(
            crate::domain::AgentType::ClaudeCode,
            TurnOptions {
                model: Some("claude-opus-5"),
                permission_mode: Some("plan"),
                ..Default::default()
            },
        ),
        vec!["--model", "claude-opus-5", "--permission-mode", "plan"]
    );
    assert_eq!(
        args_for(
            crate::domain::AgentType::Antigravity,
            TurnOptions {
                reasoning_effort: Some("high"),
                model: Some("gemini-3.8-flash-high[1M]"),
                permission_mode: Some("bypassPermissions"),
                ..Default::default()
            },
        ),
        vec![
            "--effort",
            "high",
            "--model",
            "gemini-3.8-flash-high[1M]",
            "--dangerously-skip-permissions",
        ]
    );
    assert!(!crate::collectors::process::is_safe_model_id("-bad"));
    assert!(!crate::collectors::process::is_safe_model_id("a..b"));
    assert!(crate::collectors::process::is_safe_model_id(
        "claude-opus-4-6[1m]"
    ));
}

#[test]
fn turn_options_support_explicit_session_exists_override() {
    let opt_new = TurnOptions {
        session_exists: Some(false),
        ..Default::default()
    };
    assert_eq!(opt_new.session_exists, Some(false));

    let opt_resume = TurnOptions {
        session_exists: Some(true),
        ..Default::default()
    };
    assert_eq!(opt_resume.session_exists, Some(true));
}

#[test]
fn a_sessions_subagents_are_read_from_their_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects").join("-p");
    let session = project.join("s1");
    let subs = session.join("subagents");
    fs::create_dir_all(&subs).unwrap();
    let main = project.join("s1.jsonl");
    fs::write(&main, "").unwrap();
    fs::write(
            subs.join("agent-aaa.jsonl"),
            concat!(
                r#"{"type":"assistant","message":{"id":"m1","model":"deepseek-v4.1-flash","usage":{"input_tokens":10,"output_tokens":100,"cache_read_input_tokens":900,"cache_creation_input_tokens":0}}}"#,
                "\n",
                // the same message's next block repeats the usage: one call
                r#"{"type":"assistant","message":{"id":"m1","model":"deepseek-v4.1-flash","usage":{"input_tokens":10,"output_tokens":100,"cache_read_input_tokens":900,"cache_creation_input_tokens":0}}}"#,
                "\n",
            ),
        )
        .unwrap();
    fs::write(
        subs.join("agent-aaa.meta.json"),
        r#"{"agentType":"Explore","description":"Survey the module","spawnDepth":1}"#,
    )
    .unwrap();
    fs::write(
            subs.join("agent-bbb.jsonl"),
            r#"{"type":"assistant","message":{"id":"m2","model":"gemini-3.8-flash-n","usage":{"input_tokens":5,"output_tokens":50}}}"#,
        )
        .unwrap();
    fs::write(
            subs.join("agent-bbb.meta.json"),
            r#"{"agentType":"Plan","description":"Write the plan","parentAgentId":"aaa","spawnDepth":2}"#,
        )
        .unwrap();

    let mut found = collect_claude_subagents(&main);
    found.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(found.len(), 2, "one entry per subagent file");
    assert_eq!(found[0].id, "aaa");
    assert_eq!(found[0].kind, "Explore");
    assert_eq!(found[0].description, "Survey the module");
    assert_eq!(found[0].request_count, 1, "a message counts once");
    assert_eq!(found[0].completion_tokens, 100);
    assert_eq!(found[1].parent, "aaa", "a nested subagent names its parent");
    assert_eq!(found[1].depth, 2);
    fs::remove_dir_all(dir).ok();
}

/// The console asks for the session list every few seconds, and the store
/// holds tens of megabytes of finished subagent transcripts. A file whose
/// length and last write have not moved is answered from memory; one that
/// grows is read again.
#[test]
fn a_finished_subagent_is_read_once() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("agent-ccc.jsonl");
    let line = r#"{"type":"assistant","message":{"id":"m1","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":42}}}"#;
    // the moment is pinned ahead of wall-clock `change_time` (Windows NTFS
    // `live_file_mark` uses `change_time.max(last_write_time)`), so each write
    // keeps the exact same `(length, moment)` key deterministically
    let stamp = SystemTime::now() + Duration::from_secs(600);
    let write_at = |text: &str| {
        fs::write(&file, text).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(stamp)
            .unwrap();
    };

    write_at(&format!("{line}\n"));
    let first = read_claude_subagent_cached(&file).expect("first read");
    assert_eq!(first.completion_tokens, 42);

    // same length, same moment: the memo answers even though the bytes
    // now say something else
    write_at(&format!("{line}\n").replace("42", "99"));
    let cached = read_claude_subagent_cached(&file).expect("cached read");
    assert_eq!(
        cached.completion_tokens, 42,
        "a file that has not moved is not read again"
    );

    // a longer file has moved: it is read again, whatever its moment
    write_at(&format!(
        "{line}\n{}\n",
        r#"{"type":"assistant","message":{"id":"m2","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":8}}}"#
    ));
    let reread = read_claude_subagent_cached(&file).expect("second read");
    assert_eq!(reread.request_count, 2, "a grown transcript is read again");
    assert_eq!(reread.completion_tokens, 50);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn deleting_a_session_moves_its_files_to_the_trash() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude");
    let project = config.join("projects").join("-p");
    let session = project.join("s1");
    fs::create_dir_all(session.join("subagents")).unwrap();
    fs::write(project.join("s1.jsonl"), "{}\n").unwrap();
    fs::write(session.join("subagents").join("agent-a.jsonl"), "{}\n").unwrap();
    let trash = dir.path().join("trash");
    // a session written just now is still being used
    let refused = delete_claude_session_in(&config, &trash, "s1");
    assert!(refused.is_err(), "a session written this minute is kept");

    // backdate every file past the guard, then delete
    let old = SystemTime::now() - Duration::from_secs(600);
    let backdate = |path: &Path| {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(old).unwrap();
    };
    backdate(&project.join("s1.jsonl"));
    backdate(&session.join("subagents").join("agent-a.jsonl"));
    let moved = delete_claude_session_in(&config, &trash, "s1").expect("deleted");
    assert!(!project.join("s1.jsonl").exists());
    assert!(!session.exists(), "the subagent folder goes with it");
    assert!(moved.starts_with(&trash));
    assert!(
        moved.join("s1.jsonl").is_file(),
        "the transcript is in the trash, not gone"
    );

    // an id the store never had is refused, not created
    assert!(delete_claude_session_in(&config, &trash, "missing").is_err());
    // and an id that could walk the tree is refused before any lookup
    assert!(delete_claude_session_in(&config, &trash, "../s1").is_err());
    fs::remove_dir_all(dir).ok();
}

/// Coding Assistant keeps `file-history/<id>` and `session-env/<id>` beside
/// the conversation's own `<id>` folder. All three carry the same name, and
/// a session with all three has to delete in one go: the moves must land
/// somewhere apart instead of the second one colliding with the first.
#[test]
fn deleting_a_session_that_has_two_folders_named_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("claude");
    let project = config.join("projects").join("-p");
    let session = project.join("s1");
    fs::create_dir_all(session.join("subagents")).unwrap();
    fs::write(project.join("s1.jsonl"), "{}\n").unwrap();
    fs::write(session.join("subagents").join("agent-a.jsonl"), "{}\n").unwrap();
    let history = config.join("file-history").join("s1");
    fs::create_dir_all(&history).unwrap();
    fs::write(history.join("old-version.txt"), "before\n").unwrap();
    let env = config.join("session-env").join("s1");
    fs::create_dir_all(&env).unwrap();
    fs::write(env.join("env.json"), "{}").unwrap();
    fs::create_dir_all(config.join("todos")).unwrap();
    fs::write(config.join("todos").join("s1-agent-a.json"), "[]").unwrap();

    // backdate every file past the write guard
    let old = SystemTime::now() - Duration::from_secs(600);
    let backdate = |path: &Path| {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(old).unwrap();
    };
    for file in [
        project.join("s1.jsonl"),
        session.join("subagents").join("agent-a.jsonl"),
        history.join("old-version.txt"),
        env.join("env.json"),
        config.join("todos").join("s1-agent-a.json"),
    ] {
        backdate(&file);
    }

    let trash = dir.path().join("trash");
    let moved = delete_claude_session_in(&config, &trash, "s1").expect("all of it moves");
    for gone in [&project.join("s1.jsonl"), &session, &history, &env] {
        assert!(!gone.exists(), "{} is out of the store", gone.display());
    }
    // every moved piece is in the trash, none of them merged into another
    assert!(moved.join("s1.jsonl").is_file());
    assert!(moved.join("s1/subagents/agent-a.jsonl").is_file());
    let names: Vec<String> = fs::read_dir(&moved)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        names.len(),
        5,
        "each of the five items landed on its own name: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "session-env-s1"),
        "the env folder is kept under a name of its own: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "file-history-s1"),
        "the history folder is kept under a name of its own: {names:?}"
    );
    assert!(
        moved.join("session-env-s1/env.json").is_file(),
        "the env file is inside it"
    );
    assert!(
        names.iter().any(|n| n.ends_with("s1-agent-a.json")),
        "the todo is kept: {names:?}"
    );
    fs::remove_dir_all(dir).ok();
}

/// A delete that cannot finish must not leave the session half moved: the
/// transcript that already went to the trash comes back, so the row the
/// page still shows can be deleted again.
#[test]
fn a_delete_that_fails_part_way_puts_the_session_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let transcript = dir.path().join("s1.jsonl");
    fs::write(&transcript, "{}\n").unwrap();
    let folder = dir.path().join("s1");
    fs::create_dir_all(folder.join("subagents")).unwrap();
    fs::write(folder.join("subagents").join("agent-a.jsonl"), "{}\n").unwrap();
    // a source that has gone between the listing and the move
    let vanished = store.join("session-env").join("s1");
    let trash = dir.path().join("trash").join("1-s1");
    fs::create_dir_all(&trash).unwrap();

    let paths = vec![transcript.clone(), folder.clone(), vanished];
    let failure = move_all_into(&trash, &paths).expect_err("the missing one stops the move");

    assert!(transcript.is_file(), "the transcript is back where it was");
    assert!(
        folder.join("subagents").join("agent-a.jsonl").is_file(),
        "the folder is back with its subagents"
    );
    let left: Vec<String> = fs::read_dir(&trash)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        left.is_empty(),
        "nothing is left claimed by the trash: {left:?}"
    );
    let message = failure.to_string();
    assert!(message.contains("put it all back"), "{message}");
    assert!(message.contains("moved 2 of 3"), "{message}");
    fs::remove_dir_all(dir).ok();
}
#[test]
fn codex_enrichment_folds_subagents_strips_verbatim_paths_and_reads_cached_input_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let sessions_dir = dir
        .path()
        .join("sessions")
        .join("2026")
        .join("11")
        .join("18");
    fs::create_dir_all(&sessions_dir).unwrap();
    let rollout = sessions_dir.join("rollout-parent-1.jsonl");
    fs::write(
        &rollout,
        concat!(
            "{\"type\":\"turn_context\",\"payload\":{\"model\":\"gpt-6.1-sol\",\"effort\":\"high\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{",
            "\"model_context_window\":475000,",
            "\"last_token_usage\":{\"input_tokens\":50000,\"cached_input_tokens\":45000,\"output_tokens\":800,\"total_tokens\":50800},",
            "\"total_token_usage\":{\"input_tokens\":120000,\"cached_input_tokens\":108000,\"output_tokens\":2400,\"total_tokens\":122400}}}}\n"
        ),
    )
    .unwrap();

    let db_path = dir.path().join("state_5.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(&format!(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            cwd TEXT NOT NULL,
            title TEXT,
            preview TEXT,
            source TEXT NOT NULL,
            thread_source TEXT,
            model TEXT,
            tokens_used INTEGER,
            rollout_path TEXT,
            updated_at INTEGER,
            updated_at_ms INTEGER,
            recency_at_ms INTEGER,
            agent_nickname TEXT,
            agent_role TEXT,
            archived INTEGER DEFAULT 0
        );
        INSERT INTO threads VALUES
            ('parent-1', '\\\\?\\D:\\ClaudeSpace', 'Ship Codex Parity', 'Ship Codex Parity', 'vscode', NULL, 'gpt-6.1-sol', 122400, '{}', 1794976694, 1794976694563, 1794976694563, NULL, NULL, 0),
            ('sub-bohr', '\\\\?\\D:\\ClaudeSpace', 'Audit tests', 'Audit tests', '{{\"subagent\":{{\"thread_spawn\":{{\"parent_thread_id\":\"parent-1\",\"depth\":1,\"agent_nickname\":\"Bohr\",\"agent_role\":\"worker\"}}}}}}', NULL, 'gpt-6.1-sol', 14000, NULL, 1794976690, 1794976690000, 1794976690000, 'Bohr', 'worker', 0);",
        rollout.display().to_string().replace('\\', "\\\\")
    ))
    .unwrap();
    drop(conn);

    let index = crate::collectors::codex_enrich::load_codex_enrichment_from_db(&db_path);
    let mut items = vec![
        json!({
            "id": "parent-1",
            "cwd": r"\\?\D:\ClaudeSpace",
            "updatedAt": 1794976694,
            "status": { "type": "notLoaded" },
            "provider": "codex"
        }),
        json!({
            "id": "sub-bohr",
            "cwd": r"\\?\D:\ClaudeSpace",
            "updatedAt": 1794976690,
            "status": { "type": "notLoaded" },
            "provider": "codex"
        }),
    ];
    crate::collectors::codex_enrich::enrich_codex_thread_items_with_index(&mut items, &index);

    assert_eq!(items.len(), 1, "child subagent should fold into parent");
    let root = &items[0];
    assert_eq!(root["cwd"], r"D:\ClaudeSpace");
    assert_eq!(root["status"]["type"], "idle");
    assert_eq!(root["last_activity_at_ms"], 1_794_976_694_563_i64);
    assert_eq!(root["model"], "gpt-6.1-sol");
    assert_eq!(root["total_tokens"], 122_400);
    assert_eq!(root["cache_read_tokens"], 108_000);
    assert_eq!(root["context_used_tokens"], 50_800);
    assert_eq!(root["context_window_tokens"], 475_000);
    let subs = root["subagents"].as_array().expect("subagents array");
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0]["id"], "sub-bohr");
    assert_eq!(subs[0]["kind"], "Bohr · worker");
    assert_eq!(subs[0]["description"], "Audit tests");
}

#[test]
fn delete_codex_session_moves_rollouts_to_trash_and_respects_write_guard() {
    let dir = tempfile::tempdir().unwrap();
    let codex_root = dir.path().join(".codex");
    let sess_dir = codex_root
        .join("sessions")
        .join("2026")
        .join("11")
        .join("18");
    fs::create_dir_all(&sess_dir).unwrap();
    let rollout = sess_dir.join("rollout-2026-11-18-01a1ecf2-test.jsonl");
    fs::write(&rollout, "{\"type\":\"session_meta\"}\n").unwrap();
    let trash_root = dir.path().join("trash");

    let err = crate::collectors::codex_trash::delete_codex_session_in(
        &codex_root,
        &trash_root,
        "01a1ecf2-test",
    )
    .expect_err("fresh rollout should be refused by 60s write guard");
    assert!(err.to_string().contains("last minute"));

    if let Ok(f) = fs::OpenOptions::new().write(true).open(&rollout) {
        let _ = f.set_modified(SystemTime::now() - Duration::from_secs(120));
        let trashed = crate::collectors::codex_trash::delete_codex_session_in(
            &codex_root,
            &trash_root,
            "01a1ecf2-test",
        )
        .expect("aged rollout moves to trash");
        assert!(trashed.is_dir());
        assert!(!rollout.exists());
    }
}

#[test]
fn normalize_acp_message_resolves_prefixed_plugin_session_turn_ids() {
    let mut pending_prompts = std::collections::HashMap::new();
    let mut pending_permissions = std::collections::HashMap::new();
    let mut active_turns = std::collections::HashMap::new();
    active_turns.insert("sess_42".to_string(), "turn_real_99".to_string());

    let msg = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "zcode_sess_42",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "hello chunk" }
            }
        }
    });

    let out = crate::collectors::acp_app_server::normalize_acp_message(
        msg,
        &mut pending_prompts,
        &mut pending_permissions,
        &mut active_turns,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["method"], "item/agentMessage/delta");
    assert_eq!(out[0]["params"]["turnId"], "turn_real_99");
    assert_eq!(out[0]["params"]["itemId"], "acp_msg:turn_real_99");
}
#[test]
fn claude_trash_cross_device_preserves_backup_and_refuses_non_exdev() {
    use crate::collectors::claude_trash::{
        copy_and_remove_cross_device_with, is_cross_device_error, move_into,
    };

    assert!(is_cross_device_error(&std::io::Error::new(
        std::io::ErrorKind::CrossesDevices,
        "exdev",
    )));
    #[cfg(unix)]
    assert!(is_cross_device_error(&std::io::Error::from_raw_os_error(
        18
    )));
    assert!(!is_cross_device_error(&std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "locked",
    )));

    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("sess-dir");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join("a.jsonl"), "keep-a").unwrap();
    fs::write(src_dir.join("b.jsonl"), "keep-b").unwrap();

    // Non-cross-device rename error (missing target parent) must fail without deleting source.
    let bad_target = dir.path().join("missing-parent").join("sess-dir");
    let err = move_into(&src_dir, &bad_target).expect_err("non-EXDEV rename error must fail");
    assert!(!is_cross_device_error(&err));
    assert!(src_dir.join("a.jsonl").is_file());
    assert!(src_dir.join("b.jsonl").is_file());

    // Simulated cross-device directory move where `remove_dir_all` fails partway through:
    // the intact backup in `target` must NEVER be deleted.
    let trash_target = dir.path().join("trash-sess-dir");
    let err = copy_and_remove_cross_device_with(
        &src_dir,
        &trash_target,
        |p| {
            let _ = fs::remove_file(p.join("a.jsonl"));
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "b.jsonl locked",
            ))
        },
        |p| fs::remove_file(p),
    )
    .expect_err("partial dir removal must report error");
    assert!(err.to_string().contains("intact backup preserved"));
    assert!(!src_dir.join("a.jsonl").exists());
    assert_eq!(
        fs::read_to_string(trash_target.join("a.jsonl")).unwrap(),
        "keep-a"
    );
    assert_eq!(
        fs::read_to_string(trash_target.join("b.jsonl")).unwrap(),
        "keep-b"
    );

    // Single-file cross-device removal failure is atomic on source, so duplicate target is cleaned.
    let single_src = dir.path().join("one.jsonl");
    let single_target = dir.path().join("trash-one.jsonl");
    fs::write(&single_src, "intact-file").unwrap();
    copy_and_remove_cross_device_with(
        &single_src,
        &single_target,
        |p| fs::remove_dir_all(p),
        |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "file locked",
            ))
        },
    )
    .expect_err("failed single-file removal must error");
    assert!(single_src.is_file());
    assert!(!single_target.exists());
}

#[cfg(unix)]
#[test]
fn claude_trash_rejects_symlinks_and_outside_containment() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside");
    let outside_secret = outside.join("secret.jsonl");
    let outside_env = outside.join("env");
    fs::create_dir_all(&outside_env).unwrap();
    fs::write(&outside_secret, "do-not-move").unwrap();
    fs::write(outside_env.join("keep.txt"), "do-not-move").unwrap();

    let config_dir = dir.path().join(".claude");
    let project_dir = config_dir.join("projects").join("proj-a");
    let env_dir = config_dir.join("session-env");
    fs::create_dir_all(&project_dir).unwrap();
    fs::create_dir_all(&env_dir).unwrap();

    // Symlinked main transcript must be rejected.
    let linked_main = project_dir.join("sess-link.jsonl");
    std::os::unix::fs::symlink(&outside_secret, &linked_main).unwrap();
    let trash_root = dir.path().join("trash");
    crate::collectors::claude_trash::delete_claude_session_in(
        &config_dir,
        &trash_root,
        "sess-link",
    )
    .expect_err("symlinked transcript must be rejected");
    assert!(outside_secret.is_file());

    // Real transcript with symlinked session-env must move only the real transcript.
    let real_main = project_dir.join("sess-real.jsonl");
    fs::write(&real_main, "{}\n").unwrap();
    if let Ok(f) = fs::OpenOptions::new().write(true).open(&real_main) {
        let _ = f.set_modified(SystemTime::now() - Duration::from_secs(120));
    }
    std::os::unix::fs::symlink(&outside_env, env_dir.join("sess-real")).unwrap();
    let moved = crate::collectors::claude_trash::delete_claude_session_in(
        &config_dir,
        &trash_root,
        "sess-real",
    )
    .expect("real transcript should move cleanly");
    assert!(moved.is_dir());
    assert!(!real_main.exists());
    assert!(outside_env.join("keep.txt").is_file());
}

#[test]
fn codex_trash_rejects_uncontained_or_mismatched_sqlite_rollout_path() {
    let dir = tempfile::tempdir().unwrap();
    let codex_root = dir.path().join(".codex");
    let sessions_dir = codex_root.join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    let outside_sessions = codex_root.join("target-sess.jsonl");
    let wrong_name = sessions_dir.join("rollout-2026-other-session.jsonl");
    fs::write(&outside_sessions, "{}\n").unwrap();
    fs::write(&wrong_name, "{}\n").unwrap();
    for p in [&outside_sessions, &wrong_name] {
        if let Ok(f) = fs::OpenOptions::new().write(true).open(p) {
            let _ = f.set_modified(SystemTime::now() - Duration::from_secs(120));
        }
    }

    let db_path = codex_root.join("state_5.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, source TEXT, rollout_path TEXT, archived INTEGER DEFAULT 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads (id, source, rollout_path) VALUES (?1, '', ?2)",
        rusqlite::params![
            "target-sess",
            outside_sessions.to_string_lossy().to_string()
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads (id, source, rollout_path) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            "child-sess",
            r#"{"subagent":{"thread_spawn":{"parent_thread_id":"target-sess"}}}"#,
            wrong_name.to_string_lossy().to_string()
        ],
    )
    .unwrap();
    drop(conn);

    let trash_root = dir.path().join("trash");
    crate::collectors::codex_trash::delete_codex_session_in(
        &codex_root,
        &trash_root,
        "target-sess",
    )
    .expect_err("uncontained or mismatched rollout_path must be rejected");
    assert!(outside_sessions.is_file());
    assert!(wrong_name.is_file());
}

#[test]
fn lookup_session_context_usage_resolves_antigravity_and_codex_state_n() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    // Antigravity store under `$HOME/.gemini/antigravity/brain/<id>`
    let ag_logs = home
        .join(".gemini")
        .join("antigravity")
        .join("brain")
        .join("ag-conv-1")
        .join(".system_generated")
        .join("logs");
    fs::create_dir_all(&ag_logs).unwrap();
    fs::write(
        ag_logs.join("transcript.jsonl"),
        "{\"step_index\":0,\"source\":\"MODEL\",\"type\":\"PLANNER_RESPONSE\",\"input_tokens\":12000,\"output_tokens\":300,\"cache_read_tokens\":4000}\n",
    )
    .unwrap();

    let ag_usage = lookup_session_context_usage(
        home,
        crate::domain::AgentType::Antigravity,
        "antigravity_ag-conv-1",
        None,
    )
    .expect("Antigravity context usage must resolve from $HOME/.gemini/antigravity");
    assert!(ag_usage.used >= 12_000);

    // Codex store with `state_6.sqlite` and `codex_` prefix
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    let db_path = codex_dir.join("state_6.sqlite");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE threads (id TEXT PRIMARY KEY, cwd TEXT, title TEXT, tokens_used INTEGER, updated_at INTEGER)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO threads (id, cwd, title, tokens_used, updated_at) VALUES ('cx-thread-6', '/tmp/ws', 'Hi', 42000, 1700000000)",
        [],
    )
    .unwrap();
    drop(conn);

    let cx_usage = lookup_session_context_usage(
        home,
        crate::domain::AgentType::Codex,
        "codex_cx-thread-6",
        None,
    )
    .expect("Codex context usage must strip codex_ prefix and discover state_6.sqlite");
    assert_eq!(cx_usage.used, 42_000);
}

#[test]
fn oversized_claude_line_ignores_tool_use_fields_and_synthetic_model() {
    let head = r#"{"type":"assistant","message":{"model":"claude-opus-4-6","id":"msg_real_01","role":"assistant","content":[{"type":"tool_use","id":"toolu_fake_99","input":{"model":"gpt-4o","note":"quoted \"model\":\"wrong\""}}]"#;
    let tail = r#","stop_reason":"tool_use","usage":{"input_tokens":500,"output_tokens":50,"cache_read_input_tokens":100,"cache_creation_input_tokens":0}}}"#;

    let mut ctx =
        LocalSessionContext::new(crate::domain::AgentType::ClaudeCode, "sess-ov".to_owned());
    apply_oversized_claude_record(head, tail, &mut ctx);
    assert_eq!(ctx.model.as_deref(), Some("claude-opus-4-6"));
    let header_slice = claude_message_header_slice(head);
    assert_eq!(
        extract_first_quoted_json_field(header_slice, "id").as_deref(),
        Some("msg_real_01")
    );
    assert_eq!(
        extract_first_quoted_json_field(header_slice, "model").as_deref(),
        Some("claude-opus-4-6")
    );
    assert_eq!(
        extract_first_quoted_json_field(
            r#"{"note":"escaped \"model\":\"fake\"","model":"claude-real"}"#,
            "model"
        )
        .as_deref(),
        Some("claude-real")
    );

    // Normal line with `<synthetic>` must be skipped in favor of subsequent real model.
    let dir = tempfile::tempdir().unwrap();
    let transcript = dir.path().join("sess-model.jsonl");
    let pad = "a".repeat(600_000);
    let oversized_line = format!(
        r#"{{"type":"assistant","message":{{"model":"claude-opus-4-6","id":"msg_big","content":[{{"type":"tool_use","id":"toolu_1","input":{{"model":"gpt-4","blob":"{pad}"}}}}]}}}}"#
    );
    fs::write(
        &transcript,
        format!(
            "{{\"type\":\"assistant\",\"message\":{{\"model\":\"<synthetic>\"}}}}\n{oversized_line}\n"
        ),
    )
    .unwrap();
    assert_eq!(
        find_claude_session_model_in_file(&transcript).as_deref(),
        Some("claude-opus-4-6")
    );
}

#[test]
fn bounded_message_usage_preserves_nonzero_figures_when_zero_follows() {
    let mut usage = BoundedMessageUsage::default();
    usage.record("msg_1".to_owned(), (500, 100, 200));
    usage.record("msg_1".to_owned(), (0, 0, 0));
    assert_eq!(usage.totals(), (500, 100, 200));
    assert_eq!(usage.request_count, 1);

    usage.record("msg_2".to_owned(), (0, 0, 0));
    usage.record("msg_2".to_owned(), (300, 40, 10));
    assert_eq!(usage.totals(), (800, 140, 210));
    assert_eq!(usage.request_count, 2);
}
#[test]
fn claude_process_start_fails_fast_on_invalid_resume_session_id() {
    let tmp = tempfile::tempdir().unwrap();
    let err = ClaudeProcessSession::start(
        tmp.path().join("nonexistent-claude"),
        tmp.path(),
        Some("--inject-flag"),
        TurnOptions::default(),
    )
    .err()
    .expect("invalid resume_session_id must fail fast");
    assert!(
        matches!(err, CollectorError::Protocol { ref code, .. } if code == "invalid_session_id"),
        "expected invalid_session_id protocol error, got: {err:?}"
    );
}
