use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::{Duration, SystemTime},
};

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    BoundedLruCache, LocalSessionContext,
    codex::{
        discover_codex_state_database, load_codex_history_counts, load_magpie_codex_stats,
        millis_expression, optional_column,
    },
    codex_context::apply_codex_rollout_context_cached,
    home_dir, local_prompt_title, metadata_label,
};

/// Subagent session spawned from a parent Codex multi-agent conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexSubagentEntry {
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

#[derive(Debug, Clone, Default)]
pub(crate) struct CodexEnrichmentRow {
    pub id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub last_activity_at_ms: i64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    pub context_used_tokens: u64,
    pub context_window_tokens: u64,
    pub request_count: u64,
    pub parent_id: Option<String>,
    pub subagent_depth: u32,
    pub subagent_kind: Option<String>,
}

struct CachedCodexIndex {
    db_path: PathBuf,
    db_len: u64,
    db_mtime: Option<SystemTime>,
    wal_len: u64,
    wal_mtime: Option<SystemTime>,
    rows: HashMap<String, CodexEnrichmentRow>,
}

type RolloutCtxCache = BoundedLruCache<PathBuf, (u64, Option<SystemTime>, LocalSessionContext)>;

fn enrichment_cache() -> &'static RwLock<Option<CachedCodexIndex>> {
    static CACHE: OnceLock<RwLock<Option<CachedCodexIndex>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub(crate) fn rollout_ctx_cache() -> &'static RwLock<RolloutCtxCache> {
    static CACHE: OnceLock<RwLock<RolloutCtxCache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BoundedLruCache::new(2_048)))
}

pub fn clear_codex_enrichment_cache() {
    if let Ok(mut g) = enrichment_cache().write() {
        *g = None;
    }
    if let Ok(mut g) = rollout_ctx_cache().write() {
        g.clear();
    }
}

/// Strips Windows extended-length verbatim path prefixes (`\\?\` or `//?/`).
pub fn strip_windows_verbatim(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = trimmed
        .strip_prefix(r"\\?\")
        .or_else(|| trimmed.strip_prefix("//?/"))
    {
        return rest.to_owned();
    }
    trimmed.to_owned()
}

/// Normalizes epoch seconds vs milliseconds into epoch milliseconds.
pub fn normalize_codex_timestamp_ms(val: i64) -> i64 {
    if val <= 0 {
        0
    } else if val < 100_000_000_000 {
        val.saturating_mul(1_000)
    } else {
        val
    }
}

/// Parses Codex subagent relationship from `source` JSON (`{"subagent":{"thread_spawn":{"parent_thread_id":...}}}`)
/// or explicit parent/role columns.
pub fn parse_codex_subagent_meta(
    source_raw: &str,
    explicit_parent: Option<&str>,
    nickname: Option<&str>,
    role: Option<&str>,
) -> Option<(String, u32, String)> {
    let clean_parent = explicit_parent.map(str::trim).filter(|s| !s.is_empty());
    let mut parsed_parent: Option<String> = clean_parent.map(str::to_owned);
    let mut depth = 1_u32;
    let mut json_nick: Option<String> = None;
    let mut json_role: Option<String> = None;

    let trimmed = source_raw.trim();
    if trimmed.starts_with('{')
        && let Ok(val) = serde_json::from_str::<Value>(trimmed)
        && let Some(sub) = val.get("subagent").and_then(Value::as_object)
    {
        for (variant, inner) in sub {
            if let Some(pid) = inner
                .get("parent_thread_id")
                .or_else(|| inner.get("parentThreadId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                parsed_parent = Some(pid.to_owned());
            }
            if let Some(d) = inner.get("depth").and_then(Value::as_u64) {
                depth = (d as u32).max(1);
            }
            json_nick = inner
                .get("agent_nickname")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            json_role = inner
                .get("agent_role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .or_else(|| Some(variant.clone()));
        }
    }

    let pid = parsed_parent?;
    let eff_nick = nickname
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(json_nick.as_deref());
    let eff_role = role
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(json_role.as_deref());

    let kind = match (eff_nick, eff_role) {
        (Some(n), Some(r)) => format!("{n} · {r}"),
        (Some(n), None) => n.to_owned(),
        (None, Some(r)) => r.to_owned(),
        (None, None) => "subagent".to_owned(),
    };
    Some((pid, depth, kind))
}

pub(crate) fn load_codex_enrichment_from_db(
    database: &Path,
) -> HashMap<String, CodexEnrichmentRow> {
    let db_meta = fs::metadata(database).ok();
    let db_len = db_meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let db_mtime = db_meta.and_then(|m| m.modified().ok());
    let wal_path = database.with_file_name(format!(
        "{}-wal",
        database
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("state_5.sqlite")
    ));
    let wal_meta = fs::metadata(&wal_path).ok();
    let wal_len = wal_meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let wal_mtime = wal_meta.and_then(|m| m.modified().ok());

    if let Ok(guard) = enrichment_cache().read()
        && let Some(ref c) = *guard
        && c.db_path == database
        && c.db_len == db_len
        && c.db_mtime == db_mtime
        && c.wal_len == wal_len
        && c.wal_mtime == wal_mtime
    {
        return c.rows.clone();
    }

    let Ok(conn) = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return HashMap::new();
    };
    let _ = conn.busy_timeout(Duration::from_millis(500));
    let Ok(columns) = conn
        .prepare("PRAGMA table_info(threads)")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(1))?
                .collect::<Result<HashSet<_>, _>>()
        })
    else {
        return HashMap::new();
    };
    if !columns.contains("id") {
        return HashMap::new();
    }

    let cwd_expr = optional_column(&columns, &["cwd"]);
    let created_ms = millis_expression(&columns, "created_at_ms", "created_at");
    let updated_ms = millis_expression(&columns, "updated_at_ms", "updated_at");
    let recency_ms = millis_expression(&columns, "recency_at_ms", "recency_at");
    let source_expr = match (
        columns.contains("thread_source"),
        columns.contains("source"),
    ) {
        (true, true) => {
            "CASE WHEN source LIKE '{%' THEN source ELSE COALESCE(NULLIF(source, ''), NULLIF(thread_source, ''), '') END"
        }
        (true, false) => "COALESCE(thread_source, '')",
        (false, true) => "COALESCE(source, '')",
        (false, false) => "''",
    };
    let parent_expr = optional_column(&columns, &["attached_thread_parent_id"]);
    let nick_expr = optional_column(&columns, &["agent_nickname"]);
    let role_expr = optional_column(&columns, &["agent_role"]);
    let model_expr = optional_column(&columns, &["model"]);
    let effort_expr = optional_column(&columns, &["reasoning_effort"]);
    let tokens_expr = optional_column(&columns, &["tokens_used"]);
    let rollout_expr = optional_column(&columns, &["rollout_path"]);
    let title_parts: Vec<String> = ["name", "title", "first_user_message", "preview"]
        .iter()
        .filter(|c| columns.contains(**c))
        .map(|c| format!("NULLIF(TRIM({c}), '')"))
        .collect();
    let title_expr = if title_parts.is_empty() {
        "NULL".to_owned()
    } else {
        format!("COALESCE({})", title_parts.join(", "))
    };
    let archived_filter = if columns.contains("archived") {
        " WHERE archived = 0"
    } else {
        ""
    };

    let root = database.parent().unwrap_or_else(|| Path::new("."));
    let magpie_codex = load_magpie_codex_stats();
    let history_counts = load_codex_history_counts(&root.join("thread_history_1.sqlite"));

    let sql = format!(
        "SELECT id, {cwd_expr}, {created_ms}, {updated_ms}, {recency_ms}, {source_expr}, \
         {parent_expr}, {nick_expr}, {role_expr}, {model_expr}, {effort_expr}, {tokens_expr}, \
         {rollout_expr}, {title_expr} FROM threads{archived_filter} \
         ORDER BY COALESCE({recency_ms}, {updated_ms}, {created_ms}, 0) DESC LIMIT 5000"
    );
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return HashMap::new();
    };

    let Ok(iter) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1).unwrap_or_default(),
            row.get::<_, Option<i64>>(2).unwrap_or_default(),
            row.get::<_, Option<i64>>(3).unwrap_or_default(),
            row.get::<_, Option<i64>>(4).unwrap_or_default(),
            row.get::<_, Option<String>>(5).unwrap_or_default(),
            row.get::<_, Option<String>>(6).unwrap_or_default(),
            row.get::<_, Option<String>>(7).unwrap_or_default(),
            row.get::<_, Option<String>>(8).unwrap_or_default(),
            row.get::<_, Option<String>>(9).unwrap_or_default(),
            row.get::<_, Option<String>>(10).unwrap_or_default(),
            row.get::<_, Option<i64>>(11).unwrap_or_default(),
            row.get::<_, Option<String>>(12).unwrap_or_default(),
            row.get::<_, Option<String>>(13).unwrap_or_default(),
        ))
    }) else {
        return HashMap::new();
    };

    let mut out = HashMap::new();
    for row_res in iter {
        let Ok(r) = row_res else {
            return HashMap::new();
        };
        let (
            id,
            cwd_opt,
            c_ms,
            u_ms,
            r_ms,
            src_opt,
            parent_opt,
            nick_opt,
            role_opt,
            model_opt,
            effort_opt,
            tok_opt,
            rollout_opt,
            raw_title_opt,
        ) = r;
        let cwd = cwd_opt
            .as_deref()
            .map(strip_windows_verbatim)
            .unwrap_or_default();
        let last_ms = [r_ms, u_ms, c_ms]
            .into_iter()
            .flatten()
            .map(normalize_codex_timestamp_ms)
            .max()
            .unwrap_or(0);
        let db_tokens = tok_opt.unwrap_or(0).max(0) as u64;
        let sub_meta = parse_codex_subagent_meta(
            src_opt.as_deref().unwrap_or_default(),
            parent_opt.as_deref(),
            nick_opt.as_deref(),
            role_opt.as_deref(),
        );

        let mut title = raw_title_opt
            .as_deref()
            .and_then(local_prompt_title)
            .or_else(|| {
                raw_title_opt
                    .as_deref()
                    .and_then(|t| metadata_label(t, 160))
                    .filter(|t| !t.contains("AGENTS.md instructions"))
            });
        let mut model = model_opt.and_then(|m| metadata_label(&m, 96));
        let mut effort = effort_opt.and_then(|e| metadata_label(&e, 64));
        let mut r_in = 0_u64;
        let mut r_out = 0_u64;
        let mut r_cr = 0_u64;
        let mut r_total = 0_u64;
        let mut ctx_used = 0_u64;
        let mut ctx_win = 0_u64;

        if let Some(ref rp) = rollout_opt {
            let clean_rp = strip_windows_verbatim(rp);
            if let Some(ctx) = apply_codex_rollout_context_cached(root, Path::new(&clean_rp)) {
                if title.is_none() {
                    title = ctx.title;
                }
                if ctx.model.is_some() {
                    model = ctx.model;
                }
                if ctx.reasoning_effort.is_some() {
                    effort = ctx.reasoning_effort;
                }
                r_in = ctx.input_tokens.unwrap_or(0);
                r_out = ctx.output_tokens.unwrap_or(0);
                r_cr = ctx.cache_read_tokens.unwrap_or(0);
                r_total = ctx.session_total_tokens.unwrap_or(0);
                ctx_used = ctx.context_used_tokens.unwrap_or(0);
                ctx_win = ctx.context_window_tokens.unwrap_or(0);
            }
        }

        let base_reqs = history_counts.get(&id).copied().unwrap_or(1).max(1);
        let (tt, pt, ct, cr, reqs) =
            if let Some(&(m_pt, m_ct, m_cr, m_reqs)) = magpie_codex.get(&id) {
                let tot = db_tokens.max(r_total).max(m_pt.saturating_add(m_ct));
                (tot, m_pt, m_ct, m_cr.max(r_cr), m_reqs.max(base_reqs))
            } else if r_total > 0 || r_in > 0 || r_out > 0 {
                let tot = db_tokens.max(r_total).max(r_in.saturating_add(r_out));
                let prompt = if tot > r_in.saturating_add(r_out) {
                    tot.saturating_sub(r_out)
                } else {
                    r_in
                };
                (tot, prompt, r_out, r_cr, base_reqs)
            } else if db_tokens > 0 {
                (db_tokens, db_tokens, 0, 0, base_reqs)
            } else {
                (0, 0, 0, 0, base_reqs)
            };

        out.insert(
            id.clone(),
            CodexEnrichmentRow {
                id,
                cwd,
                title,
                model,
                reasoning_effort: effort,
                last_activity_at_ms: last_ms,
                prompt_tokens: pt,
                completion_tokens: ct,
                cache_read_tokens: cr,
                total_tokens: tt,
                context_used_tokens: ctx_used,
                context_window_tokens: ctx_win,
                request_count: reqs,
                parent_id: sub_meta.as_ref().map(|(p, _, _)| p.clone()),
                subagent_depth: sub_meta.as_ref().map(|(_, d, _)| *d).unwrap_or(0),
                subagent_kind: sub_meta.map(|(_, _, k)| k),
            },
        );
    }

    if let Ok(mut guard) = enrichment_cache().write() {
        *guard = Some(CachedCodexIndex {
            db_path: database.to_path_buf(),
            db_len,
            db_mtime,
            wal_len,
            wal_mtime,
            rows: out.clone(),
        });
    }
    out
}

/// Enriches Codex `thread/list` items (from either `codex app-server` or `state_5.sqlite`)
/// with millisecond timestamps, stripped `\\?\` paths, real token/context metrics, and
/// folded multi-agent `subagents` trees matching Claude Desktop & Antigravity.
pub fn enrich_codex_thread_items(items: &mut Vec<Value>) {
    let index = discover_codex_state_database()
        .ok()
        .flatten()
        .map(|db| load_codex_enrichment_from_db(&db))
        .unwrap_or_default();
    enrich_codex_thread_items_with_index(items, &index);
}

pub(crate) fn enrich_codex_thread_items_with_index(
    items: &mut Vec<Value>,
    index: &HashMap<String, CodexEnrichmentRow>,
) {
    // Collect known Codex thread IDs (both in `items` and in `index`)
    let mut known_ids: HashSet<String> = index.keys().cloned().collect();
    for item in items.iter() {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            known_ids.insert(id.to_owned());
        }
    }

    // Build subagent entries grouped by ultimate root parent thread
    let mut subs_by_root: HashMap<String, Vec<CodexSubagentEntry>> = HashMap::new();
    let mut child_subagent_ids: HashSet<String> = HashSet::new();

    for (sid, row) in index {
        if let Some(ref pid) = row.parent_id
            && pid != sid
            && known_ids.contains(pid)
        {
            let mut root_id = pid.clone();
            let mut hops = 0_u8;
            while hops < 8 {
                if let Some(p_row) = index.get(&root_id)
                    && let Some(ref gp) = p_row.parent_id
                    && gp != &root_id
                    && known_ids.contains(gp)
                {
                    root_id = gp.clone();
                    hops += 1;
                } else {
                    break;
                }
            }
            child_subagent_ids.insert(sid.clone());
            subs_by_root
                .entry(root_id.clone())
                .or_default()
                .push(CodexSubagentEntry {
                    id: sid.clone(),
                    kind: row
                        .subagent_kind
                        .clone()
                        .unwrap_or_else(|| "subagent".to_owned()),
                    description: row.title.clone().unwrap_or_default(),
                    parent: if pid == &root_id {
                        String::new()
                    } else {
                        pid.clone()
                    },
                    depth: row.subagent_depth.max(1),
                    last_activity_at_ms: row.last_activity_at_ms,
                    model: row.model.clone(),
                    prompt_tokens: row.prompt_tokens,
                    completion_tokens: row.completion_tokens,
                    cache_read_tokens: row.cache_read_tokens,
                    request_count: row.request_count,
                });
        }
    }

    // Also catch any subAgent items emitted directly by `codex app-server` not yet in index
    for item in items.iter() {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        if child_subagent_ids.contains(id) {
            continue;
        }
        let parent = item
            .get("parentThreadId")
            .or_else(|| item.get("attachedToThreadId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let src = item
            .get("source")
            .and_then(Value::as_str)
            .filter(|s| s.trim_start().starts_with('{'))
            .or_else(|| item.get("threadSource").and_then(Value::as_str))
            .or_else(|| item.get("source").and_then(Value::as_str))
            .unwrap_or_default();
        if let Some((pid, depth, kind)) = parse_codex_subagent_meta(
            src,
            parent,
            item.get("agentNickname").and_then(Value::as_str),
            item.get("agentRole").and_then(Value::as_str),
        ) && pid != id
            && known_ids.contains(&pid)
        {
            child_subagent_ids.insert(id.to_owned());
            let raw_ts = item
                .get("last_activity_at_ms")
                .or_else(|| item.get("recencyAt"))
                .or_else(|| item.get("updatedAt"))
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let desc = item
                .get("name")
                .or_else(|| item.get("preview"))
                .and_then(Value::as_str)
                .and_then(local_prompt_title)
                .unwrap_or_default();
            subs_by_root
                .entry(pid)
                .or_default()
                .push(CodexSubagentEntry {
                    id: id.to_owned(),
                    kind,
                    description: desc,
                    parent: String::new(),
                    depth,
                    last_activity_at_ms: normalize_codex_timestamp_ms(raw_ts),
                    model: item.get("model").and_then(Value::as_str).map(str::to_owned),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cache_read_tokens: 0,
                    request_count: 1,
                });
        }
    }

    // Remove child subagent rows from top-level items and enrich every root Codex thread
    items.retain(|item| {
        item.get("id")
            .and_then(Value::as_str)
            .is_none_or(|id| !child_subagent_ids.contains(id))
    });

    for item in items.iter_mut() {
        let is_codex = item
            .get("provider")
            .and_then(Value::as_str)
            .is_none_or(|p| p == "codex");
        if !is_codex {
            continue;
        }
        let Some(id) = item.get("id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        if item.get("session_id").is_none() {
            item["session_id"] = json!(id);
        }
        if let Some(raw_cwd) = item.get("cwd").and_then(Value::as_str) {
            let clean = strip_windows_verbatim(raw_cwd);
            item["cwd"] = json!(clean);
            item["workspace_path"] = json!(clean);
        }

        // Normalize notLoaded / persisted status to idle for UI consistency
        if matches!(
            item.pointer("/status/type").and_then(Value::as_str),
            Some("notLoaded" | "persisted")
        ) {
            item["status"] = json!({ "type": "idle" });
        }

        // Normalize timestamp to epoch milliseconds
        let raw_ts = item
            .get("last_activity_at_ms")
            .and_then(Value::as_i64)
            .filter(|&v| v > 0)
            .or_else(|| item.get("recencyAt").and_then(Value::as_i64))
            .or_else(|| item.get("updatedAt").and_then(Value::as_i64))
            .or_else(|| item.get("createdAt").and_then(Value::as_i64))
            .unwrap_or(0);
        let mut last_ms = normalize_codex_timestamp_ms(raw_ts);

        if let Some(row) = index.get(&id) {
            if row.last_activity_at_ms > last_ms {
                last_ms = row.last_activity_at_ms;
            }
            if item
                .get("cwd")
                .and_then(Value::as_str)
                .is_none_or(|s| s.is_empty())
                && !row.cwd.is_empty()
            {
                item["cwd"] = json!(row.cwd);
                item["workspace_path"] = json!(row.cwd);
            }
            if item
                .get("model")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
                && let Some(ref m) = row.model
            {
                item["model"] = json!(m);
            }
            if item.get("reasoningEffort").is_none()
                && let Some(ref eff) = row.reasoning_effort
            {
                item["reasoningEffort"] = json!(eff);
            }
            let existing_name = item
                .get("name")
                .or_else(|| item.get("title"))
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty() && !s.contains("AGENTS.md instructions"));
            if existing_name.is_none()
                && let Some(ref t) = row.title
            {
                item["title"] = json!(t);
                item["name"] = json!(t);
            }
            if row.total_tokens > 0 {
                item["total_tokens"] = json!(row.total_tokens);
                item["prompt_tokens"] = json!(row.prompt_tokens);
                item["completion_tokens"] = json!(row.completion_tokens);
                item["cache_read_tokens"] = json!(row.cache_read_tokens);
            }
            if row.context_used_tokens > 0 {
                item["context_used_tokens"] = json!(row.context_used_tokens);
            }
            if row.context_window_tokens > 0 {
                item["context_window_tokens"] = json!(row.context_window_tokens);
            }
            item["request_count"] = json!(row.request_count.max(1));
            item["calls"] = json!(row.request_count.max(1));
        } else {
            if item.get("request_count").is_none() {
                item["request_count"] = json!(1);
                item["calls"] = json!(1);
            }
            // Sanitize preview into clean title when index has no entry
            if item
                .get("name")
                .and_then(Value::as_str)
                .is_none_or(|s| s.trim().is_empty())
                && let Some(prev) = item.get("preview").and_then(Value::as_str)
                && let Some(clean_t) = local_prompt_title(prev)
            {
                item["title"] = json!(clean_t);
                item["name"] = json!(clean_t);
            }
        }

        if last_ms > 0 {
            item["last_activity_at_ms"] = json!(last_ms);
        }

        if let Some(mut subs) = subs_by_root.remove(&id) {
            subs.sort_by_key(|s| s.last_activity_at_ms);
            item["subagents"] = json!(subs);
        }
    }

    let _ = home_dir;
}
