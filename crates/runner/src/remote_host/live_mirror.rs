use super::*;
use crate::conversation::TranscriptFileFingerprint;

const MIRROR_CALM_INTERVAL: Duration = Duration::from_millis(1_000);
const MIRROR_ACTIVE_INTERVAL: Duration = Duration::from_millis(350);
const MIRROR_ACTIVE_WINDOW: Duration = Duration::from_secs(15);
const MIRROR_LOCAL_TURN_COOLDOWN: Duration = Duration::from_millis(800);
const MIRROR_MISSING_BASE_INTERVAL: Duration = Duration::from_secs(4);
const MIRROR_MISSING_MAX_INTERVAL: Duration = Duration::from_secs(30);
const MAX_MIRROR_CHECKS_PER_POLL: usize = 4;

#[derive(Debug, Clone)]
struct MirrorEntry {
    file_fingerprints: Vec<TranscriptFileFingerprint>,
    last_items_hash: Option<[u8; 32]>,
    baseline_seeded: bool,
    last_usage: Option<(u64, Option<u64>)>,
    missing_streak: u32,
    next_check_at: Instant,
    active_until: Option<Instant>,
}

/// Mirrors live on-disk transcript updates from native desktop sessions
/// (Codex Desktop/CLI, Claude Code, Antigravity, ACP plugins) to connected
/// remote viewers without seizing exclusive write control (`TakeControl`).
///
/// File `(len, mtime)` fingerprints are retained directly per session so idle
/// sessions only pay O(1) `stat()` calls even after `conversation_cache()` TTL
/// expires, with bounded per-poll work (`MAX_MIRROR_CHECKS_PER_POLL`).
#[derive(Debug, Default)]
pub(crate) struct LiveTranscriptMirror {
    entries: HashMap<String, MirrorEntry>,
}

impl LiveTranscriptMirror {
    pub(crate) fn seed_from_items(
        &mut self,
        hub_session_id: &str,
        items: &[Value],
        usage: Option<(u64, Option<u64>)>,
        now: Instant,
    ) {
        let items_hash = crate::collectors::json_sha256(&Value::Array(items.to_vec()));
        self.entries.insert(
            hub_session_id.to_owned(),
            MirrorEntry {
                file_fingerprints: Vec::new(),
                last_items_hash: Some(items_hash),
                baseline_seeded: true,
                last_usage: usage,
                missing_streak: 0,
                next_check_at: now + MIRROR_CALM_INTERVAL,
                active_until: None,
            },
        );
    }

    pub(crate) fn defer_after_local_turn(&mut self, hub_session_id: &str, now: Instant) {
        let entry = self
            .entries
            .entry(hub_session_id.to_owned())
            .or_insert(MirrorEntry {
                file_fingerprints: Vec::new(),
                last_items_hash: None,
                baseline_seeded: false,
                last_usage: None,
                missing_streak: 0,
                next_check_at: now + MIRROR_LOCAL_TURN_COOLDOWN,
                active_until: None,
            });
        // Absorb the turn Runner just streamed as the new baseline on the next check.
        entry.file_fingerprints.clear();
        entry.baseline_seeded = false;
        entry.missing_streak = 0;
        entry.next_check_at = now + MIRROR_LOCAL_TURN_COOLDOWN;
    }

    pub(crate) fn poll_sessions(
        &mut self,
        home: &Path,
        generation: &str,
        sessions: &HashMap<String, SessionBinding>,
        now: Instant,
    ) -> Vec<HostEvent> {
        self.entries
            .retain(|hub_id, _| sessions.contains_key(hub_id));
        let mut emitted = Vec::new();
        let mut checked = 0usize;

        for (hub_id, binding) in sessions {
            if checked >= MAX_MIRROR_CHECKS_PER_POLL {
                break;
            }
            // Never mirror while Runner itself owns an active turn on this session.
            if binding.current_turn_id.is_some()
                || matches!(
                    binding.execution_state,
                    ExecutionState::Running
                        | ExecutionState::AwaitingPermission
                        | ExecutionState::AwaitingInput
                        | ExecutionState::ReleasingControl
                )
            {
                continue;
            }

            let entry = self
                .entries
                .entry(hub_id.clone())
                .or_insert_with(|| MirrorEntry {
                    file_fingerprints: Vec::new(),
                    last_items_hash: None,
                    baseline_seeded: false,
                    last_usage: None,
                    missing_streak: 0,
                    next_check_at: now,
                    active_until: None,
                });

            if now < entry.next_check_at {
                continue;
            }
            checked += 1;

            let provider = provider_for_session_id(&binding.provider_session_id);
            let changed = crate::conversation::load_session_conversation_if_changed(
                home,
                provider.clone(),
                &binding.provider_session_id,
                Some(&binding.workspace_path),
                &entry.file_fingerprints,
            );

            let mut pushed_update = false;
            if let Some((fresh_fingerprints, items)) = changed {
                if fresh_fingerprints.is_empty() && items.is_empty() {
                    entry.missing_streak = entry.missing_streak.saturating_add(1);
                } else {
                    entry.missing_streak = 0;
                    entry.file_fingerprints = fresh_fingerprints;
                }
                if !items.is_empty() {
                    let items_hash = crate::collectors::json_sha256(&Value::Array(items.clone()));
                    let content_changed = entry.last_items_hash != Some(items_hash);
                    let was_seeded = entry.baseline_seeded;
                    entry.last_items_hash = Some(items_hash);
                    entry.baseline_seeded = true;
                    if was_seeded && content_changed {
                        // Keep `binding: None` so mirrored transcript updates
                        // stream directly to open session views without triggering
                        // global `/v3/remote/sessions` list reloads.
                        emitted.push(HostEvent {
                            event_id: uuid::Uuid::new_v4().to_string(),
                            host_generation: generation.to_owned(),
                            binding: None,
                            hub_session_id: Some(hub_id.clone()),
                            provider_session_id: Some(binding.provider_session_id.clone()),
                            kind: "history/snapshot".to_owned(),
                            raw: json!({
                                "method": "history/snapshot",
                                "params": {
                                    "items": items,
                                    "mirrored": true
                                }
                            }),
                            approval_id: None,
                            approval: None,
                            question_id: None,
                            question: None,
                        });
                        pushed_update = true;
                    }
                }
            }

            if pushed_update {
                if let Some(usage) = crate::collectors::lookup_session_context_usage(
                    home,
                    provider,
                    &binding.provider_session_id,
                    Some(&binding.workspace_path),
                ) {
                    let pair = (usage.used, usage.window);
                    if entry.last_usage != Some(pair) {
                        entry.last_usage = Some(pair);
                        emitted.push(HostEvent {
                            event_id: uuid::Uuid::new_v4().to_string(),
                            host_generation: generation.to_owned(),
                            binding: None,
                            hub_session_id: Some(hub_id.clone()),
                            provider_session_id: Some(binding.provider_session_id.clone()),
                            kind: "thread/tokenUsage/updated".to_owned(),
                            raw: json!({
                                "method": "thread/tokenUsage/updated",
                                "params": {
                                    "threadId": binding.provider_session_id,
                                    "tokenUsage": {
                                        "used": usage.used,
                                        "last": {"inputTokens": usage.used},
                                        "window": usage.window,
                                        "modelContextWindow": usage.window
                                    }
                                }
                            }),
                            approval_id: None,
                            approval: None,
                            question_id: None,
                            question: None,
                        });
                    }
                }
                entry.active_until = Some(now + MIRROR_ACTIVE_WINDOW);
                entry.next_check_at = now + MIRROR_ACTIVE_INTERVAL;
            } else if entry.missing_streak > 0 {
                let shift = entry.missing_streak.saturating_sub(1).min(3);
                let backoff = MIRROR_MISSING_BASE_INTERVAL
                    .saturating_mul(1u32 << shift)
                    .min(MIRROR_MISSING_MAX_INTERVAL);
                entry.active_until = None;
                entry.next_check_at = now + backoff;
            } else {
                let in_active_window = entry.active_until.is_some_and(|until| now < until);
                entry.next_check_at = if in_active_window {
                    now + MIRROR_ACTIVE_INTERVAL
                } else {
                    entry.active_until = None;
                    now + MIRROR_CALM_INTERVAL
                };
            }
        }

        emitted
    }
}

pub(crate) fn provider_for_session_id(id: &str) -> AgentType {
    if id.starts_with("claude_") {
        AgentType::ClaudeCode
    } else if id.starts_with("antigravity_") {
        AgentType::Antigravity
    } else if let Some(plugin) = crate::plugins::find_enabled_plugin_for_session(id) {
        AgentType::Plugin(plugin.id)
    } else {
        AgentType::Codex
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn mirrors_desktop_transcript_append_without_duplicate_snapshots() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let codex_sessions = home.path().join(".codex").join("sessions");
        std::fs::create_dir_all(&codex_sessions).unwrap();

        let thread_id = "11111111-2222-3333-4444-555555555555";
        let transcript = codex_sessions.join(format!("rollout-2026-11-23-{thread_id}.jsonl"));
        let line1 = json!({"type": "session_meta", "payload": {"id": thread_id}});
        let line2 = json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "hello desktop"}]
            }
        });
        std::fs::write(&transcript, format!("{line1}\n{line2}\n")).unwrap();

        let binding = SessionBinding {
            hub_session_id: "hub-mirror-1".into(),
            provider_session_id: thread_id.into(),
            workspace_path: workspace.path().to_path_buf(),
            control_connected: false,
            execution_state: ExecutionState::Unknown,
            current_turn_id: None,
            host_generation: "gen-1".into(),
        };
        let mut sessions = HashMap::new();
        sessions.insert(binding.hub_session_id.clone(), binding.clone());

        let initial_items = crate::conversation::load_session_conversation_with_workspace(
            home.path(),
            AgentType::Codex,
            thread_id,
            Some(workspace.path()),
        );
        assert_eq!(initial_items.len(), 1);

        let mut mirror = LiveTranscriptMirror::default();
        let t0 = Instant::now();
        mirror.seed_from_items(&binding.hub_session_id, &initial_items, None, t0);

        // No change yet -> nothing emitted and file fingerprints get cached.
        let t1 = t0 + MIRROR_CALM_INTERVAL + Duration::from_millis(10);
        assert!(
            mirror
                .poll_sessions(home.path(), "gen-1", &sessions, t1)
                .is_empty()
        );
        assert!(
            !mirror.entries[&binding.hub_session_id]
                .file_fingerprints
                .is_empty()
        );

        // Desktop appends an assistant reply to the rollout JSONL file.
        let line3 = json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "live reply from desktop"}]
            }
        });
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&transcript)
            .unwrap();
        writeln!(file, "{line3}").unwrap();
        file.flush().unwrap();

        let t2 = t1 + MIRROR_CALM_INTERVAL + Duration::from_millis(10);
        let events = mirror.poll_sessions(home.path(), "gen-1", &sessions, t2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "history/snapshot");
        assert!(events[0].binding.is_none());
        assert_eq!(events[0].raw["params"]["mirrored"], json!(true));
        let items = events[0].raw["params"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1]["text"], json!("live reply from desktop"));

        // Polling again with no further writes emits nothing (active cadence checked too).
        let t3 = t2 + MIRROR_ACTIVE_INTERVAL + Duration::from_millis(10);
        assert!(
            mirror
                .poll_sessions(home.path(), "gen-1", &sessions, t3)
                .is_empty()
        );
    }

    #[test]
    fn empty_seeded_session_mirrors_first_desktop_message_immediately() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let codex_sessions = home.path().join(".codex").join("sessions");
        std::fs::create_dir_all(&codex_sessions).unwrap();

        let thread_id = "22222222-3333-4444-5555-666666666666";
        let transcript = codex_sessions.join(format!("rollout-2026-11-23-{thread_id}.jsonl"));

        let binding = SessionBinding {
            hub_session_id: "hub-mirror-empty".into(),
            provider_session_id: thread_id.into(),
            workspace_path: workspace.path().to_path_buf(),
            control_connected: false,
            execution_state: ExecutionState::Idle,
            current_turn_id: None,
            host_generation: "gen-1".into(),
        };
        let mut sessions = HashMap::new();
        sessions.insert(binding.hub_session_id.clone(), binding.clone());

        let mut mirror = LiveTranscriptMirror::default();
        let t0 = Instant::now();
        mirror.seed_from_items(&binding.hub_session_id, &[], None, t0);

        let line1 = json!({"type": "session_meta", "payload": {"id": thread_id}});
        let line2 = json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": "first turn on desktop"}]
            }
        });
        std::fs::write(&transcript, format!("{line1}\n{line2}\n")).unwrap();

        let t1 = t0 + MIRROR_CALM_INTERVAL + Duration::from_millis(10);
        let events = mirror.poll_sessions(home.path(), "gen-1", &sessions, t1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "history/snapshot");
        let items = events[0].raw["params"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["text"], json!("first turn on desktop"));
    }
}
