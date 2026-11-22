use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

use super::*;

struct NonClaudeContextUsageCache {
    updated_at: Instant,
    home: PathBuf,
    populated_agents: HashSet<crate::domain::AgentType>,
    usages: HashMap<(crate::domain::AgentType, String), ContextUsage>,
}

fn non_claude_context_usage_cache() -> &'static RwLock<Option<NonClaudeContextUsageCache>> {
    static CACHE: OnceLock<RwLock<Option<NonClaudeContextUsageCache>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Resolves the latest known context usage (`used` and `window` tokens) for a
/// local native session so resumed/restored sessions immediately report their
/// real context fullness to the remote console.
pub fn lookup_session_context_usage(
    home: &Path,
    agent_type: crate::domain::AgentType,
    session_id: &str,
    workspace: Option<&Path>,
) -> Option<ContextUsage> {
    let raw_id = session_id
        .strip_prefix("claude_")
        .or_else(|| session_id.strip_prefix("antigravity_"))
        .unwrap_or(session_id);

    let is_claude = matches!(
        agent_type,
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop
    );
    if !is_claude
        && let Ok(guard) = non_claude_context_usage_cache().read()
        && let Some(ref cache) = *guard
        && cache.home == home
        && cache.updated_at.elapsed() < Duration::from_secs(15)
    {
        if let Some(usage) = cache
            .usages
            .get(&(agent_type.clone(), raw_id.to_string()))
            .or_else(|| {
                cache
                    .usages
                    .get(&(agent_type.clone(), session_id.to_string()))
            })
        {
            return Some(*usage);
        }
        if cache.populated_agents.contains(&agent_type) {
            return None;
        }
    }

    match agent_type {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            if let Some(usage) = cached_claude_context_usage(raw_id) {
                return Some(usage);
            }
            let path = workspace
                .and_then(|ws| {
                    crate::conversation::find_claude_workspace_transcript(home, ws, raw_id)
                })
                .or_else(|| {
                    let config_dir =
                        discover_claude_config_dir().unwrap_or_else(|_| home.join(".claude"));
                    crate::conversation::find_transcript_file(
                        &config_dir.join("projects"),
                        raw_id,
                        0,
                    )
                })?;
            let ctx = read_claude_session_context(&path, "Claude transcript context").ok()??;
            let used = ctx.context_used_tokens.filter(|v| *v > 0)?;
            Some(ContextUsage {
                used,
                window: ctx.context_window_tokens.filter(|v| *v > 0),
            })
        }
        crate::domain::AgentType::Codex | crate::domain::AgentType::Antigravity => {
            let contexts = match agent_type {
                crate::domain::AgentType::Codex => {
                    let codex_dir = home.join(".codex");
                    let db_path = ["state_5.sqlite", "state_4.sqlite", "state.sqlite"]
                        .into_iter()
                        .map(|name| codex_dir.join(name))
                        .find(|path| path.is_file())?;
                    collect_codex_session_contexts_from(&db_path, 200).ok()?
                }
                crate::domain::AgentType::Antigravity => {
                    collect_antigravity_session_contexts_from(home, 200).ok()?
                }
                _ => return None,
            };
            let mut found = None;
            if let Ok(mut guard) = non_claude_context_usage_cache().write() {
                let cache = guard.get_or_insert_with(|| NonClaudeContextUsageCache {
                    updated_at: Instant::now(),
                    home: home.to_path_buf(),
                    populated_agents: HashSet::new(),
                    usages: HashMap::new(),
                });
                if cache.home != home || cache.updated_at.elapsed() >= Duration::from_secs(15) {
                    cache.home = home.to_path_buf();
                    cache.populated_agents.clear();
                    cache.usages.clear();
                    cache.updated_at = Instant::now();
                }
                cache.populated_agents.insert(agent_type.clone());
                for ctx in contexts {
                    if let Some(used) = ctx
                        .context_used_tokens
                        .or(ctx.session_total_tokens)
                        .filter(|v| *v > 0)
                    {
                        let usage = ContextUsage {
                            used,
                            window: ctx.context_window_tokens.filter(|v| *v > 0),
                        };
                        if ctx.agent_session_id == raw_id || ctx.agent_session_id == session_id {
                            found = Some(usage);
                        }
                        cache
                            .usages
                            .insert((agent_type.clone(), ctx.agent_session_id), usage);
                    }
                }
            }
            found
        }
        crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => None,
    }
}
