use super::*;
use std::io::Write;

fn message(role: &str, text: &str) -> Value {
    json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":"output_text","text":text}]}})
}

#[test]
fn oversized_rollout_keeps_real_opening_and_latest_answer_after_tool_flood() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".codex/sessions");
    fs::create_dir_all(&dir).unwrap();
    let mut file = fs::File::create(dir.join("rollout-large.jsonl")).unwrap();
    writeln!(file, "{}", message("user", "actual opening")).unwrap();
    let padding = json!({"type":"turn_context","payload":{"type":"turn_context","padding":"x".repeat(18000)}}).to_string();
    for _ in 0..2200 {
        writeln!(file, "{padding}").unwrap();
    }
    writeln!(
        file,
        "{}",
        message("assistant", "final natural language answer")
    )
    .unwrap();
    for i in 0..500 {
        writeln!(file, "{}", json!({"type":"response_item","payload":{"type":"function_call","name":"Read","arguments":format!("tool-{i}")}})).unwrap();
    }
    drop(file);
    let items = load_session_conversation(home.path(), AgentType::Codex, "large");
    assert_eq!(items[0]["content"], "actual opening");
    assert!(
        items
            .iter()
            .any(|i| i["content"] == "final natural language answer")
    );
    assert!(items.iter().any(|i| i["label"] == "历史记录折叠"));
    assert!(serde_json::to_vec(&items).unwrap().len() < MAX_CONVERSATION_RESPONSE_BYTES);
}

#[test]
fn verified_fragments_merge_by_record_time_not_file_mtime_and_deduplicate_overlap() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".codex/sessions");
    fs::create_dir_all(&dir).unwrap();
    let mut first = message("user", "opening");
    first["timestamp"] = json!("2026-01-01T00:00:00Z");
    let mut last = message("assistant", "latest");
    last["timestamp"] = json!("2026-01-02T00:00:00Z");
    let meta =
        json!({"type":"session_meta","timestamp":"2026-01-01T00:00:00Z","payload":{"id":"parts"}});
    fs::write(
        dir.join("a-parts.jsonl"),
        format!("{meta}\n{first}\n{last}\n"),
    )
    .unwrap();
    fs::write(dir.join("z-parts.jsonl"), format!("{meta}\n{first}\n")).unwrap();
    let wrong = json!({"type":"session_meta","payload":{"id":"another"}});
    fs::write(
        dir.join("other-parts.jsonl"),
        format!("{wrong}\n{}\n", message("assistant", "wrong session")),
    )
    .unwrap();
    let items = load_session_conversation(home.path(), AgentType::Codex, "parts");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["content"], "opening");
    assert_eq!(items[1]["content"], "latest");
}

#[test]
fn partial_lines_are_not_misparsed_and_single_message_keeps_both_ends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("window.jsonl");
    fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n{{unfinished",
            message("user", "start"),
            message("assistant", &"x".repeat(2000)),
            message("assistant", "end")
        ),
    )
    .unwrap();
    let (values, omitted) = read_transcript_windows(&path, 512);
    assert!(omitted);
    let texts = values
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    assert!(texts.iter().any(|l| l.contains("start")));
    assert!(texts.iter().any(|l| l.contains("end")));
    // Every returned record is already-parsed JSON, so a truncated line can
    // never reach the caller.
    assert!(values.iter().all(Value::is_object));
    let item = conversation_item(
        "assistant",
        "message",
        "",
        &format!("FIRST{}LAST", "中".repeat(30000)),
        "completed",
    );
    assert!(item["content"].as_str().unwrap().starts_with("FIRST"));
    assert!(item["content"].as_str().unwrap().ends_with("LAST"));
}

#[test]
fn claude_internal_injection_is_not_a_user_turn_but_genuine_text_is_preserved() {
    for text in [
        "Base directory for this skill: C:/skills/playwright\n# Playwright CLI Skill",
        "<local-command-stdout>done</local-command-stdout>",
    ] {
        let value = json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":text}]}});
        assert!(normalize_local_conversation_items(&value).is_empty());
    }
    let real = json!({"type":"user","message":{"role":"user","content":"请修复 Playwright CLI Skill 的文档\n<system-reminder>internal</system-reminder>"}});
    let items = normalize_local_conversation_items(&real);
    assert_eq!(items[0]["content"], "请修复 Playwright CLI Skill 的文档");
    let meta = json!({"type":"user","isMeta":true,"message":{"role":"user","content":"injected"}});
    assert!(normalize_local_conversation_items(&meta).is_empty());
}

#[test]
fn serialized_budget_counts_json_escapes_and_unicode() {
    let mut items = vec![conversation_item(
        "user",
        "message",
        "",
        "FIRST",
        "completed",
    )];
    for _ in 0..500 {
        items.push(conversation_item(
            "assistant",
            "message",
            "",
            &"\"\\\n中文".repeat(4000),
            "completed",
        ));
    }
    items.push(conversation_item(
        "assistant",
        "message",
        "",
        "LAST",
        "completed",
    ));
    let output = budget_conversation(items, false);
    assert_eq!(output[0]["content"], "FIRST");
    assert_eq!(output.last().unwrap()["content"], "LAST");
    assert!(serde_json::to_vec(&output).unwrap().len() <= MAX_CONVERSATION_RESPONSE_BYTES);
}
