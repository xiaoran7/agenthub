#[cfg(test)]
use std::sync::Mutex;

use rusqlite::{Connection, params};

use super::*;

/// Files older than 35 days (past Today / 7d / 30d + timezone/retry margins)
/// are spilled cleanly from RAM into `~/.agenthub/usage_index.db`.
pub(crate) const COLD_AGE_MS: i64 = 35 * 86_400_000;
/// Maximum number of recently touched files kept hot in RAM per source.
pub(crate) const HOT_FILE_KEEP_PER_SOURCE: usize = 256;
/// Excess files beyond [`HOT_FILE_KEEP_PER_SOURCE`] must also be idle for >= 24h before spilling.
pub(crate) const IDLE_SPILL_AGE_MS: i64 = 86_400_000;
/// Active transcripts idle for >= 1h drop their per-message `index` map to free heap strings.
pub(crate) const INDEX_PRUNE_AGE_MS: i64 = 3_600_000;

const FREELIST_VACUUM_THRESHOLD: i64 = 64;
const INCREMENTAL_VACUUM_PAGES: usize = 64;

#[cfg(test)]
static TEST_INDEX_DB_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[cfg(test)]
pub(crate) struct TestIndexDbGuard;

#[cfg(test)]
impl Drop for TestIndexDbGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = TEST_INDEX_DB_OVERRIDE
            .get_or_init(|| Mutex::new(None))
            .lock()
        {
            *guard = None;
        }
    }
}

/// Sets an isolated SQLite cold-index path for the duration of one `TEST_MUTEX`-guarded test.
#[cfg(test)]
pub(crate) fn set_test_index_db(path: Option<PathBuf>) -> TestIndexDbGuard {
    if let Ok(mut guard) = TEST_INDEX_DB_OVERRIDE
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *guard = path;
    }
    TestIndexDbGuard
}

/// Returns the active cold-tier SQLite database path, or `None` when disabled (default in tests).
pub(crate) fn active_index_db_path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        if let Some(lock) = TEST_INDEX_DB_OVERRIDE.get()
            && let Ok(guard) = lock.lock()
        {
            return guard.clone();
        }
        None
    }
    #[cfg(not(test))]
    {
        if let Some(override_path) = std::env::var_os("AGENTHUB_USAGE_INDEX_DB") {
            let trimmed = override_path.to_string_lossy();
            let s = trimmed.trim();
            if s.is_empty() || s.eq_ignore_ascii_case("off") {
                return None;
            }
            return Some(PathBuf::from(override_path));
        }
        home_dir().map(|h| h.join(".agenthub").join("usage_index.db"))
    }
}

pub(crate) fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn agent_static(raw: &str) -> &'static str {
    match raw {
        "claude_code" => "claude_code",
        "claude_desktop" => "claude_desktop",
        "codex" => "codex",
        "antigravity" => "antigravity",
        "deepseek_harness" => "deepseek_harness",
        "zcode" => "zcode",
        "opencode" => "opencode",
        "gemini_cli" => "gemini_cli",
        "grok_build" => "grok_build",
        other if crate::domain::AgentType::is_valid_slug(other) => {
            static SLUG_CACHE: std::sync::OnceLock<
                std::sync::RwLock<std::collections::HashMap<String, &'static str>>,
            > = std::sync::OnceLock::new();
            let cache = SLUG_CACHE.get_or_init(Default::default);
            if let Ok(guard) = cache.read()
                && let Some(&interned) = guard.get(other)
            {
                return interned;
            }
            if let Ok(mut guard) = cache.write() {
                if let Some(&interned) = guard.get(other) {
                    return interned;
                }
                if guard.len() < 256 {
                    let leaked: &'static str = Box::leak(other.to_owned().into_boxed_str());
                    guard.insert(other.to_owned(), leaked);
                    return leaked;
                }
            }
            "plugin_acp"
        }
        _ => "claude_code",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexedFileMeta {
    pub(crate) source: String,
    pub(crate) file_len: u64,
    pub(crate) modified_ms: i64,
    pub(crate) min_t: Option<i64>,
    pub(crate) max_t: Option<i64>,
}

pub(crate) struct ColdSpillItem {
    pub(crate) path: PathBuf,
    pub(crate) source: &'static str,
    pub(crate) file_len: u64,
    pub(crate) modified_ms: i64,
    pub(crate) records: Vec<LedgerRecord>,
}

/// Persistent SQLite cold archive (`~/.agenthub/usage_index.db`) storing
/// historical transcripts/rollouts so unchanged old files stay out of RAM.
pub(crate) struct UsageIndexDb {
    connection: Connection,
}

impl UsageIndexDb {
    pub(crate) fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_millis(500))?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;",
        )?;
        // SQLite requires `PRAGMA auto_vacuum = INCREMENTAL` to be followed by a one-time
        // `VACUUM` when migrating a database that was created with `NONE` (`0`).
        let auto_vacuum: i64 = connection.query_row("PRAGMA auto_vacuum;", [], |r| r.get(0))?;
        if auto_vacuum != 2 {
            connection.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
        }
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS indexed_files (
                 path TEXT PRIMARY KEY,
                 source TEXT NOT NULL,
                 file_len INTEGER NOT NULL,
                 modified_ms INTEGER NOT NULL,
                 record_count INTEGER NOT NULL,
                 min_t INTEGER,
                 max_t INTEGER,
                 indexed_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS usage_records (
                 file_path TEXT NOT NULL,
                 t INTEGER NOT NULL,
                 asked INTEGER NOT NULL,
                 agent TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 model TEXT NOT NULL,
                 session TEXT NOT NULL,
                 cwd TEXT NOT NULL,
                 input INTEGER NOT NULL,
                 output INTEGER NOT NULL,
                 cache_read INTEGER NOT NULL,
                 cache_write INTEGER NOT NULL,
                 duration_ms INTEGER,
                 error TEXT,
                 cost REAL NOT NULL,
                 retried INTEGER NOT NULL,
                 recovered INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_usage_records_file ON usage_records(file_path);
             CREATE INDEX IF NOT EXISTS idx_usage_records_t ON usage_records(t);
             COMMIT;",
        )?;
        Ok(Self { connection })
    }

    pub(crate) fn load_manifest(&self) -> rusqlite::Result<HashMap<PathBuf, IndexedFileMeta>> {
        let mut stmt = self.connection.prepare(
            "SELECT path, source, file_len, modified_ms, min_t, max_t FROM indexed_files",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                IndexedFileMeta {
                    source: row.get::<_, String>(1)?,
                    file_len: row.get::<_, i64>(2)?.max(0) as u64,
                    modified_ms: row.get::<_, i64>(3)?,
                    min_t: row.get::<_, Option<i64>>(4)?,
                    max_t: row.get::<_, Option<i64>>(5)?,
                },
            ))
        })?;
        let mut manifest = HashMap::new();
        for row in rows.flatten() {
            manifest.insert(PathBuf::from(row.0), row.1);
        }
        let _ = self.connection.execute_batch("PRAGMA shrink_memory;");
        Ok(manifest)
    }

    pub(crate) fn upsert_files(&mut self, items: &[ColdSpillItem]) -> rusqlite::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let now_ms = now_epoch_ms();
        let tx = self.connection.transaction()?;
        {
            let mut del_rec = tx.prepare("DELETE FROM usage_records WHERE file_path = ?1")?;
            let mut ins_rec = tx.prepare(
                "INSERT INTO usage_records (
                     file_path, t, asked, agent, provider, model, session, cwd,
                     input, output, cache_read, cache_write, duration_ms, error,
                     cost, retried, recovered
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            )?;
            let mut ins_file = tx.prepare(
                "INSERT INTO indexed_files (
                     path, source, file_len, modified_ms, record_count, min_t, max_t, indexed_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(path) DO UPDATE SET
                     source = excluded.source,
                     file_len = excluded.file_len,
                     modified_ms = excluded.modified_ms,
                     record_count = excluded.record_count,
                     min_t = excluded.min_t,
                     max_t = excluded.max_t,
                     indexed_at_ms = excluded.indexed_at_ms",
            )?;

            for item in items {
                let path_key = item.path.to_string_lossy();
                del_rec.execute(params![path_key.as_ref()])?;
                let min_t = item.records.iter().map(|r| r.t).min();
                let max_t = item.records.iter().map(|r| r.t).max();
                for r in &item.records {
                    ins_rec.execute(params![
                        path_key.as_ref(),
                        r.t,
                        r.asked,
                        r.agent,
                        r.provider.as_ref(),
                        r.model.as_ref(),
                        r.session.as_ref(),
                        r.cwd.as_ref(),
                        r.input as i64,
                        r.output as i64,
                        r.cache_read as i64,
                        r.cache_write as i64,
                        r.duration_ms.map(|v| v as i64),
                        r.error.as_deref(),
                        r.cost,
                        i64::from(r.retried),
                        i64::from(r.recovered),
                    ])?;
                }
                ins_file.execute(params![
                    path_key.as_ref(),
                    item.source,
                    item.file_len as i64,
                    item.modified_ms,
                    item.records.len() as i64,
                    min_t,
                    max_t,
                    now_ms,
                ])?;
            }
        }
        tx.commit()?;
        self.reclaim_freelist_if_needed(FREELIST_VACUUM_THRESHOLD, INCREMENTAL_VACUUM_PAGES);
        let _ = self.connection.execute_batch("PRAGMA shrink_memory;");
        Ok(())
    }

    pub(crate) fn remove_paths(&mut self, paths: &[PathBuf]) -> rusqlite::Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let tx = self.connection.transaction()?;
        {
            let mut del_rec = tx.prepare("DELETE FROM usage_records WHERE file_path = ?1")?;
            let mut del_file = tx.prepare("DELETE FROM indexed_files WHERE path = ?1")?;
            for p in paths {
                let key = p.to_string_lossy();
                del_rec.execute(params![key.as_ref()])?;
                del_file.execute(params![key.as_ref()])?;
            }
        }
        tx.commit()?;
        self.reclaim_freelist_if_needed(FREELIST_VACUUM_THRESHOLD, INCREMENTAL_VACUUM_PAGES);
        let _ = self.connection.execute_batch("PRAGMA shrink_memory;");
        Ok(())
    }

    pub(crate) fn query_records_since(
        &self,
        min_since_ms: i64,
        exclude_hot: &HashSet<PathBuf>,
    ) -> rusqlite::Result<Vec<LedgerRecord>> {
        let mut stmt = self.connection.prepare(
            "SELECT file_path, t, asked, agent, provider, model, session, cwd,
                    input, output, cache_read, cache_write, duration_ms, error,
                    cost, retried, recovered
               FROM usage_records
              WHERE t >= ?1",
        )?;
        let rows = stmt.query_map([min_since_ms], |row| {
            let file_path = row.get::<_, String>(0)?;
            let agent = row.get::<_, String>(3)?;
            let provider = row.get::<_, String>(4)?;
            let model = row.get::<_, String>(5)?;
            let session = row.get::<_, String>(6)?;
            let cwd = row.get::<_, String>(7)?;
            let duration_ms = row.get::<_, Option<i64>>(12)?.map(|v| v.max(0) as u64);
            let error = row.get::<_, Option<String>>(13)?;
            Ok((
                file_path,
                LedgerRecord {
                    t: row.get(1)?,
                    asked: row.get(2)?,
                    agent: agent_static(&agent),
                    provider: intern(&provider),
                    model: intern(&model),
                    session: intern(&session),
                    cwd: intern(&cwd),
                    input: row.get::<_, i64>(8)?.max(0) as u64,
                    output: row.get::<_, i64>(9)?.max(0) as u64,
                    cache_read: row.get::<_, i64>(10)?.max(0) as u64,
                    cache_write: row.get::<_, i64>(11)?.max(0) as u64,
                    duration_ms,
                    error: error.map(|e| intern(&e)),
                    cost: row.get(14)?,
                    retried: row.get::<_, i64>(15)? != 0,
                    recovered: row.get::<_, i64>(16)? != 0,
                },
            ))
        })?;
        let mut out = Vec::new();
        for item in rows.flatten() {
            if !exclude_hot.is_empty() && exclude_hot.contains(Path::new(&item.0)) {
                continue;
            }
            out.push(item.1);
        }
        let _ = self.connection.execute_batch("PRAGMA shrink_memory;");
        Ok(out)
    }

    pub(crate) fn reclaim_freelist_if_needed(&self, min_pages: i64, pages: usize) {
        let freelist: i64 = self
            .connection
            .query_row("PRAGMA freelist_count;", [], |r| r.get(0))
            .unwrap_or(0);
        if freelist >= min_pages {
            let _ = self.reclaim_space(pages);
        }
    }

    pub(crate) fn reclaim_space(&self, pages: usize) -> rusqlite::Result<()> {
        // `PRAGMA incremental_vacuum` yields one result row per freed page;
        // stepping via `prepare`/`query` drains all pages instead of stopping after 1.
        {
            let mut stmt = self
                .connection
                .prepare(&format!("PRAGMA incremental_vacuum({pages});"))?;
            let mut rows = stmt.query([])?;
            while rows.next()?.is_some() {}
        }
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE); PRAGMA shrink_memory;")
    }

    #[cfg(test)]
    pub(crate) fn auto_vacuum_mode(&self) -> rusqlite::Result<i64> {
        self.connection
            .query_row("PRAGMA auto_vacuum;", [], |r| r.get(0))
    }

    #[cfg(test)]
    pub(crate) fn freelist_count(&self) -> rusqlite::Result<i64> {
        self.connection
            .query_row("PRAGMA freelist_count;", [], |r| r.get(0))
    }

    #[cfg(test)]
    pub(crate) fn row_count(&self) -> rusqlite::Result<i64> {
        self.connection
            .query_row("SELECT COUNT(*) FROM usage_records", [], |r| r.get(0))
    }

    #[cfg(test)]
    pub(crate) fn indexed_file_count(&self) -> rusqlite::Result<i64> {
        self.connection
            .query_row("SELECT COUNT(*) FROM indexed_files", [], |r| r.get(0))
    }
}

/// Ensures `cold_manifest` is loaded once before the first walk of disk stores.
pub(crate) fn ensure_cold_manifest_loaded() {
    let already_loaded = ledger_cache().read().map(|g| g.cold_loaded).unwrap_or(true);
    if already_loaded {
        return;
    }
    let loaded = active_index_db_path()
        .filter(|p| p.is_file())
        .and_then(|db_path| UsageIndexDb::open(&db_path).ok())
        .and_then(|db| db.load_manifest().ok());

    if let Ok(mut guard) = ledger_cache().write() {
        if !guard.cold_loaded {
            if let Some(manifest) = loaded {
                guard.cold_manifest = manifest;
                guard.recompute_cold_max_t();
            }
            guard.cold_loaded = true;
        }
    }
}

fn eligible_for_cold_spill(
    rank: usize,
    modified_ms: i64,
    max_record_t: Option<i64>,
    now_ms: i64,
) -> bool {
    let age_eligible = modified_ms > 0
        && modified_ms < now_ms.saturating_sub(COLD_AGE_MS)
        && max_record_t.is_none_or(|t| t < now_ms.saturating_sub(COLD_AGE_MS));
    let excess_idle = rank >= HOT_FILE_KEEP_PER_SOURCE
        && modified_ms > 0
        && modified_ms < now_ms.saturating_sub(IDLE_SPILL_AGE_MS)
        && max_record_t.is_none_or(|t| t < now_ms.saturating_sub(IDLE_SPILL_AGE_MS));
    age_eligible || excess_idle
}

fn collect_spill_for_source<'a, I>(
    source: &'static str,
    entries: I,
    cold_manifest: &HashMap<PathBuf, IndexedFileMeta>,
    now_ms: i64,
    out: &mut Vec<ColdSpillItem>,
) where
    I: Iterator<Item = (&'a PathBuf, u64, i64, &'a [LedgerRecord])>,
{
    let mut ranked: Vec<(&'a PathBuf, u64, i64, &'a [LedgerRecord])> = entries.collect();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.2));
    for (rank, (path, file_len, modified_ms, recs)) in ranked.into_iter().enumerate() {
        if cold_manifest
            .get(path)
            .is_some_and(|m| m.file_len == file_len && m.modified_ms == modified_ms)
        {
            continue;
        }
        let max_t = recs.iter().map(|r| r.t).max();
        if eligible_for_cold_spill(rank, modified_ms, max_t, now_ms) {
            out.push(ColdSpillItem {
                path: path.clone(),
                source,
                file_len,
                modified_ms,
                records: recs.to_vec(),
            });
        }
    }
}

/// Prunes stale in-memory `TranscriptState.index` hash maps and spills cold/excess
/// immutable files into `usage_index.db` without holding `ledger_cache()` across SQLite I/O.
pub(crate) fn compact_and_spill_cold() {
    let now_ms = now_epoch_ms();
    let db_path = active_index_db_path();
    let mut removals = Vec::new();
    let mut spill_batch = Vec::new();

    if let Ok(mut guard) = ledger_cache().write() {
        let prune_cutoff = now_ms.saturating_sub(INDEX_PRUNE_AGE_MS);
        for state in guard.files.values_mut() {
            if !state.cold_archived
                && !state.index.is_empty()
                && state.modified_ms > 0
                && state.modified_ms < prune_cutoff
            {
                state.index = HashMap::new();
                state.index_pruned = true;
                state.calls.shrink_to_fit();
            }
        }
        if db_path.is_some() {
            removals = guard.dirty_db_removals.drain().collect();
            collect_spill_for_source(
                "claude",
                guard
                    .files
                    .iter()
                    .filter(|(_, s)| !s.cold_archived)
                    .map(|(p, s)| (p, s.offset, s.modified_ms, s.calls.as_slice())),
                &guard.cold_manifest,
                now_ms,
                &mut spill_batch,
            );
            collect_spill_for_source(
                "codex",
                guard
                    .codex
                    .iter()
                    .filter(|(p, (_, recs))| {
                        !recs.is_empty() || !guard.cold_manifest.contains_key(*p)
                    })
                    .map(|(p, (m, recs))| (p, m.len, m.modified_ms, recs.as_slice())),
                &guard.cold_manifest,
                now_ms,
                &mut spill_batch,
            );
            collect_spill_for_source(
                "other",
                guard
                    .other_files
                    .iter()
                    .filter(|(p, (_, recs))| {
                        !recs.is_empty() || !guard.cold_manifest.contains_key(*p)
                    })
                    .map(|(p, (m, recs))| (p, m.len, m.modified_ms, recs.as_slice())),
                &guard.cold_manifest,
                now_ms,
                &mut spill_batch,
            );
        } else {
            guard.dirty_db_removals.clear();
        }
    }

    let Some(db_path) = db_path else {
        return;
    };
    if removals.is_empty() && spill_batch.is_empty() {
        return;
    }

    let Ok(mut db) = UsageIndexDb::open(&db_path) else {
        return;
    };
    let _ = db.remove_paths(&removals);
    if db.upsert_files(&spill_batch).is_err() {
        return;
    }

    if let Ok(mut guard) = ledger_cache().write() {
        for item in spill_batch {
            let min_t = item.records.iter().map(|r| r.t).min();
            let max_t = item.records.iter().map(|r| r.t).max();
            match item.source {
                "claude" => {
                    if let Some(state) = guard.files.get_mut(&item.path)
                        && state.offset == item.file_len
                        && state.modified_ms == item.modified_ms
                    {
                        state.cold_archived = true;
                        state.index_pruned = true;
                        state.calls = Vec::new();
                        state.index = HashMap::new();
                        guard.cold_manifest.insert(
                            item.path,
                            IndexedFileMeta {
                                source: "claude".to_owned(),
                                file_len: item.file_len,
                                modified_ms: item.modified_ms,
                                min_t,
                                max_t,
                            },
                        );
                    }
                }
                "codex" => {
                    if let Some((mark, recs)) = guard.codex.get_mut(&item.path)
                        && mark.len == item.file_len
                        && mark.modified_ms == item.modified_ms
                    {
                        *recs = Vec::new();
                        guard.cold_manifest.insert(
                            item.path,
                            IndexedFileMeta {
                                source: "codex".to_owned(),
                                file_len: item.file_len,
                                modified_ms: item.modified_ms,
                                min_t,
                                max_t,
                            },
                        );
                    }
                }
                "other" => {
                    if let Some((mark, recs)) = guard.other_files.get_mut(&item.path)
                        && mark.len == item.file_len
                        && mark.modified_ms == item.modified_ms
                    {
                        *recs = Vec::new();
                        guard.cold_manifest.insert(
                            item.path,
                            IndexedFileMeta {
                                source: "other".to_owned(),
                                file_len: item.file_len,
                                modified_ms: item.modified_ms,
                                min_t,
                                max_t,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        guard.recompute_cold_max_t();
    }
}
