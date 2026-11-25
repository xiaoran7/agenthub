use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OpenFlags};

use super::{
    CollectorError,
    claude_trash::{move_all_into, written_within},
    codex::codex_threads_cache,
    codex_enrich::{
        clear_codex_enrichment_cache, parse_codex_subagent_meta, strip_windows_verbatim,
    },
    home_dir,
};

fn is_valid_codex_id(id: &str) -> bool {
    id.len() >= 2
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn fname_matches_session_id(fname: &str, raw_id: &str) -> bool {
    let Some(stem) = fname
        .strip_suffix(".jsonl.zst")
        .or_else(|| fname.strip_suffix(".jsonl"))
    else {
        return false;
    };
    stem == raw_id || stem.ends_with(&format!("-{raw_id}"))
}

fn has_symlink_component_between(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return true;
    };
    let mut cur = root.to_path_buf();
    for comp in rel.components() {
        match comp {
            std::path::Component::Normal(part) => {
                cur.push(part);
                if fs::symlink_metadata(&cur)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(true)
                {
                    return true;
                }
            }
            std::path::Component::CurDir => {}
            _ => return true,
        }
    }
    false
}

fn collect_matching_rollouts(dir: &Path, raw_id: &str, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 6
        || !fs::symlink_metadata(dir)
            .is_ok_and(|meta| !meta.file_type().is_symlink() && meta.is_dir())
    {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            collect_matching_rollouts(&path, raw_id, depth + 1, out);
        } else if ft.is_file() {
            let fname = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if fname_matches_session_id(fname, raw_id) && !out.contains(&path) {
                out.push(path);
            }
        }
    }
}

/// Moves a Codex session's rollout `.jsonl` files (and any child subagent rollouts) into
/// `~/.agenthub/trash/sessions/`, refusing sessions modified within the last minute.
pub fn delete_codex_session(session_id: &str) -> Result<PathBuf, CollectorError> {
    let raw_id = session_id.strip_prefix("codex_").unwrap_or(session_id);
    let user_home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    let codex_root = std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| user_home.join(".codex"));
    let trash_root = user_home.join(".agenthub").join("trash").join("sessions");
    delete_codex_session_in(&codex_root, &trash_root, raw_id)
}

pub(crate) fn delete_codex_session_in(
    codex_root: &Path,
    trash_root: &Path,
    raw_id: &str,
) -> Result<PathBuf, CollectorError> {
    let program = "Codex session store";
    let io = |source: std::io::Error| CollectorError::Io {
        program: program.to_owned(),
        source,
    };
    if !is_valid_codex_id(raw_id) {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a valid Codex session id",
        )));
    }

    let Ok(codex_root_canon) = dunce::canonicalize(codex_root) else {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such Codex session",
        )));
    };
    let allowed_roots: Vec<PathBuf> = ["sessions", "archived_sessions"]
        .into_iter()
        .filter_map(|sub| {
            let p = codex_root_canon.join(sub);
            let meta = fs::symlink_metadata(&p).ok()?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return None;
            }
            let canon = dunce::canonicalize(&p).ok()?;
            canon.starts_with(&codex_root_canon).then_some(canon)
        })
        .collect();
    let mut target_ids = vec![raw_id.to_owned()];
    let mut paths: Vec<PathBuf> = Vec::new();

    // Check SQLite for rollout_path and child subagent threads
    let mut db_candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&codex_root_canon) {
        for e in entries.flatten() {
            if !e
                .file_type()
                .is_ok_and(|ft| !ft.is_symlink() && ft.is_file())
            {
                continue;
            }
            let p = e.path();
            if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n == "state.sqlite" || (n.starts_with("state_") && n.ends_with(".sqlite"))
            }) {
                db_candidates.push(p);
            }
        }
    }

    for db_path in &db_candidates {
        if let Ok(conn) = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            let _ = conn.busy_timeout(Duration::from_millis(500));
            if let Ok(mut stmt) = conn.prepare("SELECT id, source, rollout_path FROM threads")
                && let Ok(rows) = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1).unwrap_or_default(),
                        r.get::<_, Option<String>>(2).unwrap_or_default(),
                    ))
                })
            {
                for (tid, src, rp) in rows.flatten() {
                    let is_child = src
                        .as_deref()
                        .and_then(|s| parse_codex_subagent_meta(s, None, None, None))
                        .is_some_and(|(pid, _, _)| pid == raw_id);
                    if tid == raw_id || (is_child && is_valid_codex_id(&tid)) {
                        if is_child && !target_ids.contains(&tid) {
                            target_ids.push(tid.clone());
                        }
                        if let Some(raw_p) = rp {
                            let clean_p = PathBuf::from(strip_windows_verbatim(&raw_p));
                            let leaf_ok = fs::symlink_metadata(&clean_p)
                                .is_ok_and(|m| !m.file_type().is_symlink() && m.is_file());
                            let chain_ok =
                                !has_symlink_component_between(&codex_root_canon, &clean_p)
                                    || !has_symlink_component_between(codex_root, &clean_p);
                            if leaf_ok
                                && chain_ok
                                && let Ok(canon_p) = dunce::canonicalize(&clean_p)
                                && allowed_roots
                                    .iter()
                                    .any(|r| canon_p.starts_with(r) && canon_p != *r)
                                && canon_p
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .is_some_and(|n| fname_matches_session_id(n, &tid))
                                && !paths.contains(&canon_p)
                            {
                                paths.push(canon_p);
                            }
                        }
                    }
                }
            }
        }
    }

    for tid in &target_ids {
        for root in &allowed_roots {
            collect_matching_rollouts(root, tid, 0, &mut paths);
        }
    }

    if paths.is_empty() {
        return Err(io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such Codex session",
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
    let trash = trash_root.join(format!("{stamp}-codex_{raw_id}"));
    fs::create_dir_all(&trash).map_err(io)?;
    move_all_into(&trash, &paths).map_err(io)?;

    // Best-effort mark archived = 1 in SQLite so deleted threads hide immediately
    for db_path in &db_candidates {
        if let Ok(conn) = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            let _ = conn.busy_timeout(Duration::from_millis(500));
            for tid in &target_ids {
                let _ = conn.execute("UPDATE threads SET archived = 1 WHERE id = ?1", [tid]);
            }
        }
    }

    if let Ok(mut guard) = codex_threads_cache().write() {
        *guard = None;
    }
    clear_codex_enrichment_cache();
    Ok(trash)
}
