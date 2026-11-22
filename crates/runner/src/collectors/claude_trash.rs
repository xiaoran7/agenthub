use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::*;

/// Moves a Coding Assistant session's files to the runner's trash: the
/// conversation's own transcript, the folder beside it (its subagents and tool
/// results), and what Coding Assistant keeps by the session's id. A session
/// any file of which was written in the last minute is refused — its agent may
/// still be running it.
///
/// Returns the folder the files were moved into, or the reason they were not.
pub fn delete_claude_session(session_id: &str) -> Result<PathBuf, CollectorError> {
    let config_dir = discover_claude_config_dir()?;
    let trash_root = home_dir()
        .unwrap_or_else(|| config_dir.clone())
        .join(".agenthub")
        .join("trash")
        .join("sessions");
    delete_claude_session_in(&config_dir, &trash_root, session_id)
}

/// [`delete_claude_session`] against a store the caller names, so a test can
/// run it on a scratch store rather than the reader's own.
pub(crate) fn delete_claude_session_in(
    config_dir: &Path,
    trash_root: &Path,
    session_id: &str,
) -> Result<PathBuf, CollectorError> {
    let program = "Claude transcript store";
    let io = |source: std::io::Error| CollectorError::Io {
        program: program.to_owned(),
        source,
    };
    let valid = session_id.len() >= 2
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a session id",
        )));
    }
    let projects = config_dir.join("projects");
    let mut main: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&projects) {
        for project in entries.flatten() {
            let candidate = project.path().join(format!("{session_id}.jsonl"));
            if candidate.is_file() {
                main = Some(candidate);
                break;
            }
        }
    }
    let Some(main) = main else {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such session",
        )));
    };
    let mut paths: Vec<PathBuf> = vec![main.clone()];
    let folder = main.with_extension("");
    if folder.is_dir() {
        paths.push(folder);
    }
    for extra in ["file-history", "session-env"] {
        let dir = config_dir.join(extra).join(session_id);
        if dir.is_dir() {
            paths.push(dir);
        }
    }
    if let Ok(todos) = fs::read_dir(config_dir.join("todos")) {
        for todo in todos.flatten() {
            let name = todo.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{session_id}-")) && name.ends_with(".json") {
                paths.push(todo.path());
            }
        }
    }
    // a session being written to is not deleted: its agent may still be using it
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
    let trash = trash_root.join(format!("{stamp}-{session_id}"));
    fs::create_dir_all(&trash).map_err(io)?;
    // The conversation's folder and Coding Assistant's own per-id folders all
    // carry the session's id, so a bare file name would put them in the same
    // place: the second move would find its target taken, fall back to a copy,
    // and could never remove the original directory. Each item keeps its
    // source folder in the name when a bare name is already in use, so every
    // move lands on a place of its own.
    move_all_into(&trash, &paths).map_err(io)?;
    forget_subagents_under(&paths);
    if let Ok(mut guard) = transcript_metadata_cache().write() {
        guard.remove(&main);
    }
    if let Ok(mut guard) = session_context_cache().write() {
        guard.remove(&main);
    }
    Ok(trash)
}

/// Moves every one of a session's files into its trash folder, or none of
/// them: a move that fails part way puts back what it already moved. A delete
/// that reports failure has to leave the session whole, or the page would keep
/// the row, the retry would find no transcript, and the reader would have lost
/// a conversation their agent still points at.
pub(crate) fn move_all_into(trash: &Path, paths: &[PathBuf]) -> std::io::Result<()> {
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(paths.len());
    for path in paths {
        let target = trash_target(trash, path);
        if let Err(failure) = move_into(path, &target) {
            let unreturned: Vec<String> = done
                .iter()
                .rev()
                .filter(|(from, to)| move_into(to, from).is_err())
                .map(|(from, _)| from.display().to_string())
                .collect();
            let restored = if unreturned.is_empty() {
                " and put it all back".to_owned()
            } else {
                format!(" and could not put back {}", unreturned.join(", "))
            };
            return Err(std::io::Error::other(format!(
                "moved {} of {} files, then {}: {failure}{restored}",
                done.len(),
                paths.len(),
                path.display()
            )));
        }
        done.push((path.clone(), target));
    }
    Ok(())
}

/// Where one of a session's files goes in its trash folder: its own name,
/// qualified by the folder it came from when the name is already taken.
fn trash_target(trash: &Path, path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_owned());
    let mut target = trash.join(&name);
    if target.exists() {
        let parent = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_owned());
        target = trash.join(format!("{parent}-{name}"));
    }
    target
}

/// Moves one file or folder into place, across volumes when it must.
fn move_into(path: &Path, target: &Path) -> Result<(), std::io::Error> {
    if fs::rename(path, target).is_ok() {
        return Ok(());
    }
    // another volume: a copy then a remove is the same move
    let copied = if path.is_dir() {
        copy_dir(path, target)
    } else {
        fs::copy(path, target).map(|_| ())
    };
    copied?;
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if let Err(error) = removed {
        // the copy is a duplicate of something still in the store: take it
        // back out, so the trash never claims a session it did not get
        if target.is_dir() {
            let _ = fs::remove_dir_all(target);
        } else {
            let _ = fs::remove_file(target);
        }
        return Err(error);
    }
    Ok(())
}

/// How deep the write guard looks into a session's folder. A transcript's own
/// nesting is a handful of levels; anything deeper is a link or a surprise,
/// and the guard says "not writing" rather than walking it.
const WRITE_GUARD_DEPTH: u32 = 16;

pub(crate) fn written_within(path: &Path, now: SystemTime, window: Duration) -> bool {
    written_within_at(path, now, window, 0)
}

fn written_within_at(path: &Path, now: SystemTime, window: Duration, depth: u32) -> bool {
    // `symlink_metadata` never follows a link: a junction back into the tree
    // would otherwise make this recurse until the stack ran out
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    let age_is_fresh = || {
        meta.modified()
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age < window)
    };
    if meta.file_type().is_symlink() {
        // a link is not a file being written; what it points at is the
        // store's business, and following it is how a cycle is walked
        return false;
    }
    if !meta.is_dir() {
        return age_is_fresh();
    }
    // a folder's own mtime moves when a child is added or removed, which is
    // not a write to a transcript: only what is under it counts
    if depth >= WRITE_GUARD_DEPTH {
        return false;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| written_within_at(&entry.path(), now, window, depth + 1))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
