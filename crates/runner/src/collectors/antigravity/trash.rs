use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{
    clear_antigravity_transcript_cache, delete_daemon_trajectory, discover_antigravity_homes,
    discover_antigravity_projects_map, is_antigravity_session_running, read_antigravity_db,
};
use crate::collectors::{
    CollectorError,
    claude_trash::{move_all_into, written_within},
    home_dir,
};

/// Moves an Antigravity conversation's `brain/<id>` directory (plus any nested subagent
/// `brain/<child_id>` directories) into `~/.agenthub/trash/sessions/`, refusing sessions
/// that are actively running or modified within the last minute.
pub fn delete_antigravity_session(session_id: &str) -> Result<PathBuf, CollectorError> {
    let raw_id = session_id
        .strip_prefix("antigravity_")
        .unwrap_or(session_id);
    let homes = discover_antigravity_homes();
    let user_home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    let trash_root = user_home.join(".agenthub").join("trash").join("sessions");
    delete_antigravity_session_in(&user_home, &homes, &trash_root, raw_id, true)
}

pub(crate) fn delete_antigravity_session_in(
    user_home: &Path,
    ag_homes: &[PathBuf],
    trash_root: &Path,
    raw_id: &str,
    notify_daemon: bool,
) -> Result<PathBuf, CollectorError> {
    let program = "Antigravity session store";
    let io = |source: std::io::Error| CollectorError::Io {
        program: program.to_owned(),
        source,
    };
    let valid = raw_id.len() >= 2
        && raw_id.len() <= 128
        && raw_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a valid Antigravity session id",
        )));
    }
    if notify_daemon && is_antigravity_session_running(raw_id) {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the Antigravity conversation is currently running",
        )));
    }

    let projects_map = discover_antigravity_projects_map(user_home);
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut found_in_db = false;

    for ag_home in ag_homes {
        let brain_dir = ag_home.join("brain").join(raw_id);
        if brain_dir.is_dir() {
            paths.push(brain_dir);
        }
        let db = read_antigravity_db(ag_home, &projects_map);
        if db.contains_key(raw_id) {
            found_in_db = true;
        }
        for (cid, rec) in &db {
            if rec.parent_conversation_id.as_deref() == Some(raw_id) {
                let child_dir = ag_home.join("brain").join(cid);
                if child_dir.is_dir() && !paths.contains(&child_dir) {
                    paths.push(child_dir);
                }
            }
        }
    }

    if paths.is_empty() && !found_in_db {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such Antigravity session",
        )));
    }

    let now = SystemTime::now();
    for path in &paths {
        if written_within(path, now, Duration::from_secs(60)) {
            return Err(io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the session was written to in the last minute; it may still be running",
            )));
        }
    }

    let stamp = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let trash = trash_root.join(format!("{stamp}-antigravity_{raw_id}"));
    fs::create_dir_all(&trash).map_err(io)?;
    if !paths.is_empty() {
        move_all_into(&trash, &paths).map_err(io)?;
    }
    if notify_daemon {
        let _ = delete_daemon_trajectory(raw_id);
    }
    clear_antigravity_transcript_cache();
    Ok(trash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_antigravity_session_moves_brain_folder_and_respects_write_guard() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ag_home = tmp.path().join(".gemini").join("antigravity");
        let brain = ag_home.join("brain").join("conv-123");
        let logs = brain.join(".system_generated").join("logs");
        fs::create_dir_all(&logs).expect("mkdir");
        let transcript = logs.join("transcript.jsonl");
        fs::write(
            &transcript,
            "{\"type\":\"USER_INPUT\",\"content\":\"hi\"}\n",
        )
        .expect("write");

        let trash_root = tmp.path().join("trash");
        let err = delete_antigravity_session_in(
            tmp.path(),
            std::slice::from_ref(&ag_home),
            &trash_root,
            "conv-123",
            false,
        )
        .expect_err("fresh session must be refused");
        assert!(err.to_string().contains("last minute"));
        assert!(brain.is_dir());

        let old = filetime_or_touch_back(&transcript);
        if old {
            let moved = delete_antigravity_session_in(
                tmp.path(),
                std::slice::from_ref(&ag_home),
                &trash_root,
                "conv-123",
                false,
            )
            .expect("aged session moves to trash");
            assert!(moved.is_dir());
            assert!(!brain.exists());
        }
    }

    fn filetime_or_touch_back(path: &Path) -> bool {
        let Ok(file) = fs::OpenOptions::new().write(true).open(path) else {
            return false;
        };
        let past = SystemTime::now() - Duration::from_secs(120);
        file.set_modified(past).is_ok()
    }
}
