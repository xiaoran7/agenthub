//! Magpie-grade Workspace Discovery and Aggregation Engine.
//!
//! Unlike static or shallow directory scanners, this module dynamically aggregates all
//! workspaces from the authoritative session records of all installed AI agent platforms:
//! - Claude Code (`~/.claude/projects/*/` and `.jsonl` transcript records)
//! - OpenAI Codex (`~/.codex/state_5.db` and rollout thread contexts)
//! - ZCode (`~/.zcode/cli/db/db.sqlite` session table)
//! - Antigravity (`~/.gemini/antigravity/brain/*/transcript.jsonl`)
//! - Grok Build (`~/.grok/sessions/*/*/summary.json`)
//!
//! It performs robust path normalization (stripping Windows `\\?\` UNC prefixes,
//! standardizing separators, resolving drive letter casing, and deduplicating aliases),
//! calculates per-workspace session counts, determines the latest active timestamp,
//! identifies participating agents, and detects Git repository status.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::{Duration, Instant, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An aggregated workspace discovered from agent session histories.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredWorkspace {
    /// Canonical normalized physical directory path (e.g. `D:\Claudespace\Project\agenthub`).
    pub path: String,
    /// Friendly human-readable folder name (e.g. `agenthub`).
    pub name: String,
    /// Total number of sessions linked to this workspace across all agents.
    pub session_count: usize,
    /// Epoch timestamp in milliseconds of the latest session activity.
    pub last_active_at_ms: i64,
    /// Formatted relative or readable active time.
    pub last_active_str: String,
    /// List of agent platforms that have run in this workspace.
    pub agents: Vec<String>,
    /// Whether this workspace contains a `.git` repository folder.
    pub is_git: bool,
}

/// Normalizes raw directory paths from various session formats into a clean, canonical Windows path.
pub fn normalize_workspace_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return None;
    }

    // Strip UNC/extended prefixes
    let clean = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);
    let clean = clean.strip_prefix(r"\\.\").unwrap_or(clean);

    let path = PathBuf::from(clean);
    // If the path exists on disk, canonicalize it to resolve actual disk casing
    let canonical = if let Ok(canon) = dunce::canonicalize(&path) {
        canon
    } else {
        path
    };

    let s = canonical.to_string_lossy().to_string();
    let s_clean = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let mut normalized = s_clean.replace('/', "\\");

    // Standardize drive letter to uppercase (e.g. "d:\" -> "D:\")
    if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        let first = normalized.chars().next().unwrap().to_ascii_uppercase();
        normalized.replace_range(0..1, &first.to_string());
    }

    // Strip trailing slash unless it's a root drive like "C:\"
    if normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }

    Some(normalized)
}

/// Derives a clean workspace display name from its path.
pub fn derive_workspace_name(path_str: &str) -> String {
    let p = Path::new(path_str);
    if let Some(file_name) = p.file_name().and_then(|n| n.to_str()) {
        if !file_name.is_empty() {
            return file_name.to_string();
        }
    }
    path_str.to_string()
}

/// Formats millisecond timestamp into human-readable relative time string.
pub fn format_relative_time(epoch_ms: i64) -> String {
    if epoch_ms <= 0 {
        return "未知".to_string();
    }

    let now_ms = Utc::now().timestamp_millis();
    let diff_ms = now_ms.saturating_sub(epoch_ms);

    if diff_ms < 60_000 {
        "刚刚".to_string()
    } else if diff_ms < 3_600_000 {
        format!("{} 分钟前", diff_ms / 60_000)
    } else if diff_ms < 86_400_000 {
        format!("{} 小时前", diff_ms / 3_600_000)
    } else if diff_ms < 86_400_000 * 30 {
        format!("{} 天前", diff_ms / 86_400_000)
    } else if let Some(dt) = DateTime::from_timestamp_millis(epoch_ms) {
        dt.format("%Y-%m-%d").to_string()
    } else {
        "历史会话".to_string()
    }
}

/// Internal workspace accumulator.
#[derive(Default)]
struct WorkspaceAccumulator {
    path: String,
    name: String,
    session_count: usize,
    last_active_at_ms: i64,
    agents: HashSet<String>,
    is_git: bool,
}

fn workspace_cache() -> &'static RwLock<Option<(Instant, Vec<DiscoveredWorkspace>)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, Vec<DiscoveredWorkspace>)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Explicitly invalidates the cached workspace list.
pub fn invalidate_workspace_cache() {
    if let Ok(mut guard) = workspace_cache().write() {
        *guard = None;
    }
}

/// Discovers and aggregates all workspaces across all supported agents.
pub fn discover_all_workspaces() -> Vec<DiscoveredWorkspace> {
    if let Ok(guard) = workspace_cache().read() {
        if let Some((cached_at, ref list)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(6) {
                return list.clone();
            }
        }
    }

    let mut map: HashMap<String, WorkspaceAccumulator> = HashMap::new();

    // 1. Scan Claude Code projects
    scan_claude_workspaces(&mut map);

    // 2. Scan ZCode SQLite database
    scan_zcode_workspaces(&mut map);

    // 3. Scan Codex local state database
    scan_codex_workspaces(&mut map);

    // 4. Scan Antigravity sessions
    scan_antigravity_workspaces(&mut map);

    // 5. Scan Grok Build sessions
    scan_grok_workspaces(&mut map);

    // Convert accumulated map to sorted list
    let mut results: Vec<DiscoveredWorkspace> = map
        .into_values()
        .map(|acc| {
            let mut agent_list: Vec<String> = acc.agents.into_iter().collect();
            agent_list.sort();
            let relative_str = format_relative_time(acc.last_active_at_ms);

            DiscoveredWorkspace {
                path: acc.path,
                name: acc.name,
                session_count: acc.session_count,
                last_active_at_ms: acc.last_active_at_ms,
                last_active_str: relative_str,
                agents: agent_list,
                is_git: acc.is_git,
            }
        })
        .collect();

    // Sort by latest active activity descending; workspaces last touched in
    // the same second keep a stable order by path, so the ranking does not
    // reshuffle between snapshots (the map hands its values out unordered)
    results.sort_by(|a, b| {
        b.last_active_at_ms
            .cmp(&a.last_active_at_ms)
            .then_with(|| a.path.cmp(&b.path))
    });

    if let Ok(mut guard) = workspace_cache().write() {
        *guard = Some((Instant::now(), results.clone()));
    }
    results
}

fn add_workspace_record(
    map: &mut HashMap<String, WorkspaceAccumulator>,
    raw_path: &str,
    session_count: usize,
    last_active_ms: i64,
    agent_name: &str,
) {
    let Some(norm_path) = normalize_workspace_path(raw_path) else {
        return;
    };

    // Case-insensitive key for Windows deduplication
    let key = norm_path.to_ascii_lowercase();
    let entry = map.entry(key).or_insert_with(|| {
        let name = derive_workspace_name(&norm_path);
        let is_git = Path::new(&norm_path).join(".git").exists();
        WorkspaceAccumulator {
            path: norm_path,
            name,
            session_count: 0,
            last_active_at_ms: 0,
            agents: HashSet::new(),
            is_git,
        }
    });

    entry.session_count += session_count;
    if last_active_ms > entry.last_active_at_ms {
        entry.last_active_at_ms = last_active_ms;
    }
    entry.agents.insert(agent_name.to_string());
}

fn scan_claude_workspaces(map: &mut HashMap<String, WorkspaceAccumulator>) {
    let Some(home) = super::home_dir() else {
        return;
    };
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.is_dir() {
        return;
    }

    let Ok(entries) = fs::read_dir(&projects_dir) else {
        return;
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
            continue;
        }

        let dir_path = entry.path();
        let dir_name = entry.file_name().to_string_lossy().to_string();

        let Ok(files) = fs::read_dir(&dir_path) else {
            continue;
        };

        let mut jsonls = Vec::new();
        for file in files.flatten() {
            let p = file.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && file.file_type().map(|k| k.is_file()).unwrap_or(false)
            {
                let modified = file
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                jsonls.push((modified, p));
            }
        }

        if jsonls.is_empty() {
            continue;
        }

        // Sort descending by modified time to check the most recent session
        jsonls.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
        let latest_mtime = jsonls.first().map(|(m, _)| *m).unwrap_or(0);
        let total_sessions = jsonls.len();

        // Extract real CWD from latest session jsonl
        let mut extracted_cwd = None;
        if let Some((_, latest_path)) = jsonls.first() {
            if let Ok(file) = fs::File::open(latest_path) {
                let mut reader = BufReader::new(file);
                for _ in 0..64 {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if let Some(pos) = line.find(r#""cwd":""#) {
                        let rest = &line[pos + 7..];
                        if let Some(end_quote) = rest.find('"') {
                            let unescaped = rest[..end_quote].replace(r"\\", r"\");
                            if !unescaped.trim().is_empty() {
                                extracted_cwd = Some(unescaped);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Fallback: decode directory name if cwd not found in sample lines
        let final_path = extracted_cwd.unwrap_or_else(|| {
            if dir_name.len() >= 3
                && dir_name.as_bytes()[1] == b'-'
                && dir_name.as_bytes()[2] == b'-'
            {
                let drive = dir_name.chars().next().unwrap().to_ascii_uppercase();
                let rest = &dir_name[3..];
                format!("{}:\\{}", drive, rest.replace('-', "\\"))
            } else {
                dir_name
            }
        });

        add_workspace_record(
            map,
            &final_path,
            total_sessions,
            latest_mtime,
            "Claude Code",
        );
    }
}

fn scan_zcode_workspaces(map: &mut HashMap<String, WorkspaceAccumulator>) {
    let Some(home) = super::home_dir() else {
        return;
    };
    let db_path = home.join(".zcode").join("cli").join("db").join("db.sqlite");
    if !db_path.is_file() {
        return;
    }

    let Ok(conn) = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return;
    };

    // Query session table for directory distribution
    let mut stmt = match conn.prepare(
        "SELECT directory, count(*), max(time_updated) FROM session \
         WHERE directory IS NOT NULL AND directory != '' \
         GROUP BY directory",
    ) {
        Ok(s) => s,
        Err(_) => return,
    };

    let rows = stmt.query_map([], |row| {
        let dir: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        let max_time: Option<i64> = row.get(2)?;
        Ok((dir, count as usize, max_time.unwrap_or(0)))
    });

    if let Ok(rows) = rows {
        for row in rows.flatten() {
            let (dir, count, updated_at) = row;
            add_workspace_record(map, &dir, count, updated_at, "ZCode");
        }
    }
}

fn scan_codex_workspaces(map: &mut HashMap<String, WorkspaceAccumulator>) {
    let Some(home) = super::home_dir() else {
        return;
    };
    let db_path = home.join(".codex").join("state_5.db");
    if !db_path.is_file() {
        return;
    }

    let Ok(conn) = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return;
    };

    let mut stmt = match conn.prepare(
        "SELECT cwd, count(*), max(updated_at) FROM threads \
         WHERE cwd IS NOT NULL AND cwd != '' \
         GROUP BY cwd",
    ) {
        Ok(s) => s,
        Err(_) => return,
    };

    let rows = stmt.query_map([], |row| {
        let cwd: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        let max_time: Option<i64> = row.get(2)?;
        Ok((cwd, count as usize, max_time.unwrap_or(0)))
    });

    if let Ok(rows) = rows {
        for row in rows.flatten() {
            let (cwd, count, updated_at) = row;
            add_workspace_record(map, &cwd, count, updated_at, "Codex");
        }
    }
}

fn scan_antigravity_workspaces(map: &mut HashMap<String, WorkspaceAccumulator>) {
    let Some(home) = super::home_dir() else {
        return;
    };
    let gemini_brain = home.join(".gemini").join("antigravity").join("brain");
    let brain_dir = if gemini_brain.is_dir() {
        gemini_brain
    } else {
        let direct = home.join(".antigravity").join("brain");
        if direct.is_dir() {
            direct
        } else {
            return;
        }
    };

    let Ok(entries) = fs::read_dir(&brain_dir) else {
        return;
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|k| k.is_dir()).unwrap_or(false) {
            continue;
        }
        let transcript = entry
            .path()
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl");
        if !transcript.is_file() {
            continue;
        }

        let mtime = transcript
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        if let Ok((_, Some(cwd), _)) =
            super::read_antigravity_transcript_sample(&transcript, "antigravity")
        {
            add_workspace_record(map, &cwd, 1, mtime, "Antigravity");
        }
    }
}

fn scan_grok_workspaces(map: &mut HashMap<String, WorkspaceAccumulator>) {
    let Some(home) = super::home_dir() else {
        return;
    };
    let sessions_dir = home.join(".grok").join("sessions");
    if !sessions_dir.is_dir() {
        return;
    }

    let Ok(groups) = fs::read_dir(&sessions_dir) else {
        return;
    };

    for group in groups.flatten() {
        if !group.file_type().map(|k| k.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(children) = fs::read_dir(group.path()) else {
            continue;
        };
        for child in children.flatten() {
            if !child.file_type().map(|k| k.is_dir()).unwrap_or(false) {
                continue;
            }
            let summary_path = child.path().join("summary.json");
            if !summary_path.is_file() {
                continue;
            }
            let mtime = summary_path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);

            if let Ok(content) = fs::read_to_string(&summary_path) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(cwd) = val.pointer("/info/cwd").and_then(serde_json::Value::as_str)
                    {
                        if !cwd.trim().is_empty() {
                            add_workspace_record(map, cwd, 1, mtime, "Grok Build");
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_workspace_path() {
        assert_eq!(
            normalize_workspace_path(r"D:\Claudespace\Project\agenthub"),
            Some(r"D:\Claudespace\Project\agenthub".to_string())
        );
        assert_eq!(
            normalize_workspace_path(r"\\?\D:\Claudespace\Project\agenthub\"),
            Some(r"D:\Claudespace\Project\agenthub".to_string())
        );
        assert_eq!(
            normalize_workspace_path(r"d:/claudespace/project/agenthub"),
            Some(r"D:\Claudespace\Project\agenthub".to_string())
        );
        assert_eq!(normalize_workspace_path("."), None);
        assert_eq!(normalize_workspace_path("   "), None);
    }

    #[test]
    fn test_derive_workspace_name() {
        assert_eq!(
            derive_workspace_name(r"D:\Claudespace\Project\agenthub"),
            "agenthub"
        );
        assert_eq!(derive_workspace_name(r"C:\Users\tanner"), "tanner");
    }

    #[test]
    fn test_format_relative_time() {
        let now_ms = Utc::now().timestamp_millis();
        assert_eq!(format_relative_time(now_ms - 10_000), "刚刚");
        assert_eq!(format_relative_time(now_ms - 120_000), "2 分钟前");
        assert_eq!(format_relative_time(now_ms - 7_200_000), "2 小时前");
        assert_eq!(format_relative_time(now_ms - 86_400_000 * 3), "3 天前");
    }
}
