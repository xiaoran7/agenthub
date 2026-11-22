use super::*;

#[cfg(test)]
#[test]
fn remote_receipt_projection_keeps_only_hub_results() {
    let receipt = CommandReceipt {
        client_cmd_id: "command-one".into(),
        hub_session_id: "hub-one".into(),
        status: "accepted".into(),
        code: None,
        result: Some(json!({
            "binding": {
                "hub_session_id": "hub-one",
                "provider_session_id": "native-one",
                "workspace_path": "C:\\Users\\alice\\private",
                "host_generation": "generation-one",
                "control_connected": true,
                "execution_state": "idle",
                "current_turn_id": null
            },
            "turn": {"id":"turn-one","status":{"type":"inProgress"}},
            "provider_response": {
                "thread":{"id":"native-one","cwd":"C:\\Users\\alice\\private"},
                "access_token":"usable-secret"
            }
        })),
    };

    let projected = projected_remote_receipt(&receipt);

    assert_eq!(projected["result"]["turn"], json!({"id":"turn-one"}));
    assert!(projected["result"].get("provider_response").is_none());
    assert!(
        projected["result"]["binding"]["workspace_path"]
            .as_str()
            .is_some_and(|workspace| workspace.starts_with("ws-"))
    );
    let serialized = projected.to_string();
    for forbidden in ["alice", "usable-secret", "provider_response", "inProgress"] {
        assert!(!serialized.contains(forbidden));
    }
    assert_eq!(
        receipt.result.as_ref().unwrap()["provider_response"]["access_token"],
        "usable-secret"
    );
}

#[cfg(test)]
#[test]
fn approval_remote_projection_omits_provider_request_payload() {
    let approval = PendingApprovalSnapshot {
        approval_id: format!("approval-{}", "a".repeat(64)),
        hub_session_id: "hub-one".into(),
        host_generation: "generation-one".into(),
        turn_id: "turn-one".into(),
        can_approve: true,
        can_decline: false,
        tool_name: Some("Bash".into()),
        command_preview: Some("cargo test".into()),
        path: None,
        diff: None,
        description: Some("Run unit tests".into()),
        risk_level: Some("high".into()),
    };
    let event = crate::remote_host::HostEvent {
        event_id: "event-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("native-one".into()),
        kind: "approval_requested".into(),
        raw: json!({
            "id": 7,
            "method": "item/commandExecution/requestApproval",
            "params": {"command":"private","reason":"secret","diff":"sensitive"}
        }),
        binding: None,
        approval_id: Some(approval.approval_id.clone()),
        approval: Some(approval.clone()),
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    assert!(projected["event"].get("raw").is_none());
    assert_eq!(projected["event"]["approval"], json!(approval));
    for forbidden in ["private", "secret", "sensitive", "\"id\":7"] {
        assert!(!projected.to_string().contains(forbidden));
    }
    let local = json!({"type":"event","event":event});
    assert_eq!(local["event"]["raw"]["id"], 7);
    assert_eq!(local["event"]["raw"]["params"]["command"], "private");
}

#[cfg(test)]
#[test]
fn history_snapshot_projects_normalized_items_without_raw_paths() {
    let event = crate::remote_host::HostEvent {
        event_id: "history-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("sess-one".into()),
        kind: "history/snapshot".into(),
        raw: json!({"method":"history/snapshot","params":{
            "items":[{"role":"user","kind":"message","text":"检查 C:\\Users\\alice\\secret.txt","status":"completed"}],
            "provider_private":"do-not-send"
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    assert_eq!(
        projected["event"]["raw"]["params"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let encoded = projected.to_string();
    assert!(!encoded.contains("alice"));
    assert!(!encoded.contains("do-not-send"));
}

#[cfg(test)]
#[test]
fn history_snapshot_projects_conversation_content_as_text() {
    // `load_session_conversation` writes the body under `content`; clients read
    // `text`. The projection is the one place allowed to bridge the two.
    let event = crate::remote_host::HostEvent {
        event_id: "history-two".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("sess-one".into()),
        kind: "history/snapshot".into(),
        raw: json!({"method":"history/snapshot","params":{
            "items":[
                {"role":"user","kind":"message","label":"","status":"completed","content":"开始"},
                {"role":"assistant","kind":"tool","label":"Bash · 结果","status":"completed","content":"ok"}
            ]
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    let items = projected["event"]["raw"]["params"]["items"]
        .as_array()
        .unwrap();
    assert_eq!(items[0]["text"], "开始");
    assert_eq!(items[1]["text"], "ok");
    assert_eq!(items[1]["label"], "Bash · 结果");
    assert!(items[0].get("content").is_none());
}

#[cfg(test)]
#[test]
fn agent_message_projects_content_blocks_as_text() {
    let event = crate::remote_host::HostEvent {
        event_id: "agent-msg-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("sess-one".into()),
        kind: "item/completed".into(),
        raw: json!({"method":"item/completed","params":{
            "item": {
                "id": "msg-123",
                "type": "agentMessage",
                "content": [
                    {"type": "output_text", "text": "第一段回复"},
                    {"type": "output_text", "text": "第二段回复"}
                ]
            }
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    let item = &projected["event"]["raw"]["params"]["item"];
    assert_eq!(item["type"], "agentMessage");
    assert_eq!(item["id"], "msg-123");
    assert_eq!(item["text"], "第一段回复\n第二段回复");
}

#[cfg(test)]
#[test]
fn started_tool_items_project_only_render_fields() {
    let event = crate::remote_host::HostEvent {
        event_id: "started-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("native-one".into()),
        kind: "item/started".into(),
        raw: json!({"method":"item/started","params":{"item":{
            "id":"tool-one","type":"commandExecution","command":"cargo test",
            "input":{"secret":"do-not-send"}
        }}}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    assert_eq!(
        projected["event"]["raw"]["params"]["item"],
        json!({"id":"tool-one","type":"commandExecution","command":"cargo test"})
    );
    assert!(!projected.to_string().contains("do-not-send"));
}

#[cfg(test)]
#[test]
fn codex_token_usage_projects_a_canonical_pair() {
    let event = crate::remote_host::HostEvent {
        event_id: "usage-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("native-one".into()),
        kind: "thread/tokenUsage/updated".into(),
        raw: json!({"method":"thread/tokenUsage/updated","params":{
            "threadId":"native-one",
            "turnId":"turn-one",
            "tokenUsage":{
                "last":{"inputTokens":42000,"cachedInputTokens":9000,"outputTokens":100},
                "total":{"inputTokens":88000,"totalTokens":90000},
                "modelContextWindow":200000,
                "internalBreakdown":"do-not-send"
            }
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    assert_eq!(
        projected["event"]["raw"]["params"]["tokenUsage"],
        json!({"used":42000,"window":200000})
    );
    assert!(!projected.to_string().contains("do-not-send"));
}

#[cfg(test)]
#[test]
fn token_usage_without_a_window_still_reports_consumption() {
    let event = crate::remote_host::HostEvent {
        event_id: "usage-two".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("native-one".into()),
        kind: "thread/tokenUsage/updated".into(),
        raw: json!({"method":"thread/tokenUsage/updated","params":{
            "tokenUsage":{"last":{"inputTokens":12345}}
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    assert_eq!(
        projected["event"]["raw"]["params"]["tokenUsage"],
        json!({"used":12345})
    );
}

#[cfg(test)]
#[test]
fn error_events_project_the_cause_without_raw_provider_fields() {
    let event = crate::remote_host::HostEvent {
        event_id: "error-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("native-one".into()),
        kind: "error".into(),
        raw: json!({"method":"error","params":{
            "threadId":"native-one",
            "turnId":"turn-one",
            "willRetry":false,
            "error":{
                "message":"The 'gpt-6-luna' model is not supported; check C:\\Users\\alice\\codex\\config.toml",
                "codexErrorInfo":{"usageLimitReached":{"httpStatusCode":429,"internalStack":"do-not-send"}},
                "additionalDetails":{"internalStack":"do-not-send"}
            }
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };

    let projected = projected_remote_event(&event).unwrap();

    // The console can now explain the failure instead of showing a bare state.
    let params = &projected["event"]["raw"]["params"];
    assert_eq!(params["turnId"], json!("turn-one"));
    assert_eq!(params["willRetry"], json!(false));
    let message = params["error"]["message"].as_str().unwrap();
    assert!(message.contains("gpt-6-luna"));
    // `codexErrorInfo` is an object keyed by kind, not a string: the kind survives
    // along with the one scalar that says what to do, and nothing else.
    assert_eq!(
        params["error"]["codexErrorInfo"],
        json!({"usageLimitReached":{"httpStatusCode":429}})
    );
    // Absolute paths in the provider text are rewritten, never forwarded raw.
    assert!(!message.contains("alice"));
    assert!(!message.contains("config.toml"));
    // Fields outside the allowlist stay on the machine.
    let encoded = projected.to_string();
    assert!(!encoded.contains("do-not-send"));
    assert!(!encoded.contains("internalStack"));
    assert!(!encoded.contains("additionalDetails"));
    // The local copy keeps the original for the operator's own diagnostics.
    assert_eq!(
        event.raw["params"]["error"]["additionalDetails"]["internalStack"],
        json!("do-not-send")
    );
}

#[cfg(test)]
#[test]
fn error_events_without_a_known_shape_are_dropped_rather_than_guessed() {
    for kind in [
        json!({"method":"error"}),                       // no params
        json!({"method":"error","params":{}}),           // no error object
        json!({"method":"error","params":{"error":{}}}), // no message
    ] {
        let event = crate::remote_host::HostEvent {
            event_id: "error-one".into(),
            host_generation: "generation-one".into(),
            hub_session_id: Some("hub-one".into()),
            provider_session_id: Some("native-one".into()),
            kind: "error".into(),
            raw: kind,
            binding: None,
            approval_id: None,
            approval: None,
            question_id: None,
            question: None,
        };
        let projected = projected_remote_event(&event).unwrap();
        assert!(
            projected["event"].get("raw").is_none(),
            "an unrecognised error must not invent a cause"
        );
    }
}

#[cfg(test)]
mod inbound_journal_tests {
    use super::*;

    fn command(id: &str, generation: &str) -> RemoteCommand {
        RemoteCommand {
            client_cmd_id: id.into(),
            hub_session_id: "hub-one".into(),
            host_generation: Some(generation.into()),
            command: crate::remote_host::RemoteAction::Connect {
                mode: crate::remote_host::ConnectMode::Resume,
                workspace_path: "ws-example-0123456789abcdef".into(),
                provider_session_id: Some("claude_native-one".into()),
                model: None,
                sandbox: crate::remote_host::CodexSandbox::ReadOnly,
                approval_policy: crate::remote_host::CodexApprovalPolicy::Untrusted,
                agent_type: None,
                permission_mode: None,
            },
        }
    }

    fn receipt(id: &str) -> CommandReceipt {
        CommandReceipt {
            client_cmd_id: id.into(),
            hub_session_id: "hub-one".into(),
            status: "accepted".into(),
            code: None,
            result: None,
        }
    }

    fn open() -> (tempfile::TempDir, RemoteJournal) {
        let dir = tempfile::tempdir().unwrap();
        let journal = RemoteJournal::open(dir.path().join("remote.db")).unwrap();
        (dir, journal)
    }

    /// Applies `seq` end to end so the journal records both the intent and the
    /// receipt, exactly as a live command does.
    fn apply(journal: &mut RemoteJournal, seq: i64, command: &RemoteCommand) {
        assert!(matches!(
            journal.begin_inbound_command(seq, command).unwrap(),
            BeginInboundCommand::New
        ));
        journal
            .finish_inbound_command(seq, &receipt(&command.client_cmd_id), &[])
            .unwrap();
    }

    #[test]
    fn a_reissued_sequence_from_a_rebuilt_gateway_replaces_the_old_command() {
        // The state a rebuilt Gateway leaves behind: the applied watermark was
        // rewound, but the rows of the commands it no longer knows about stayed.
        // Sequence 1 then arrives carrying a different command, and rejecting it
        // fenced the transport on every reconnect.
        let (_dir, mut journal) = open();
        let stale = command("cmd-stale", "generation-old");
        apply(&mut journal, 1, &stale);
        journal
            .connection
            .execute("UPDATE remote_inbound SET received=0 WHERE singleton=1", [])
            .expect("emulate the watermark a rebuilt Gateway leaves behind");

        let current = command("cmd-current", "generation-new");
        assert!(matches!(
            journal.begin_inbound_command(1, &current).unwrap(),
            BeginInboundCommand::New
        ));
        journal
            .finish_inbound_command(1, &receipt("cmd-current"), &[])
            .unwrap();
        assert_eq!(journal.received_command_seq().unwrap(), 1);
    }

    #[test]
    fn a_reissued_sequence_never_overrides_an_unconfirmed_intent() {
        // An intent without a receipt may already have run. Replacing it would
        // let a different command at the same sequence execute a second time,
        // so the mismatch must keep failing even above the watermark.
        let (_dir, mut journal) = open();
        journal
            .begin_inbound_command(1, &command("cmd-stale", "generation-old"))
            .unwrap();
        assert!(matches!(
            journal.begin_inbound_command(1, &command("cmd-other", "generation-new")),
            Err(JournalError::InvalidAck)
        ));
    }

    #[test]
    fn a_different_command_inside_the_applied_range_still_fails() {
        // A mismatch the Host has already acknowledged is a real protocol
        // fault, not a rebuilt journal, and must keep fencing the transport.
        let (_dir, mut journal) = open();
        apply(&mut journal, 1, &command("cmd-one", "generation-one"));
        let conflict = command("cmd-other", "generation-one");
        assert!(matches!(
            journal.begin_inbound_command(1, &conflict),
            Err(JournalError::InvalidAck)
        ));
    }

    #[test]
    fn resetting_the_inbound_sequence_forgets_rows_above_it() {
        let (_dir, mut journal) = open();
        apply(&mut journal, 1, &command("cmd-one", "generation-one"));
        apply(&mut journal, 2, &command("cmd-two", "generation-one"));
        apply(&mut journal, 3, &command("cmd-three", "generation-one"));
        assert_eq!(journal.received_command_seq().unwrap(), 3);

        journal.reset_inbound_command_seq(1).unwrap();
        assert_eq!(journal.received_command_seq().unwrap(), 1);

        // Replaying the forgotten sequences applies them again instead of
        // colliding with rows from the vanished journal.
        let replayed = command("cmd-replayed", "generation-two");
        assert!(matches!(
            journal.begin_inbound_command(2, &replayed).unwrap(),
            BeginInboundCommand::New
        ));
        journal
            .finish_inbound_command(2, &receipt("cmd-replayed"), &[])
            .unwrap();
        assert_eq!(journal.received_command_seq().unwrap(), 2);
    }

    #[test]
    fn journal_migrates_legacy_db_to_incremental_vacuum_and_reclaims_trimmed_pages() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("legacy_journal.sqlite");

        // Pre-create a legacy database without incremental auto_vacuum.
        {
            let legacy = Connection::open(&db_path).unwrap();
            legacy
                .execute_batch(
                    "PRAGMA auto_vacuum = NONE;
                     CREATE TABLE remote_commands (cmd_id TEXT PRIMARY KEY, command_json TEXT NOT NULL, receipt_json TEXT);
                     CREATE TABLE remote_sessions (hub_id TEXT PRIMARY KEY, native_id TEXT NOT NULL UNIQUE, binding_json TEXT NOT NULL);
                     CREATE TABLE remote_outbox (seq INTEGER PRIMARY KEY AUTOINCREMENT, payload_json TEXT NOT NULL);
                     CREATE TABLE local_events (cursor INTEGER PRIMARY KEY AUTOINCREMENT, payload_json TEXT NOT NULL);
                     CREATE TABLE remote_watermarks (singleton INTEGER PRIMARY KEY CHECK(singleton=1), sent INTEGER NOT NULL, acked INTEGER NOT NULL);
                     INSERT OR IGNORE INTO remote_watermarks VALUES (1,0,0);
                     CREATE TABLE remote_inbound (singleton INTEGER PRIMARY KEY CHECK(singleton=1), received INTEGER NOT NULL);
                     INSERT OR IGNORE INTO remote_inbound VALUES (1,0);
                     CREATE TABLE remote_inbound_commands (seq INTEGER PRIMARY KEY, cmd_id TEXT NOT NULL, command_json TEXT NOT NULL);
                     CREATE TABLE remote_identity (singleton INTEGER PRIMARY KEY CHECK(singleton=1), device_id TEXT NOT NULL, server TEXT NOT NULL);",
                )
                .unwrap();
        }

        let mut journal = RemoteJournal::open_with_event_retention(&db_path, 4).unwrap();
        let av: i64 = journal
            .connection
            .query_row("PRAGMA auto_vacuum;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            av, 2,
            "opening a legacy journal must migrate it to PRAGMA auto_vacuum = INCREMENTAL"
        );

        let large_text = "x".repeat(4_096);
        let batch: Vec<_> = (0..40)
            .map(|_| json!({ "event": "burst", "blob": large_text }))
            .collect();
        for payload in &batch {
            journal.append(payload, &[]).unwrap();
        }
        for seq in 1..=batch.len() {
            journal.mark_sent(seq as i64).unwrap();
        }
        journal.acknowledge(batch.len() as i64).unwrap();
        journal.reclaim_space(512).unwrap();
        let freelist: i64 = journal
            .connection
            .query_row("PRAGMA freelist_count;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            freelist, 0,
            "incremental vacuum should reclaim trimmed/acked pages"
        );
    }
}
