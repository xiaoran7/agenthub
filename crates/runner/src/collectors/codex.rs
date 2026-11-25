use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, RwLock},
    time::{Duration, SystemTime},
};

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use super::*;

/// Reads Codex's metadata-only state database when an app-server executable is
/// unavailable (notably for Microsoft Store installs). Message bodies live in
/// rollout JSONL files and are intentionally never opened here.
pub fn collect_codex_local_threads(limit: u32) -> Result<Value, CollectorError> {
    collect_codex_local_threads_shared(limit).map(Arc::unwrap_or_clone)
}

pub fn collect_codex_local_threads_shared(limit: u32) -> Result<Arc<Value>, CollectorError> {
    let Some(database) = discover_codex_state_database()? else {
        return Ok(Arc::new(json!({ "data": [], "nextCursor": null })));
    };
    collect_codex_local_threads_shared_from(&database, limit)
}

pub(crate) struct CodexLocalThreadsCache {
    db_path: PathBuf,
    limit: u32,
    db_len: u64,
    db_mtime: Option<SystemTime>,
    wal_len: u64,
    wal_mtime: Option<SystemTime>,
    data: Arc<Value>,
}

pub(crate) fn codex_threads_cache() -> &'static RwLock<Option<CodexLocalThreadsCache>> {
    static CACHE: OnceLock<RwLock<Option<CodexLocalThreadsCache>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn collect_codex_local_threads_from(
    database: &Path,
    limit: u32,
) -> Result<Value, CollectorError> {
    collect_codex_local_threads_shared_from(database, limit).map(Arc::unwrap_or_clone)
}

pub fn collect_codex_local_threads_shared_from(
    database: &Path,
    limit: u32,
) -> Result<Arc<Value>, CollectorError> {
    let db_meta = fs::metadata(database).ok();
    let db_len = db_meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let mtime = db_meta.and_then(|m| m.modified().ok());
    let wal_path = database.with_file_name(format!(
        "{}-wal",
        database
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("state_5.db")
    ));
    let wal_meta = fs::metadata(&wal_path).ok();
    let wal_len = wal_meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let wal_mtime = wal_meta.and_then(|m| m.modified().ok());
    if let Ok(guard) = codex_threads_cache().read()
        && let Some(ref c) = *guard
        && c.db_path == database
        && c.limit == limit
        && c.db_len == db_len
        && c.db_mtime == mtime
        && c.wal_len == wal_len
        && c.wal_mtime == wal_mtime
    {
        return Ok(Arc::clone(&c.data));
    }

    let program = "Codex local state".to_owned();
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| CollectorError::Database {
        program: program.clone(),
        source,
    })?;
    let _ = connection.busy_timeout(Duration::from_millis(500));
    let columns = connection
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<HashSet<_>, _>>()
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    if !columns.contains("id") || !columns.contains("cwd") {
        return Ok(Arc::new(json!({ "data": [], "nextCursor": null })));
    }

    let created = seconds_expression(&columns, "created_at_ms", "created_at");
    let updated = seconds_expression(&columns, "updated_at_ms", "updated_at");
    let recency = seconds_expression(&columns, "recency_at_ms", "recency_at");
    let created_ms = millis_expression(&columns, "created_at_ms", "created_at");
    let updated_ms = millis_expression(&columns, "updated_at_ms", "updated_at");
    let recency_ms = millis_expression(&columns, "recency_at_ms", "recency_at");
    let source = match (
        columns.contains("thread_source"),
        columns.contains("source"),
    ) {
        (true, true) => "COALESCE(NULLIF(thread_source, ''), source, '')",
        (true, false) => "COALESCE(thread_source, '')",
        (false, true) => "COALESCE(source, '')",
        (false, false) => "''",
    };
    let archived_filter = if columns.contains("archived") {
        " WHERE archived = 0"
    } else {
        ""
    };
    let order_by = if columns.contains("recency_at_ms") {
        "recency_at_ms"
    } else if columns.contains("recency_at") {
        "recency_at"
    } else if columns.contains("updated_at_ms") {
        "updated_at_ms"
    } else {
        "updated_at"
    };
    let model_expr = optional_column(&columns, &["model"]);
    let tokens_expr = optional_column(&columns, &["tokens_used"]);

    let magpie_codex = load_magpie_codex_stats();
    let history_counts = database
        .parent()
        .map(|parent_dir| load_codex_history_counts(&parent_dir.join("thread_history_1.sqlite")))
        .unwrap_or_default();

    let sql = format!(
        "SELECT id, cwd, {created}, {updated}, {recency}, {source}, {model_expr}, {tokens_expr}, {created_ms}, {updated_ms}, {recency_ms} FROM threads{archived_filter} ORDER BY {order_by} DESC LIMIT ?1"
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    let rows = statement
        .query_map([i64::from(limit)], |row| {
            let id = row.get::<_, String>(0)?;
            let raw_cwd = row.get::<_, String>(1).unwrap_or_default();
            let cwd = super::codex_enrich::strip_windows_verbatim(&raw_cwd);
            let created_at = row.get::<_, Option<i64>>(2)?.unwrap_or_default();
            let updated_at = row.get::<_, Option<i64>>(3)?.unwrap_or_default();
            let recency_at = row.get::<_, Option<i64>>(4)?.unwrap_or_default();
            let source = row
                .get::<_, String>(5)
                .unwrap_or_else(|_| "localStore".to_owned());
            let model = row.get::<_, Option<String>>(6).ok().flatten();
            let tokens_used = row
                .get::<_, Option<i64>>(7)
                .ok()
                .flatten()
                .unwrap_or(0)
                .max(0) as u64;
            let c_ms = row.get::<_, Option<i64>>(8)?.unwrap_or_default();
            let u_ms = row.get::<_, Option<i64>>(9)?.unwrap_or_default();
            let r_ms = row.get::<_, Option<i64>>(10)?.unwrap_or_default();
            let last_activity_at_ms = r_ms.max(u_ms).max(c_ms);

            let (tt, pt, ct, cr, reqs) =
                if let Some(&(m_pt, m_ct, m_cr, m_reqs)) = magpie_codex.get(&id) {
                    let actual_tt = tokens_used.max(m_pt.saturating_add(m_ct));
                    let actual_cr = if m_cr > 0 {
                        m_cr
                    } else {
                        ((actual_tt as f64) * 0.954) as u64
                    };
                    (actual_tt, m_pt, m_ct, actual_cr, m_reqs.max(1))
                } else if tokens_used > 0 {
                    let cr = ((tokens_used as f64) * 0.954) as u64;
                    let reqs = history_counts.get(&id).copied().unwrap_or(1).max(1);
                    (tokens_used, tokens_used, 0, cr, reqs)
                } else {
                    let reqs = history_counts.get(&id).copied().unwrap_or(1).max(1);
                    (0, 0, 0, 0, reqs)
                };

            let mut item = json!({
                "id": id,
                "session_id": id,
                "cwd": cwd,
                "workspace_path": cwd,
                "createdAt": created_at,
                "updatedAt": updated_at,
                "recencyAt": recency_at,
                "last_activity_at_ms": last_activity_at_ms,
                "source": source,
                "status": { "type": "persisted" },
                "request_count": reqs,
                "calls": reqs
            });
            if let Some(m) = model {
                item["model"] = json!(m);
            }
            if tt > 0 {
                item["total_tokens"] = json!(tt);
                item["prompt_tokens"] = json!(pt);
                item["completion_tokens"] = json!(ct);
                item["cache_read_tokens"] = json!(cr);
            }
            Ok(item)
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CollectorError::Database { program, source })?;
    let result = Arc::new(json!({ "data": rows, "nextCursor": null }));
    if let Ok(mut guard) = codex_threads_cache().write() {
        *guard = Some(CodexLocalThreadsCache {
            db_path: database.to_path_buf(),
            limit,
            db_len,
            db_mtime: mtime,
            wal_len,
            wal_mtime,
            data: Arc::clone(&result),
        });
    }
    Ok(result)
}

pub(crate) fn load_magpie_codex_stats() -> HashMap<String, (u64, u64, u64, u64)> {
    static CACHE: OnceLock<
        RwLock<
            Option<(
                u64,
                Option<SystemTime>,
                HashMap<String, (u64, u64, u64, u64)>,
            )>,
        >,
    > = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(None));

    let Some(home) = home_dir() else {
        return HashMap::new();
    };
    let magpie_path = home.join(".cache").join("magpie").join("sessions.json");
    let Ok(meta) = fs::metadata(&magpie_path) else {
        return HashMap::new();
    };
    let len = meta.len();
    let mtime = meta.modified().ok();
    if let Ok(guard) = cache.read()
        && let Some((c_len, c_mtime, ref map)) = *guard
        && c_len == len
        && c_mtime == mtime
    {
        return map.clone();
    }

    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct MagpieModelStats {
        input: JsonOptU64,
        output: JsonOptU64,
        cache_read: JsonOptU64,
    }
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct MagpieDayStats {
        r: JsonOptU64,
    }
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct MagpieFileStats {
        id: Option<String>,
        models: JsonOptMap<HashMap<String, JsonOptMap<MagpieModelStats>>>,
        days: JsonOptMap<HashMap<String, JsonOptMap<MagpieDayStats>>>,
    }
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct MagpieSessionsFile {
        files: JsonOptMap<HashMap<String, JsonOptMap<MagpieFileStats>>>,
    }

    let Ok(file) = fs::File::open(&magpie_path) else {
        return HashMap::new();
    };
    let Ok(parsed) =
        serde_json::from_reader::<_, MagpieSessionsFile>(std::io::BufReader::new(file))
    else {
        return HashMap::new();
    };
    let mut magpie_codex = HashMap::new();
    if let Some(files) = parsed.files.0 {
        for (k, v_wrap) in files {
            if !k.contains(".codex") {
                continue;
            }
            let Some(v) = v_wrap.0 else {
                continue;
            };
            let Some(id) = v.id else {
                continue;
            };
            let mut m_in: u64 = 0;
            let mut m_out: u64 = 0;
            let mut m_cr: u64 = 0;
            if let Some(models) = v.models.0 {
                for m_wrap in models.into_values() {
                    if let Some(m) = m_wrap.0 {
                        m_in = m_in.saturating_add(m.input.0.unwrap_or(0));
                        m_out = m_out.saturating_add(m.output.0.unwrap_or(0));
                        m_cr = m_cr.saturating_add(m.cache_read.0.unwrap_or(0));
                    }
                }
            }
            let mut reqs: u64 = 0;
            if let Some(days) = v.days.0 {
                for day_wrap in days.into_values() {
                    if let Some(day) = day_wrap.0 {
                        reqs = reqs.saturating_add(day.r.0.unwrap_or(0));
                    }
                }
            }
            let prompt = m_in.saturating_add(m_cr);
            magpie_codex.insert(id, (prompt, m_out, m_cr, reqs));
        }
    }

    if let Ok(mut guard) = cache.write() {
        *guard = Some((len, mtime, magpie_codex.clone()));
    }
    magpie_codex
}

pub(crate) fn load_codex_history_counts(hist_path: &Path) -> HashMap<String, u64> {
    static CACHE: OnceLock<
        RwLock<
            Option<(
                PathBuf,
                u64,
                Option<SystemTime>,
                u64,
                Option<SystemTime>,
                HashMap<String, u64>,
            )>,
        >,
    > = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(None));

    let Ok(meta) = fs::metadata(hist_path) else {
        return HashMap::new();
    };
    let db_len = meta.len();
    let db_mtime = meta.modified().ok();
    let wal_path = hist_path.with_file_name("thread_history_1.sqlite-wal");
    let wal_meta = fs::metadata(&wal_path).ok();
    let wal_len = wal_meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let wal_mtime = wal_meta.and_then(|m| m.modified().ok());

    if let Ok(guard) = cache.read()
        && let Some((ref c_path, c_len, c_mtime, c_wal_len, c_wal_mtime, ref map)) = *guard
        && c_path == hist_path
        && c_len == db_len
        && c_mtime == db_mtime
        && c_wal_len == wal_len
        && c_wal_mtime == wal_mtime
    {
        return map.clone();
    }

    let Ok(hist_conn) = Connection::open_with_flags(
        hist_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return HashMap::new();
    };
    let _ = hist_conn.busy_timeout(Duration::from_millis(500));
    let Ok(mut stmt) = hist_conn.prepare(
        "SELECT thread_id, COUNT(*) FROM thread_items WHERE item_type = 'agentMessage' GROUP BY thread_id",
    ) else {
        return HashMap::new();
    };
    let Ok(rows) = stmt.query_map([], |row| {
        let tid: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        Ok((tid, count.max(0) as u64))
    }) else {
        return HashMap::new();
    };
    let mut history_counts = HashMap::new();
    for row in rows {
        let Ok((tid, count)) = row else {
            return HashMap::new();
        };
        history_counts.insert(tid, count);
    }

    if let Ok(mut guard) = cache.write() {
        *guard = Some((
            hist_path.to_path_buf(),
            db_len,
            db_mtime,
            wal_len,
            wal_mtime,
            history_counts.clone(),
        ));
    }
    history_counts
}

pub(crate) fn optional_column<'a>(columns: &HashSet<String>, candidates: &'a [&str]) -> &'a str {
    candidates
        .iter()
        .copied()
        .find(|column| columns.contains(*column))
        .unwrap_or("NULL")
}

pub(crate) fn millis_expression(columns: &HashSet<String>, millis: &str, seconds: &str) -> String {
    match (columns.contains(millis), columns.contains(seconds)) {
        (true, true) => format!("COALESCE(NULLIF({millis}, 0), {seconds} * 1000)"),
        (true, false) => millis.to_owned(),
        (false, true) => format!("{seconds} * 1000"),
        (false, false) => "NULL".to_owned(),
    }
}

fn seconds_expression(columns: &HashSet<String>, millis: &str, seconds: &str) -> String {
    match (columns.contains(millis), columns.contains(seconds)) {
        (true, true) => format!("COALESCE(NULLIF({millis}, 0) / 1000, {seconds})"),
        (true, false) => format!("{millis} / 1000"),
        (false, true) => seconds.to_owned(),
        (false, false) => "NULL".to_owned(),
    }
}

pub(crate) fn discover_codex_state_database() -> Result<Option<PathBuf>, CollectorError> {
    let root = if let Some(path) = env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(path)
    } else if let Some(home) = home_dir() {
        home.join(".codex")
    } else {
        return Ok(None);
    };
    discover_codex_state_database_in(&root)
}

pub(crate) fn discover_codex_state_database_in(
    root: &Path,
) -> Result<Option<PathBuf>, CollectorError> {
    if !root.is_dir() {
        return Ok(None);
    }
    let program = "Codex local state".to_owned();
    let mut candidates = fs::read_dir(root)
        .map_err(|source| CollectorError::Io {
            program: program.clone(),
            source,
        })?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let version = name
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse::<u32>()
                .ok()?;
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_file())
                .map(|_| (version, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(version, _)| std::cmp::Reverse(*version));
    if let Some((_, path)) = candidates.into_iter().next() {
        return Ok(Some(path));
    }
    let legacy = root.join("state.sqlite");
    Ok(legacy.is_file().then_some(legacy))
}
