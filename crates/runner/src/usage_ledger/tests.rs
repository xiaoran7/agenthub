use super::*;

fn record(t: i64, agent: &'static str, model: &str, input: u64, output: u64) -> LedgerRecord {
    LedgerRecord {
        t,
        asked: 0,
        agent,
        provider: resolve_provider_label(agent, model, &HashMap::new()),
        model: intern(model),
        session: intern("s"),
        cwd: intern(""),
        input,
        output,
        cache_read: 0,
        cache_write: 0,
        duration_ms: None,
        error: None,
        cost: calc_cost(model, input, output, 0),
        retried: false,
        recovered: false,
    }
}

#[test]
fn a_failure_and_its_retry_are_paired() {
    let now = chrono::Local::now().timestamp_millis();
    let mut failed = record(now - 20_000, "claude_code", "", 0, 0);
    failed.error = Some(intern("rate_limit"));
    let answered = record(now, "claude_code", "claude-sonnet-5", 10, 20);
    let mut records = vec![answered, failed];
    records.sort_by_key(|r| std::cmp::Reverse(r.t));
    mark_retries(&mut records);
    let d = digest(&records, Period::Today, 10);
    assert_eq!(d.calls, 2);
    assert_eq!(d.retried, 1);
    assert_eq!(d.recovered, 1);
    assert_eq!(d.errors, 0, "a failure a retry answered is not counted");
}

#[test]
fn a_failure_a_day_later_is_its_own() {
    let now = chrono::Local::now().timestamp_millis();
    let mut failed = record(now - 86_400_000, "claude_code", "", 0, 0);
    failed.error = Some(intern("server_error"));
    let mut records = vec![record(now, "claude_code", "claude-opus-5", 10, 20), failed];
    records.sort_by_key(|r| std::cmp::Reverse(r.t));
    mark_retries(&mut records);
    let d = digest(&records, Period::Days(7), 10);
    assert_eq!(d.retried, 0);
    assert_eq!(d.errors, 1);
}

#[test]
fn sessions_carry_their_last_context() {
    let now = chrono::Local::now().timestamp_millis();
    let mut first = record(now - 5_000, "claude_code", "claude-sonnet-5", 1_000, 10);
    first.session = intern("s1");
    let mut last = record(now, "claude_desktop", "claude-opus-5", 4_000, 20);
    last.session = intern("s1");
    last.cache_read = 6_000;
    let d = digest(&[first, last], Period::Today, 10);
    assert_eq!(d.sessions, 1);
    assert_eq!(d.session_rows.len(), 1);
    let row = &d.session_rows[0];
    assert_eq!(row.agent, "claude_desktop");
    assert_eq!(row.model, "claude-opus-5");
    assert_eq!(row.context, 10_000, "the prompt it was last handed");
    assert_eq!(row.calls, 2);
    assert_eq!(d.heat.len(), 1);
}

#[test]
fn digest_ranks_every_dimension() {
    let now = chrono::Local::now().timestamp_millis();
    let records = vec![
        record(now, "claude_code", "claude-sonnet-5", 1_000_000, 100_000),
        record(now, "claude_desktop", "claude-sonnet-5", 500_000, 50_000),
        record(now, "claude_desktop", "claude-opus-5", 100_000, 10_000),
    ];
    let d = digest(&records, Period::Today, 10);
    assert_eq!(d.calls, 3);
    assert_eq!(d.by.model.len(), 2);
    assert_eq!(d.by.model[0].name, "claude-sonnet-5");
    assert_eq!(d.by.model_at.len(), 3);
    assert_eq!(d.by.agent.len(), 2);
    assert_eq!(d.by.agent[0].name, "Coding Assistant");
    assert_eq!(d.recent.len(), 3);
    assert!(d.cost > 0.0);
}

#[test]
fn the_series_reads_left_to_right() {
    // the trend is drawn in the order the daemon hands it over; a
    // HashMap's order would scatter the days across the chart
    let now = chrono::Local::now().timestamp_millis();
    let day = 86_400_000i64;
    let records = vec![
        record(now, "claude_code", "claude-sonnet-5", 10, 5),
        record(now - 2 * day, "claude_code", "claude-sonnet-5", 10, 5),
        record(now - day, "claude_code", "claude-sonnet-5", 10, 5),
    ];
    // a week reads by the day, so three calls on three days are three
    // buckets — and they must arrive in time order
    let d = digest(&records, Period::Days(7), 10);
    assert_eq!(d.series.len(), 3);
    assert!(d.series.windows(2).all(|w| w[0].t < w[1].t));
}

#[test]
fn failed_calls_are_counted_but_not_ranked() {
    let now = chrono::Local::now().timestamp_millis();
    let mut failed = record(now, "claude_code", "", 0, 0);
    failed.error = Some(intern("rate_limit"));
    let d = digest(&[failed], Period::Today, 10);
    assert_eq!(d.calls, 1);
    assert_eq!(d.errors, 1);
    assert!(d.by.model.is_empty());
    assert_eq!(d.recent.len(), 1);
}

#[test]
fn weeks_start_on_monday() {
    // 2026-10-18 is a Sunday; the week it ends runs from the 12th
    let sunday = 1_792_281_600_000i64; // 2026-10-18T00:00:00Z
    let monday = bucket_start(sunday, Bucket::Week);
    assert_eq!(local_day(monday), "2026-10-12");
    assert_ne!(local_day(monday), "2026-10-18");
}

#[test]
fn a_replys_lines_collapse_to_one_call() {
    let dir = tempdir();
    let file = dir.join("s1.jsonl");
    let line = |uuid: &str, out: u64, req: &str| {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"2026-10-01T10:00:00.000Z","sessionId":"s1","requestId":"{req}","cwd":"D:/p","entrypoint":"sdk-cli","message":{{"id":"msg_1","model":"claude-sonnet-5","usage":{{"input_tokens":10,"output_tokens":{out},"cache_read_input_tokens":5}}}}}}"#
        )
    };
    fs::write(
        &file,
        format!(
            "{}\n{}\n{}\n",
            line("a", 1, "req_1"),
            line("b", 7, "req_1"),
            r#"{"type":"user","timestamp":"2026-10-01T10:00:01.000Z"}"#
        ),
    )
    .unwrap();
    let calls = read_transcript(&file).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].output, 7, "the last line carries the final usage");
    assert_eq!(calls[0].agent, "claude_code");
    assert!(calls[0].cost > 0.0);
    fs::remove_dir_all(dir).ok();
}

#[test]
fn a_transcript_tells_only_the_whole_turn() {
    // A message's first line is its first content block, not its first
    // token: the blocks land together. The line span of a burst like the
    // 17k-token / 77 ms one seen in the store is no writing window at all,
    // and the turn under a second is refused by the duration floor before
    // any speed is computed.
    let mut burst = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 17_145);
    burst.asked = 1_000_000 - 77;
    assert_eq!(
        burst.floor_ms(),
        None,
        "a turn under a second is mostly wait"
    );
    assert!(
        burst.output >= FLOOR_MIN_OUTPUT,
        "the burst is refused for its duration, not for its token count"
    );

    // The whole turn is what the file can time, and it is a floor.
    let mut turn = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 5_000);
    turn.asked = 1_000_000 - 20_000;
    assert_eq!(turn.floor_ms(), Some(20_000), "5,000 tokens in 20 s");

    // A turn long enough to be timed, still faster than any real stream:
    // the same 17k reply over 1.5 s reads 11,430 tok/s, and only the
    // speed cap rejects it.
    let mut too_fast = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 17_145);
    too_fast.asked = 1_000_000 - 1_500; // 11,430 tok/s
    assert_eq!(
        too_fast.floor_ms(),
        None,
        "past the duration floor, refused by the 10,000 tok/s cap"
    );

    // Just under the cap at the same length is told.
    let mut fast = record(1_000_000, "claude_code", "claude-sonnet-5", 0, 14_000);
    fast.asked = 1_000_000 - 1_500; // 9,333 tok/s
    assert_eq!(
        fast.floor_ms(),
        Some(1_500),
        "the cap is a ceiling, not a target"
    );
}

#[test]
fn a_whole_turn_is_a_floor_only_for_a_long_reply() {
    let mut long = record(20_000, "claude_code", "claude-sonnet-5", 0, 1_500);
    long.asked = 15_000; // asked 5 s before its last line
    assert_eq!(long.floor_ms(), Some(5_000));

    let mut short = record(20_000, "claude_code", "claude-sonnet-5", 0, 999);
    short.asked = 15_000;
    assert_eq!(short.floor_ms(), None, "a short reply is mostly wait");

    let unasked = record(20_000, "claude_code", "claude-sonnet-5", 0, 2_000);
    assert_eq!(unasked.floor_ms(), None, "no ask, no whole turn");
}

#[test]
fn floors_are_summed_not_averaged() {
    // A fast short reply and a slow long one: the group's floor is its
    // tokens over its time, not the mean of two rates.
    let now = chrono::Local::now().timestamp_millis();
    let mut fast = record(now, "claude_code", "claude-sonnet-5", 0, 2_000);
    fast.asked = now - 2_000; // 1,000 tok/s over the turn
    let mut slow = record(now, "claude_code", "claude-sonnet-5", 0, 2_000);
    slow.asked = now - 20_000; // 100 tok/s over the turn
    let d = digest(&[fast, slow], Period::Today, 10);
    assert_eq!(d.timing.floor_n, 2);
    assert!(
        (d.timing.floor() - 181.8).abs() < 0.5,
        "4,000 tokens in 22 s"
    );
}

#[test]
fn a_replys_ask_is_the_prompt_before_it() {
    let dir = tempdir();
    let file = dir.join("s3.jsonl");
    fs::write(
            &file,
            [
                r#"{"type":"user","timestamp":"2026-10-01T10:00:00.000Z"}"#,
                r#"{"type":"assistant","timestamp":"2026-10-01T10:00:02.000Z","sessionId":"s3","entrypoint":"sdk-cli","message":{"id":"m1","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":2000}}}"#,
                r#"{"type":"user","timestamp":"2026-10-01T10:00:05.000Z"}"#,
                r#"{"type":"assistant","timestamp":"2026-10-01T10:00:08.000Z","sessionId":"s3","entrypoint":"sdk-cli","message":{"id":"m2","model":"claude-sonnet-5","usage":{"input_tokens":1,"output_tokens":3000}}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
    let calls = read_transcript(&file).unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].asked, 1_790_848_800_000, "asked by the prompt");
    assert_eq!(
        calls[0].floor_ms(),
        Some(2_000),
        "two seconds from ask to last line"
    );
    assert_eq!(
        calls[1].asked, 1_790_848_805_000,
        "asked by the next prompt"
    );
    fs::remove_dir_all(dir).ok();
}

#[test]
fn subagent_transcripts_are_read() {
    let dir = tempdir();
    let project = dir.join("projects").join("-p");
    let session = project.join("s1");
    let subs = session.join("subagents");
    let run = subs.join("workflows").join("wf_1");
    fs::create_dir_all(&run).unwrap();
    let line = |session: &str, agent: &str, out: u64| {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-10-01T10:00:00.000Z","sessionId":"{session}","isSidechain":true,"agentId":"{agent}","entrypoint":"sdk-cli","message":{{"id":"msg_{agent}","model":"claude-haiku-5","usage":{{"input_tokens":10,"output_tokens":{out}}}}}}}"#
        )
    };
    fs::write(project.join("s1.jsonl"), "").unwrap();
    fs::write(subs.join("agent-a1.jsonl"), line("s1", "a1", 7)).unwrap();
    fs::write(subs.join("agent-a1.meta.json"), "{}").unwrap();
    fs::write(run.join("agent-a2.jsonl"), line("s1", "a2", 9)).unwrap();
    fs::write(run.join("journal.jsonl"), "").unwrap();

    let files: Vec<PathBuf> = transcript_files_from_roots(&[(dir.clone(), "claude_code")])
        .into_iter()
        .map(|(p, _, _, _)| p)
        .collect();
    assert_eq!(files.len(), 3, "the session and two of its subagents");
    assert!(
        files
            .iter()
            .all(|p| p.file_name().and_then(|n| n.to_str()) != Some("journal.jsonl")),
        "a workflow's journal is not a transcript"
    );
    let calls: Vec<LedgerRecord> = files
        .iter()
        .flat_map(|p| read_transcript(p).unwrap())
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c.session.as_ref() == "s1"));
    fs::remove_dir_all(dir).ok();
}

#[test]
fn desktop_entrypoints_are_their_own_agent() {
    let dir = tempdir();
    let file = dir.join("s2.jsonl");
    let line = |ep: &str, id: &str| {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-10-01T10:00:00.000Z","sessionId":"s2","entrypoint":"{ep}","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":1,"output_tokens":2}}}}}}"#
        )
    };
    fs::write(
        &file,
        format!(
            "{}\n{}\n",
            line("claude-desktop-3p", "m1"),
            line("sdk-cli", "m2")
        ),
    )
    .unwrap();
    let calls = read_transcript(&file).unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].agent, "claude_desktop");
    assert_eq!(calls[1].agent, "claude_code");
    fs::remove_dir_all(dir).ok();
}

#[test]
fn periods_start_at_the_local_day() {
    // 2026-10-19T12:00:00Z, whatever the machine's zone is
    let now = 1_792_411_200_000i64;
    let today = Period::Today.since_ms(now);
    assert!(today <= now);
    assert!(now - today <= 86_400_000);
    let week = Period::Days(7).since_ms(now);
    assert!(today - week <= 7 * 86_400_000);
    assert_eq!(Period::parse(Some("30d")), Period::Days(30));
    assert_eq!(Period::parse(Some("nonsense")), Period::All);
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agenthub-ledger-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Codex rollouts (both plain `.jsonl` and compressed `.jsonl.zst`) record
/// each turn's call under the model active for that turn, subtracting
/// `cached_input_tokens` from `input_tokens` so cached tokens are not
/// billed or counted twice.
#[test]
fn a_codex_rollout_is_its_last_total() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let home = tempdir();
    let sessions = home.join(".codex").join("sessions").join("2026").join("10");
    fs::create_dir_all(&sessions).unwrap();
    let lines = [
        r#"{"timestamp":"2026-10-01T10:00:00.000Z","type":"session_meta","payload":{"id":"01a15424"}}"#,
        r#"{"timestamp":"2026-10-01T10:00:01.000Z","type":"turn_context","payload":{"model":"gemini-4-argon-high"}}"#,
        r#"{"timestamp":"2026-10-01T10:00:05.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":400,"cached_input_tokens":300,"output_tokens":10,"total_tokens":410}}}}"#,
        r#"{"timestamp":"2026-10-01T10:00:08.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol"}}"#,
        r#"{"timestamp":"2026-10-01T10:00:09.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":900,"cached_input_tokens":700,"output_tokens":30,"total_tokens":930}}}}"#,
    ];
    // Write as an active uncompressed `.jsonl` rollout
    let file = sessions.join("rollout-2026-10-01T10-00-00-01a15424.jsonl");
    fs::write(&file, lines.join("\n")).unwrap();

    // a rollout that spent nothing is not a call the page can show
    let idle = sessions.join("rollout-2026-10-02T10-00-00-01a99999.jsonl.zst");
    let idle_lines = [
        r#"{"timestamp":"2026-10-02T10:00:00.000Z","type":"session_meta","payload":{"id":"01a99999"}}"#,
        r#"{"timestamp":"2026-10-02T10:00:01.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0}}}}"#,
    ];
    fs::write(
        &idle,
        zstd::encode_all(idle_lines.join("\n").as_bytes(), 3).unwrap(),
    )
    .unwrap();

    let (calls, _) = codex_calls(&home, &HashMap::new());
    assert_eq!(calls.len(), 2, "two turns on two models in one session");
    let total_tokens: u64 = calls.iter().map(LedgerRecord::total_tokens).sum();
    assert_eq!(total_tokens, 930, "the cumulative total across both turns");

    let first = calls
        .iter()
        .find(|c| &*c.model == "gemini-4-argon-high")
        .expect("first turn on gemini-4-argon-high must be preserved");
    assert_eq!(&*first.session, "01a15424");
    assert_eq!(first.input, 100);
    assert_eq!(first.cache_read, 300);
    assert_eq!(first.output, 10);
    assert_eq!(first.total_tokens(), 410);

    let second = calls
        .iter()
        .find(|c| &*c.model == "gpt-5.6-sol")
        .expect("second turn on gpt-5.6-sol");
    assert_eq!(second.input, 100);
    assert_eq!(second.cache_read, 400);
    assert_eq!(second.output, 20);
    assert_eq!(second.total_tokens(), 520);
    assert_eq!(second.t, 1_790_848_809_000, "the last event's moment");
    fs::remove_dir_all(&home).ok();
}

/// A rollout is cached by its live file mark so unchanged archives are not
/// re-parsed, and re-read immediately when new lines are appended.
#[test]
fn a_codex_rollout_is_read_once_and_reread_when_it_grows() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let home = tempdir();
    let sessions = home.join(".codex").join("sessions").join("2026").join("10");
    fs::create_dir_all(&sessions).unwrap();
    let file = sessions.join("rollout-2026-10-01T10-00-00-01a15424.jsonl.zst");
    let meta = r#"{"timestamp":"2026-10-01T10:00:00.000Z","type":"session_meta","payload":{"id":"01a15424","base_instructions":{"provenance":{"model":"gpt-5.6-sol"}}}}"#;
    let count = |input: u64, at: &str| {
        format!(
            r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":10}}}}}}}}"#
        )
    };
    let write = |body: &str| {
        fs::write(&file, zstd::encode_all(body.as_bytes(), 3).unwrap()).unwrap();
    };

    write(&[meta, &count(400, "2026-10-01T10:00:05.000Z")].join("\n"));
    let first_sum: u64 = codex_calls(&home, &HashMap::new())
        .0
        .iter()
        .map(LedgerRecord::total_tokens)
        .sum();
    assert_eq!(first_sum, 410);
    let kept_sum: u64 = ledger_cache()
        .read()
        .unwrap()
        .codex
        .get(&file)
        .map(|(_, recs)| recs.iter().map(LedgerRecord::total_tokens).sum())
        .expect("the walk kept the rollout it read");
    assert_eq!(kept_sum, 410);

    // a session that wrote more is read again rather than answered from
    // the copy of its earlier state
    write(
        &[
            meta,
            &count(400, "2026-10-01T10:00:05.000Z"),
            &count(900, "2026-10-01T10:00:09.000Z"),
        ]
        .join("\n"),
    );
    let grown_sum: u64 = codex_calls(&home, &HashMap::new())
        .0
        .iter()
        .map(LedgerRecord::total_tokens)
        .sum();
    assert_eq!(grown_sum, 910);
    fs::remove_dir_all(&home).ok();
}

#[test]
fn incremental_transcript_tail_reads_only_appended_bytes() {
    use std::io::Write as _;
    let dir = tempdir();
    let file = dir.join("tail.jsonl");
    let first = concat!(
        r#"{"type":"user","timestamp":"2026-10-01T10:00:00.000Z"}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-10-01T10:00:02.000Z","sessionId":"s_tail","entrypoint":"sdk-cli","message":{"id":"m1","model":"claude-sonnet-5","usage":{"input_tokens":100,"output_tokens":50}}}"#,
        "\n"
    );
    fs::write(&file, first).unwrap();

    let mut state = TranscriptState::default();
    read_transcript_incremental(&file, &mut state, &HashMap::new()).unwrap();
    assert_eq!(state.calls.len(), 1);
    assert_eq!(state.offset, first.len() as u64);
    assert_eq!(&*state.calls[0].provider, "Anthropic");
    assert_eq!(state.calls[0].duration_ms, Some(2_000));

    // Append a completed line plus an unterminated partial line in flight.
    let second_complete = concat!(
        r#"{"type":"user","timestamp":"2026-10-01T10:00:10.000Z"}"#,
        "\n",
        r#"{"type":"assistant","timestamp":"2026-10-01T10:00:13.500Z","sessionId":"s_tail","entrypoint":"sdk-cli","message":{"id":"m2","model":"claude-opus-5","usage":{"input_tokens":200,"output_tokens":80}}}"#,
        "\n"
    );
    let partial_head = r#"{"type":"assistant","timestamp":"2026-10-01T10:00:20.000Z","sessionId":"s_tail","entrypoint":"sdk-cli","message":{"id":"m3","#;
    {
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        f.write_all(second_complete.as_bytes()).unwrap();
        f.write_all(partial_head.as_bytes()).unwrap();
    }

    read_transcript_incremental(&file, &mut state, &HashMap::new()).unwrap();
    assert_eq!(state.calls.len(), 2);
    assert_eq!(
        state.offset,
        (first.len() + second_complete.len()) as u64,
        "offset pauses right before the incomplete trailing JSON line"
    );
    assert_eq!(state.calls[1].duration_ms, Some(3_500));

    // Finish writing the partial line; next incremental read picks up m3 cleanly.
    let partial_tail =
        r#""model":"claude-haiku-5","usage":{"input_tokens":300,"output_tokens":90}}}"#;
    {
        let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
        f.write_all(partial_tail.as_bytes()).unwrap();
        f.write_all(b"\n").unwrap();
    }
    read_transcript_incremental(&file, &mut state, &HashMap::new()).unwrap();
    assert_eq!(state.calls.len(), 3);
    assert_eq!(state.calls[2].output, 90);
    fs::remove_dir_all(&dir).ok();
}
#[test]
fn antigravity_calls_attribute_to_antigravity_and_resolve_bom_model() {
    let dir = tempdir();
    let logs = dir
        .join(".gemini")
        .join("antigravity")
        .join("brain")
        .join("agy-session-1")
        .join(".system_generated")
        .join("logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(
        dir.join(".gemini")
            .join("antigravity")
            .join("settings.json"),
        "\u{feff}{\"model\":\"gemini-3-pro\"}",
    )
    .unwrap();
    let transcript = logs.join("transcript_full.jsonl");
    fs::write(
        &transcript,
        concat!(
            r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","created_at":"2026-11-15T10:00:00Z","input_tokens":1000,"cache_read_tokens":2000,"output_tokens":500,"tool_calls":[{"name":"run_command","args":{"Cwd":"/Users/dev/project"}}]}"#,
            "\n"
        ),
    )
    .unwrap();

    let records = collector_extra::read_antigravity_calls(&transcript, &dir, &HashMap::new());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].agent, "antigravity");
    assert_eq!(agent_label(records[0].agent), "Antigravity");
    assert_eq!(&*records[0].provider, "Google");
    assert_eq!(&*records[0].model, "gemini-3-pro");
    assert_eq!(&*records[0].cwd, "/Users/dev/project");
    assert_eq!(records[0].input, 1000);
    assert_eq!(records[0].cache_read, 2000);
    assert_eq!(records[0].output, 500);
    assert!(records[0].cost > 0.0);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn prune_interner_reclaims_unreferenced_strings_and_preserves_live_ones() {
    let live_key = format!("__interner_live_{}", std::process::id());
    let dead_key = format!("__interner_dead_{}", std::process::id());

    let live_arc = intern(&live_key);
    let dead_weak = {
        let tmp = intern(&dead_key);
        Arc::downgrade(&tmp)
    };
    let _ = prune_interner();

    let live_after = intern(&live_key);
    assert!(
        Arc::ptr_eq(&live_arc, &live_after),
        "live interned strings must retain the shared Arc instance"
    );
    assert!(
        dead_weak.upgrade().is_none(),
        "dropped interned strings with strong_count == 1 must be reclaimed"
    );
}

#[test]
fn usage_index_db_spills_queries_and_incrementally_vacuums() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let dir = tempdir();
    let db_path = dir.join("usage_index.db");
    let _guard = set_test_index_db(Some(db_path.clone()));

    let mut db = UsageIndexDb::open(&db_path).unwrap();
    assert_eq!(
        db.auto_vacuum_mode().unwrap(),
        2,
        "usage_index.db must enable PRAGMA auto_vacuum = INCREMENTAL"
    );

    let old_file = dir.join("old-session.jsonl");
    let hot_file = dir.join("hot-session.jsonl");
    let old_t = 1_700_000_000_000i64;
    let mut old_rec = record(old_t, "claude_code", "claude-sonnet-5", 120, 40);
    old_rec.session = intern("sess_cold_1");
    let big_recs: Vec<LedgerRecord> = (0..120)
        .map(|i| {
            let mut r = old_rec.clone();
            r.t = old_t + i * 1_000;
            r.session = intern(&format!("sess_cold_{i}"));
            r
        })
        .collect();

    db.upsert_files(&[ColdSpillItem {
        path: old_file.clone(),
        source: "claude",
        file_len: 4096,
        modified_ms: old_t,
        records: big_recs,
    }])
    .unwrap();
    assert_eq!(db.row_count().unwrap(), 120);
    assert_eq!(db.indexed_file_count().unwrap(), 1);

    let manifest = db.load_manifest().unwrap();
    assert_eq!(
        manifest.get(&old_file).unwrap().max_t,
        Some(old_t + 119_000)
    );

    let mut exclude = HashSet::new();
    exclude.insert(hot_file);
    let loaded = db.query_records_since(i64::MIN, &exclude).unwrap();
    assert_eq!(loaded.len(), 120);

    // Exclude overlapping hot path to guarantee zero double-counting.
    exclude.insert(old_file.clone());
    assert!(
        db.query_records_since(i64::MIN, &exclude)
            .unwrap()
            .is_empty()
    );

    db.remove_paths(&[old_file]).unwrap();
    assert_eq!(db.row_count().unwrap(), 0);
    db.reclaim_space(128).unwrap();
    assert_eq!(db.freelist_count().unwrap(), 0);
    fs::remove_dir_all(&dir).ok();
}
#[test]
fn active_claude_session_over_40_turns_stays_incremental_without_pruned_flag() {
    let dir = tempdir();
    let path = dir.join("active-45.jsonl");
    let now_rfc3339 = chrono::Utc::now().to_rfc3339();
    let mut content = String::new();
    for i in 0..45 {
        content.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"{now_rfc3339}","sessionId":"sess-45","requestId":"req-{i}","message":{{"id":"msg-{i}","model":"claude-sonnet-4-6","usage":{{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
        ));
        content.push('\n');
    }
    fs::write(&path, &content).unwrap();

    let mut state = TranscriptState {
        modified_ms: chrono::Utc::now().timestamp_millis(),
        default_agent: "claude_code",
        ..TranscriptState::default()
    };
    read_transcript_incremental(&path, &mut state, &HashMap::new()).unwrap();
    assert_eq!(state.calls.len(), 45);
    assert!(state.index.len() <= 32, "sliding window should bound index");
    assert!(
        !state.index_pruned,
        "active >32 turn session must not set index_pruned"
    );

    // Append turn 46 + streaming update for turn 46: must remain incremental and deduplicated.
    use std::io::Write;
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(
        f,
        r#"{{"type":"assistant","timestamp":"{now_rfc3339}","sessionId":"sess-45","requestId":"req-45","message":{{"id":"msg-45","model":"claude-sonnet-4-6","usage":{{"input_tokens":100,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
    )
    .unwrap();
    writeln!(
        f,
        r#"{{"type":"assistant","timestamp":"{now_rfc3339}","sessionId":"sess-45","requestId":"req-45","message":{{"id":"msg-45","model":"claude-sonnet-4-6","usage":{{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
    )
    .unwrap();
    drop(f);

    read_transcript_incremental(&path, &mut state, &HashMap::new()).unwrap();
    assert_eq!(state.calls.len(), 46);
    assert_eq!(state.calls.last().unwrap().output, 50);
    assert!(!state.index_pruned);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn today_digest_preserves_53_week_heatmap_activity() {
    let now = chrono::Local::now().timestamp_millis();
    let forty_days_ago = now - 40 * 86_400_000;
    let old_rec = record(forty_days_ago, "claude_code", "claude-sonnet-5", 2_000, 200);
    let today_rec = record(now, "claude_code", "claude-opus-5", 1_000, 100);

    let d = digest(&[today_rec, old_rec], Period::Today, 10);
    assert_eq!(d.calls, 1, "Today totals must count only Today calls");
    assert_eq!(
        d.heat.len(),
        2,
        "Today digest must still carry prior-month activity in 53-week heatmap"
    );
    let old_day = local_day(forty_days_ago);
    assert!(d.heat.iter().any(|p| p.date == old_day && p.calls == 1));
}

#[test]
fn truncated_trailing_claude_line_tracks_seen_len_until_completed() {
    let dir = tempdir();
    let path = dir.join("partial.jsonl");
    let ts = chrono::Utc::now().to_rfc3339();
    let complete = format!(
        r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s-part","requestId":"r1","message":{{"id":"m1","model":"claude-sonnet-4-6","usage":{{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
    );
    let partial = format!(
        r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s-part","requestId":"r2","message":{{"id":"m2","model":"claude-sonnet-4-6","usage":{{"input_tokens":20"#
    );
    fs::write(&path, format!("{complete}\n{partial}")).unwrap();
    let len_with_partial = fs::metadata(&path).unwrap().len();

    let mut state = TranscriptState {
        modified_ms: chrono::Utc::now().timestamp_millis(),
        default_agent: "claude_code",
        ..TranscriptState::default()
    };
    read_transcript_incremental(&path, &mut state, &HashMap::new()).unwrap();
    assert_eq!(state.calls.len(), 1);
    assert!(state.offset < len_with_partial);
    assert_eq!(state.seen_len, len_with_partial);

    // Finish the truncated trailing line and re-read incrementally.
    use std::io::Write;
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(
        f,
        r#","output_tokens":9,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
    )
    .unwrap();
    drop(f);
    let final_len = fs::metadata(&path).unwrap().len();
    read_transcript_incremental(&path, &mut state, &HashMap::new()).unwrap();
    assert_eq!(state.calls.len(), 2);
    assert_eq!(state.offset, final_len);
    assert_eq!(state.seen_len, final_len);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn dirty_db_removals_prevent_cold_resurrection_and_retry_on_walk() {
    let _lock = TEST_MUTEX.lock().unwrap();
    forget_transcripts();

    let dir = tempdir();
    let db_path = dir.join("usage_index.db");
    let _guard = set_test_index_db(Some(db_path.clone()));

    let kept_file = dir.join("kept-cold.jsonl");
    let deleted_file = dir.join("deleted-cold.jsonl");
    let old_t = 1_700_000_000_000i64;
    let mut kept_rec = record(old_t, "claude_code", "claude-sonnet-5", 100, 20);
    kept_rec.session = intern("kept_sess");
    let mut deleted_rec = record(old_t + 1_000, "claude_code", "claude-sonnet-5", 200, 40);
    deleted_rec.session = intern("deleted_sess");

    let mut db = UsageIndexDb::open(&db_path).unwrap();
    db.upsert_files(&[
        ColdSpillItem {
            path: kept_file.clone(),
            source: "claude",
            file_len: 100,
            modified_ms: old_t,
            records: vec![kept_rec],
        },
        ColdSpillItem {
            path: deleted_file.clone(),
            source: "claude",
            file_len: 100,
            modified_ms: old_t,
            records: vec![deleted_rec],
        },
    ])
    .unwrap();
    assert_eq!(db.row_count().unwrap(), 2);
    drop(db);

    {
        let mut cache = ledger_cache().write().unwrap();
        cache.cold_loaded = true;
        cache.walked_at = Some(Instant::now() - Duration::from_secs(10));
        cache.cold_manifest.insert(
            kept_file.clone(),
            IndexedFileMeta {
                source: "claude".to_owned(),
                file_len: 100,
                modified_ms: old_t,
                min_t: Some(old_t),
                max_t: Some(old_t),
            },
        );
        cache.recompute_cold_max_t();
        cache.dirty_db_removals.insert(deleted_file.clone());
    }

    // Cold query must exclude pending `dirty_db_removals` so deleted rows never resurrect.
    let (recs, cold_ok) = cache::collect_windowed_records(i64::MIN);
    assert!(cold_ok);
    assert!(recs.iter().any(|r| r.session.as_ref() == "kept_sess"));
    assert!(
        !recs.iter().any(|r| r.session.as_ref() == "deleted_sess"),
        "pending dirty_db_removals must never resurrect from SQLite"
    );

    // Compact/spill must retry and clear `dirty_db_removals` from SQLite.
    compact_and_spill_cold();
    assert!(ledger_cache().read().unwrap().dirty_db_removals.is_empty());
    let db = UsageIndexDb::open_read_only(&db_path).unwrap();
    let remaining = db.query_records_since(i64::MIN, &HashSet::new()).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].session.as_ref(), "kept_sess");

    forget_transcripts();
    fs::remove_dir_all(&dir).ok();
}
