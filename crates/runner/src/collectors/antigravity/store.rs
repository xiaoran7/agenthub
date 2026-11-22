use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Duration,
};

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde_json::Value;

use super::config::decode_file_uri_to_path;

#[derive(Debug, Clone, Default)]
pub(crate) struct AntigravityDbRecord {
    pub conversation_id: String,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub step_count: u64,
    pub last_modified_ms: Option<i64>,
    pub workspace_uri: Option<String>,
    pub status: String,
    pub project_id: Option<String>,
    pub agent_name: Option<String>,
    pub parent_conversation_id: Option<String>,
    pub nesting_depth: u32,
    pub is_running: bool,
}

/// Parses RFC3339, Go/Protobuf space-separated UTC offsets (e.g. `2026-10-19 13:23:15.7603919+00:00`),
/// or naive timestamps into epoch milliseconds.
pub(crate) fn parse_flexible_timestamp_ms(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.timestamp_millis());
    }
    if trimmed.len() >= 19 && trimmed.as_bytes().get(10) == Some(&b' ') {
        let mut t_fixed = trimmed.to_owned();
        t_fixed.replace_range(10..11, "T");
        if let Ok(dt) = DateTime::parse_from_rfc3339(&t_fixed) {
            return Some(dt.timestamp_millis());
        }
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            return Some(Utc.from_utc_datetime(&naive).timestamp_millis());
        }
    }
    None
}

fn value_ref_to_millis(v: ValueRef<'_>) -> Option<i64> {
    match v {
        ValueRef::Text(bytes) => {
            let s = std::str::from_utf8(bytes).ok()?;
            if let Ok(n) = s.trim().parse::<i64>() {
                return normalize_epoch_num(n);
            }
            parse_flexible_timestamp_ms(s)
        }
        ValueRef::Integer(n) => normalize_epoch_num(n),
        ValueRef::Real(n) if n > 0.0 => normalize_epoch_num(n as i64),
        _ => None,
    }
}

fn normalize_epoch_num(n: i64) -> Option<i64> {
    if n <= 0 {
        None
    } else if n > 1_000_000_000_000_000 {
        Some(n / 1_000)
    } else if n > 1_000_000_000_000 {
        Some(n)
    } else {
        Some(n * 1_000)
    }
}

fn non_empty_string(row: &rusqlite::Row<'_>, idx: Option<usize>) -> Option<String> {
    let idx = idx?;
    match row.get_ref(idx).ok()? {
        ValueRef::Text(bytes) => {
            let s = std::str::from_utf8(bytes).ok()?.trim();
            (!s.is_empty()).then(|| s.to_owned())
        }
        ValueRef::Integer(n) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_u64(row: &rusqlite::Row<'_>, idx: Option<usize>) -> u64 {
    let Some(idx) = idx else {
        return 0;
    };
    match row.get_ref(idx) {
        Ok(ValueRef::Integer(n)) if n > 0 => n as u64,
        Ok(ValueRef::Real(n)) if n > 0.0 => n as u64,
        Ok(ValueRef::Text(b)) => std::str::from_utf8(b)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0),
        _ => 0,
    }
}

/// Extracts the subagent `Role` label embedded in Antigravity 2.0 `raw_summary` protobuf bytes
/// (`\x0a<len><agent_name>\x12<varint_len><role>`) when `title` and `preview` columns are empty.
pub(crate) fn extract_subagent_role_from_raw_summary(
    blob: &[u8],
    agent_name: &str,
) -> Option<String> {
    let agent = agent_name.trim();
    if blob.is_empty() || agent.is_empty() || agent.len() >= 128 {
        return None;
    }
    let mut needle = Vec::with_capacity(agent.len() + 3);
    needle.push(0x0a);
    needle.push(agent.len() as u8);
    needle.extend_from_slice(agent.as_bytes());
    needle.push(0x12);

    let pos = blob
        .windows(needle.len())
        .rposition(|window| window == needle.as_slice())?;
    let mut cursor = pos + needle.len();
    let mut role_len = 0usize;
    let mut shift = 0u32;
    loop {
        let byte = *blob.get(cursor)?;
        cursor += 1;
        role_len |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 21 {
            return None;
        }
    }
    if role_len == 0 || role_len > 240 || cursor + role_len > blob.len() {
        return None;
    }
    let text = std::str::from_utf8(&blob[cursor..cursor + role_len])
        .ok()?
        .trim();
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    Some(text.to_owned())
}

fn first_workspace_path(raw_uris: Option<&str>) -> Option<String> {
    let raw = raw_uris?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(raw) {
        for item in arr {
            if let Some(s) = item.as_str()
                && let Some(decoded) = decode_file_uri_to_path(s)
            {
                return Some(decoded);
            }
        }
    }
    decode_file_uri_to_path(raw)
}

/// Reads metadata from `~/.gemini/antigravity/conversation_summaries.db` read-only.
/// Falls back cleanly to an empty map if the database does not exist or has an unexpected schema.
pub(crate) fn read_antigravity_db(
    ag_home: &Path,
    projects_map: &HashMap<String, String>,
) -> HashMap<String, AntigravityDbRecord> {
    let db_path = ag_home.join("conversation_summaries.db");
    if !db_path.is_file() {
        return HashMap::new();
    }
    let Ok(conn) = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return HashMap::new();
    };
    let _ = conn.busy_timeout(Duration::from_millis(250));

    let Ok(mut pragma_stmt) = conn.prepare("PRAGMA table_info(conversation_summaries)") else {
        return HashMap::new();
    };
    let Ok(col_iter) = pragma_stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return HashMap::new();
    };
    let present_cols: HashSet<String> = col_iter.flatten().collect();
    if !present_cols.contains("conversation_id") {
        return HashMap::new();
    }

    const CANDIDATE_COLS: &[&str] = &[
        "conversation_id",
        "title",
        "preview",
        "step_count",
        "last_modified_time",
        "workspace_uris",
        "status",
        "project_id",
        "agent_name",
        "parent_conversation_id",
        "nesting_depth",
        "not_fully_idle",
        "killed",
        "raw_summary",
    ];
    let mut selected = Vec::new();
    let mut col_idx: HashMap<&'static str, usize> = HashMap::new();
    for &col in CANDIDATE_COLS {
        if present_cols.contains(col) {
            col_idx.insert(col, selected.len());
            selected.push(col);
        }
    }

    let sql = format!("SELECT {} FROM conversation_summaries", selected.join(", "));
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    let Ok(rows) = stmt.query_map([], |row| {
        let cid =
            non_empty_string(row, col_idx.get("conversation_id").copied()).unwrap_or_default();
        let title = non_empty_string(row, col_idx.get("title").copied());
        let preview = non_empty_string(row, col_idx.get("preview").copied());
        let step_count = opt_u64(row, col_idx.get("step_count").copied());
        let last_modified_ms = col_idx
            .get("last_modified_time")
            .copied()
            .and_then(|i| row.get_ref(i).ok())
            .and_then(value_ref_to_millis);
        let raw_uris = non_empty_string(row, col_idx.get("workspace_uris").copied());
        let project_id = non_empty_string(row, col_idx.get("project_id").copied());
        let workspace_uri = first_workspace_path(raw_uris.as_deref()).or_else(|| {
            project_id
                .as_ref()
                .and_then(|pid| projects_map.get(pid).cloned())
        });
        let status = non_empty_string(row, col_idx.get("status").copied()).unwrap_or_default();
        let agent_name = non_empty_string(row, col_idx.get("agent_name").copied());
        let raw_role = if preview.is_none() && title.is_none() {
            match (
                agent_name.as_deref(),
                col_idx
                    .get("raw_summary")
                    .copied()
                    .and_then(|i| row.get_ref(i).ok()),
            ) {
                (Some(agent), Some(ValueRef::Blob(bytes))) => {
                    extract_subagent_role_from_raw_summary(bytes, agent)
                }
                _ => None,
            }
        } else {
            None
        };
        let preview = preview.or(raw_role);
        let parent_conversation_id =
            non_empty_string(row, col_idx.get("parent_conversation_id").copied());
        let nesting_depth = opt_u64(row, col_idx.get("nesting_depth").copied()) as u32;
        let not_fully_idle = opt_u64(row, col_idx.get("not_fully_idle").copied());
        let killed = opt_u64(row, col_idx.get("killed").copied());
        let is_running = status.eq_ignore_ascii_case("CASCADE_RUN_STATUS_RUNNING")
            || (not_fully_idle != 0 && killed == 0);

        Ok(AntigravityDbRecord {
            conversation_id: cid,
            title,
            preview,
            step_count,
            last_modified_ms,
            workspace_uri,
            status,
            project_id,
            agent_name,
            parent_conversation_id,
            nesting_depth,
            is_running,
        })
    }) else {
        return HashMap::new();
    };

    for rec in rows.flatten() {
        if !rec.conversation_id.is_empty() {
            out.insert(rec.conversation_id.clone(), rec);
        }
    }

    // Backfill parent links from `conversation_parents` table (`parent_id` or `parent_conversation_id`).
    if let Ok(mut p_pragma) = conn.prepare("PRAGMA table_info(conversation_parents)")
        && let Ok(p_cols_iter) = p_pragma.query_map([], |row| row.get::<_, String>(1))
    {
        let p_cols: HashSet<String> = p_cols_iter.flatten().collect();
        let parent_col = if p_cols.contains("parent_id") {
            Some("parent_id")
        } else if p_cols.contains("parent_conversation_id") {
            Some("parent_conversation_id")
        } else {
            None
        };
        if p_cols.contains("conversation_id")
            && let Some(p_col) = parent_col
            && let Ok(mut p_stmt) = conn.prepare(&format!(
                "SELECT conversation_id, {p_col} FROM conversation_parents"
            ))
            && let Ok(p_rows) = p_stmt.query_map([], |row| {
                let cid = row.get::<_, String>(0)?;
                let pid = row.get::<_, String>(1)?;
                Ok((cid, pid))
            })
        {
            for (cid, pid) in p_rows.flatten() {
                if !pid.trim().is_empty()
                    && let Some(rec) = out.get_mut(&cid)
                    && rec.parent_conversation_id.is_none()
                {
                    rec.parent_conversation_id = Some(pid);
                    if rec.nesting_depth == 0 {
                        rec.nesting_depth = 1;
                    }
                }
            }
        }
    }

    // Reconcile running status against live daemon when reachable so crashed/stale SQLite flags do not stick.
    #[cfg(not(test))]
    if let Some(live_map) = super::query_all_cascade_trajectories() {
        for (cid, rec) in &mut out {
            if let Some(live) = live_map.get(cid) {
                rec.is_running = live.is_running;
                if rec.workspace_uri.is_none() {
                    rec.workspace_uri = live.workspace_cwd.clone();
                }
                if live.step_count > rec.step_count {
                    rec.step_count = live.step_count;
                }
            } else if rec.is_running {
                rec.is_running = false;
            }
        }
    }

    out
}
