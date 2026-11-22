use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub(crate) const TELEMETRY_REPUBLISH_INTERVAL: Duration = Duration::from_secs(120);

/// Canonicalizes `discovery` to structural session fields only (id, provider,
/// workspace_path, title, model, resumable, execution_state, subagent ids/kinds),
/// omitting rapidly-changing token counts and activity timestamps so idle
/// background IDE sessions do not enqueue 680 KB `ready` snapshots every 5 s.
pub(crate) fn structural_discovery_fingerprint(discovery: Option<&Value>) -> Option<[u8; 32]> {
    let val = discovery?;
    let raw_list = val
        .get("candidates")
        .or_else(|| val.get("sessions"))
        .and_then(Value::as_array);
    let mut normalized: Vec<Value> = raw_list
        .map(|arr| {
            arr.iter()
                .map(|entry| {
                    let subagent_keys: Vec<Value> = entry
                        .get("subagents")
                        .and_then(Value::as_array)
                        .map(|subs| {
                            let mut items: Vec<(String, String)> = subs
                                .iter()
                                .filter_map(|s| {
                                    let id = s.get("id").and_then(Value::as_str)?.trim().to_owned();
                                    let kind = s
                                        .get("kind")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .trim()
                                        .to_owned();
                                    Some((id, kind))
                                })
                                .collect();
                            items.sort();
                            items
                                .into_iter()
                                .map(|(id, kind)| json!({ "id": id, "kind": kind }))
                                .collect()
                        })
                        .unwrap_or_default();
                    json!({
                        "provider_session_id": entry
                            .get("provider_session_id")
                            .or_else(|| entry.get("session_id"))
                            .or_else(|| entry.get("id")),
                        "provider": entry.get("provider"),
                        "workspace_path": entry.get("workspace_path").or_else(|| entry.get("cwd")),
                        "title": entry.get("title").or_else(|| entry.get("name")),
                        "model": entry.get("model"),
                        "resumable": entry.get("resumable"),
                        "execution_state": entry
                            .get("execution_state")
                            .or_else(|| entry.pointer("/status/type"))
                            .or_else(|| entry.get("status")),
                        "subagents": subagent_keys,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    normalized.sort_by(|a, b| {
        let ka = a
            .get("provider_session_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let kb = b
            .get("provider_session_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        ka.cmp(kb)
    });

    Some(crate::collectors::json_sha256(&json!({
        "candidates": normalized,
    })))
}

pub(crate) struct ReadyPublishDecision {
    pub publish: bool,
    pub next_models_hash: Option<[u8; 32]>,
    pub next_discovery_hash: Option<[u8; 32]>,
    pub next_structural_hash: Option<[u8; 32]>,
}

pub(crate) fn evaluate_ready_publish(
    next_models: Option<&Value>,
    next_discovery: Option<&Value>,
    published_models: &Option<[u8; 32]>,
    published_discovery: &Option<[u8; 32]>,
    published_structural: &Option<[u8; 32]>,
    last_ready_publish: Instant,
) -> ReadyPublishDecision {
    let next_models_hash = next_models.map(crate::collectors::json_sha256);
    let next_discovery_hash = next_discovery.map(crate::collectors::json_sha256);
    let next_structural_hash = structural_discovery_fingerprint(next_discovery);

    let models_changed = &next_models_hash != published_models;
    let structural_changed = &next_structural_hash != published_structural;
    let telemetry_changed = &next_discovery_hash != published_discovery;
    let telemetry_due =
        telemetry_changed && last_ready_publish.elapsed() >= TELEMETRY_REPUBLISH_INTERVAL;

    ReadyPublishDecision {
        publish: models_changed || structural_changed || telemetry_due,
        next_models_hash,
        next_discovery_hash,
        next_structural_hash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volatile_token_and_timestamp_bumps_do_not_change_structural_fingerprint() {
        let a = json!({
            "candidates": [
                {
                    "provider_session_id": "antigravity_1",
                    "provider": "antigravity",
                    "workspace_path": "ws-1",
                    "title": "Fix bug",
                    "model": "gemini-3-pro-high",
                    "resumable": true,
                    "execution_state": "idle",
                    "total_tokens": 100,
                    "last_activity_at_ms": 1_700_000_001_i64
                }
            ]
        });
        let b = json!({
            "candidates": [
                {
                    "provider_session_id": "antigravity_1",
                    "provider": "antigravity",
                    "workspace_path": "ws-1",
                    "title": "Fix bug",
                    "model": "gemini-3-pro-high",
                    "resumable": true,
                    "execution_state": "idle",
                    "total_tokens": 999_999,
                    "last_activity_at_ms": 1_700_000_999_i64
                }
            ]
        });
        let c = json!({
            "candidates": [
                {
                    "provider_session_id": "antigravity_1",
                    "provider": "antigravity",
                    "workspace_path": "ws-1",
                    "title": "Renamed title",
                    "model": "gemini-3-pro-high",
                    "resumable": true,
                    "execution_state": "idle",
                    "total_tokens": 999_999,
                    "last_activity_at_ms": 1_700_000_999_i64
                }
            ]
        });

        assert_eq!(
            structural_discovery_fingerprint(Some(&a)),
            structural_discovery_fingerprint(Some(&b))
        );
        assert_ne!(
            structural_discovery_fingerprint(Some(&a)),
            structural_discovery_fingerprint(Some(&c))
        );
    }
}
