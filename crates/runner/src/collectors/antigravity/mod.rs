use std::{
    collections::{HashMap, HashSet},
    env,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::*;

pub mod config;
pub mod contexts;
pub mod daemon;
pub mod daemon_turn;
pub mod models;
pub mod process;
pub mod store;
pub mod stream;
pub mod transcript;
pub mod trash;

#[cfg(test)]
mod tests;

pub use config::*;
pub use contexts::*;
pub use daemon::*;
pub use daemon_turn::*;
pub use models::*;
pub use process::*;
pub(crate) use store::*;
pub(crate) use stream::*;
pub use transcript::extract_antigravity_cwd;
pub(crate) use transcript::*;
pub use trash::*;

/// Subagent session spawned from a parent Antigravity conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AntigravitySubagentEntry {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parent: String,
    #[serde(default)]
    pub depth: u32,
    pub last_activity_at_ms: i64,
    pub model: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    pub request_count: u64,
}

/// Rich remote session DTO for Antigravity, matching Claude Desktop / Coding Assistant fidelity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AntigravityRemoteSessionEntry {
    pub session_id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub last_activity_at_ms: i64,
    pub model: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    pub context_used_tokens: u64,
    pub context_window_tokens: u64,
    pub request_count: u64,
    pub is_running: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subagents: Vec<AntigravitySubagentEntry>,
}

/// Returns all existing Antigravity runtime home roots in preference order.
pub fn discover_antigravity_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    let mut seen = HashSet::new();
    let mut push_if_dir = |p: PathBuf| {
        if p.is_dir() && seen.insert(p.clone()) {
            homes.push(p);
        }
    };

    if let Some(path) = env::var_os("ANTIGRAVITY_HOME").filter(|v| !v.is_empty()) {
        push_if_dir(PathBuf::from(path));
    }
    if let Some(home) = home_dir() {
        push_if_dir(home.join(".gemini").join("antigravity"));
        push_if_dir(home.join(".gemini").join("antigravity-cli"));
        push_if_dir(home.join(".antigravity"));
    }
    homes
}

pub fn discover_antigravity_home() -> Result<Option<PathBuf>, CollectorError> {
    Ok(discover_antigravity_homes().into_iter().next())
}

/// Consolidated lookup metadata for an Antigravity session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravitySessionLookup {
    pub cwd: PathBuf,
    pub title: Option<String>,
    pub model: Option<String>,
    pub is_running: bool,
}

/// Looks up workspace path, title, recorded model, and live running flag from local stores.
pub fn find_antigravity_session_detail(
    session_id: &str,
) -> Result<Option<AntigravitySessionLookup>, CollectorError> {
    let raw_id = session_id
        .strip_prefix("antigravity_")
        .unwrap_or(session_id);
    let user_home = home_dir();
    let projects_map = user_home
        .as_deref()
        .map(discover_antigravity_projects_map)
        .unwrap_or_default();

    for ag_home in discover_antigravity_homes() {
        let db = read_antigravity_db(&ag_home, &projects_map);
        let db_rec = db.get(raw_id);
        let transcript = resolve_brain_session_transcript(&ag_home, raw_id);
        if db_rec.is_none() && transcript.is_none() {
            continue;
        }
        let metrics = transcript
            .as_deref()
            .and_then(|p| read_antigravity_transcript_metrics(p).ok())
            .unwrap_or_default();
        let cwd = db_rec.and_then(|r| r.workspace_uri.clone()).or(metrics.cwd);
        let title = db_rec.and_then(|r| r.title.clone()).or(metrics.title);
        let cwd_path = cwd.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let cwd_path = dunce::canonicalize(&cwd_path).unwrap_or(cwd_path);
        #[cfg(not(test))]
        let model = metrics
            .model
            .or_else(|| query_trajectory_model(raw_id, db_rec.map(|r| r.step_count).unwrap_or(0)));
        #[cfg(test)]
        let model = metrics.model;
        return Ok(Some(AntigravitySessionLookup {
            cwd: cwd_path,
            title,
            model,
            is_running: db_rec.is_some_and(|r| r.is_running),
        }));
    }
    Ok(None)
}

pub fn find_antigravity_session_info(
    session_id: &str,
) -> Result<Option<(PathBuf, Option<String>)>, CollectorError> {
    Ok(find_antigravity_session_detail(session_id)?.map(|d| (d.cwd, d.title)))
}

pub fn find_antigravity_session_model(session_id: &str) -> Option<String> {
    find_antigravity_session_detail(session_id)
        .ok()
        .flatten()
        .and_then(|d| d.model)
}

pub fn is_antigravity_session_running(session_id: &str) -> bool {
    find_antigravity_session_detail(session_id)
        .ok()
        .flatten()
        .is_some_and(|d| d.is_running)
}

/// Collects full Antigravity session telemetry across `conversation_summaries.db`,
/// `~/.gemini/config/projects/*.json`, and `transcript*.jsonl`, folding child subagents cleanly.
pub fn collect_antigravity_sessions_detailed()
-> Result<Vec<AntigravityRemoteSessionEntry>, CollectorError> {
    let ag_homes = discover_antigravity_homes();
    if ag_homes.is_empty() {
        return Ok(Vec::new());
    }
    let Some(user_home) = home_dir() else {
        return Ok(Vec::new());
    };
    let entries = collect_antigravity_sessions_detailed_from(&user_home, &ag_homes)?;
    note_store_snapshot(STORE_ANTIGRAVITY, &entries);
    Ok(entries)
}

pub(crate) fn collect_antigravity_sessions_detailed_from(
    user_home: &Path,
    ag_homes: &[PathBuf],
) -> Result<Vec<AntigravityRemoteSessionEntry>, CollectorError> {
    let program = "Antigravity remote session store".to_owned();
    let projects_map = discover_antigravity_projects_map(user_home);
    let default_model =
        resolve_antigravity_model_for_home(user_home, ag_homes.first().map(PathBuf::as_path));

    struct RawSession {
        session_id: String,
        cwd: String,
        title: Option<String>,
        last_activity_at_ms: i64,
        model: Option<String>,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_read_tokens: u64,
        context_used_tokens: u64,
        context_window_tokens: u64,
        request_count: u64,
        is_running: bool,
        parent_id: Option<String>,
        depth: u32,
        agent_name: String,
        preview: String,
    }

    let mut by_id: HashMap<String, RawSession> = HashMap::new();

    for ag_home in ag_homes {
        let db = read_antigravity_db(ag_home, &projects_map);
        let brain_dir = ag_home.join("brain");
        let mut seen_ids: HashSet<String> = HashSet::new();

        let mut candidates: Vec<(i64, String, Option<PathBuf>)> = Vec::new();
        if brain_dir.is_dir() {
            for entry in read_dir_entries(&brain_dir, &program)? {
                if !entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
                    continue;
                }
                let sid = entry.file_name().to_string_lossy().to_string();
                if sid.starts_with('.') {
                    continue;
                }
                let transcript = resolve_brain_transcript_path(&entry.path());
                let db_rec = db.get(&sid);
                if transcript.is_none() && db_rec.is_none() {
                    continue;
                }
                let mtime = transcript
                    .as_deref()
                    .and_then(file_modified_millis)
                    .or_else(|| db_rec.and_then(|r| r.last_modified_ms))
                    .unwrap_or(0);
                seen_ids.insert(sid.clone());
                candidates.push((mtime, sid, transcript));
            }
        }

        // Also include sessions present only in conversation_summaries.db
        for (sid, rec) in &db {
            if !seen_ids.contains(sid) {
                candidates.push((rec.last_modified_ms.unwrap_or(0), sid.clone(), None));
            }
        }

        candidates.sort_by_key(|(m, _, _)| std::cmp::Reverse(*m));
        candidates.truncate(2000);

        for (mtime, sid, transcript) in candidates {
            if by_id.contains_key(&sid) {
                continue;
            }
            let db_rec = db.get(&sid);
            let metrics = transcript
                .as_deref()
                .and_then(|p| read_antigravity_transcript_metrics(p).ok())
                .unwrap_or_default();

            #[cfg(not(test))]
            let daemon_model = if metrics.model.is_none() {
                query_trajectory_model(&sid, db_rec.map(|r| r.step_count).unwrap_or(0))
            } else {
                None
            };
            #[cfg(test)]
            let daemon_model: Option<String> = None;
            let model = metrics
                .model
                .clone()
                .or(daemon_model)
                .unwrap_or_else(|| default_model.clone());
            let context_window = resolve_antigravity_context_window(&model);
            let last_activity = metrics
                .last_activity_ms
                .or_else(|| db_rec.and_then(|r| r.last_modified_ms))
                .unwrap_or(mtime);
            let title = db_rec.and_then(|r| r.title.clone()).or(metrics.title);
            let cwd = db_rec
                .and_then(|r| r.workspace_uri.clone())
                .or(metrics.cwd)
                .unwrap_or_default();
            let request_count = metrics.request_count.max(
                if metrics.prompt_tokens > 0 || metrics.completion_tokens > 0 {
                    1
                } else {
                    0
                },
            );

            by_id.insert(
                sid.clone(),
                RawSession {
                    session_id: sid,
                    cwd,
                    title: title.clone(),
                    last_activity_at_ms: last_activity,
                    model: Some(model),
                    prompt_tokens: metrics.prompt_tokens,
                    completion_tokens: metrics.completion_tokens,
                    cache_read_tokens: metrics.cache_read_tokens,
                    context_used_tokens: metrics.context_used_tokens,
                    context_window_tokens: context_window,
                    request_count,
                    is_running: db_rec.is_some_and(|r| r.is_running),
                    parent_id: db_rec.and_then(|r| r.parent_conversation_id.clone()),
                    depth: db_rec.map(|r| r.nesting_depth).unwrap_or(0),
                    agent_name: db_rec
                        .and_then(|r| r.agent_name.clone())
                        .unwrap_or_default(),
                    preview: db_rec
                        .and_then(|r| r.preview.clone())
                        .or_else(|| db_rec.and_then(|r| r.title.clone()))
                        .or_else(|| title.clone())
                        .unwrap_or_default(),
                },
            );
        }
    }

    // Separate root conversations vs subagents whose parent exists in this store
    let known_ids: HashSet<String> = by_id.keys().cloned().collect();
    let mut children_by_root: HashMap<String, Vec<AntigravitySubagentEntry>> = HashMap::new();
    let mut roots: Vec<AntigravityRemoteSessionEntry> = Vec::new();

    for (sid, item) in &by_id {
        if let Some(ref pid) = item.parent_id
            && pid != sid
            && known_ids.contains(pid)
        {
            // Walk up to ultimate known root so tree folding stays flat per root session
            let mut root_id = pid.clone();
            let mut hops = 0_u8;
            while hops < 8 {
                if let Some(parent) = by_id.get(&root_id)
                    && let Some(ref gp) = parent.parent_id
                    && gp != &root_id
                    && known_ids.contains(gp)
                {
                    root_id = gp.clone();
                    hops += 1;
                } else {
                    break;
                }
            }
            let direct_parent = if pid == &root_id {
                String::new()
            } else {
                pid.clone()
            };
            children_by_root
                .entry(root_id)
                .or_default()
                .push(AntigravitySubagentEntry {
                    id: sid.clone(),
                    kind: if item.agent_name.is_empty() {
                        "subagent".to_owned()
                    } else {
                        item.agent_name.clone()
                    },
                    description: item.preview.clone(),
                    parent: direct_parent,
                    depth: item.depth.max(1),
                    last_activity_at_ms: item.last_activity_at_ms,
                    model: item.model.clone(),
                    prompt_tokens: item.prompt_tokens,
                    completion_tokens: item.completion_tokens,
                    cache_read_tokens: item.cache_read_tokens,
                    request_count: item.request_count,
                });
        }
    }

    for (sid, item) in by_id {
        if let Some(ref pid) = item.parent_id
            && pid != &sid
            && known_ids.contains(pid)
        {
            continue;
        }
        let mut subagents = children_by_root.remove(&sid).unwrap_or_default();
        subagents.sort_by_key(|s| s.last_activity_at_ms);
        roots.push(AntigravityRemoteSessionEntry {
            session_id: item.session_id,
            cwd: item.cwd,
            title: item.title,
            last_activity_at_ms: item.last_activity_at_ms,
            model: item.model,
            prompt_tokens: item.prompt_tokens,
            completion_tokens: item.completion_tokens,
            cache_read_tokens: item.cache_read_tokens,
            context_used_tokens: item.context_used_tokens,
            context_window_tokens: item.context_window_tokens,
            request_count: item.request_count,
            is_running: item.is_running,
            subagents,
        });
    }

    roots.sort_by_key(|r| std::cmp::Reverse(r.last_activity_at_ms));
    Ok(roots)
}

pub fn collect_antigravity_sessions_for_remote()
-> Result<Vec<(String, String, Option<String>, i64)>, CollectorError> {
    let detailed = collect_antigravity_sessions_detailed()?;
    Ok(detailed
        .into_iter()
        .map(|s| (s.session_id, s.cwd, s.title, s.last_activity_at_ms))
        .collect())
}

pub fn find_antigravity_session_cwd(session_id: &str) -> Result<Option<PathBuf>, CollectorError> {
    Ok(find_antigravity_session_info(session_id)?.map(|(p, _)| p))
}
