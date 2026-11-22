use std::{
    borrow::Cow,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::UNIX_EPOCH,
};

use serde::Deserialize;

use super::*;

/// The most agents one conversation lists. A workflow run writes one file per
/// agent, and a session can outgrow any cap; the cap keeps the newest work.
const MAX_SUBAGENTS: usize = 500;

/// The subagents a conversation spawned, from the folder beside its own
/// transcript: `<session>/subagents/agent-<id>.jsonl`, and a workflow's agents
/// at `<session>/subagents/workflows/<run>/agent-*.jsonl` (the run's
/// `journal.jsonl` is not a transcript). Each one's `.meta.json` says what it
/// was for; its own lines carry the usage, and `sessionId` stays the parent's,
/// so it is never listed as a conversation of its own.
pub(crate) fn collect_claude_subagents(session_file: &Path) -> Vec<ClaudeSubagentEntry> {
    let Some(stem) = session_file.file_stem().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let Some(project) = session_file.parent() else {
        return Vec::new();
    };
    let subagents = project.join(stem).join("subagents");
    if !subagents.is_dir() {
        return Vec::new();
    }
    let mut files: Vec<(i64, PathBuf)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&subagents) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
            if name.starts_with("agent-")
                && path.extension().and_then(|v| v.to_str()) == Some("jsonl")
            {
                push_subagent_file(&path, &mut files);
            }
        }
    }
    if let Ok(runs) = fs::read_dir(subagents.join("workflows")) {
        for run in runs.flatten() {
            let Ok(agents) = fs::read_dir(run.path()) else {
                continue;
            };
            for agent in agents.flatten() {
                let path = agent.path();
                let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                if name.starts_with("agent-")
                    && path.extension().and_then(|v| v.to_str()) == Some("jsonl")
                {
                    push_subagent_file(&path, &mut files);
                }
            }
        }
    }
    // newest first, so a cap drops the oldest agents rather than the work the
    // reader just watched run
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    files.truncate(MAX_SUBAGENTS);
    files
        .into_iter()
        .filter_map(|(_, path)| read_claude_subagent_cached(&path))
        .collect()
}

fn push_subagent_file(path: &Path, files: &mut Vec<(i64, PathBuf)>) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if !meta.is_file() {
        return;
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    files.push((modified, path.to_path_buf()));
}

/// One subagent's read, memoised on the file's size and last write: a finished
/// agent's transcript never changes again, and the console asks for the list
/// every few seconds. A file still being written is read again each time.
type SubagentCacheMap = BoundedLruCache<PathBuf, (u64, i64, Option<ClaudeSubagentEntry>)>;

pub(crate) fn subagent_cache() -> &'static RwLock<SubagentCacheMap> {
    static CACHE: OnceLock<RwLock<SubagentCacheMap>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BoundedLruCache::new(4 * MAX_SUBAGENTS)))
}

/// Drops memoised subagent reads belonging to any of the deleted `roots`.
pub(crate) fn forget_subagents_under(roots: &[PathBuf]) {
    if roots.is_empty() {
        return;
    }
    if let Ok(mut guard) = subagent_cache().write() {
        guard.retain(|cached_path| !roots.iter().any(|root| cached_path.starts_with(root)));
    }
}

pub(crate) fn read_claude_subagent_cached(path: &Path) -> Option<ClaudeSubagentEntry> {
    let (len, modified) = crate::usage_ledger::live_file_mark(path)?;
    if let Ok(guard) = subagent_cache().read()
        && let Some((cached_len, cached_modified, entry)) = guard.get(path)
        && *cached_len == len
        && *cached_modified == modified
    {
        return entry.clone();
    }
    let entry = read_claude_subagent(path);
    if let Ok(mut guard) = subagent_cache().write() {
        // a store the reader keeps for years: the map holds one small record
        // per agent, never a transcript
        guard.insert(path.to_path_buf(), (len, modified, entry.clone()));
    }
    entry
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentMeta<'a> {
    #[serde(borrow)]
    agent_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    description: Option<Cow<'a, str>>,
    #[serde(borrow)]
    parent_agent_id: Option<Cow<'a, str>>,
    spawn_depth: Option<u64>,
}

#[derive(Default, Deserialize)]
struct SubagentMessage<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    usage: Option<ClaudeUsageFields>,
}

#[derive(Default, Deserialize)]
struct SubagentLine<'a> {
    #[serde(borrow, rename = "type")]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    message: Option<SubagentMessage<'a>>,
}

/// One subagent's transcript: its meta file's own words, and the usage of
/// its calls, which a sidechain line still records.
pub(crate) fn read_claude_subagent(path: &Path) -> Option<ClaudeSubagentEntry> {
    let id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|name| name.strip_prefix("agent-"))
        .filter(|id| !id.is_empty())?
        .to_owned();
    let meta_path = path.with_file_name(format!("agent-{id}.meta.json"));
    let meta_text = fs::read_to_string(&meta_path).ok();
    let meta = meta_text
        .as_deref()
        .and_then(|text| serde_json::from_str::<SubagentMeta<'_>>(text).ok())
        .unwrap_or_default();
    let clean_meta = |value: Option<Cow<'_, str>>| {
        value
            .as_deref()
            .filter(|v| !v.is_empty())
            .and_then(|v| metadata_label(v, MAX_CONFIG_VALUE_CHARS))
            .unwrap_or_default()
    };
    let depth = meta.spawn_depth.unwrap_or(1).clamp(1, 16) as u32;
    let kind = clean_meta(meta.agent_type);
    let description = clean_meta(meta.description);
    let parent = clean_meta(meta.parent_agent_id);

    let file = fs::File::open(path).ok()?;
    let last_activity_at_ms = file
        .metadata()
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let mut reader = BufReader::new(file);
    let mut model: Option<String> = None;
    let mut usage_acc = BoundedMessageUsage::default();
    loop {
        let line = match read_bounded_line(&mut reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => continue,
            BoundedLine::Io(_) => break,
        };
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(record) = serde_json::from_str::<SubagentLine<'_>>(&line) else {
            continue;
        };
        if record.record_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(msg) = record.message.as_ref() else {
            continue;
        };
        let Some(usage) = msg.usage.as_ref() else {
            continue;
        };
        let input = usage.input_tokens.unwrap_or(0);
        let output = usage.output_tokens.unwrap_or(0);
        let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
        let cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
        let key = msg
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("line-{}", usage_acc.request_count));
        usage_acc.record(
            key,
            (
                input.saturating_add(cache_read).saturating_add(cache_write),
                output,
                cache_read,
            ),
        );
        if model.is_none() {
            model = msg
                .model
                .as_deref()
                .filter(|m| !m.is_empty() && *m != "<synthetic>")
                .and_then(|v| metadata_label(v, MAX_CONFIG_VALUE_CHARS));
        }
    }
    if usage_acc.request_count == 0 {
        return None;
    }
    let (prompt_tokens, completion_tokens, cache_read_tokens) = usage_acc.totals();
    Some(ClaudeSubagentEntry {
        id,
        kind,
        description,
        parent,
        depth,
        last_activity_at_ms,
        model,
        prompt_tokens,
        completion_tokens,
        cache_read_tokens,
        request_count: usage_acc.request_count,
    })
}
