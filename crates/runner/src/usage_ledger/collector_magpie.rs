use super::*;

/// Reads Magpie's own gateway request log (`~/.config/magpie/usage.jsonl` or
/// `%APPDATA%/magpie/usage.jsonl`) so requests proxied through Magpie (including
/// SSH remote sessions, custom Gemini 4 routes, or Antigravity) are captured
/// with the exact model that answered (port of Magpie's `internal/usage/usage.go`).
pub(crate) fn update_magpie_gateway_calls(home: &Path, routed: &HashMap<String, String>) -> bool {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |p: PathBuf| {
        if p.is_file() {
            let canon = dunce::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if seen.insert(canon) {
                paths.push(p);
            }
        }
    };
    push(home.join(".config").join("magpie").join("usage.jsonl"));
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        push(PathBuf::from(xdg).join("magpie").join("usage.jsonl"));
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
            push(PathBuf::from(appdata).join("magpie").join("usage.jsonl"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            push(PathBuf::from(local).join("magpie").join("usage.jsonl"));
        }
    }

    if paths.is_empty() {
        return false;
    }

    let mut file_marks = Vec::with_capacity(paths.len());
    for path in paths {
        if let Some((len, modified_ms)) = live_file_mark(&path) {
            file_marks.push((path, FileMark { len, modified_ms }));
        }
    }

    if let Ok(guard) = ledger_cache().read()
        && guard.walked_at.is_some()
        && guard.magpie_gateway.len() == file_marks.len()
        && file_marks.iter().all(|(p, mark)| {
            guard
                .magpie_gateway
                .get(p)
                .is_some_and(|(old, _)| old == mark)
        })
    {
        return false;
    }

    let Ok(mut guard) = ledger_cache().write() else {
        return false;
    };
    let mark_set: HashSet<&Path> = file_marks.iter().map(|(p, _)| p.as_path()).collect();
    let mut any_changed = guard.magpie_gateway.len() != file_marks.len();
    guard
        .magpie_gateway
        .retain(|p, _| mark_set.contains(p.as_path()));

    for (path, mark) in file_marks {
        if let Some((old_mark, _)) = guard.magpie_gateway.get(&path) {
            if *old_mark == mark {
                continue;
            }
        }
        let records = read_magpie_usage_log(&path, routed);
        guard.magpie_gateway.insert(path, (mark, records));
        any_changed = true;
    }
    any_changed
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct MagpieUsageLine<'a> {
    rejected: crate::collectors::JsonOptBool,
    handed_on: crate::collectors::JsonOptBool,
    #[serde(borrow)]
    t: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    agent: crate::collectors::JsonOptStr<'a>,
    status: crate::collectors::JsonOptI64,
    #[serde(borrow)]
    err_type: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    err: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    served: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    model: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    req: crate::collectors::JsonOptStr<'a>,
    #[serde(rename = "in")]
    input: crate::collectors::JsonOptU64,
    out: crate::collectors::JsonOptU64,
    cache_read: crate::collectors::JsonOptU64,
    cache_write: crate::collectors::JsonOptU64,
    ms: crate::collectors::JsonOptU64,
    #[serde(borrow)]
    session: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    native_session: crate::collectors::JsonOptStr<'a>,
    #[serde(borrow)]
    provider: crate::collectors::JsonOptStr<'a>,
}

pub(crate) fn read_magpie_usage_log(
    path: &Path,
    routed: &HashMap<String, String>,
) -> Vec<LedgerRecord> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut out = Vec::new();

    loop {
        line.clear();
        if line.capacity() > 512 * 1024 {
            line = Vec::new();
        }
        match read_bounded_ledger_line(&mut reader, &mut line, LEDGER_LINE_LIMIT) {
            Ok((0, _, _)) => break,
            Ok((_, _, true)) => continue,
            Ok(_) => {}
            Err(_) => break,
        }
        if line.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_slice::<MagpieUsageLine>(&line) else {
            continue;
        };
        if val.rejected.0 == Some(true) || val.handed_on.0 == Some(true) {
            continue;
        }
        let Some(t) = val
            .t
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
        else {
            continue;
        };
        let raw_agent = val.agent.as_deref().unwrap_or("").to_ascii_lowercase();
        let agent: &'static str = match raw_agent.as_str() {
            "claude" | "claude_code" | "claude-code" => "claude_code",
            "claude-desktop" | "claude_desktop" => "claude_desktop",
            "codex" => "codex",
            "gemini" | "gemini-cli" | "gemini_cli" => "gemini_cli",
            "agy" | "antigravity" => "antigravity",
            "grok" | "grok_build" => "grok_build",
            "zcode" | "opencode" => "zcode",
            _ => "claude_code",
        };
        let status = val.status.0.unwrap_or(200);
        let err_str = val
            .err_type
            .as_deref()
            .or(val.err.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let failed = status >= 400 || err_str.is_some();

        let model = val
            .served
            .as_deref()
            .or(val.model.as_deref())
            .or(val.req.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_default()
            .trim_end_matches("[1m]")
            .trim_end_matches("[1M]")
            .trim()
            .to_owned();
        if model.is_empty() && !failed {
            continue;
        }
        let input = val.input.0.unwrap_or(0);
        let output = val.out.0.unwrap_or(0);
        let cache_read = val.cache_read.0.unwrap_or(0);
        let cache_write = val.cache_write.0.unwrap_or(0);
        if !failed && input == 0 && output == 0 && cache_read == 0 && cache_write == 0 {
            continue;
        }
        let ms = val.ms.0.filter(|&m| m > 0);
        let asked = ms.map(|d| t.saturating_sub(d as i64)).unwrap_or(t);
        let session = val
            .session
            .as_deref()
            .or(val.native_session.as_deref())
            .filter(|s| !s.is_empty())
            .unwrap_or("gateway")
            .to_owned();
        let provider = val
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "session-unknown")
            .map(intern)
            .unwrap_or_else(|| resolve_provider_label(agent, &model, routed));
        let error = if failed {
            Some(intern(err_str.unwrap_or("http_error")))
        } else {
            None
        };
        out.push(LedgerRecord {
            t,
            asked,
            agent,
            provider,
            model: if failed { intern("") } else { intern(&model) },
            session: intern(&session),
            cwd: intern(""),
            input,
            output,
            cache_read,
            cache_write,
            duration_ms: ms,
            error,
            cost: if failed {
                0.0
            } else {
                calc_cost(
                    &model,
                    input.saturating_add(cache_read).saturating_add(cache_write),
                    output,
                    cache_read,
                )
            },
            retried: false,
            recovered: false,
        });
    }
    out
}
