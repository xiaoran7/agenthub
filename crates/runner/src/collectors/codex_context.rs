use std::{
    borrow::Cow,
    collections::HashSet,
    fmt, fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::Path,
};

use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};

use super::*;

/// Reads Codex's metadata-only SQLite columns and a bounded tail of each
/// rollout for configuration/token events. Message and command payloads are
/// ignored, and the result is not part of the synchronized observation model.
pub fn collect_codex_session_contexts(
    limit: u32,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let Some(database) = super::codex::discover_codex_state_database()? else {
        return Ok(Vec::new());
    };
    collect_codex_session_contexts_from(&database, limit)
}

pub fn collect_codex_session_contexts_from(
    database: &Path,
    limit: u32,
) -> Result<Vec<LocalSessionContext>, CollectorError> {
    let program = "Codex local context".to_owned();
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|source| CollectorError::Database {
        program: program.clone(),
        source,
    })?;
    let _ = connection.busy_timeout(std::time::Duration::from_millis(500));
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
    if !columns.contains("id") {
        return Ok(Vec::new());
    }
    let title = codex_title_expression(&columns);
    let model = super::codex::optional_column(&columns, &["model"]);
    let effort = super::codex::optional_column(&columns, &["reasoning_effort"]);
    let approval = super::codex::optional_column(&columns, &["approval_mode", "approval_policy"]);
    let sandbox = super::codex::optional_column(&columns, &["sandbox_policy", "sandbox_mode"]);
    let tokens = super::codex::optional_column(&columns, &["tokens_used"]);
    let rollout = super::codex::optional_column(&columns, &["rollout_path"]);
    let archived_filter = if columns.contains("archived") {
        " WHERE archived = 0"
    } else {
        ""
    };
    let order_by = ["recency_at_ms", "recency_at", "updated_at_ms", "updated_at"]
        .into_iter()
        .find(|column| columns.contains(*column))
        .unwrap_or("id");
    let sql = format!(
        "SELECT id, {title}, {model}, {effort}, {approval}, {sandbox}, {tokens}, {rollout} FROM threads{archived_filter} ORDER BY {order_by} DESC LIMIT ?1"
    );
    let root = database.parent().unwrap_or_else(|| Path::new("."));
    let mut statement = connection
        .prepare(&sql)
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;
    let rows = statement
        .query_map([i64::from(limit)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1).unwrap_or_default(),
                row.get::<_, Option<String>>(2).unwrap_or_default(),
                row.get::<_, Option<String>>(3).unwrap_or_default(),
                row.get::<_, Option<String>>(4).unwrap_or_default(),
                row.get::<_, Option<String>>(5).unwrap_or_default(),
                row.get::<_, Option<i64>>(6).unwrap_or_default(),
                row.get::<_, Option<String>>(7).unwrap_or_default(),
            ))
        })
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CollectorError::Database {
            program: program.clone(),
            source,
        })?;

    let mut contexts = Vec::with_capacity(rows.len());
    for (session_id, title, model, effort, approval, sandbox, tokens, rollout_path) in rows {
        let Some(session_id) = metadata_label(&session_id, MAX_CONFIG_VALUE_CHARS) else {
            continue;
        };
        let mut context = LocalSessionContext::new(crate::domain::AgentType::Codex, session_id);
        context.title = title.as_deref().and_then(local_prompt_title).or_else(|| {
            title
                .as_deref()
                .and_then(|value| metadata_label(value, MAX_SESSION_TITLE_CHARS))
                .filter(|t| {
                    let lower = t.to_lowercase();
                    lower != "untitled"
                        && lower != "未命名会话"
                        && !t.contains("AGENTS.md instructions")
                })
        });
        context.model = model.and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
        context.reasoning_effort =
            effort.and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
        context.approval_policy =
            approval.and_then(|value| metadata_label(&value, MAX_CONFIG_VALUE_CHARS));
        context.sandbox_mode = sandbox.and_then(|value| codex_sandbox_label(&value));
        context.session_total_tokens = tokens
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0);
        if let Some(rollout_path) = rollout_path {
            let _ =
                apply_codex_rollout_context(root, Path::new(&rollout_path), &mut context, &program);
        }
        contexts.push(context);
    }
    Ok(contexts)
}

#[derive(Default)]
struct ConfigObjectOrStr(Option<String>);

impl<'de> Deserialize<'de> for ConfigObjectOrStr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LabelVisitor;

        impl<'de> Visitor<'de> for LabelVisitor {
            type Value = ConfigObjectOrStr;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("string, object with type/mode, or ignored value")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(ConfigObjectOrStr(metadata_label(v, MAX_CONFIG_VALUE_CHARS)))
            }

            serde_ignore_scalar_primitives!();

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(ConfigObjectOrStr(None))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut ty: Option<String> = None;
                let mut mode: Option<String> = None;
                while let Some(key) = map.next_key::<Cow<'de, str>>()? {
                    match key.as_ref() {
                        "type" => {
                            ty = map
                                .next_value::<JsonOptStr<'de>>()?
                                .as_deref()
                                .and_then(|v| metadata_label(v, MAX_CONFIG_VALUE_CHARS));
                        }
                        "mode" => {
                            mode = map
                                .next_value::<JsonOptStr<'de>>()?
                                .as_deref()
                                .and_then(|v| metadata_label(v, MAX_CONFIG_VALUE_CHARS));
                        }
                        _ => {
                            let _ = map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(ConfigObjectOrStr(ty.or(mode)))
            }
        }

        deserializer.deserialize_any(LabelVisitor)
    }
}

fn codex_sandbox_label(value: &str) -> Option<String> {
    serde_json::from_str::<ConfigObjectOrStr>(value)
        .ok()
        .and_then(|parsed| parsed.0)
        .or_else(|| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
}

pub(crate) fn apply_codex_rollout_context_cached(
    root: &Path,
    rollout_path: &Path,
) -> Option<LocalSessionContext> {
    let clean_str = super::codex_enrich::strip_windows_verbatim(&rollout_path.to_string_lossy());
    let clean_rel = Path::new(&clean_str);
    let root_canon = dunce::canonicalize(root).ok()?;
    let candidate = if clean_rel.is_absolute() {
        clean_rel.to_owned()
    } else {
        root_canon.join(clean_rel)
    };
    let candidate = dunce::canonicalize(candidate).ok()?;
    if !candidate.starts_with(&root_canon) || !candidate.is_file() {
        return None;
    }
    let meta = fs::metadata(&candidate).ok()?;
    let len = meta.len();
    let mtime = meta.modified().ok();
    if let Ok(guard) = super::codex_enrich::rollout_ctx_cache().read()
        && let Some((c_len, c_mtime, cached)) = guard.get(&candidate)
        && *c_len == len
        && *c_mtime == mtime
    {
        return Some(cached.clone());
    }
    let mut ctx = LocalSessionContext::new(crate::domain::AgentType::Codex, String::new());
    apply_codex_rollout_context(root, &candidate, &mut ctx, "Codex rollout cache").ok()?;
    if let Ok(mut guard) = super::codex_enrich::rollout_ctx_cache().write() {
        guard.insert(candidate, (len, mtime, ctx.clone()));
    }
    Some(ctx)
}

fn apply_codex_rollout_context(
    root: &Path,
    rollout_path: &Path,
    context: &mut LocalSessionContext,
    program: &str,
) -> Result<(), CollectorError> {
    let root = dunce::canonicalize(root).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let clean_str = super::codex_enrich::strip_windows_verbatim(&rollout_path.to_string_lossy());
    let clean_path = Path::new(&clean_str);
    let candidate = if clean_path.is_absolute() {
        clean_path.to_owned()
    } else {
        root.join(clean_path)
    };
    let Ok(candidate) = dunce::canonicalize(candidate) else {
        return Ok(());
    };
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Ok(());
    }
    let file = fs::File::open(&candidate).map_err(|source| CollectorError::Io {
        program: program.to_owned(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?
        .len();
    let mut reader = BufReader::new(file);
    scan_codex_context_region(
        &mut reader,
        LOCAL_CONTEXT_HEAD_BYTES.min(length),
        context,
        program,
    )?;
    let head_end_pos = reader.stream_position().unwrap_or(LOCAL_CONTEXT_HEAD_BYTES);
    if length > head_end_pos {
        let tail_start = length
            .saturating_sub(LOCAL_CONTEXT_TAIL_BYTES)
            .max(head_end_pos);
        let mut remaining = length.saturating_sub(tail_start);
        if tail_start > head_end_pos {
            reader
                .seek(SeekFrom::Start(tail_start))
                .map_err(|source| CollectorError::Io {
                    program: program.to_owned(),
                    source,
                })?;
            let skipped =
                discard_until_newline(&mut reader).map_err(|source| CollectorError::Io {
                    program: program.to_owned(),
                    source,
                })?;
            remaining = remaining.saturating_sub(skipped);
        }
        if remaining > 0 {
            scan_codex_context_region(&mut reader, remaining, context, program)?;
        }
    }
    Ok(())
}

fn scan_codex_context_region(
    reader: &mut impl BufRead,
    byte_limit: u64,
    context: &mut LocalSessionContext,
    program: &str,
) -> Result<(), CollectorError> {
    let mut consumed = 0_u64;
    while consumed < byte_limit {
        let line = match read_bounded_line(reader) {
            BoundedLine::Line(line) => line,
            BoundedLine::Eof => break,
            BoundedLine::TooLarge => {
                consumed = consumed.saturating_add(MAX_PROTOCOL_LINE_BYTES as u64);
                continue;
            }
            BoundedLine::Io(source) => {
                return Err(CollectorError::Io {
                    program: program.to_owned(),
                    source,
                });
            }
        };
        consumed = consumed.saturating_add(line.len() as u64);
        apply_codex_context_line(&line, context);
    }
    Ok(())
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexTokenUsage {
    input_tokens: JsonOptU64,
    output_tokens: JsonOptU64,
    cached_input_tokens: JsonOptU64,
    cache_read_input_tokens: JsonOptU64,
    cache_write_input_tokens: JsonOptU64,
    cache_creation_input_tokens: JsonOptU64,
    reasoning_output_tokens: JsonOptU64,
    total_tokens: JsonOptU64,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexTotalTokens {
    input_tokens: JsonOptU64,
    output_tokens: JsonOptU64,
    cached_input_tokens: JsonOptU64,
    cache_read_input_tokens: JsonOptU64,
    total_tokens: JsonOptU64,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CodexTokenInfo {
    model_context_window: JsonOptU64,
    last_token_usage: Option<CodexTokenUsage>,
    total_token_usage: Option<CodexTotalTokens>,
}

#[derive(Default)]
struct CodexTokenInfoOpt(Option<CodexTokenInfo>);

impl<'de> Deserialize<'de> for CodexTokenInfoOpt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct InfoVisitor;

        impl<'de> Visitor<'de> for InfoVisitor {
            type Value = CodexTokenInfoOpt;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("token info map or ignored value")
            }

            serde_ignore_non_map!();

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut info = CodexTokenInfo::default();
                while let Some(key) = map.next_key::<Cow<'de, str>>()? {
                    match key.as_ref() {
                        "model_context_window" => {
                            info.model_context_window = map.next_value::<JsonOptU64>()?;
                        }
                        "last_token_usage" => {
                            info.last_token_usage = map.next_value::<Option<CodexTokenUsage>>()?;
                        }
                        "total_token_usage" => {
                            info.total_token_usage =
                                map.next_value::<Option<CodexTotalTokens>>()?;
                        }
                        _ => {
                            let _ = map.next_value::<IgnoredAny>()?;
                        }
                    }
                }
                Ok(CodexTokenInfoOpt(Some(info)))
            }
        }

        deserializer.deserialize_any(InfoVisitor)
    }
}

#[derive(Default)]
struct ContentTitleOpt(Option<String>);

impl<'de> Deserialize<'de> for ContentTitleOpt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Item<'a> {
            #[serde(borrow)]
            text: JsonOptStr<'a>,
        }

        struct ContentVisitor;

        impl<'de> Visitor<'de> for ContentVisitor {
            type Value = ContentTitleOpt;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("content array or ignored value")
            }

            serde_ignore_non_seq!();

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut found = None;
                while let Some(item) = seq.next_element::<Item<'de>>()? {
                    if let Some(title) = item.text.as_deref().and_then(local_prompt_title) {
                        found = Some(title);
                        while seq.next_element::<IgnoredAny>()?.is_some() {}
                        break;
                    }
                }
                Ok(ContentTitleOpt(found))
            }
        }

        deserializer.deserialize_any(ContentVisitor)
    }
}

#[derive(Default)]
struct ParsedPayload<'a> {
    kind: Option<Cow<'a, str>>,
    role: Option<Cow<'a, str>>,
    model: Option<Cow<'a, str>>,
    effort: Option<Cow<'a, str>>,
    reasoning_effort: Option<Cow<'a, str>>,
    approval_policy: Option<Cow<'a, str>>,
    approval_policy_camel: Option<Cow<'a, str>>,
    permissions: Option<Cow<'a, str>>,
    sandbox_policy: Option<String>,
    sandbox_policy_camel: Option<String>,
    sandbox: Option<String>,
    model_context_window: Option<u64>,
    info: Option<CodexTokenInfo>,
    message: Option<Cow<'a, str>>,
    text: Option<Cow<'a, str>>,
    content_title: Option<String>,
}

struct PayloadSeed {
    need_title: bool,
}

impl<'de> Visitor<'de> for PayloadSeed {
    type Value = Option<ParsedPayload<'de>>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Codex context payload map or ignored value")
    }

    serde_ignore_non_map!();

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut p = ParsedPayload::default();
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            match key.as_ref() {
                "type" => p.kind = map.next_value::<JsonOptStr<'de>>()?.0,
                "role" => p.role = map.next_value::<JsonOptStr<'de>>()?.0,
                "model" => p.model = map.next_value::<JsonOptStr<'de>>()?.0,
                "effort" => p.effort = map.next_value::<JsonOptStr<'de>>()?.0,
                "reasoning_effort" => {
                    p.reasoning_effort = map.next_value::<JsonOptStr<'de>>()?.0;
                }
                "approval_policy" => {
                    p.approval_policy = map.next_value::<JsonOptStr<'de>>()?.0;
                }
                "approvalPolicy" => {
                    p.approval_policy_camel = map.next_value::<JsonOptStr<'de>>()?.0;
                }
                "permissions" => p.permissions = map.next_value::<JsonOptStr<'de>>()?.0,
                "sandbox_policy" => {
                    p.sandbox_policy = map.next_value::<ConfigObjectOrStr>()?.0;
                }
                "sandboxPolicy" => {
                    p.sandbox_policy_camel = map.next_value::<ConfigObjectOrStr>()?.0;
                }
                "sandbox" => p.sandbox = map.next_value::<ConfigObjectOrStr>()?.0,
                "model_context_window" => {
                    p.model_context_window = map.next_value::<JsonOptU64>()?.0;
                }
                "info" => p.info = map.next_value::<CodexTokenInfoOpt>()?.0,
                "message"
                    if self.need_title && p.kind.as_deref().is_none_or(|k| k == "user_message") =>
                {
                    p.message = map.next_value::<JsonOptStr<'de>>()?.0;
                }
                "text"
                    if self.need_title && p.kind.as_deref().is_none_or(|k| k == "user_message") =>
                {
                    p.text = map.next_value::<JsonOptStr<'de>>()?.0;
                }
                "content" if self.need_title && p.role.as_deref().is_none_or(|r| r == "user") => {
                    p.content_title = map.next_value::<ContentTitleOpt>()?.0;
                }
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(Some(p))
    }
}

impl<'de> DeserializeSeed<'de> for PayloadSeed {
    type Value = Option<ParsedPayload<'de>>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

struct ParsedLine<'a> {
    kind: Option<Cow<'a, str>>,
    payload: Option<ParsedPayload<'a>>,
}

struct CodexLineSeed {
    need_title: bool,
}

impl<'de> Visitor<'de> for CodexLineSeed {
    type Value = ParsedLine<'de>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Codex context JSON line")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut kind: Option<Cow<'de, str>> = None;
        let mut payload: Option<ParsedPayload<'de>> = None;
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            match key.as_ref() {
                "type" => kind = map.next_value::<JsonOptStr<'de>>()?.0,
                "payload"
                    if kind.as_deref().is_none_or(|k| {
                        matches!(k, "turn_context" | "event_msg" | "response_item")
                    }) =>
                {
                    payload = map.next_value_seed(PayloadSeed {
                        need_title: self.need_title,
                    })?;
                }
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(ParsedLine { kind, payload })
    }
}

impl<'de> DeserializeSeed<'de> for CodexLineSeed {
    type Value = ParsedLine<'de>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(self)
    }
}

fn label_opt(v: Option<&str>) -> Option<String> {
    v.and_then(|value| metadata_label(value, MAX_CONFIG_VALUE_CHARS))
}

pub(crate) fn apply_codex_context_line(line: &str, context: &mut LocalSessionContext) {
    let mut de = serde_json::Deserializer::from_str(line);
    let Ok(parsed) = (CodexLineSeed {
        need_title: context.title.is_none(),
    })
    .deserialize(&mut de) else {
        return;
    };
    let Some(payload) = parsed.payload else {
        return;
    };
    match parsed.kind.as_deref() {
        Some("turn_context") => {
            context.model = label_opt(payload.model.as_deref()).or_else(|| context.model.clone());
            context.reasoning_effort = label_opt(
                payload
                    .effort
                    .as_deref()
                    .or(payload.reasoning_effort.as_deref()),
            )
            .or_else(|| context.reasoning_effort.clone());
            context.approval_policy = label_opt(
                payload
                    .approval_policy
                    .as_deref()
                    .or(payload.approval_policy_camel.as_deref()),
            )
            .or_else(|| context.approval_policy.clone());
            context.permission_mode = label_opt(payload.permissions.as_deref())
                .or_else(|| context.permission_mode.clone());
            context.sandbox_mode = payload
                .sandbox_policy
                .or(payload.sandbox_policy_camel)
                .or(payload.sandbox)
                .or_else(|| context.sandbox_mode.clone());
        }
        Some("event_msg") if payload.kind.as_deref() == Some("token_count") => {
            if let Some(info) = payload.info {
                context.context_window_tokens = info
                    .model_context_window
                    .0
                    .or(context.context_window_tokens);
                if let Some(last) = info.last_token_usage {
                    let input = last.input_tokens.0.unwrap_or(0);
                    let output = last
                        .output_tokens
                        .0
                        .unwrap_or(0)
                        .max(last.reasoning_output_tokens.0.unwrap_or(0));
                    let cache_read = last
                        .cached_input_tokens
                        .0
                        .or(last.cache_read_input_tokens.0)
                        .unwrap_or(0);
                    let cache_write = last
                        .cache_write_input_tokens
                        .0
                        .or(last.cache_creation_input_tokens.0)
                        .unwrap_or(0);
                    let prompt_used = if cache_read <= input {
                        input
                    } else {
                        input.saturating_add(cache_read)
                    };
                    let used = last.total_tokens.0.unwrap_or(0).max(prompt_used);
                    if used > 0 {
                        context.context_used_tokens = Some(used);
                        context.input_tokens = Some(prompt_used);
                        context.output_tokens = Some(output);
                        context.cache_read_tokens = Some(cache_read);
                        context.cache_write_tokens = Some(cache_write);
                    }
                }
                if let Some(tot) = info.total_token_usage {
                    if let Some(tt) = tot.total_tokens.0 {
                        context.session_total_tokens = Some(tt);
                    }
                    if let Some(t_in) = tot.input_tokens.0.filter(|&v| v > 0) {
                        context.input_tokens = Some(t_in);
                    }
                    if let Some(t_out) = tot.output_tokens.0.filter(|&v| v > 0) {
                        context.output_tokens = Some(t_out);
                    }
                    if let Some(t_cr) = tot
                        .cached_input_tokens
                        .0
                        .or(tot.cache_read_input_tokens.0)
                        .filter(|&v| v > 0)
                    {
                        context.cache_read_tokens = Some(t_cr);
                    }
                }
            }
        }
        Some("event_msg") if payload.kind.as_deref() == Some("task_started") => {
            context.context_window_tokens = payload
                .model_context_window
                .or(context.context_window_tokens);
        }
        Some("event_msg")
            if context.title.is_none() && payload.kind.as_deref() == Some("user_message") =>
        {
            if let Some(text) = payload.message.as_deref().or(payload.text.as_deref()) {
                context.title = local_prompt_title(text);
            }
        }
        Some("response_item")
            if context.title.is_none() && payload.role.as_deref() == Some("user") =>
        {
            if let Some(title) = payload.content_title {
                context.title = Some(title);
            }
        }
        _ => {}
    }
}

fn codex_title_expression(columns: &HashSet<String>) -> String {
    let mut parts = Vec::new();
    for col in &["title", "name", "first_user_message", "preview"] {
        if columns.contains(*col) {
            parts.push(format!("NULLIF(TRIM({col}), '')"));
        }
    }
    if parts.is_empty() {
        "NULL".to_owned()
    } else if parts.len() == 1 {
        parts.remove(0)
    } else {
        format!("COALESCE({})", parts.join(", "))
    }
}
