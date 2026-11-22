use std::{fs, path::Path};

use serde_json::json;

use super::*;

const ANTIGRAVITY_RECENT_SESSION_LIMIT: usize = 100;
const ANTIGRAVITY_ALL_SESSION_LIMIT: usize = 1_000;

pub fn collect_antigravity_sessions(include_completed: bool) -> Result<String, CollectorError> {
    let Some(home) = discover_antigravity_home()? else {
        return Ok("[]".to_owned());
    };
    collect_antigravity_sessions_from(&home, include_completed)
}

pub fn collect_antigravity_sessions_from(
    home: &Path,
    include_completed: bool,
) -> Result<String, CollectorError> {
    let program = "Antigravity session store".to_owned();
    let brain_dir = home.join("brain");
    if !brain_dir.is_dir() {
        return Ok("[]".to_owned());
    }
    let user_home = infer_user_home_from_ag_root(home);
    let projects_map = user_home
        .as_deref()
        .map(discover_antigravity_projects_map)
        .unwrap_or_default();
    let db = read_antigravity_db(home, &projects_map);

    let entries = fs::read_dir(&brain_dir).map_err(|source| CollectorError::Io {
        program: program.clone(),
        source,
    })?;
    let mut candidate_transcripts = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|k| k.is_dir()) {
            continue;
        }
        let session_id = entry.file_name().to_string_lossy().to_string();
        if session_id.starts_with('.') {
            continue;
        }
        let Some(transcript_path) = resolve_brain_transcript_path(&entry.path()) else {
            continue;
        };
        let modified_at_ms = file_modified_millis(&transcript_path).unwrap_or(0);
        candidate_transcripts.push((modified_at_ms, session_id, transcript_path));
    }

    candidate_transcripts.sort_by_key(|(m, _, _)| std::cmp::Reverse(*m));
    let max_limit = if include_completed {
        ANTIGRAVITY_ALL_SESSION_LIMIT
    } else {
        ANTIGRAVITY_RECENT_SESSION_LIMIT
    };
    candidate_transcripts.truncate(max_limit);

    let mut sessions = Vec::with_capacity(candidate_transcripts.len());
    for (modified_at_ms, session_id, transcript_path) in candidate_transcripts {
        let (started_at_ms, transcript_cwd, steps) =
            read_antigravity_transcript_sample(&transcript_path, &program)?;
        let db_rec = db.get(&session_id);
        let cwd = db_rec
            .and_then(|r| r.workspace_uri.clone())
            .or(transcript_cwd)
            .unwrap_or_default();
        let last_activity_at_ms = if modified_at_ms > 0 {
            Some(modified_at_ms)
        } else {
            db_rec.and_then(|r| r.last_modified_ms).or(started_at_ms)
        };
        sessions.push(json!({
            "sessionId": session_id,
            "cwd": cwd,
            "title": db_rec.and_then(|r| r.title.clone()),
            "startedAt": started_at_ms,
            "lastActivityAt": last_activity_at_ms,
            "steps": steps,
        }));
    }

    serde_json::to_string(&sessions)
        .map_err(|source| CollectorError::InvalidJson { program, source })
}

pub fn collect_antigravity_session_contexts(
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let Some(home) = discover_antigravity_home()? else {
        return Ok(Vec::new());
    };
    collect_antigravity_session_contexts_from(&home, limit)
}

pub fn collect_antigravity_session_contexts_from(
    home: &Path,
    limit: usize,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let program = "Antigravity session context".to_owned();
    let brain_dir = home.join("brain");
    if !brain_dir.is_dir() {
        return Ok(Vec::new());
    }

    let user_home = infer_user_home_from_ag_root(home);
    let local_cfg = user_home
        .as_deref()
        .map(|uh| load_antigravity_local_config_from(uh, Some(home)))
        .unwrap_or_else(|| load_antigravity_local_config_from(home, Some(home)));
    let default_model = local_cfg
        .resolved_model
        .clone()
        .unwrap_or_else(|| "gemini-3.8-flash".to_owned());
    let permission_mode = local_cfg
        .auto_execution_policy
        .clone()
        .unwrap_or_else(|| "local_confirmation".to_owned());
    let projects_map = user_home
        .as_deref()
        .map(discover_antigravity_projects_map)
        .unwrap_or_default();
    let db = read_antigravity_db(home, &projects_map);

    let mut candidates = Vec::new();
    for entry in read_dir_entries(&brain_dir, &program)? {
        if !entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some(transcript) = resolve_brain_transcript_path(&entry.path()) else {
            continue;
        };
        let modified = file_modified_millis(&transcript).unwrap_or(0);
        candidates.push((modified, transcript, entry.file_name()));
    }
    candidates.sort_by_key(|(m, _, _)| std::cmp::Reverse(*m));
    candidates.truncate(limit);

    let mut contexts = Vec::with_capacity(candidates.len());
    for (_, transcript, dir_name) in candidates {
        let session_id = dir_name.to_string_lossy().to_string();
        let db_rec = db.get(&session_id);
        let metrics = read_antigravity_transcript_metrics(&transcript).unwrap_or_default();
        #[cfg(not(test))]
        let daemon_model = if metrics.model.is_none() {
            query_trajectory_model(&session_id, db_rec.map(|r| r.step_count).unwrap_or(0))
        } else {
            None
        };
        #[cfg(test)]
        let daemon_model: Option<String> = None;
        let resolved_model = metrics
            .model
            .or(daemon_model)
            .unwrap_or_else(|| default_model.clone());
        let window = resolve_antigravity_context_window(&resolved_model);

        let mut context =
            LocalSessionContext::new(crate::domain::AgentType::Antigravity, session_id);
        context.model = Some(resolved_model);
        context.permission_mode = Some(permission_mode.clone());
        context.title = db_rec.and_then(|r| r.title.clone()).or(metrics.title);
        context.context_window_tokens = Some(window);
        if metrics.context_used_tokens > 0 {
            context.context_used_tokens = Some(metrics.context_used_tokens);
        }
        if metrics.prompt_tokens > 0 || metrics.completion_tokens > 0 {
            context.input_tokens = Some(metrics.prompt_tokens);
            context.output_tokens = Some(metrics.completion_tokens);
            context.cache_read_tokens = Some(metrics.cache_read_tokens);
            context.session_total_tokens = Some(
                metrics
                    .prompt_tokens
                    .saturating_add(metrics.completion_tokens),
            );
        }
        contexts.push(context);
    }
    Ok(contexts)
}
