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
    let config_dir_canon = dunce::canonicalize(config_dir).map_err(io)?;
    let projects = config_dir.join("projects");
    let mut main: Option<(PathBuf, PathBuf)> = None;
    if is_non_symlink_dir(&projects)
        && dunce::canonicalize(&projects).is_ok_and(|p| p.starts_with(&config_dir_canon))
        && let Ok(entries) = fs::read_dir(&projects)
    {
        for project in entries.flatten() {
            if !project
                .file_type()
                .is_ok_and(|ft| !ft.is_symlink() && ft.is_dir())
            {
                continue;
            }
            let candidate = project.path().join(format!("{session_id}.jsonl"));
            if let Some(canon_main) = contained_non_symlink(&candidate, &config_dir_canon, false) {
                main = Some((candidate, canon_main));
                break;
            }
        }
    }
    let Some((raw_main, main)) = main else {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such session",
        )));
    };
    let mut paths: Vec<PathBuf> = vec![main.clone()];
    let folder = raw_main.with_extension("");
    if let Some(canon_folder) = contained_non_symlink(&folder, &config_dir_canon, true)
        && !paths.contains(&canon_folder)
    {
        paths.push(canon_folder);
    }
    for extra in ["file-history", "session-env"] {
        let extra_root = config_dir.join(extra);
        if !is_non_symlink_dir(&extra_root) {
            continue;
        }
        let dir = extra_root.join(session_id);
        if let Some(canon_dir) = contained_non_symlink(&dir, &config_dir_canon, true)
            && !paths.contains(&canon_dir)
        {
            paths.push(canon_dir);
        }
    }
    let todos_root = config_dir.join("todos");
    if is_non_symlink_dir(&todos_root)
        && let Ok(todos) = fs::read_dir(&todos_root)
    {
        for todo in todos.flatten() {
            let name = todo.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{session_id}-"))
                && name.ends_with(".json")
                && let Some(canon_todo) =
                    contained_non_symlink(&todo.path(), &config_dir_canon, false)
                && !paths.contains(&canon_todo)
            {
                paths.push(canon_todo);
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
        guard.remove(&raw_main);
    }
    if let Ok(mut guard) = session_context_cache().write() {
        guard.remove(&main);
        guard.remove(&raw_main);
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

fn is_non_symlink_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| !meta.file_type().is_symlink() && meta.is_dir())
}

fn contained_non_symlink(path: &Path, root_canon: &Path, expect_dir: bool) -> Option<PathBuf> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        return None;
    }
    if expect_dir {
        if !meta.is_dir() {
            return None;
        }
    } else if !meta.is_file() {
        return None;
    }
    let parent_canon = dunce::canonicalize(path.parent()?).ok()?;
    if !parent_canon.starts_with(root_canon) {
        return None;
    }
    let canon = dunce::canonicalize(path).ok()?;
    if !canon.starts_with(root_canon) || canon == root_canon {
        return None;
    }
    Some(canon)
}

pub(crate) fn is_cross_device_error(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::CrossesDevices {
        return true;
    }
    #[cfg(unix)]
    if err.raw_os_error() == Some(18) {
        return true;
    }
    #[cfg(windows)]
    if err.raw_os_error() == Some(17) {
        return true;
    }
    false
}

/// Moves one file or folder into place, falling back to copy+remove ONLY on
/// genuine cross-device errors.
pub(crate) fn move_into(path: &Path, target: &Path) -> Result<(), std::io::Error> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to move symlink into trash",
        ));
    }
    match fs::rename(path, target) {
        Ok(()) => Ok(()),
        Err(err) if is_cross_device_error(&err) => copy_and_remove_cross_device_with(
            path,
            target,
            |p| fs::remove_dir_all(p),
            |p| fs::remove_file(p),
        ),
        Err(err) => Err(err),
    }
}

pub(crate) fn copy_and_remove_cross_device_with<FD, FF>(
    path: &Path,
    target: &Path,
    remove_dir: FD,
    remove_file: FF,
) -> std::io::Result<()>
where
    FD: FnOnce(&Path) -> std::io::Result<()>,
    FF: FnOnce(&Path) -> std::io::Result<()>,
{
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to copy symlink into trash",
        ));
    }
    let is_dir = meta.is_dir();
    let copied = if is_dir {
        copy_dir(path, target)
    } else {
        fs::copy(path, target).map(|_| ())
    };
    if let Err(err) = copied {
        if is_dir {
            let _ = fs::remove_dir_all(target);
        } else {
            let _ = fs::remove_file(target);
        }
        return Err(err);
    }
    if is_dir {
        if let Err(error) = remove_dir(path) {
            // Never delete `target` after a failed `remove_dir_all`: the source
            // directory may already be partially removed, making `target` the
            // only intact backup of the session.
            return Err(std::io::Error::new(
                error.kind(),
                format!(
                    "copied directory to trash, then failed removing source {}: {error}; intact backup preserved at {}",
                    path.display(),
                    target.display()
                ),
            ));
        }
    } else if let Err(error) = remove_file(path) {
        // Single-file removal is atomic, so `path` is still intact.
        let _ = fs::remove_file(target);
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

const COPY_DIR_MAX_DEPTH: u32 = 32;

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    copy_dir_bounded(from, to, COPY_DIR_MAX_DEPTH)
}

fn copy_dir_bounded(from: &Path, to: &Path, max_depth: u32) -> std::io::Result<()> {
    if max_depth == 0 {
        return Err(std::io::Error::other(
            "directory nesting exceeded maximum copy depth",
        ));
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let target = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_bounded(&entry.path(), &target, max_depth - 1)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
