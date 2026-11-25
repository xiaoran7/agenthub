use std::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    fmt, fs,
    io::BufReader,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use super::super::*;
use super::config::decode_file_uri_to_path;

/// Prefers `transcript_full.jsonl` (untruncated history) and falls back to `transcript.jsonl`.
pub(crate) fn resolve_brain_transcript_path(conv_dir: &Path) -> Option<PathBuf> {
    let logs = conv_dir.join(".system_generated").join("logs");
    let full = logs.join("transcript_full.jsonl");
    if full.is_file() {
        return Some(full);
    }
    let plain = logs.join("transcript.jsonl");
    plain.is_file().then_some(plain)
}

pub(crate) fn is_safe_antigravity_session_id(raw_id: &str) -> bool {
    validate_safe_session_id(raw_id) && raw_id != "." && !raw_id.contains(':')
}

pub(crate) fn resolve_brain_session_transcript(
    ag_home: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    if !is_safe_antigravity_session_id(session_id) {
        return None;
    }
    resolve_brain_transcript_path(&ag_home.join("brain").join(session_id))
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AntigravityTranscriptMetrics {
    pub started_at_ms: Option<i64>,
    pub last_activity_ms: Option<i64>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    pub context_used_tokens: u64,
    pub request_count: u64,
}

type CachedMetricsMap = BoundedLruCache<PathBuf, (u64, i64, AntigravityTranscriptMetrics)>;

fn antigravity_metrics_cache() -> &'static RwLock<CachedMetricsMap> {
    static CACHE: OnceLock<RwLock<CachedMetricsMap>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BoundedLruCache::new(2_048)))
}

pub(crate) fn clear_antigravity_transcript_cache() {
    if let Ok(mut guard) = antigravity_metrics_cache().write() {
        guard.clear();
    }
}

/// Strips Antigravity `<USER_REQUEST>` / `<ADDITIONAL_METADATA>` / `<SYSTEM_MESSAGE>` wrappers before extracting title.
pub(crate) fn clean_antigravity_title(raw: &str) -> Option<String> {
    let mut text = raw.to_owned();
    if let Some(start) = text.find("<USER_REQUEST>") {
        let after = start + "<USER_REQUEST>".len();
        if let Some(end_rel) = text[after..].find("</USER_REQUEST>") {
            text = text[after..after + end_rel].to_owned();
        }
    }
    for (open, close) in [
        ("<ADDITIONAL_METADATA>", "</ADDITIONAL_METADATA>"),
        ("<SYSTEM_MESSAGE>", "</SYSTEM_MESSAGE>"),
        ("<USER_SETTINGS_CHANGE>", "</USER_SETTINGS_CHANGE>"),
    ] {
        while let Some(start) = text.find(open) {
            if let Some(end) = text[start..].find(close) {
                text.replace_range(start..start + end + close.len(), "");
            } else {
                text.truncate(start);
                break;
            }
        }
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    local_prompt_title(trimmed)
}

const CWD_DIRECT_KEYS: [&str; 6] = [
    "Cwd",
    "cwd",
    "WorkingDirectory",
    "SearchDirectory",
    "TargetDir",
    "Path",
];
const CWD_PARENT_KEYS: [&str; 5] = [
    "TargetFile",
    "AbsolutePath",
    "file_path",
    "path",
    "DirectoryPath",
];

/// Extracts model label from `<USER_SETTINGS_CHANGE>The user changed setting `Model Selection` from ... to <Label>.`
pub(crate) fn extract_model_selection_from_content(content: &str) -> Option<String> {
    let mut search = content;
    let mut latest = None;
    const MARKER: &str = "changed setting `Model Selection` from ";
    while let Some(pos) = search.find(MARKER) {
        let after = &search[pos + MARKER.len()..];
        if let Some(to_pos) = after.find(" to ") {
            let rest = &after[to_pos + 4..];
            let end = rest
                .find(". ")
                .or_else(|| rest.find(".\n"))
                .or_else(|| rest.find(".\r"))
                .or_else(|| rest.find("</USER_SETTINGS_CHANGE>"))
                .or_else(|| rest.find('\n'))
                .unwrap_or(rest.len());
            let raw_label = rest[..end].trim().trim_end_matches('.');
            if let Some(resolved) = super::resolve_antigravity_model_reference(raw_label) {
                latest = Some(resolved);
            }
            search = &rest[end..];
        } else {
            break;
        }
    }
    latest
}

fn extract_cwd_from_content(content: &str) -> Option<String> {
    if let Some(pos) =
        content.find("The mapping is shown as follows in the format [URI] -> [CorpusName]:")
    {
        let tail = &content[pos..];
        if let Some(line) = tail.lines().nth(1)
            && let Some(first) = line.split("->").next()
        {
            let cleaned = first.trim().trim_matches(|c| c == '[' || c == ']');
            if let Some(decoded) = decode_file_uri_to_path(cleaned) {
                return Some(decoded);
            }
        }
    }
    for prefix in [
        "file:///", "d:/", "D:/", "c:/", "C:/", "d:\\", "D:\\", "c:\\", "C:\\", "/Users/",
        "/home/", "/www/",
    ] {
        if let Some(pos) = content.find(prefix) {
            let tail = &content[pos..];
            let end_pos = tail
                .find(['\n', '\r', ' ', '"', '\'', '>', '`', ']', ')'])
                .unwrap_or(tail.len());
            let extracted = tail[..end_pos].trim();
            if let Some(decoded) = decode_file_uri_to_path(extracted) {
                return Some(decoded);
            }
        }
    }
    None
}

#[derive(Default)]
struct ToolParamsCwd(Option<String>);

impl<'de> serde::Deserialize<'de> for ToolParamsCwd {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ParamsVisitor;

        impl<'de> Visitor<'de> for ParamsVisitor {
            type Value = ToolParamsCwd;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("tool call parameter map or ignored value")
            }

            serde_ignore_non_map!();

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut best_rank = usize::MAX;
                let mut best_val: Option<String> = None;
                while let Some(key) = map.next_key::<Cow<'de, str>>()? {
                    if let Some(pos) = CWD_DIRECT_KEYS.iter().position(|k| *k == key.as_ref()) {
                        let val = map.next_value::<JsonOptStr<'de>>()?;
                        if pos < best_rank
                            && let Some(raw) = val.as_deref().map(str::trim)
                            && !raw.is_empty()
                        {
                            best_rank = pos;
                            best_val = Some(raw.to_owned());
                        }
                    } else if let Some(pos) =
                        CWD_PARENT_KEYS.iter().position(|k| *k == key.as_ref())
                    {
                        let rank = CWD_DIRECT_KEYS.len() + pos;
                        let val = map.next_value::<JsonOptStr<'de>>()?;
                        if rank < best_rank
                            && let Some(path_str) = val.as_deref()
                            && let Some(parent) = Path::new(path_str).parent()
                        {
                            let s = parent.to_string_lossy();
                            let trimmed = s.trim();
                            if !trimmed.is_empty() {
                                best_rank = rank;
                                best_val = Some(trimmed.to_owned());
                            }
                        }
                    } else {
                        let _ = map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(ToolParamsCwd(best_val))
            }
        }

        deserializer.deserialize_any(ParamsVisitor)
    }
}

#[derive(Default)]
struct ToolCallsCwd(Option<String>);

impl<'de> serde::Deserialize<'de> for ToolCallsCwd {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Default, serde::Deserialize)]
        #[serde(default)]
        struct Entry {
            args: Option<ToolParamsCwd>,
            parameters: Option<ToolParamsCwd>,
            arguments: Option<ToolParamsCwd>,
        }

        struct CallsVisitor;

        impl<'de> Visitor<'de> for CallsVisitor {
            type Value = ToolCallsCwd;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("tool_calls sequence or ignored value")
            }

            serde_ignore_non_seq!();

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut found = None;
                while let Some(entry) = seq.next_element::<Entry>()? {
                    let cwd = entry
                        .args
                        .and_then(|p| p.0)
                        .or_else(|| entry.parameters.and_then(|p| p.0))
                        .or_else(|| entry.arguments.and_then(|p| p.0));
                    if let Some(cwd) = cwd {
                        found = Some(cwd);
                        while seq.next_element::<IgnoredAny>()?.is_some() {}
                        break;
                    }
                }
                Ok(ToolCallsCwd(found))
            }
        }

        deserializer.deserialize_any(CallsVisitor)
    }
}

pub(crate) struct ParsedAntigravityLine<'a> {
    pub(crate) created_at: Option<Cow<'a, str>>,
    pub(crate) step_type: Option<Cow<'a, str>>,
    pub(crate) source: Option<Cow<'a, str>>,
    pub(crate) model: Option<Cow<'a, str>>,
    pub(crate) model_version: Option<Cow<'a, str>>,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cache_read_tokens: u64,
    pub(crate) step_index: Option<i64>,
    pub(crate) cwd_from_tools: Option<String>,
    pub(crate) cwd_from_content: Option<String>,
    pub(crate) title_from_content: Option<String>,
    pub(crate) settings_model: Option<String>,
}

pub(crate) struct AntigravityLineSeed {
    pub(crate) need_cwd: bool,
    pub(crate) need_title: bool,
}

impl<'de> Visitor<'de> for AntigravityLineSeed {
    type Value = ParsedAntigravityLine<'de>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Antigravity transcript step JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut created_at: Option<Cow<'de, str>> = None;
        let mut step_type: Option<Cow<'de, str>> = None;
        let mut source: Option<Cow<'de, str>> = None;
        let mut model: Option<Cow<'de, str>> = None;
        let mut model_version: Option<Cow<'de, str>> = None;
        let mut input_tokens = 0_u64;
        let mut output_tokens = 0_u64;
        let mut cache_read_tokens = 0_u64;
        let mut step_index: Option<i64> = None;
        let mut cwd_from_tools: Option<String> = None;
        let mut cwd_from_content: Option<String> = None;
        let mut title_from_content: Option<String> = None;
        let mut settings_model: Option<String> = None;
        let mut deferred_content: Option<Cow<'de, str>> = None;

        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            match key.as_ref() {
                "created_at" => created_at = map.next_value::<JsonOptStr<'de>>()?.0,
                "type" => step_type = map.next_value::<JsonOptStr<'de>>()?.0,
                "source" => source = map.next_value::<JsonOptStr<'de>>()?.0,
                "model" => model = map.next_value::<JsonOptStr<'de>>()?.0,
                "modelVersion" => model_version = map.next_value::<JsonOptStr<'de>>()?.0,
                "input_tokens" => {
                    input_tokens = map.next_value::<JsonOptU64>()?.0.unwrap_or(0);
                }
                "output_tokens" => {
                    output_tokens = map.next_value::<JsonOptU64>()?.0.unwrap_or(0);
                }
                "cache_read_tokens" => {
                    cache_read_tokens = map.next_value::<JsonOptU64>()?.0.unwrap_or(0);
                }
                "step_index" => {
                    step_index = map.next_value::<JsonOptI64>()?.0;
                }
                "tool_calls" if self.need_cwd => {
                    cwd_from_tools = map.next_value::<ToolCallsCwd>()?.0;
                }
                "content"
                    if self.need_cwd || step_type.as_deref().is_none_or(|t| t == "USER_INPUT") =>
                {
                    if let Some(text) = map.next_value::<JsonOptStr<'de>>()?.0 {
                        if text.contains("Model Selection") {
                            settings_model = extract_model_selection_from_content(text.as_ref())
                                .or(settings_model);
                        }
                        if self.need_cwd && cwd_from_content.is_none() {
                            cwd_from_content = extract_cwd_from_content(text.as_ref());
                        }
                        if self.need_title {
                            match step_type.as_deref() {
                                Some("USER_INPUT") => {
                                    title_from_content = clean_antigravity_title(text.as_ref());
                                }
                                None => {
                                    deferred_content = Some(text);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                }
            }
        }

        if self.need_title
            && title_from_content.is_none()
            && step_type.as_deref() == Some("USER_INPUT")
            && let Some(text) = deferred_content
        {
            title_from_content = clean_antigravity_title(text.as_ref());
        }

        Ok(ParsedAntigravityLine {
            created_at,
            step_type,
            source,
            model,
            model_version,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            step_index,
            cwd_from_tools,
            cwd_from_content,
            title_from_content,
            settings_model,
        })
    }
}

impl<'de> DeserializeSeed<'de> for AntigravityLineSeed {
    type Value = ParsedAntigravityLine<'de>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}

/// Reads and caches token, request, model, title, and CWD metrics from one `transcript*.jsonl` file.
pub(crate) fn read_antigravity_transcript_metrics(
    path: &Path,
) -> Result<AntigravityTranscriptMetrics, CollectorError> {
    let meta = fs::metadata(path).map_err(|source| CollectorError::Io {
        program: "Antigravity transcript metrics".to_owned(),
        source,
    })?;
    let len = meta.len();
    let mtime_ms = file_modified_millis(path).unwrap_or(0);

    if let Ok(guard) = antigravity_metrics_cache().read()
        && let Some((c_len, c_mtime, metrics)) = guard.get(path)
        && *c_len == len
        && *c_mtime == mtime_ms
    {
        return Ok(metrics.clone());
    }

    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: "Antigravity transcript metrics".to_owned(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut metrics = AntigravityTranscriptMetrics::default();
    let mut by_step: BTreeMap<i64, (u64, u64, u64)> = BTreeMap::new();
    let mut fallback_step_counter = 0_i64;
    let mut line_no = 0_usize;

    loop {
        if line_no >= 50_000 {
            break;
        }
        line_no += 1;
        let line = match read_bounded_line(&mut reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => continue,
            BoundedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: "Antigravity transcript metrics".to_owned(),
                    source,
                });
            }
        };
        let mut de = serde_json::Deserializer::from_str(&line);
        let Ok(parsed) = (AntigravityLineSeed {
            need_cwd: metrics.cwd.is_none(),
            need_title: metrics.title.is_none(),
        })
        .deserialize(&mut de) else {
            continue;
        };

        if let Some(ts) = parse_rfc3339_millis(parsed.created_at.as_deref()) {
            if metrics.started_at_ms.is_none() {
                metrics.started_at_ms = Some(ts);
            }
            metrics.last_activity_ms = Some(ts);
        }

        if metrics.cwd.is_none() {
            metrics.cwd = parsed.cwd_from_tools.or(parsed.cwd_from_content);
        }

        if metrics.title.is_none() && parsed.step_type.as_deref() == Some("USER_INPUT") {
            metrics.title = parsed.title_from_content;
        }
        if let Some(sm) = parsed.settings_model {
            metrics.model = Some(sm);
        }

        if parsed.source.as_deref() == Some("MODEL") {
            if let Some(m) = parsed
                .model
                .as_deref()
                .or(parsed.model_version.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                metrics.model = Some(m.to_owned());
            }

            let in_tok = parsed.input_tokens;
            let out_tok = parsed.output_tokens;
            let cache_tok = parsed.cache_read_tokens;
            if in_tok > 0 || out_tok > 0 || cache_tok > 0 {
                let step_key = parsed.step_index.unwrap_or_else(|| {
                    fallback_step_counter -= 1;
                    fallback_step_counter
                });
                let total_turn_prompt = in_tok.saturating_add(cache_tok);
                by_step.insert(step_key, (total_turn_prompt, out_tok, cache_tok));
                if total_turn_prompt > 0 {
                    metrics.context_used_tokens = total_turn_prompt;
                }
            }
        }
    }

    metrics.request_count = by_step.len() as u64;
    for (pt, ct, cr) in by_step.into_values() {
        metrics.prompt_tokens = metrics.prompt_tokens.saturating_add(pt);
        metrics.completion_tokens = metrics.completion_tokens.saturating_add(ct);
        metrics.cache_read_tokens = metrics.cache_read_tokens.saturating_add(cr);
    }

    if let Ok(mut guard) = antigravity_metrics_cache().write() {
        guard.insert(path.to_path_buf(), (len, mtime_ms, metrics.clone()));
    }

    Ok(metrics)
}

/// Extracts workspace/working directory from an Antigravity transcript step (`args`, `parameters`, or corpus mapping).
pub fn extract_antigravity_cwd(value: &Value) -> Option<String> {
    if let Some(tool_calls) = value.get("tool_calls").and_then(Value::as_array) {
        for tc in tool_calls {
            let params = tc
                .get("args")
                .or_else(|| tc.get("parameters"))
                .or_else(|| tc.get("arguments"));
            if let Some(params) = params {
                for key in CWD_DIRECT_KEYS {
                    if let Some(raw) = params.get(key).and_then(Value::as_str) {
                        let trimmed = raw.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_owned());
                        }
                    }
                }
                for field in CWD_PARENT_KEYS {
                    if let Some(path_str) = params.get(field).and_then(Value::as_str) {
                        let path = Path::new(path_str);
                        if let Some(parent) = path.parent() {
                            let s = parent.to_string_lossy();
                            if !s.trim().is_empty() {
                                return Some(s.trim().to_owned());
                            }
                        }
                    }
                }
            }
        }
    }
    value
        .get("content")
        .and_then(Value::as_str)
        .and_then(extract_cwd_from_content)
}

pub(crate) type AntigravitySampleResult = (Option<i64>, Option<String>, Vec<Value>);

pub(crate) fn read_antigravity_transcript_sample(
    path: &Path,
    program: &str,
) -> Result<AntigravitySampleResult, CollectorError> {
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    let mut started_at_ms = None;
    let mut cwd = None;
    let mut recent_steps: VecDeque<Value> = VecDeque::with_capacity(32);

    for _ in 0..64 {
        let line = match read_bounded_line(&mut reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => continue,
            BoundedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: program.to_owned(),
                    source,
                });
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if started_at_ms.is_none() {
            started_at_ms = parse_rfc3339_millis(value.get("created_at").and_then(Value::as_str));
        }
        if cwd.is_none() {
            cwd = extract_antigravity_cwd(&value);
        }
        if recent_steps.len() >= 32 {
            recent_steps.pop_front();
        }
        recent_steps.push_back(value);
    }

    if cwd.is_none() {
        for step in recent_steps.iter().rev() {
            if let Some(found_cwd) = extract_antigravity_cwd(step) {
                cwd = Some(found_cwd);
                break;
            }
        }
    }

    Ok((started_at_ms, cwd, recent_steps.into_iter().collect()))
}
