use super::*;

fn local_store_cache() -> &'static Mutex<LocalStoreCache> {
    static CACHE: OnceLock<Mutex<LocalStoreCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(LocalStoreCache::default()))
}

#[derive(Default)]
struct LocalStoreCache {
    /// The last scan's result (`Arc`-shared), its content SHA-256 digest, and when it was taken.
    items: Option<(Instant, [u8; 32], Arc<Vec<Value>>)>,
    /// Monotonically incremented each time a scan produces a different list.
    generation: u64,
    /// Set while a scan is running, so only one runs at a time.
    scanning: bool,
    /// Set by a caller that needs a pass sooner than the one in flight.
    again: bool,
}

/// Start a background scan of the local session stores, if one is not running.
///
/// A caller that arrives while a pass is running does not need to start
/// another: that pass is reading the stores now, and a store written between
/// its start and its read of that path is seen by it. Only a caller that
/// arrives *after* the pass began — a deleted session, a file the pass never
/// touched — asks for a follow-up with [`restart_local_store_scan`].
pub(crate) fn start_local_store_scan() {
    let mut cache = match local_store_cache().lock() {
        Ok(cache) => cache,
        Err(_) => return,
    };
    if cache.scanning {
        return;
    }
    cache.scanning = true;
    std::thread::spawn(run_scan_pass);
}

fn run_scan_pass() {
    loop {
        // Cleared before the stores are read, so only a store that reported a
        // change during this pass can make it publish.
        let _ = crate::collectors::take_store_change();
        let items = local_store_sessions();
        let digest = crate::collectors::json_sha256(&items);
        let items = Arc::new(items);
        let stores_changed = crate::collectors::take_store_change();
        // The usage ledger is walked in the same pass, after the session
        // stores: both the session list and the ledger's figures describe
        // one instant, so a console that re-reads on the notification sees
        // them agree.
        let usage_changed = crate::usage_ledger::refresh_change();
        let Ok(mut cache) = local_store_cache().lock() else {
            return;
        };
        let sessions_changed = match &cache.items {
            Some((_, previous_digest, _)) => previous_digest != &digest,
            None => true,
        };
        cache.items = Some((Instant::now(), digest, items));
        if sessions_changed {
            cache.generation = cache.generation.wrapping_add(1).max(1);
        }
        if !cache.again {
            cache.scanning = false;
            drop(cache);
            // Published only now, once the snapshot is in place: a subscriber
            // that wakes up and reads /api/dashboard sees the pass that
            // notified it, not the one before.
            if sessions_changed || stores_changed || usage_changed {
                crate::live::publish();
            }
            return;
        }
        cache.again = false;
    }
}

/// Asks for a scan pass on the next opportunity. Used by the tick thread.
pub fn wake_scan() {
    start_local_store_scan();
}

/// Asks for a pass that begins after this call, even if one is running now.
///
/// Used by endpoints whose own write happened mid-pass — deleting a session,
/// renaming the device — where the running pass may have already read the
/// state that write changed.
fn restart_local_store_scan() {
    let mut cache = match local_store_cache().lock() {
        Ok(cache) => cache,
        Err(_) => return,
    };
    if cache.scanning {
        cache.again = true;
        return;
    }
    cache.scanning = true;
    drop(cache);
    std::thread::spawn(run_scan_pass);
}

/// Forces an immediate local store scan and waits briefly for incremental completion.
pub fn force_local_store_scan() {
    restart_local_store_scan();
    wait_for_active_local_store_scan(Duration::from_millis(250));
}

/// Returns the generation counter of the last completed local store scan.
pub fn local_store_scan_generation() -> u64 {
    local_store_cache()
        .lock()
        .ok()
        .map(|cache| cache.generation)
        .unwrap_or(0)
}

pub(crate) fn wait_for_active_local_store_scan(timeout: Duration) {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let scanning = match local_store_cache().lock() {
            Ok(cache) => cache.scanning,
            Err(_) => return,
        };
        if !scanning {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Pre-warms the local store scan in the background so sessions from Claude,
/// ZCode, Grok, etc. are loaded by the time the first request arrives.
pub fn warm_up_local_store_scan() {
    start_local_store_scan();
}

/// Returns true if the initial local store scan has produced cached results.
pub fn is_local_store_ready() -> bool {
    local_store_cache()
        .lock()
        .ok()
        .map(|cache| cache.items.is_some())
        .unwrap_or(false)
}

/// Returns true if a local store scan is currently executing.
pub fn is_local_store_scanning() -> bool {
    local_store_cache()
        .lock()
        .ok()
        .map(|cache| cache.scanning)
        .unwrap_or(false)
}

/// Wait for the initial local store scan to finish if it hasn't completed yet, up to `timeout`.
/// This prevents returning only partial provider results (like Codex-only) on cold start.
pub fn ensure_initial_local_store_scan(timeout: Duration) {
    start_local_store_scan();
    let start = Instant::now();
    while start.elapsed() < timeout {
        let (ready, scanning) = match local_store_cache().lock() {
            Ok(cache) => (cache.items.is_some(), cache.scanning),
            Err(_) => return,
        };
        if ready && !scanning {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The local store sessions found by the last scan.
///
/// An empty list means the scan has not finished yet, not that the machine
/// has no sessions, so the caller publishes what it has rather than claiming
/// the store is empty.
pub(crate) fn cached_local_store_sessions() -> Arc<Vec<Value>> {
    local_store_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.items.as_ref().map(|(_, _, items)| Arc::clone(items)))
        .unwrap_or_default()
}

/// Sessions that live in local stores rather than in a provider's own server.
///
/// Codex, ZCode, Claude, Antigravity and Grok all keep their history on this
/// machine, so the console has to read those stores directly. The scan is
/// pure filesystem and SQLite work — it needs no provider connection — and on
/// a large store it costs far longer than a provider round trip, so it runs
/// on its own thread and its result is cached between refreshes.
fn local_store_sessions() -> Vec<Value> {
    let mut items = Vec::new();
    if let Ok(claude_entries) = crate::collectors::collect_claude_sessions_for_remote() {
        for entry in claude_entries {
            // Desktop and the CLI share one transcript tree; the entry point is
            // what tells them apart, and they are distinct products in the
            // console.
            let provider = if entry.desktop {
                "claude_desktop"
            } else {
                "claude_code"
            };
            items.push(json!({
                "id": format!("claude_{}", entry.session_id),
                "session_id": format!("claude_{}", entry.session_id),
                "cwd": entry.cwd,
                "workspace_path": entry.cwd,
                "name": entry.title.clone().unwrap_or_else(|| entry.session_id.clone()),
                "title": entry.title,
                "provider": provider,
                "agent_name": provider,
                "model": entry.model,
                "prompt_tokens": entry.prompt_tokens,
                "completion_tokens": entry.completion_tokens,
                "total_tokens": entry.prompt_tokens + entry.completion_tokens,
                "cache_read_tokens": entry.cache_read_tokens,
                "context_used_tokens": entry.context_used_tokens,
                "context_window_tokens": entry.context_window_tokens,
                "request_count": entry.request_count,
                "calls": entry.request_count.max(1),
                "last_activity_at_ms": entry.last_activity_at_ms,
                // the conversation's subagents, folded into it: the console
                // lists the conversation once and shows them under it, as
                // magpie does
                "subagents": entry.subagents,
                "status": { "type": "idle" }
            }));
        }
    }

    if let Ok(antigravity_entries) = crate::collectors::collect_antigravity_sessions_detailed() {
        let ag_bin = crate::collectors::discover_antigravity_binary();
        let ag_resumable = crate::collectors::is_antigravity_runtime_available(&ag_bin);
        for entry in antigravity_entries {
            let status_type = if entry.is_running { "running" } else { "idle" };
            items.push(json!({
                "id": format!("antigravity_{}", entry.session_id),
                "session_id": format!("antigravity_{}", entry.session_id),
                "cwd": entry.cwd,
                "workspace_path": entry.cwd,
                "name": entry.title.clone().unwrap_or_else(|| entry.session_id.clone()),
                "title": entry.title,
                "provider": "antigravity",
                "agent_name": "antigravity",
                "model": entry.model,
                "prompt_tokens": entry.prompt_tokens,
                "completion_tokens": entry.completion_tokens,
                "total_tokens": entry.prompt_tokens.saturating_add(entry.completion_tokens),
                "cache_read_tokens": entry.cache_read_tokens,
                "context_used_tokens": entry.context_used_tokens,
                "context_window_tokens": entry.context_window_tokens,
                "request_count": entry.request_count,
                "calls": entry.request_count.max(1),
                "last_activity_at_ms": entry.last_activity_at_ms,
                "resumable": ag_resumable || !entry.is_running,
                "subagents": entry.subagents,
                "status": { "type": status_type }
            }));
        }
    }

    items.extend(crate::plugins::discover_all_plugin_sessions());

    items
}
