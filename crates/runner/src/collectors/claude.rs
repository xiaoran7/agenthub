use std::{
    borrow::Cow,
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::UNIX_EPOCH,
};

use serde::Deserialize;
use serde_json::json;

use super::*;

pub fn collect_claude_transcripts(include_completed: bool) -> Result<String, CollectorError> {
    let config_dir = discover_claude_config_dir()?;
    collect_claude_transcripts_from(&config_dir, include_completed)
}

pub fn collect_claude_transcripts_from(
    config_dir: &Path,
    include_completed: bool,
) -> Result<String, CollectorError> {
    let projects_dir = config_dir.join("projects");
    if !projects_dir.is_dir() {
        return Ok("[]".to_owned());
    }

    let program = "Claude transcript store".to_owned();
    let mut entries = Vec::new();
    for project in fs::read_dir(&projects_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })? {
        let project = project.map_err(|source| CollectorError::Io {
            program: program.clone(),
            source,
        })?;
        if !project
            .file_type()
            .map_err(|source| CollectorError::Io {
                program: program.clone(),
                source,
            })?
            .is_dir()
        {
            continue;
        }
        let files = match fs::read_dir(project.path()) {
            Ok(files) => files,
            Err(_) => continue,
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
                || !file.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            if let Some(entry) = read_claude_transcript_metadata(&path, &program)?
                && entry.has_output
            {
                entries.push(entry);
            }
        }
    }

    entries.sort_by_key(|entry| std::cmp::Reverse(entry.last_activity_at_ms));
    entries.truncate(if include_completed {
        CLAUDE_ALL_SESSION_LIMIT
    } else {
        CLAUDE_RECENT_SESSION_LIMIT
    });
    serde_json::to_string(
        &entries
            .into_iter()
            .map(|entry| {
                json!({
                    "sessionId": entry.session_id,
                    "cwd": entry.cwd,
                    "title": entry.title,
                    "kind": "transcript",
                    "state": "persisted",
                    "startedAt": entry.started_at_ms,
                    "lastActivityAt": entry.last_activity_at_ms
                })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|source| CollectorError::InvalidJson { program, source })
}

/// Reads only explicit, bounded session metadata for the desktop detail view.
/// Titles prefer provider-owned title events and fall back to a bounded local
/// summary of the first meaningful user request. They remain separate from
/// SessionObservation so neither the title nor prompt enters the sync outbox.
pub fn collect_claude_session_contexts(
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let config_dir = discover_claude_config_dir()?;
    collect_claude_session_contexts_from(&config_dir, limit)
}

pub fn collect_claude_session_contexts_from(
    config_dir: &Path,
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let projects_dir = config_dir.join("projects");
    if !projects_dir.is_dir() {
        return Ok(Vec::new());
    }
    let program = "Claude transcript context".to_owned();
    let mut candidates = Vec::new();
    for project in fs::read_dir(&projects_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })? {
        let Ok(project) = project else { continue };
        if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
                || !file.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            let modified = file
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis())
                .unwrap_or_default();
            candidates.push((modified, path));
        }
    }
    candidates.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    candidates.truncate(limit);

    let mut contexts = Vec::with_capacity(candidates.len());
    for (_, path) in candidates {
        if let Some(context) = read_claude_session_context(&path, &program)? {
            contexts.push(context);
        }
    }
    Ok(contexts)
}

pub(crate) fn session_context_cache()
-> &'static RwLock<BoundedLruCache<PathBuf, (u64, u128, Option<LocalSessionContext>)>> {
    static CACHE: OnceLock<
        RwLock<BoundedLruCache<PathBuf, (u64, u128, Option<LocalSessionContext>)>>,
    > = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BoundedLruCache::new(4_000)))
}

pub(crate) fn discover_claude_config_dir() -> Result<PathBuf, CollectorError> {
    if let Some(path) = env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    home_dir()
        .map(|home| home.join(".claude"))
        .ok_or_else(|| CollectorError::Io {
            program: "Claude transcript store".to_owned(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot resolve the current user's home directory",
            ),
        })
}

pub fn find_claude_session_info(
    session_id: &str,
) -> Result<Option<(PathBuf, Option<String>)>, CollectorError> {
    let target_id = session_id.trim_start_matches("claude_");
    if let Some(cached_path) = cached_claude_session_path(target_id)
        && cached_path.is_file()
        && let Some(entry) =
            read_claude_transcript_metadata(&cached_path, "Claude transcript lookup")?
    {
        let cwd = PathBuf::from(entry.cwd);
        let cwd = dunce::canonicalize(&cwd).unwrap_or(cwd);
        return Ok(Some((cwd, entry.title)));
    }

    let config_dir = discover_claude_config_dir()?;
    let projects_dir = config_dir.join("projects");
    if !projects_dir.is_dir() {
        return Ok(None);
    }
    let program = "Claude transcript lookup".to_owned();
    let target_filename = format!("{}.jsonl", target_id);
    for project in fs::read_dir(&projects_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })? {
        let Ok(project) = project else { continue };
        if !project.file_type().is_ok_and(|k| k.is_dir()) {
            continue;
        }
        let candidate_file = project.path().join(&target_filename);
        if candidate_file.is_file()
            && let Some(entry) = read_claude_transcript_metadata(&candidate_file, &program)?
        {
            let cwd = PathBuf::from(entry.cwd);
            let cwd = dunce::canonicalize(&cwd).unwrap_or(cwd);
            return Ok(Some((cwd, entry.title)));
        }
    }
    Ok(None)
}

#[derive(Default, Deserialize)]
struct ClaudeModelMessage<'a> {
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
}

#[derive(Default, Deserialize)]
struct ClaudeModelIdentity<'a> {
    #[serde(borrow, rename = "modelId")]
    model_id: Option<Cow<'a, str>>,
}

#[derive(Default, Deserialize)]
struct ClaudeModelAttachment<'a> {
    #[serde(borrow)]
    identity: Option<ClaudeModelIdentity<'a>>,
}

#[derive(Default, Deserialize)]
struct ClaudeModelLine<'a> {
    #[serde(borrow)]
    message: Option<ClaudeModelMessage<'a>>,
    #[serde(borrow)]
    attachment: Option<ClaudeModelAttachment<'a>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
}

pub fn collect_claude_sessions_for_remote() -> Result<Vec<ClaudeRemoteSessionEntry>, CollectorError>
{
    let mut roots = crate::usage_ledger::claude_transcript_roots();
    if let Ok(cfg) = discover_claude_config_dir()
        && cfg.join("projects").is_dir()
        && !roots.iter().any(|(r, _)| r == &cfg)
    {
        roots.insert(0, (cfg, "claude_code"));
    }
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let program = "Claude transcript store".to_owned();
    let mut entries = Vec::new();
    let mut seen_paths = HashSet::new();

    for (config_dir, default_agent) in roots {
        let projects_dir = config_dir.join("projects");
        let Ok(projects) = fs::read_dir(&projects_dir) else {
            continue;
        };
        for project in projects {
            let Ok(project) = project else { continue };
            if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Ok(files) = fs::read_dir(project.path()) else {
                continue;
            };
            let mut project_candidates = Vec::new();
            for file in files.flatten() {
                let path = file.path();
                if path.extension().and_then(|value| value.to_str()) != Some("jsonl")
                    || !file.file_type().is_ok_and(|kind| kind.is_file())
                    || !seen_paths.insert(path.clone())
                {
                    continue;
                }
                let Some((_, modified_i64)) = crate::usage_ledger::live_file_mark(&path) else {
                    continue;
                };
                let modified = modified_i64.max(0) as u128;
                project_candidates.push((modified, path));
            }
            // Limit scanning to most recent files per project to prevent unbounded I/O
            project_candidates.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
            project_candidates.truncate(500);

            for (_, path) in project_candidates {
                if let Some(entry) = read_claude_transcript_metadata(&path, &program)?
                    && entry.has_output
                {
                    let subagents = collect_claude_subagents(&path);
                    entries.push(ClaudeRemoteSessionEntry {
                        session_id: entry.session_id,
                        cwd: entry.cwd,
                        title: entry.title,
                        last_activity_at_ms: entry.last_activity_at_ms.unwrap_or(0),
                        model: entry.model,
                        prompt_tokens: entry.prompt_tokens,
                        completion_tokens: entry.completion_tokens,
                        cache_read_tokens: entry.cache_read_tokens,
                        context_used_tokens: entry.context_used_tokens,
                        context_window_tokens: entry.context_window_tokens,
                        request_count: entry.request_count,
                        desktop: entry.desktop || default_agent == "claude_desktop",
                        subagents,
                    });
                }
            }
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.last_activity_at_ms));
    entries.truncate(5000);
    note_store_snapshot(STORE_CLAUDE, &entries);
    Ok(entries)
}

/// Inspects the Claude Code session transcript to find the model recorded for the session.
pub fn find_claude_session_model(session_id: &str) -> Option<String> {
    let home = home_dir()?;
    let raw_session_id = session_id.strip_prefix("claude_").unwrap_or(session_id);
    let path = crate::conversation::find_transcript_file(
        &home.join(".claude").join("projects"),
        raw_session_id,
        0,
    )?;
    let file = fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    for line in reader.lines().take(60).flatten() {
        if let Ok(record) = serde_json::from_str::<ClaudeModelLine<'_>>(&line) {
            if let Some(model) = record.message.as_ref().and_then(|m| m.model.as_deref()) {
                let trimmed = model.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            if let Some(model) = record
                .attachment
                .as_ref()
                .and_then(|a| a.identity.as_ref())
                .and_then(|i| i.model_id.as_deref())
            {
                let trimmed = model.trim();
                if !trimmed.is_empty() {
                    let cleaned = trimmed.split('[').next().unwrap_or(trimmed).trim();
                    if !cleaned.is_empty() {
                        return Some(cleaned.to_string());
                    }
                }
            }
            if let Some(model) = record.model.as_deref() {
                let trimmed = model.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}
