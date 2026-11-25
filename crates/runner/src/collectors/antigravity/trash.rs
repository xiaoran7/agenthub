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

fn contained_non_symlink_dir(path: &Path, root_canon: &Path) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return None;
    }
    let canon = dunce::canonicalize(path).ok()?;
    if !canon.starts_with(root_canon) || canon == root_canon {
        return None;
    }
    Some(canon)
}

fn is_valid_antigravity_id(id: &str) -> bool {
    id.len() >= 2
        && id.len() <= 128
        && !id.starts_with('-')
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
    if !is_valid_antigravity_id(raw_id) {
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
        let brain_root = ag_home.join("brain");
        let brain_root_canon = fs::symlink_metadata(&brain_root)
            .ok()
            .filter(|m| !m.file_type().is_symlink() && m.is_dir())
            .and_then(|_| dunce::canonicalize(&brain_root).ok());
        let brain_dir = brain_root.join(raw_id);
        if let Some(ref root_canon) = brain_root_canon
            && let Some(canon_dir) = contained_non_symlink_dir(&brain_dir, root_canon)
            && !paths.contains(&canon_dir)
        {
            paths.push(canon_dir);
        }
        let db = read_antigravity_db(ag_home, &projects_map);
        if db.contains_key(raw_id) {
            found_in_db = true;
        }
        for (cid, rec) in &db {
            if rec.parent_conversation_id.as_deref() == Some(raw_id) {
                if !is_valid_antigravity_id(cid) {
                    continue;
                }
                let child_dir = brain_root.join(cid);
                if let Some(ref root_canon) = brain_root_canon
                    && let Some(canon_child) = contained_non_symlink_dir(&child_dir, root_canon)
                    && !paths.contains(&canon_child)
                {
                    paths.push(canon_child);
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

    #[cfg(unix)]
    #[test]
    fn delete_antigravity_session_rejects_same_root_symlink() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ag_home = tmp.path().join(".gemini").join("antigravity");
        let brain = ag_home.join("brain");
        let victim = brain.join("victim-conv");
        let logs = victim.join(".system_generated").join("logs");
        fs::create_dir_all(&logs).expect("mkdir");
        let transcript = logs.join("transcript.jsonl");
        fs::write(&transcript, "{}\n").expect("write");
        let _ = filetime_or_touch_back(&transcript);

        let symlink_path = brain.join("evil-link");
        std::os::unix::fs::symlink(&victim, &symlink_path).expect("symlink");

        let trash_root = tmp.path().join("trash");
        let err = delete_antigravity_session_in(
            tmp.path(),
            std::slice::from_ref(&ag_home),
            &trash_root,
            "evil-link",
            false,
        )
        .expect_err("symlink session dir must be rejected");
        assert!(matches!(err, CollectorError::Io { .. }));
        assert!(victim.is_dir(), "symlink target must remain untouched");
        assert!(transcript.is_file());
    }

    fn filetime_or_touch_back(path: &Path) -> bool {
        let Ok(file) = fs::OpenOptions::new().write(true).open(path) else {
            return false;
        };
        let past = SystemTime::now() - Duration::from_secs(120);
        file.set_modified(past).is_ok()
    }
}
