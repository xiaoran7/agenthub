use super::*;

/// Incrementally reads Antigravity
/// (`~/.gemini/antigravity(-cli)/brain/*/.system_generated/logs/transcript*.jsonl`)
/// call records (port of Magpie's `calls_agy.go`).
pub(crate) fn update_extra_client_calls(home: &Path, routed: &HashMap<String, String>) -> bool {
    let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();

    for ag_root in [
        home.join(".gemini").join("antigravity"),
        home.join(".gemini").join("antigravity-cli"),
        home.join(".antigravity"),
    ] {
        let brain = ag_root.join("brain");
        if let Ok(conv_dirs) = fs::read_dir(&brain) {
            for conv in conv_dirs.flatten() {
                let logs_dir = conv.path().join(".system_generated").join("logs");
                let full = logs_dir.join("transcript_full.jsonl");
                let plain = logs_dir.join("transcript.jsonl");
                if full.is_file() {
                    candidates.push((full, "antigravity"));
                } else if plain.is_file() {
                    candidates.push((plain, "antigravity"));
                }
            }
        }
    }

    let mut file_marks = Vec::with_capacity(candidates.len());
    for (path, kind) in candidates {
        if let Some((len, modified_ms)) = live_file_mark(&path) {
            file_marks.push((path, kind, FileMark { len, modified_ms }));
        }
    }

    if let Ok(guard) = ledger_cache().read()
        && guard.walked_at.is_some()
        && guard.other_files.len() == file_marks.len()
        && file_marks
            .iter()
            .all(|(p, _, mark)| guard.other_files.get(p).is_some_and(|(old, _)| old == mark))
    {
        return false;
    }

    let Ok(mut guard) = ledger_cache().write() else {
        return false;
    };
    let mark_set: HashSet<&Path> = file_marks.iter().map(|(p, ..)| p.as_path()).collect();
    let mut any_changed = guard.other_files.len() != file_marks.len();
    guard
        .other_files
        .retain(|p, _| mark_set.contains(p.as_path()));
    any_changed |= guard.remove_missing_cold_paths("other", &mark_set);

    for (path, kind, mark) in file_marks {
        if let Some((old_mark, _)) = guard.other_files.get(&path) {
            if *old_mark == mark {
                continue;
            }
        }
        if !guard.other_files.contains_key(&path)
            && guard.cold_manifest.get(&path).is_some_and(|m| {
                m.source == "other" && m.file_len == mark.len && m.modified_ms == mark.modified_ms
            })
        {
            guard.other_files.insert(path, (mark, Vec::new()));
            any_changed = true;
            continue;
        }
        guard.mark_cold_path_dirty(&path);
        let records = match kind {
            "antigravity" => read_antigravity_calls(&path, home, routed),
            _ => Vec::new(),
        };
        guard.other_files.insert(path, (mark, records));
        any_changed = true;
    }
    any_changed
}

pub(crate) fn read_antigravity_calls(
    path: &Path,
    home: &Path,
    routed: &HashMap<String, String>,
) -> Vec<LedgerRecord> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let ag_home = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.parent());
    let session = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("agy")
        .to_owned();
    #[cfg(not(test))]
    let default_model = crate::collectors::antigravity::query_trajectory_model(&session, 0)
        .unwrap_or_else(|| {
            crate::collectors::antigravity::resolve_antigravity_model_for_home(home, ag_home)
        });
    #[cfg(test)]
    let default_model =
        crate::collectors::antigravity::resolve_antigravity_model_for_home(home, ag_home);

    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut session_cwd = String::new();
    let mut active_settings_model: Option<String> = None;
    let mut by_step: std::collections::BTreeMap<i64, LedgerRecord> = Default::default();
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
        let need_cwd = session_cwd.is_empty();
        let has_model_switch = contains(&line, b"Model Selection");
        let maybe_model_call =
            contains(&line, b"\"output_tokens\"") && contains(&line, b"\"MODEL\"");
        if !need_cwd && !has_model_switch && !maybe_model_call {
            continue;
        }
        let seed = crate::collectors::antigravity::AntigravityLineSeed {
            need_cwd,
            need_title: false,
        };
        let mut de = serde_json::Deserializer::from_slice(&line);
        let Ok(parsed) = serde::de::DeserializeSeed::deserialize(seed, &mut de) else {
            continue;
        };
        if let Some(sm) = parsed.settings_model {
            active_settings_model = Some(sm);
        }
        if session_cwd.is_empty()
            && let Some(found) = parsed.cwd_from_tools.or(parsed.cwd_from_content)
        {
            session_cwd = found;
        }
        if !maybe_model_call || parsed.source.as_deref() != Some("MODEL") {
            continue;
        }
        let Some(step) = parsed.step_index else {
            continue;
        };
        let Some(ts) = parsed
            .created_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
        else {
            continue;
        };
        let input = parsed.input_tokens;
        let output = parsed.output_tokens;
        let cache_read = parsed.cache_read_tokens;
        if input == 0 && output == 0 && cache_read == 0 {
            continue;
        }
        let model = parsed
            .model
            .as_deref()
            .or(parsed.model_version.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or(active_settings_model.as_deref())
            .unwrap_or(&default_model)
            .to_owned();
        let provider = resolve_provider_label("antigravity", &model, routed);
        by_step.insert(
            step,
            LedgerRecord {
                t: ts,
                asked: ts,
                agent: "antigravity",
                provider,
                model: intern(&model),
                session: intern(&session),
                cwd: intern(&session_cwd),
                input,
                output,
                cache_read,
                cache_write: 0,
                duration_ms: None,
                error: None,
                cost: calc_cost(&model, input.saturating_add(cache_read), output, cache_read),
                retried: false,
                recovered: false,
            },
        );
    }
    by_step.into_values().collect()
}
