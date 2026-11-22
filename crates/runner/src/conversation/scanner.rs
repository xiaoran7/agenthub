use super::*;

pub(crate) fn find_agent_transcript(
    home: &Path,
    agent_type: &AgentType,
    session_id: &str,
) -> Option<PathBuf> {
    let raw_session_id = session_id
        .strip_prefix("claude_")
        .or_else(|| session_id.strip_prefix("antigravity_"))
        .unwrap_or(session_id);

    match agent_type {
        AgentType::ClaudeCode | AgentType::ClaudeDesktop => {
            find_transcript_file(&home.join(".claude").join("projects"), raw_session_id, 0)
        }
        AgentType::Codex => find_provider_transcript(&home.join(".codex"), raw_session_id, 0),
        AgentType::Antigravity => {
            let roots = [
                home.join(".gemini").join("antigravity"),
                home.join(".gemini").join("antigravity-cli"),
                home.join(".antigravity"),
            ];
            roots.into_iter().find_map(|root| {
                let logs = root
                    .join("brain")
                    .join(raw_session_id)
                    .join(".system_generated")
                    .join("logs");
                let full = logs.join("transcript_full.jsonl");
                if full.is_file() {
                    return Some(full);
                }
                let plain = logs.join("transcript.jsonl");
                plain.is_file().then_some(plain)
            })
        }
        AgentType::DeepseekHarness | AgentType::Plugin(_) => None,
    }
}

pub(crate) fn deduplicate_consecutive_user_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut deduped: Vec<Value> = Vec::with_capacity(messages.len());
    for item in messages {
        let is_user = item.get("role").and_then(Value::as_str) == Some("user");
        let content = item
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();

        if is_user && !content.is_empty() {
            // If the immediately preceding message is also a user message with identical content,
            // overwrite/replace it with this later attempt (or skip duplicates).
            if let Some(prev) = deduped.last_mut() {
                let prev_is_user = prev.get("role").and_then(Value::as_str) == Some("user");
                let prev_content = prev
                    .get("content")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if prev_is_user && prev_content == content {
                    // Replace previous duplicate with the updated one
                    *prev = item;
                    continue;
                }
            }
        }
        deduped.push(item);
    }
    deduped
}

pub(crate) fn find_provider_transcript(
    root: &Path,
    session_id: &str,
    depth: usize,
) -> Option<PathBuf> {
    let mut matches = Vec::new();
    collect_provider_transcripts(root, session_id, depth, &mut matches);
    // Sort descending by file modification time so that resumed or continued sessions
    // (e.g. Codex rollouts across days or continuation fragments) pick the newest file
    matches.sort_by(|a, b| {
        let mtime_b = b.metadata().and_then(|m| m.modified()).ok();
        let mtime_a = a.metadata().and_then(|m| m.modified()).ok();
        mtime_b.cmp(&mtime_a)
    });
    matches.into_iter().next()
}

pub(crate) fn collect_provider_transcripts(
    root: &Path,
    session_id: &str,
    depth: usize,
    results: &mut Vec<PathBuf>,
) {
    if depth > 7 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let parent_matches = path
                .ancestors()
                .any(|part| part.file_name().and_then(|value| value.to_str()) == Some(session_id));
            if name.contains(session_id) || parent_matches {
                results.push(path);
                continue;
            }
        }
        if path.is_dir() {
            let dir_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if matches!(
                dir_name,
                "node_modules"
                    | "plugins"
                    | "cache"
                    | "tmp"
                    | "temp"
                    | ".git"
                    | ".venv"
                    | "target"
                    | ".npm"
                    | ".cargo"
            ) {
                continue;
            }
            collect_provider_transcripts(&path, session_id, depth + 1, results);
        }
    }
}

/// Deterministically sanitizes a workspace path to Claude's on-disk project directory format.
/// Following Claude Code conventions (and Magpie's workspace sanitization), path separators
/// (`/`, `\`) and drive colon (`:`) are converted to hyphens (`-`).
pub fn sanitize_claude_project_dir(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            ':' | '/' | '\\' => out.push('-'),
            _ => out.push(ch),
        }
    }
    // Collapse any redundant hyphens if necessary or trim leading/trailing
    out
}

/// Directly resolves a Claude Code transcript file for a specific workspace and session ID
/// without sweeping entire directory trees.
pub fn find_claude_workspace_transcript(
    home: &Path,
    workspace: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    let sanitized = sanitize_claude_project_dir(workspace);
    let direct = home
        .join(".claude")
        .join("projects")
        .join(&sanitized)
        .join(format!("{session_id}.jsonl"));
    if direct.is_file() {
        return Some(direct);
    }
    None
}

pub(crate) fn find_transcript_file(root: &Path, session_id: &str, depth: usize) -> Option<PathBuf> {
    let mut matches = Vec::new();
    collect_transcript_files(root, session_id, depth, &mut matches);
    matches.sort_by(|a, b| {
        let mtime_b = b.metadata().and_then(|m| m.modified()).ok();
        let mtime_a = a.metadata().and_then(|m| m.modified()).ok();
        mtime_b.cmp(&mtime_a)
    });
    matches.into_iter().next()
}

pub(crate) fn collect_transcript_files(
    root: &Path,
    session_id: &str,
    depth: usize,
    results: &mut Vec<PathBuf>,
) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
            && path.file_stem().and_then(|value| value.to_str()) == Some(session_id)
        {
            results.push(path);
            continue;
        }
        if path.is_dir() {
            collect_transcript_files(&path, session_id, depth + 1, results);
        }
    }
}
