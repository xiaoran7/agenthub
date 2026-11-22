use std::{collections::HashMap, fs};

use rusqlite::Connection;
use tempfile::tempdir;

use super::*;

#[test]
fn decodes_file_uris_across_platforms() {
    let win_uri = decode_file_uri_to_path("file:///d%3A/ClaudeSpace/Project%20A").unwrap();
    #[cfg(windows)]
    assert_eq!(win_uri, r"D:\ClaudeSpace\Project A");
    #[cfg(not(windows))]
    assert_eq!(win_uri, "D:/ClaudeSpace/Project A");

    assert_eq!(
        decode_file_uri_to_path("file:///Users/dev/workspace/"),
        Some("/Users/dev/workspace".to_owned())
    );
    assert_eq!(
        decode_file_uri_to_path("file://localhost/home/tanner/app"),
        Some("/home/tanner/app".to_owned())
    );
}

#[test]
fn parses_flexible_antigravity_timestamps() {
    let space_sep = parse_flexible_timestamp_ms("2026-10-19 13:23:15.7603919+00:00");
    let iso_sep = parse_flexible_timestamp_ms("2026-10-19T13:23:15.7603919+00:00");
    assert!(space_sep.is_some());
    assert_eq!(space_sep, iso_sep);
}

#[test]
fn reads_bom_config_and_state_without_failing() {
    let dir = tempdir().unwrap();
    let user_home = dir.path();
    let cfg_dir = user_home.join(".gemini").join("config");
    let ag_dir = user_home.join(".gemini").join("antigravity");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::create_dir_all(&ag_dir).unwrap();

    fs::write(
        cfg_dir.join("config.json"),
        "\u{feff}{\"plugins\":{\"p1\":{\"enabled\":true}},\"userSettings\":{\"autoExecutionPolicy\":\"CASCADE_COMMANDS_AUTO_EXECUTION_EAGER\"}}",
    )
    .unwrap();
    fs::write(
        ag_dir.join("antigravity_state.pbtxt"),
        "last_selected_agent_model: MODEL_PLACEHOLDER_M528\ninstallation_uuid: \"uuid-123\"\n",
    )
    .unwrap();
    fs::write(
        ag_dir.join("settings.json"),
        "\u{feff}{\"model\":\"gemini-3-pro\"}",
    )
    .unwrap();

    let cfg = load_antigravity_local_config_from(user_home, Some(&ag_dir));
    assert_eq!(cfg.enabled_plugins_count, 1);
    assert_eq!(
        cfg.auto_execution_policy.as_deref(),
        Some("CASCADE_COMMANDS_AUTO_EXECUTION_EAGER")
    );
    assert_eq!(cfg.installation_uuid.as_deref(), Some("uuid-123"));
    assert_eq!(cfg.state_model.as_deref(), Some("MODEL_PLACEHOLDER_M528"));
    assert_eq!(cfg.resolved_model.as_deref(), Some("gemini-3-pro"));
}

#[test]
fn detailed_collection_combines_sqlite_projects_and_subagents() {
    let dir = tempdir().unwrap();
    let user_home = dir.path();
    let ag_dir = user_home.join(".gemini").join("antigravity");
    let proj_dir = user_home.join(".gemini").join("config").join("projects");
    let parent_logs = ag_dir
        .join("brain")
        .join("sess-root")
        .join(".system_generated")
        .join("logs");
    let child_logs = ag_dir
        .join("brain")
        .join("sess-child")
        .join(".system_generated")
        .join("logs");
    fs::create_dir_all(&proj_dir).unwrap();
    fs::create_dir_all(&parent_logs).unwrap();
    fs::create_dir_all(&child_logs).unwrap();

    fs::write(
        proj_dir.join("proj-1.json"),
        "\u{feff}{\"id\":\"proj-1\",\"projectResources\":{\"resources\":[{\"folderUri\":\"file:///Users/dev/demo-repo\"}]}}",
    )
    .unwrap();

    let db_path = ag_dir.join("conversation_summaries.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            step_count INTEGER,
            last_modified_time TEXT,
            workspace_uris TEXT,
            status TEXT,
            project_id TEXT,
            agent_name TEXT,
            parent_conversation_id TEXT,
            nesting_depth INTEGER,
            not_fully_idle INTEGER,
            killed INTEGER
        );
        INSERT INTO conversation_summaries VALUES
            ('sess-root', 'Refactor API', 'Refactor API routes', 12, '2026-11-15 10:00:00+00:00', '', 'CASCADE_RUN_STATUS_RUNNING', 'proj-1', '', '', 0, 1, 0),
            ('sess-child', 'Subtask Inspect', 'Check handlers', 4, '2026-11-15 10:01:00+00:00', '[\"file:///Users/dev/demo-repo\"]', 'CASCADE_RUN_STATUS_IDLE', 'proj-1', 'research', 'sess-root', 1, 0, 0);",
    )
    .unwrap();
    drop(conn);

    // transcript_full.jsonl preferred over transcript.jsonl
    fs::write(
        parent_logs.join("transcript_full.jsonl"),
        concat!(
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","created_at":"2026-11-15T10:00:00Z","content":"<USER_REQUEST>Ignored fallback title</USER_REQUEST>"}"#,
            "\n",
            r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-11-15T10:00:05Z","input_tokens":1000,"cache_read_tokens":2000,"output_tokens":300,"tool_calls":[{"name":"run_command","args":{"CommandLine":"cargo test"}}]}"#,
            "\n"
        ),
    )
    .unwrap();
    fs::write(
        child_logs.join("transcript.jsonl"),
        r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-11-15T10:01:02Z","input_tokens":500,"cache_read_tokens":100,"output_tokens":150}"#,
    )
    .unwrap();

    let entries =
        collect_antigravity_sessions_detailed_from(user_home, std::slice::from_ref(&ag_dir))
            .unwrap();
    assert_eq!(entries.len(), 1);
    let root = &entries[0];
    assert_eq!(root.session_id, "sess-root");
    assert_eq!(root.title.as_deref(), Some("Refactor API"));
    assert_eq!(root.cwd, "/Users/dev/demo-repo");
    assert!(root.is_running);
    assert_eq!(root.prompt_tokens, 3000);
    assert_eq!(root.cache_read_tokens, 2000);
    assert_eq!(root.completion_tokens, 300);
    assert_eq!(root.context_used_tokens, 3000);
    assert_eq!(root.context_window_tokens, 1_000_000);
    assert_eq!(root.request_count, 1);
    assert_eq!(root.subagents.len(), 1);
    assert_eq!(root.subagents[0].id, "sess-child");
    assert_eq!(root.subagents[0].kind, "research");
    assert_eq!(root.subagents[0].prompt_tokens, 600);

    // Check contexts also expose resolved model & tokens
    let contexts = collect_antigravity_session_contexts_from(&ag_dir, 10).unwrap();
    let root_ctx = contexts
        .iter()
        .find(|c| c.agent_session_id == "sess-root")
        .unwrap();
    assert_eq!(root_ctx.title.as_deref(), Some("Refactor API"));
    assert_eq!(root_ctx.context_used_tokens, Some(3000));
    assert_eq!(root_ctx.model.as_deref(), Some("gemini-3.8-flash"));

    let _ = HashMap::<String, String>::new();
}

#[test]
fn build_stream_args_maps_permissions_and_filters_model_placeholders() {
    assert_eq!(
        process::build_antigravity_stream_args(
            Some("conv-1"),
            Some("MODEL_PLACEHOLDER_M528"),
            Some("bypassPermissions"),
        ),
        vec![
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--conversation",
            "conv-1",
            "--dangerously-skip-permissions",
        ]
    );
    assert_eq!(
        process::build_antigravity_stream_args(None, Some("gemini-3.8-flash"), Some("acceptEdits")),
        vec![
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--model",
            "gemini-3.8-flash",
            "--mode",
            "accept-edits",
        ]
    );
}

#[test]
fn install_probe_recognizes_agy_binary_under_local_bin() {
    let dir = tempdir().unwrap();
    let local_bin = dir.path().join(".local").join("bin");
    fs::create_dir_all(&local_bin).unwrap();
    let agy = local_bin.join("agy");
    fs::write(&agy, "#!/bin/sh\n").unwrap();

    let (installed, path) = config::check_antigravity_installed_at(Some(dir.path()));
    assert!(installed);
    assert_eq!(path.as_deref(), Some(agy.to_string_lossy().as_ref()));
}

#[test]
fn parses_antigravity_stream_json_events_usage_and_final_message() {
    let ndjson = concat!(
        r#"{"event":"init","conversation_id":"conv-999","model":"gemini-3.8-flash"}"#,
        "\n",
        r#"{"event":"step_update","step_update":{"thinking":"planning fix","step_type":"thinking"}}"#,
        "\n",
        r#"{"event":"step_update","step_update":{"tool_name":"edit_file","state":"ACTIVE"}}"#,
        "\n",
        r#"{"event":"step_update","step_update":{"text_delta":"Part A "}}"#,
        "\n",
        r#"{"event":"result","result":{"conversation_id":"conv-999","status":"SUCCESS","response":"Part A + Part B","usage":{"input_tokens":1200,"cache_read_tokens":800,"output_tokens":90}}}"#,
        "\n"
    );
    assert_eq!(
        crate::collectors::extract_session_id_from_output(
            &crate::domain::AgentType::Antigravity,
            ndjson,
        )
        .as_deref(),
        Some("conv-999")
    );
    assert_eq!(
        crate::collectors::final_agent_message(&crate::domain::AgentType::Antigravity, ndjson)
            .as_deref(),
        Some("Part A + Part B")
    );
    let usage = crate::collectors::extract_context_usage_from_output(
        &crate::domain::AgentType::Antigravity,
        ndjson,
    )
    .expect("usage extracted");
    assert_eq!(usage.used, 2000);
    assert_eq!(usage.window, Some(1_000_000));

    let tool_ev: serde_json::Value = serde_json::from_str(
        r#"{"event":"step_update","step_update":{"tool_name":"edit_file","state":"ACTIVE"}}"#,
    )
    .unwrap();
    assert_eq!(
        stream::parse_antigravity_turn_update(&tool_ev),
        Some(crate::collectors::AgentTurnUpdate::Activity(
            "Antigravity 正在修改文件"
        ))
    );
}
#[test]
fn extracts_model_selection_from_user_settings_change_and_parent_id_table() {
    let dir = tempdir().unwrap();
    let user_home = dir.path();
    let ag_dir = user_home.join(".gemini").join("antigravity");
    let parent_logs = ag_dir
        .join("brain")
        .join("conv-parent")
        .join(".system_generated")
        .join("logs");
    let child_logs = ag_dir
        .join("brain")
        .join("conv-sub")
        .join(".system_generated")
        .join("logs");
    fs::create_dir_all(&parent_logs).unwrap();
    fs::create_dir_all(&child_logs).unwrap();

    let db_path = ag_dir.join("conversation_summaries.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            step_count INTEGER,
            last_modified_time TEXT,
            workspace_uris TEXT,
            status TEXT,
            agent_name TEXT
        );
        CREATE TABLE conversation_parents (
            conversation_id TEXT NOT NULL,
            parent_id TEXT NOT NULL,
            PRIMARY KEY (conversation_id, parent_id)
        ) WITHOUT ROWID;
        INSERT INTO conversation_summaries VALUES
            ('conv-parent', 'Parent Task', '', 4, '2026-11-18T05:00:00Z', '[\"file:///Users/dev/app\"]', 'CASCADE_RUN_STATUS_IDLE', ''),
            ('conv-sub', 'Research Codebase', '', 2, '2026-11-18T05:01:00Z', '[\"file:///Users/dev/app\"]', 'CASCADE_RUN_STATUS_IDLE', 'research');
        INSERT INTO conversation_parents VALUES ('conv-sub', 'conv-parent');",
    )
    .unwrap();

    fs::write(
        parent_logs.join("transcript.jsonl"),
        concat!(
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","created_at":"2026-11-18T05:00:00Z","content":"<USER_REQUEST>Fix auth</USER_REQUEST>\n<USER_SETTINGS_CHANGE>\nThe user changed setting `Model Selection` from None to Gemini 4 Carbon (High). No need to comment on this change.\n</USER_SETTINGS_CHANGE>"}"#,
            "\n",
            r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-11-18T05:00:05Z","input_tokens":2000,"output_tokens":150}"#,
            "\n"
        ),
    )
    .unwrap();
    fs::write(
        child_logs.join("transcript.jsonl"),
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","created_at":"2026-11-18T05:01:00Z","content":"Inspect auth middleware"}"#,
    )
    .unwrap();

    let entries =
        collect_antigravity_sessions_detailed_from(user_home, std::slice::from_ref(&ag_dir))
            .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].model.as_deref(), Some("gemini-4-carbon-high"));
    assert_eq!(entries[0].subagents.len(), 1);
    assert_eq!(entries[0].subagents[0].id, "conv-sub");
    assert_eq!(entries[0].subagents[0].description, "Research Codebase");
}

#[test]
fn normalizes_antigravity_generic_tool_outputs_and_edit_diffs() {
    let records: Vec<serde_json::Value> = vec![
        serde_json::from_str(
            r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>Update main.rs</USER_REQUEST>"}"#,
        )
        .unwrap(),
        serde_json::from_str(
            r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","thinking":"Checking and patching main.rs","tool_calls":[{"name":"edit_file","args":{"Path":"src/main.rs","Target":"fn old() {}","Replacement":"fn updated() {}"}}],"content":""}"#,
        )
        .unwrap(),
        serde_json::from_str(
            r#"{"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","content":"Created At: 2026-11-18T05:00:01Z\nCompleted At: 2026-11-18T05:00:02Z\nEdited file successfully."}"#,
        )
        .unwrap(),
        serde_json::from_str(
            r#"{"step_index":3,"source":"MODEL","type":"PLANNER_RESPONSE","content":"Done updating `src/main.rs`."}"#,
        )
        .unwrap(),
    ];

    let normalized = crate::conversation::antigravity::normalize_antigravity_records(&records);
    assert_eq!(normalized.len(), 4);
    assert_eq!(normalized[0]["kind"], "message");
    assert_eq!(normalized[0]["role"], "user");
    assert_eq!(normalized[1]["kind"], "thinking");
    assert_eq!(normalized[2]["kind"], "tool");
    let tool_body = normalized[2]["content"].as_str().unwrap();
    assert!(tool_body.contains("--- a/src/main.rs"));
    assert!(tool_body.contains("-fn old() {}"));
    assert!(tool_body.contains("+fn updated() {}"));
    assert!(tool_body.contains("〔结果〕\nEdited file successfully."));
    assert!(!tool_body.contains("Created At:"));
    assert_eq!(normalized[3]["kind"], "message");
    assert_eq!(normalized[3]["role"], "assistant");
    assert_eq!(normalized[3]["content"], "Done updating `src/main.rs`.");
}
