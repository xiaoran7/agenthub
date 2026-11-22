use std::{
    borrow::Cow,
    collections::VecDeque,
    fmt, fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
    time::UNIX_EPOCH,
};

use chrono::DateTime;
use serde::{
    Deserialize,
    de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;

use super::*;

const MESSAGE_USAGE_WINDOW: usize = 128;

#[derive(Debug, Clone, Default)]
pub(crate) struct BoundedMessageUsage {
    pub(crate) settled_prompt_tokens: u64,
    pub(crate) settled_completion_tokens: u64,
    pub(crate) settled_cache_read_tokens: u64,
    pub(crate) recent: VecDeque<(String, (u64, u64, u64))>,
    pub(crate) request_count: u64,
}

impl BoundedMessageUsage {
    pub(crate) fn record(&mut self, key: String, figures: (u64, u64, u64)) {
        if let Some((_, existing)) = self.recent.iter_mut().rev().find(|(k, _)| *k == key) {
            *existing = figures;
            return;
        }
        self.request_count = self.request_count.saturating_add(1);
        if self.recent.len() >= MESSAGE_USAGE_WINDOW
            && let Some((_, (prompt, output, cache_read))) = self.recent.pop_front()
        {
            self.settled_prompt_tokens = self.settled_prompt_tokens.saturating_add(prompt);
            self.settled_completion_tokens = self.settled_completion_tokens.saturating_add(output);
            self.settled_cache_read_tokens =
                self.settled_cache_read_tokens.saturating_add(cache_read);
        }
        self.recent.push_back((key, figures));
    }

    pub(crate) fn totals(&self) -> (u64, u64, u64) {
        let mut prompt = self.settled_prompt_tokens;
        let mut completion = self.settled_completion_tokens;
        let mut cache_read = self.settled_cache_read_tokens;
        for (_, (p, o, c)) in &self.recent {
            prompt = prompt.saturating_add(*p);
            completion = completion.saturating_add(*o);
            cache_read = cache_read.saturating_add(*c);
        }
        (prompt, completion, cache_read)
    }
}

pub(crate) fn normalize_claude_model(raw: &str) -> Option<String> {
    let cleaned = raw
        .trim()
        .trim_end_matches(" [1m]")
        .trim_end_matches("[1m]")
        .trim_end_matches(" [1M]")
        .trim_end_matches("[1M]")
        .trim();
    if cleaned.is_empty() {
        None
    } else {
        metadata_label(cleaned, MAX_CONFIG_VALUE_CHARS)
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ClaudeUsageFields {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) cache_read_input_tokens: Option<u64>,
    pub(crate) cache_creation_input_tokens: Option<u64>,
    pub(crate) context_window: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeContextWindowSnake {
    context_window_size: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeContextWindowCamel {
    #[serde(rename = "contextWindowSize")]
    context_window_size: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeAttachmentIdentity<'a> {
    #[serde(borrow, rename = "modelId")]
    model_id: Option<Cow<'a, str>>,
}

#[derive(Debug, Default, Deserialize)]
struct ClaudeAttachment<'a> {
    #[serde(borrow)]
    identity: Option<ClaudeAttachmentIdentity<'a>>,
}

macro_rules! ignore_non_matching_json {
    ($ret:expr) => {
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok($ret)
        }
        fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok($ret)
        }
        fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok($ret)
        }
        fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok($ret)
        }
        fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok($ret)
        }
    };
}

#[derive(Default)]
struct ContentHasText(bool);

struct StrFlagVisitor(bool);
impl<'de> Visitor<'de> for StrFlagVisitor {
    type Value = bool;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("string")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<bool, E> {
        Ok(if self.0 {
            v == "text"
        } else {
            !v.trim().is_empty()
        })
    }
    ignore_non_matching_json!(false);
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<bool, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(false)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<bool, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(false)
    }
}

struct IsTextKind(bool);
impl<'de> Deserialize<'de> for IsTextKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(d.deserialize_any(StrFlagVisitor(true))?))
    }
}

struct NonEmptyTextFlag(bool);
impl<'de> Deserialize<'de> for NonEmptyTextFlag {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(d.deserialize_any(StrFlagVisitor(false))?))
    }
}

struct ContentBlockHasText(bool);
struct ContentBlockHasTextVisitor;
impl<'de> Visitor<'de> for ContentBlockHasTextVisitor {
    type Value = ContentBlockHasText;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("block")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ContentBlockHasText, A::Error> {
        let mut is_text = false;
        let mut has_text = false;
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            match key.as_ref() {
                "type" => is_text = map.next_value::<IsTextKind>()?.0,
                "text" => has_text = map.next_value::<NonEmptyTextFlag>()?.0,
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(ContentBlockHasText(is_text && has_text))
    }
    ignore_non_matching_json!(ContentBlockHasText(false));
    fn visit_str<E: de::Error>(self, _: &str) -> Result<ContentBlockHasText, E> {
        Ok(ContentBlockHasText(false))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ContentBlockHasText, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(ContentBlockHasText(false))
    }
}
impl<'de> Deserialize<'de> for ContentBlockHasText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ContentBlockHasTextVisitor)
    }
}

struct ContentHasTextVisitor;
impl<'de> Visitor<'de> for ContentHasTextVisitor {
    type Value = ContentHasText;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("content")
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<ContentHasText, E> {
        Ok(ContentHasText(!v.trim().is_empty()))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ContentHasText, A::Error> {
        let mut found = false;
        while !found {
            let Some(block) = seq.next_element::<ContentBlockHasText>()? else {
                break;
            };
            found = block.0;
        }
        if found {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
        }
        Ok(ContentHasText(found))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ContentHasText, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(ContentHasText(false))
    }
    ignore_non_matching_json!(ContentHasText(false));
}
impl<'de> Deserialize<'de> for ContentHasText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ContentHasTextVisitor)
    }
}

#[derive(Default, Deserialize)]
struct ClaudeMessageMeta<'a> {
    #[serde(borrow)]
    id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    usage: Option<ClaudeUsageFields>,
    content: Option<ContentHasText>,
}

#[derive(Default, Deserialize)]
struct ClaudeTranscriptLine<'a> {
    #[serde(borrow, rename = "type")]
    record_type: Option<Cow<'a, str>>,
    #[serde(borrow)]
    role: Option<Cow<'a, str>>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
    #[serde(rename = "isApiErrorMessage")]
    is_api_error_message: Option<bool>,
    #[serde(borrow)]
    entrypoint: Option<Cow<'a, str>>,
    context_window: Option<ClaudeContextWindowSnake>,
    #[serde(rename = "contextWindow")]
    context_window_camel: Option<ClaudeContextWindowCamel>,
    #[serde(borrow)]
    attachment: Option<ClaudeAttachment<'a>>,
    #[serde(borrow)]
    message: Option<ClaudeMessageMeta<'a>>,
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    #[serde(borrow, rename = "requestedModel")]
    requested_model: Option<Cow<'a, str>>,
    content: Option<ContentHasText>,
    #[serde(borrow, rename = "sessionId")]
    session_id: Option<Cow<'a, str>>,
    #[serde(borrow)]
    cwd: Option<Cow<'a, str>>,
    #[serde(borrow)]
    timestamp: Option<Cow<'a, str>>,
    #[serde(borrow)]
    title: Option<Cow<'a, str>>,
    #[serde(borrow, rename = "aiTitle")]
    ai_title: Option<Cow<'a, str>>,
    #[serde(borrow, rename = "customTitle")]
    custom_title: Option<Cow<'a, str>>,
    #[serde(borrow, rename = "lastPrompt")]
    last_prompt: Option<Cow<'a, str>>,
}

/// True when a transcript record is real assistant output rather than a prompt,
/// a tool result, or provider bookkeeping. Used only to decide whether a
/// session is worth listing; the record body is never retained.
pub(crate) fn is_assistant_output(value: &Value) -> bool {
    let record_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let role = value
        .pointer("/message/role")
        .or_else(|| value.get("role"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let is_assistant =
        record_type == "assistant" || (record_type.is_empty() && role == "assistant");
    if !is_assistant
        || value.get("isSidechain").and_then(Value::as_bool) == Some(true)
        || value.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true)
        || value.pointer("/message/model").and_then(Value::as_str) == Some("<synthetic>")
    {
        return false;
    }
    value
        .pointer("/message/content")
        .or_else(|| value.get("content"))
        .and_then(|content| match content {
            Value::String(text) => Some(!text.trim().is_empty()),
            Value::Array(blocks) => Some(blocks.iter().any(|block| {
                block.get("type").and_then(Value::as_str) == Some("text")
                    && block
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
            })),
            _ => None,
        })
        .unwrap_or(false)
}

fn record_is_assistant_output(record: &ClaudeTranscriptLine<'_>) -> bool {
    let record_type = record.record_type.as_deref().unwrap_or_default();
    let role = record
        .message
        .as_ref()
        .and_then(|m| m.role.as_deref())
        .or(record.role.as_deref())
        .unwrap_or_default();
    let is_assistant =
        record_type == "assistant" || (record_type.is_empty() && role == "assistant");
    if !is_assistant
        || record.is_sidechain == Some(true)
        || record.is_api_error_message == Some(true)
        || record.message.as_ref().and_then(|m| m.model.as_deref()) == Some("<synthetic>")
    {
        return false;
    }
    record
        .message
        .as_ref()
        .and_then(|m| m.content.as_ref())
        .or(record.content.as_ref())
        .is_some_and(|content| content.0)
}

/// Incremental per-file reader state for Claude transcript metadata.
#[derive(Debug, Clone, Default)]
pub(crate) struct TranscriptMetaState {
    offset: u64,
    modified_ms: Option<i64>,
    session_id: Option<String>,
    cwd: Option<String>,
    title: Option<String>,
    started_at_ms: Option<i64>,
    has_output: bool,
    model: Option<String>,
    context_used_tokens: u64,
    explicit_context_window: Option<u64>,
    usage: BoundedMessageUsage,
    desktop: bool,
}

impl TranscriptMetaState {
    fn materialize(
        &self,
        path: &Path,
        last_activity_at_ms: Option<i64>,
    ) -> Option<ClaudeTranscriptEntry> {
        let session_id = self
            .session_id
            .clone()
            .or_else(|| path.file_stem().and_then(|v| v.to_str()).map(str::to_owned))
            .filter(|value| !value.trim().is_empty())?;
        let (prompt_tokens, completion_tokens, cache_read_tokens) = self.usage.totals();
        Some(ClaudeTranscriptEntry {
            session_id,
            cwd: self.cwd.clone().unwrap_or_default(),
            title: self.title.clone(),
            started_at_ms: self.started_at_ms,
            last_activity_at_ms,
            has_output: self.has_output,
            model: self.model.clone(),
            prompt_tokens,
            completion_tokens,
            cache_read_tokens,
            context_used_tokens: self.context_used_tokens,
            context_window_tokens: self.resolved_context_window(),
            request_count: self.usage.request_count,
            desktop: self.desktop,
        })
    }

    fn resolved_context_window(&self) -> u64 {
        self.explicit_context_window
            .or_else(|| self.model.as_deref().map(claude_context_window))
            .unwrap_or(200_000)
    }

    fn matches_session_id(&self, path: &Path, raw_id: &str) -> bool {
        self.session_id
            .as_deref()
            .is_some_and(|id| id.trim_start_matches("claude_") == raw_id)
            || path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == raw_id)
    }
}

pub(crate) fn transcript_metadata_cache()
-> &'static RwLock<BoundedLruCache<PathBuf, TranscriptMetaState>> {
    static CACHE: OnceLock<RwLock<BoundedLruCache<PathBuf, TranscriptMetaState>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(BoundedLruCache::new(8_000)))
}

pub(crate) fn cached_claude_session_path(raw_id: &str) -> Option<PathBuf> {
    let guard = transcript_metadata_cache().read().ok()?;
    guard
        .iter()
        .find(|(path, state)| state.matches_session_id(path, raw_id))
        .map(|(path, _)| path.clone())
}

pub(crate) fn cached_claude_context_usage(raw_id: &str) -> Option<ContextUsage> {
    let guard = transcript_metadata_cache().read().ok()?;
    let (_, state) = guard.iter().find(|(path, state)| {
        state.context_used_tokens > 0 && state.matches_session_id(path, raw_id)
    })?;
    let window = state.resolved_context_window();
    Some(ContextUsage {
        used: state.context_used_tokens,
        window: (window > 0).then_some(window),
    })
}

fn read_incremental_jsonl_line(
    reader: &mut impl BufRead,
    buf: &mut Vec<u8>,
) -> std::io::Result<Option<(u64, bool, bool)>> {
    buf.clear();
    let mut consumed = 0_u64;
    let mut oversized = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            return Ok((consumed > 0).then_some((consumed, false, oversized)));
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(at) => {
                if !oversized && buf.len() + at <= MAX_PROTOCOL_LINE_BYTES {
                    buf.extend_from_slice(&available[..at]);
                } else {
                    oversized = true;
                    buf.clear();
                }
                let step = at + 1;
                reader.consume(step);
                return Ok(Some((
                    consumed.saturating_add(step as u64),
                    true,
                    oversized,
                )));
            }
            None => {
                let step = available.len();
                if !oversized && buf.len() + step <= MAX_PROTOCOL_LINE_BYTES {
                    buf.extend_from_slice(available);
                } else {
                    oversized = true;
                    buf.clear();
                }
                reader.consume(step);
                consumed = consumed.saturating_add(step as u64);
            }
        }
    }
}

pub(crate) fn read_claude_transcript_metadata(
    path: &Path,
    program: &str,
) -> Result<Option<ClaudeTranscriptEntry>, CollectorError> {
    let (file_len, last_activity_at_ms) =
        if let Some((len, stamp_ms)) = crate::usage_ledger::live_file_mark(path) {
            (len, Some(stamp_ms))
        } else {
            let file_metadata = fs::metadata(path).map_err(|source| CollectorError::Io {
                program: program.to_owned(),
                source,
            })?;
            let mtime = file_metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|duration| i64::try_from(duration.as_millis()).ok());
            (file_metadata.len(), mtime)
        };

    let mut state = if let Ok(guard) = transcript_metadata_cache().read()
        && let Some(cached) = guard.get(path)
    {
        if cached.offset == file_len && cached.modified_ms == last_activity_at_ms {
            return Ok(cached.materialize(path, last_activity_at_ms));
        }
        if file_len > cached.offset {
            cached.clone()
        } else {
            TranscriptMetaState::default()
        }
    } else {
        TranscriptMetaState::default()
    };

    read_claude_transcript_metadata_incremental(path, program, file_len, &mut state)?;
    state.modified_ms = last_activity_at_ms;
    let entry = state.materialize(path, last_activity_at_ms);

    if let Ok(mut guard) = transcript_metadata_cache().write() {
        guard.insert(path.to_path_buf(), state);
    }
    Ok(entry)
}

pub(crate) fn read_claude_transcript_metadata_incremental(
    path: &Path,
    program: &str,
    file_len: u64,
    state: &mut TranscriptMetaState,
) -> Result<(), CollectorError> {
    if file_len <= state.offset {
        return Ok(());
    }
    let mut file = fs::File::open(path).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    if state.offset > 0 {
        file.seek(SeekFrom::Start(state.offset))
            .map_err(|source| CollectorError::Io {
                program: program.to_owned(),
                source,
            })?;
    }
    let mut reader = BufReader::new(file);
    let mut raw_line = Vec::new();

    loop {
        let Some((consumed, terminated, oversized)) =
            read_incremental_jsonl_line(&mut reader, &mut raw_line).map_err(|source| {
                CollectorError::Io {
                    program: program.to_owned(),
                    source,
                }
            })?
        else {
            break;
        };
        if !terminated && !oversized {
            let trimmed_end = raw_line
                .iter()
                .rev()
                .find(|&&b| !b.is_ascii_whitespace())
                .copied();
            if trimmed_end != Some(b'}') {
                break;
            }
        }
        state.offset = state.offset.saturating_add(consumed);
        if oversized {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<ClaudeTranscriptLine<'_>>(&raw_line) else {
            continue;
        };
        if !state.desktop
            && record
                .entrypoint
                .as_deref()
                .is_some_and(|e| e.starts_with("claude-desktop"))
        {
            state.desktop = true;
        }
        if let Some(win) = record
            .context_window
            .as_ref()
            .and_then(|c| c.context_window_size)
            .or_else(|| {
                record
                    .context_window_camel
                    .as_ref()
                    .and_then(|c| c.context_window_size)
            })
            .or_else(|| {
                record
                    .message
                    .as_ref()
                    .and_then(|m| m.usage.as_ref())
                    .and_then(|u| u.context_window)
            })
            .filter(|v| *v > 0)
        {
            state.explicit_context_window = Some(win);
        }
        if state.model.is_none()
            && let Some(mid) = record
                .attachment
                .as_ref()
                .and_then(|a| a.identity.as_ref())
                .and_then(|i| i.model_id.as_deref())
                .and_then(normalize_claude_model)
        {
            state.model = Some(mid);
        }
        if record.record_type.as_deref() == Some("assistant") {
            state.has_output = true;
            let message_id = record
                .message
                .as_ref()
                .and_then(|m| m.id.as_deref())
                .filter(|id| !id.is_empty());
            if let Some(usage) = record.message.as_ref().and_then(|m| m.usage.as_ref()) {
                let input = usage.input_tokens.unwrap_or(0);
                let output = usage.output_tokens.unwrap_or(0);
                let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                let cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
                let total_input = input.saturating_add(cache_read).saturating_add(cache_write);
                if total_input > 0 {
                    state.context_used_tokens = total_input;
                }
                let figures = (total_input, output, cache_read);
                let key = message_id.map(str::to_owned).unwrap_or_else(|| {
                    format!("line-{}", state.usage.request_count.saturating_add(1))
                });
                state.usage.record(key, figures);
                if let Some(m) = record
                    .message
                    .as_ref()
                    .and_then(|m| m.model.as_deref())
                    .or(record.model.as_deref())
                    .filter(|m| !m.is_empty() && *m != "<synthetic>")
                    .or_else(|| {
                        record
                            .requested_model
                            .as_deref()
                            .filter(|m| !m.is_empty() && *m != "<synthetic>")
                    })
                    .and_then(normalize_claude_model)
                {
                    state.model = Some(m);
                }
            } else if let Some(id) = message_id {
                state.usage.record(id.to_owned(), (0, 0, 0));
            }
        }
        if !state.has_output && record_is_assistant_output(&record) {
            state.has_output = true;
        }
        if state.session_id.is_none() {
            state.session_id = record.session_id.map(Cow::into_owned);
        }
        if state.cwd.is_none() {
            state.cwd = record.cwd.map(Cow::into_owned);
        }
        if state.title.is_none() {
            state.title = record
                .custom_title
                .or(record.ai_title)
                .or(record.title)
                .or(record.last_prompt)
                .and_then(|t| metadata_label(&t, MAX_SESSION_TITLE_CHARS));
        }
        if let Some(timestamp) = record.timestamp.as_deref() {
            let parsed = DateTime::parse_from_rfc3339(timestamp)
                .ok()
                .map(|v| v.timestamp_millis());
            state.started_at_ms = match (state.started_at_ms, parsed) {
                (Some(current), Some(candidate)) => Some(current.min(candidate)),
                (None, candidate) => candidate,
                (current, None) => current,
            };
        }
    }
    Ok(())
}
