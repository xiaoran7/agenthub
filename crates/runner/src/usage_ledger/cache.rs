use super::*;

/// One file's calls, kept across walks: a store this size should not be
/// re-read because one unrelated transcript grew.
#[derive(Debug, Default)]
pub(crate) struct LedgerCache {
    /// When the store was last walked.
    pub(crate) walked_at: Option<Instant>,
    /// Monotonically bumped only when underlying records actually change.
    pub(crate) generation: u64,
    /// Whether `usage_index.db` metadata has been loaded for this cache epoch.
    pub(crate) cold_loaded: bool,
    /// File fingerprints backed by SQLite cold storage (`usage_index.db`).
    pub(crate) cold_manifest: HashMap<PathBuf, IndexedFileMeta>,
    /// Maximum timestamp across cold archived files (`None` when none contain calls).
    pub(crate) cold_max_t: Option<i64>,
    /// Cold paths invalidated on disk and awaiting deletion from `usage_index.db`.
    pub(crate) dirty_db_removals: HashSet<PathBuf>,
    /// Each Claude / Claude Desktop transcript file's incremental byte-offset state.
    pub(crate) files: HashMap<PathBuf, TranscriptState>,
    /// Each Codex rollout's fingerprint and the per-call records it held.
    pub(crate) codex: HashMap<PathBuf, (FileMark, Vec<LedgerRecord>)>,
    /// Antigravity transcript caches keyed by file path.
    pub(crate) other_files: HashMap<PathBuf, (FileMark, Vec<LedgerRecord>)>,
    /// Magpie gateway `usage.jsonl` caches keyed by file path.
    pub(crate) magpie_gateway: HashMap<PathBuf, (FileMark, Vec<LedgerRecord>)>,
    /// Non-retaining handle to the last full-ledger merge while active callers borrow it.
    pub(crate) records_weak: std::sync::Weak<Vec<LedgerRecord>>,
    /// Memoised digests keyed by `(period, limit, local_day)` for the current generation.
    pub(crate) digests: HashMap<(&'static str, usize, String), Arc<LedgerDigest>>,
}

impl LedgerCache {
    pub(crate) fn recompute_cold_max_t(&mut self) {
        self.cold_max_t = self.cold_manifest.values().filter_map(|m| m.max_t).max();
    }

    pub(crate) fn mark_cold_path_dirty(&mut self, path: &Path) {
        if self.cold_manifest.remove(path).is_some() {
            self.dirty_db_removals.insert(path.to_path_buf());
            self.recompute_cold_max_t();
        }
    }

    pub(crate) fn remove_missing_cold_paths(&mut self, source: &str, live: &HashSet<&Path>) {
        let missing: Vec<PathBuf> = self
            .cold_manifest
            .iter()
            .filter(|(p, meta)| meta.source == source && !live.contains(p.as_path()))
            .map(|(p, _)| p.clone())
            .collect();
        if !missing.is_empty() {
            for p in missing {
                self.cold_manifest.remove(&p);
                self.dirty_db_removals.insert(p);
            }
            self.recompute_cold_max_t();
        }
    }
}

pub(crate) fn ledger_cache() -> &'static RwLock<LedgerCache> {
    static CACHE: OnceLock<RwLock<LedgerCache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(LedgerCache::default()))
}

/// How long a walk is reused when nobody forces it.
///
/// The scan pass that drives the console calls [`refresh_change`] once per
/// tick, which walks the stores without this gate and is what keeps the
/// ledger on the same clock as the session list. The gate only bounds work
/// done by a request that arrives between ticks.
const LEDGER_TTL: Duration = Duration::from_millis(1200);

/// Drops the memoised walk. A session moved to the trash is still on disk at
/// its old path in the cache until the next park, and the fingerprint of a
/// path that no longer exists never matches, so the calls of a deleted
/// conversation would otherwise stay in the ledger for its TTL.
pub fn forget_transcripts() {
    if let Ok(mut guard) = ledger_cache().write() {
        *guard = LedgerCache::default();
    }
    prune_interner();
}

/// Queries the live file size and change/modification timestamp directly from
/// the OS file control block (port of Magpie's `statLogFile` in
/// `internal/usage/log_change_windows.go`).
///
/// On Windows NTFS, `std::fs::metadata(path)` (`GetFileAttributesExW`) and
/// `DirEntry::metadata()` (`FindNextFileW`) read the parent directory index
/// entry, whose size and `LastWriteTime` stay frozen for 30+ minutes while an
/// agent process holds the `.jsonl` file handle open for appending. Opening
/// a handle with `FILE_READ_ATTRIBUTES` and querying `GetFileInformationByHandleEx`
/// (`FileBasicInfo`) reads the live kernel FCB `ChangeTime` and size immediately.
#[cfg(windows)]
pub(crate) fn live_file_mark(path: &Path) -> Option<(u64, i64)> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FileBasicInfo, GetFileInformationByHandleEx,
        GetFileSizeEx, OPEN_EXISTING,
    };

    #[repr(C)]
    struct WinFileBasicInfo {
        creation_time: i64,
        last_access_time: i64,
        last_write_time: i64,
        change_time: i64,
        file_attributes: u32,
        _pad: u32,
    }

    fn filetime_to_unix_ms(ft: i64) -> i64 {
        if ft <= 0 {
            0
        } else {
            (ft / 10_000).saturating_sub(11_644_473_600_000)
        }
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
            let mut size: i64 = 0;
            let size_ok = GetFileSizeEx(handle, &mut size) != 0 && size >= 0;
            let mut basic: WinFileBasicInfo = std::mem::zeroed();
            let basic_ok = GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                (&raw mut basic).cast(),
                std::mem::size_of::<WinFileBasicInfo>() as u32,
            ) != 0;
            CloseHandle(handle);
            if size_ok && basic_ok {
                if (basic.file_attributes & 0x10) != 0 {
                    return None;
                }
                let stamp_ms = filetime_to_unix_ms(basic.change_time)
                    .max(filetime_to_unix_ms(basic.last_write_time));
                return Some((size as u64, stamp_ms));
            }
        }
    }

    if let Ok(file) = fs::File::open(path)
        && let Ok(meta) = file.metadata()
        && meta.is_file()
    {
        let mut mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        unsafe {
            let mut basic: WinFileBasicInfo = std::mem::zeroed();
            if GetFileInformationByHandleEx(
                file.as_raw_handle() as HANDLE,
                FileBasicInfo,
                (&raw mut basic).cast(),
                std::mem::size_of::<WinFileBasicInfo>() as u32,
            ) != 0
            {
                mtime = mtime
                    .max(filetime_to_unix_ms(basic.change_time))
                    .max(filetime_to_unix_ms(basic.last_write_time));
            }
        }
        return Some((meta.len(), mtime));
    }

    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

#[cfg(not(windows))]
pub(crate) fn live_file_mark(path: &Path) -> Option<(u64, i64)> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

/// Walks incremental stores if stale or `force`, bumps `generation`, clears stale
/// digests, spills eligible cold files to SQLite, and returns `true` if data changed.
pub(crate) fn walk_stores_if_needed(force: bool) -> bool {
    if !force
        && let Ok(guard) = ledger_cache().read()
        && let Some(at) = guard.walked_at
        && at.elapsed() < LEDGER_TTL
    {
        return false;
    }

    ensure_cold_manifest_loaded();
    let routed = active_routed_providers();
    let claude_changed = update_claude_calls(&routed);
    let other_changed = update_other_agent_calls(&routed);
    let any_changed = claude_changed || other_changed;

    let first_walk = ledger_cache()
        .read()
        .map(|g| g.walked_at.is_none())
        .unwrap_or(true);

    if !any_changed && !first_walk {
        if let Ok(mut guard) = ledger_cache().write() {
            guard.walked_at = Some(Instant::now());
        }
        return false;
    }

    compact_and_spill_cold();

    if let Ok(mut guard) = ledger_cache().write() {
        guard.walked_at = Some(Instant::now());
        if any_changed {
            guard.generation = guard.generation.wrapping_add(1).max(1);
            guard.digests.clear();
            guard.records_weak = std::sync::Weak::new();
        } else {
            guard.generation = guard.generation.max(1);
        }
    }
    if any_changed || first_walk {
        prune_interner();
    }
    any_changed || first_walk
}

/// Assembles, deduplicates, and retry-marks records with `t >= min_since_ms`.
/// Hot-window queries skip cloning cold history and avoid touching `usage_index.db`
/// unless cold archived files overlap `[min_since_ms, now]`.
fn collect_windowed_records(min_since_ms: i64) -> Vec<LedgerRecord> {
    let mut records = Vec::new();
    let mut gateway_candidates = Vec::new();
    let mut need_cold_db = false;
    let mut hot_paths = HashSet::new();

    if let Ok(guard) = ledger_cache().read() {
        need_cold_db = !guard.cold_manifest.is_empty()
            && (min_since_ms == i64::MIN
                || guard.cold_max_t.is_some_and(|max_t| min_since_ms <= max_t));

        for (path, state) in &guard.files {
            if state.cold_archived {
                continue;
            }
            if need_cold_db {
                hot_paths.insert(path.clone());
            }
            records.extend(state.calls.iter().filter(|r| r.t >= min_since_ms).cloned());
        }
        for (path, (_, recs)) in &guard.codex {
            if recs.is_empty() {
                continue;
            }
            if need_cold_db {
                hot_paths.insert(path.clone());
            }
            records.extend(recs.iter().filter(|r| r.t >= min_since_ms).cloned());
        }
        for (path, (_, recs)) in &guard.other_files {
            if recs.is_empty() {
                continue;
            }
            if need_cold_db {
                hot_paths.insert(path.clone());
            }
            records.extend(recs.iter().filter(|r| r.t >= min_since_ms).cloned());
        }
        for (_, recs) in guard.magpie_gateway.values() {
            gateway_candidates.extend(recs.iter().filter(|r| r.t >= min_since_ms).cloned());
        }
    }

    if need_cold_db
        && let Some(db_path) = active_index_db_path()
        && db_path.is_file()
        && let Ok(db) = UsageIndexDb::open(&db_path)
        && let Ok(cold) = db.query_records_since(min_since_ms, &hot_paths)
    {
        records.extend(cold);
    }

    // Fold in Magpie gateway records (`usage.jsonl`) that were not already
    // captured in a local session file (port of Magpie's `gatewayMatches`).
    if !gateway_candidates.is_empty() {
        for gw in gateway_candidates {
            let matched = records.iter().any(|local| {
                local.agent == gw.agent
                    && (!gw.session.is_empty() && local.session == gw.session
                        || (local.t - gw.t).abs() <= 5_000)
                    && local.input == gw.input
                    && local.output == gw.output
                    && local.cache_read == gw.cache_read
            });
            if !matched {
                records.push(gw);
            }
        }
    }

    records.sort_by_key(|r| std::cmp::Reverse(r.t));
    dedupe(&mut records);
    mark_retries(&mut records);
    records
}

/// Every call in the local stores, newest first.
///
/// Unchanged files are skipped without opening; appended `.jsonl` transcripts
/// seek directly to their previous byte `offset` and parse only newly written
/// lines.
pub fn claude_records() -> Vec<LedgerRecord> {
    claude_records_arc_inner(false).as_ref().clone()
}

pub(crate) fn claude_records_arc_inner(force: bool) -> Arc<Vec<LedgerRecord>> {
    walk_stores_if_needed(force);
    let gen_before = if let Ok(guard) = ledger_cache().read() {
        if let Some(hit) = guard.records_weak.upgrade() {
            return hit;
        }
        guard.generation
    } else {
        0
    };

    let merged = Arc::new(collect_windowed_records(i64::MIN));
    if let Ok(mut guard) = ledger_cache().write()
        && guard.generation == gen_before
    {
        guard.records_weak = Arc::downgrade(&merged);
    }
    prune_interner();
    merged
}

/// Releases free allocator heap pages back to the OS (explicitly avoids Windows
/// `SetProcessWorkingSetSize` thrashing during periodic scans).
pub(crate) fn trim_working_set() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        let _ = malloc_trim(0);
    }
}

/// Walks every usage store now and reports whether anything changed.
///
/// The scan pass calls this once per tick, after the session stores were
/// refreshed, so the usage page and the session list move together: a
/// notification is only published when both saw the same new data.
pub fn refresh_change() -> bool {
    let before = ledger_cache()
        .read()
        .map(|guard| guard.generation)
        .unwrap_or(0);
    walk_stores_if_needed(true);
    let after = ledger_cache()
        .read()
        .map(|guard| guard.generation)
        .unwrap_or(0);
    after != before
}

/// Reports whether the local transcript and SQLite stores have been walked at least once.
pub fn is_ledger_warm() -> bool {
    ledger_cache()
        .read()
        .map(|guard| guard.walked_at.is_some())
        .unwrap_or(false)
}

/// Returns `(generation, Arc<LedgerDigest>)`, reusing the memoised digest when
/// no underlying transcript or SQLite row changed within the current local day.
pub fn digest_cached(period: Period, recent_limit: usize) -> (u64, Arc<LedgerDigest>) {
    digest_cached_with_force(period, recent_limit, false)
}

/// Like [`digest_cached`], with `force = true` bypassing `LEDGER_TTL` so a
/// manual refresh immediately tail-reads any newly written transcript lines.
pub fn digest_cached_with_force(
    period: Period,
    recent_limit: usize,
    force: bool,
) -> (u64, Arc<LedgerDigest>) {
    walk_stores_if_needed(force);
    let now_ms = chrono::Local::now().timestamp_millis();
    let today = local_day(now_ms);
    let key = (period.key(), recent_limit, today);

    let (gen_before, cached_all) = if let Ok(guard) = ledger_cache().read() {
        if let Some(hit) = guard.digests.get(&key) {
            return (guard.generation.max(1), Arc::clone(hit));
        }
        (guard.generation, guard.records_weak.upgrade())
    } else {
        (0, None)
    };

    let built = if let Some(all_records) = cached_all {
        Arc::new(digest(&all_records, period, recent_limit))
    } else if matches!(period, Period::All) {
        let all_records = claude_records_arc_inner(false);
        Arc::new(digest(&all_records, period, recent_limit))
    } else {
        let window_since = period
            .since_ms(now_ms)
            .saturating_sub(RETRY_WINDOW_MS + 60_000);
        let windowed = collect_windowed_records(window_since);
        let d = Arc::new(digest(&windowed, period, recent_limit));
        drop(windowed);
        prune_interner();
        d
    };

    let mut generation = gen_before.max(1);
    if let Ok(mut guard) = ledger_cache().write() {
        generation = guard.generation.max(1);
        if guard.generation == gen_before {
            if guard.digests.len() > 16 {
                guard.digests.clear();
            }
            guard.digests.insert(key, Arc::clone(&built));
        }
    }
    (generation, built)
}

/// Coding Assistant and Claude Desktop's own calls, incrementally reading only
/// newly appended bytes per `.jsonl` transcript file across `~/.claude` and
/// Claude Desktop's `local-agent-mode-sessions` directories.
pub(crate) fn update_claude_calls(routed: &HashMap<String, String>) -> bool {
    let roots = claude_transcript_roots();
    if roots.is_empty() {
        return false;
    }
    let file_marks = transcript_files_from_roots(&roots);
    if file_marks.is_empty() {
        return false;
    }

    // Fast path: check under read lock without cloning any TranscriptState or LedgerRecord.
    if let Ok(guard) = ledger_cache().read()
        && guard.walked_at.is_some()
        && guard.files.len() == file_marks.len()
        && file_marks.iter().all(|(p, _, len, mtime)| {
            guard
                .files
                .get(p)
                .is_some_and(|old| old.offset == *len && old.modified_ms == *mtime)
        })
    {
        return false;
    }

    let Ok(mut guard) = ledger_cache().write() else {
        return false;
    };
    let mark_set: HashSet<&Path> = file_marks.iter().map(|(p, ..)| p.as_path()).collect();
    let count_diff = guard.files.len() != file_marks.len();
    guard.files.retain(|p, _| mark_set.contains(p.as_path()));
    guard.remove_missing_cold_paths("claude", &mark_set);
    let mut any_changed = count_diff;

    for (path, default_agent, len, modified_ms) in file_marks {
        if let Some(state) = guard.files.get_mut(&path) {
            if state.offset == len
                && state.modified_ms == modified_ms
                && state.default_agent == default_agent
            {
                continue;
            }
            let was_cold = state.cold_archived;
            let was_pruned = state.index_pruned;
            if was_cold {
                guard.mark_cold_path_dirty(&path);
            }
            let state = guard.files.get_mut(&path).expect("state present");
            if !was_cold
                && !was_pruned
                && len > state.offset
                && state.default_agent == default_agent
            {
                let _ = read_transcript_incremental(&path, state, routed);
                state.modified_ms = modified_ms;
                any_changed = true;
            } else {
                state.offset = 0;
                state.calls.clear();
                state.index.clear();
                state.cold_archived = false;
                state.index_pruned = false;
                state.default_agent = default_agent;
                let _ = read_transcript_incremental(&path, state, routed);
                state.modified_ms = modified_ms;
                any_changed = true;
            }
        } else if guard.cold_manifest.get(&path).is_some_and(|m| {
            m.source == "claude" && m.file_len == len && m.modified_ms == modified_ms
        }) {
            guard.files.insert(
                path,
                TranscriptState {
                    offset: len,
                    modified_ms,
                    default_agent,
                    cold_archived: true,
                    index_pruned: true,
                    ..TranscriptState::default()
                },
            );
            any_changed = true;
        } else {
            guard.mark_cold_path_dirty(&path);
            let mut fresh = TranscriptState {
                default_agent,
                ..TranscriptState::default()
            };
            let _ = read_transcript_incremental(&path, &mut fresh, routed);
            fresh.modified_ms = modified_ms;
            guard.files.insert(path, fresh);
            any_changed = true;
        }
    }
    any_changed
}

/// The calls of the other CLIs and local gateways on this machine, each read
/// incrementally from its own authority rather than re-reading unchanged archives.
pub(crate) fn update_other_agent_calls(routed: &HashMap<String, String>) -> bool {
    let Some(home) = home_dir() else {
        return false;
    };
    let cx_changed = update_codex_calls(&home, routed);
    let other_changed = update_extra_client_calls(&home, routed);
    let gw_changed = update_magpie_gateway_calls(&home, routed);
    cx_changed || other_changed || gw_changed
}

pub(crate) fn dedupe(records: &mut Vec<LedgerRecord>) {
    // Records carry `t` to the millisecond; a resumed transcript can replay a
    // message with a later timestamp, so identity is the message itself.
    // `read_transcript` already keys a file's messages; across files, the
    // same call is the same session, model and four token counts at the same
    // moment, which is the closest thing to a portable identity.
    let mut seen: std::collections::HashSet<(i64, &'static str, u64, u64, u64, u64)> =
        std::collections::HashSet::new();
    records.retain(|r| seen.insert((r.t, r.agent, r.input, r.output, r.cache_read, r.cache_write)));
}

/// How soon after a failure the next successful call counts as its retry.
/// The CLI retries a rate limit or a server error within seconds to a few
/// minutes; a call a day later is a new question, not a second try.
const RETRY_WINDOW_MS: i64 = 10 * 60 * 1000;

/// Marks the pairs a failed request and the call that answered it form: the
/// failure reads as answered on a retry, and the success reads as a retry.
/// Both keep their own record; the page's status filter tells them apart.
pub(crate) fn mark_retries(records: &mut [LedgerRecord]) {
    // oldest first, so the first success after a failure is the one that
    // answers it
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&i| records[i].t);
    let mut last_failure: HashMap<Arc<str>, usize> = HashMap::new();
    for i in order {
        let session = Arc::clone(&records[i].session);
        if records[i].error.is_some() {
            // a later failure replaces an earlier one: only the latest failure
            // before a success is the one that success answers
            last_failure.insert(session, i);
            continue;
        }
        if let Some(failed_at) = last_failure.remove(&session) {
            let gap = records[i].t.saturating_sub(records[failed_at].t);
            if (0..=RETRY_WINDOW_MS).contains(&gap) {
                records[failed_at].recovered = true;
                records[i].retried = true;
            }
        }
    }
}
