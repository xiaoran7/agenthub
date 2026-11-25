use super::*;

pub(crate) const MAX_SESSION_ID_CHARS: usize = 160;
pub(crate) const MAX_CONVERSATION_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_CONVERSATION_MESSAGES: usize = 300;
/// Response budget for one IPC frame (`MAX_DAEMON_FRAME_BYTES` is 1 MiB);
/// keeps the serialized preview comfortably inside a single frame.
pub(crate) const MAX_CONVERSATION_RESPONSE_BYTES: usize = 512 * 1024;

pub(crate) struct CachedConversationEntry {
    pub(crate) fingerprints: Vec<(PathBuf, u64, Option<SystemTime>)>,
    pub(crate) messages: Vec<Value>,
}

pub(crate) fn conversation_cache() -> &'static RwLock<
    crate::lru_cache::BoundedLruCache<(PathBuf, AgentType, String), CachedConversationEntry>,
> {
    static CACHE: OnceLock<
        RwLock<
            crate::lru_cache::BoundedLruCache<
                (PathBuf, AgentType, String),
                CachedConversationEntry,
            >,
        >,
    > = OnceLock::new();
    CACHE.get_or_init(|| {
        RwLock::new(crate::lru_cache::BoundedLruCache::with_ttl(
            256,
            Duration::from_secs(10),
        ))
    })
}

pub(crate) fn claude_path_index()
-> &'static RwLock<Option<(Instant, PathBuf, HashMap<String, PathBuf>)>> {
    static INDEX: OnceLock<RwLock<Option<(Instant, PathBuf, HashMap<String, PathBuf>)>>> =
        OnceLock::new();
    INDEX.get_or_init(|| RwLock::new(None))
}

pub(crate) fn ensure_claude_path_index(projects_root: &Path) {
    if let Ok(guard) = claude_path_index().read() {
        if let Some((cached_at, ref cached_root, ref map)) = *guard {
            if cached_root == projects_root
                && cached_at.elapsed() < Duration::from_secs(30)
                && !map.is_empty()
            {
                return;
            }
        }
    }

    let mut map = HashMap::new();
    populate_claude_index(projects_root, 0, &mut map);
    if let Ok(mut guard) = claude_path_index().write() {
        *guard = Some((Instant::now(), projects_root.to_path_buf(), map));
    }
}

pub(crate) fn codex_path_index()
-> &'static RwLock<Option<(Instant, PathBuf, Vec<(String, PathBuf)>)>> {
    static INDEX: OnceLock<RwLock<Option<(Instant, PathBuf, Vec<(String, PathBuf)>)>>> =
        OnceLock::new();
    INDEX.get_or_init(|| RwLock::new(None))
}

pub(crate) fn ensure_codex_path_index(codex_root: &Path) {
    if let Ok(guard) = codex_path_index().read() {
        if let Some((cached_at, ref cached_root, ref list)) = *guard {
            if cached_root == codex_root
                && cached_at.elapsed() < Duration::from_secs(30)
                && !list.is_empty()
            {
                return;
            }
        }
    }

    let mut list = Vec::new();
    let sessions_dir = codex_root.join("sessions");
    let archived_dir = codex_root.join("archived_sessions");
    if sessions_dir.is_dir() {
        populate_codex_index(&sessions_dir, 0, &mut list);
    }
    if archived_dir.is_dir() {
        populate_codex_index(&archived_dir, 0, &mut list);
    }
    if let Ok(mut guard) = codex_path_index().write() {
        *guard = Some((Instant::now(), codex_root.to_path_buf(), list));
    }
}

fn populate_codex_index(root: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    if depth > 7 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            populate_codex_index(&path, depth + 1, out);
        } else if path.extension().and_then(|v| v.to_str()) == Some("jsonl")
            && let Some(name) = path.file_name().and_then(|v| v.to_str())
        {
            out.push((name.to_string(), path));
        }
    }
}

fn populate_claude_index(root: &Path, depth: usize, map: &mut HashMap<String, PathBuf>) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|v| v.to_str()) == Some("jsonl") {
            if let Some(stem) = path.file_stem().and_then(|v| v.to_str()) {
                map.entry(stem.to_string()).or_insert(path);
            }
        } else if path.is_dir() {
            populate_claude_index(&path, depth + 1, map);
        }
    }
}

pub fn validate_session_id(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.chars().count() > MAX_SESSION_ID_CHARS
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("invalid_session_id");
    }
    Ok(())
}
