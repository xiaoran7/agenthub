use std::{
    env, fs,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock, atomic::Ordering},
    time::{Duration, UNIX_EPOCH},
};

use chrono::DateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;

pub(crate) fn context_usage_from_values(
    usage: &Value,
    window: Option<u64>,
) -> Option<ContextUsage> {
    let input = usage_u64(usage, "input_tokens");
    let cache_read = usage_u64(usage, "cache_read_input_tokens");
    let cache_write = usage_u64(usage, "cache_creation_input_tokens");
    let used = input
        .saturating_add(cache_read)
        .saturating_add(cache_write)
        .max(usage_u64(usage, "total_tokens"));
    if used == 0 {
        return None;
    }
    let reported_window = window
        .or_else(|| usage.get("context_window_tokens").and_then(Value::as_u64))
        .or_else(|| usage.get("model_context_window").and_then(Value::as_u64))
        .filter(|value| *value > 0);
    Some(ContextUsage {
        used,
        window: reported_window,
    })
}

pub(crate) fn usage_u64(value: &Value, field: &str) -> u64 {
    value.get(field).and_then(Value::as_u64).unwrap_or_default()
}

pub(crate) fn metadata_label(value: &str, limit: usize) -> Option<String> {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect::<String>();
    (!normalized.is_empty()).then_some(normalized)
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .or_else(|| env::var_os("HOME").filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

/// Truncates `text` in-place to at most `max_bytes` on a UTF-8 char boundary.
pub(crate) fn truncate_utf8_bytes(text: &mut String, max_bytes: usize) {
    if text.len() > max_bytes {
        text.truncate(text.floor_char_boundary(max_bytes));
    }
}

/// ticks. They are not caches: nothing is served from them, they only answer
/// "did anything in this store change since the last pass", so a pass that
/// found nothing new can leave every console alone.
pub(crate) const STORE_CLAUDE: usize = 0;
pub(crate) const STORE_ANTIGRAVITY: usize = 1;

pub(crate) struct Sha256Writer(pub(crate) Sha256);

impl Write for Sha256Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn json_sha256<T: serde::Serialize + ?Sized>(val: &T) -> [u8; 32] {
    let mut writer = Sha256Writer(Sha256::new());
    if serde_json::to_writer(&mut writer, val).is_err() {
        return [0u8; 32];
    }
    writer.0.finalize().into()
}

pub(crate) fn store_fingerprints() -> &'static RwLock<[Option<[u8; 32]>; 2]> {
    static CACHE: OnceLock<RwLock<[Option<[u8; 32]>; 2]>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(Default::default()))
}

/// Records what a store's scan just produced, and says whether it differs
/// from what the previous pass saw.
pub(crate) fn note_store_snapshot<T: serde::Serialize>(label: usize, entries: &[T]) -> bool {
    let digest = json_sha256(entries);
    let Ok(mut guard) = store_fingerprints().write() else {
        return false;
    };
    let changed = guard[label].as_ref() != Some(&digest);
    guard[label] = Some(digest);
    if changed {
        STORE_CHANGED.store(true, Ordering::Release);
    }
    changed
}

/// True when a store changed since this flag was last cleared.
pub(crate) static STORE_CHANGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Takes and clears "a store changed" for the caller.
///
/// The scan pass clears this before it refreshes the stores and reads it
/// after; a store that reported a change during the pass is what makes the
/// pass publish, and clearing it here means the previous pass's change can
/// never be attributed to this one.
pub fn take_store_change() -> bool {
    STORE_CHANGED.swap(false, Ordering::AcqRel)
}

/// Forgets every fingerprint, so the next pass reports a change for all of
/// them. Used when the console asks for a forced refresh.
pub(crate) fn forget_store_snapshots() {
    if let Ok(mut guard) = store_fingerprints().write() {
        *guard = Default::default();
    }
    STORE_CHANGED.store(true, Ordering::Release);
}

pub(crate) fn file_modified_ago(path: &Path) -> Option<Duration> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    modified.elapsed().ok()
}

pub(crate) fn file_modified_millis(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

pub(crate) fn parse_rfc3339_millis(value: Option<&str>) -> Option<i64> {
    DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(|value| value.timestamp_millis())
}

pub(crate) fn read_dir_entries(
    path: &Path,
    program: &str,
) -> Result<Vec<fs::DirEntry>, CollectorError> {
    let entries = fs::read_dir(path)
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(entries)
}

pub(crate) fn read_bounded_json_file(
    path: &Path,
    limit: u64,
    program: &str,
) -> Result<Option<String>, CollectorError> {
    if !path.is_file() {
        return Ok(None);
    }
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
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
    if length > limit {
        return Ok(None);
    }
    let mut contents = String::new();
    let mut reader = BufReader::new(file.take(limit.saturating_add(1)));
    reader
        .read_to_string(&mut contents)
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })?;
    if contents.len() as u64 > limit {
        return Ok(None);
    }
    Ok(Some(contents))
}

/// Streams and deserializes a bounded JSON file directly via `BufReader` without
/// buffering the entire file contents into an intermediate `String`.
pub(crate) fn read_bounded_json_de<T: serde::de::DeserializeOwned>(
    path: &Path,
    limit: u64,
    program: &str,
) -> Result<Option<T>, CollectorError> {
    if !path.is_file() {
        return Ok(None);
    }
    let file = fs::File::open(path).map_err(|source| CollectorError::Io {
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
    if length > limit {
        return Ok(None);
    }
    let reader = BufReader::new(file.take(limit.saturating_add(1)));
    Ok(serde_json::from_reader::<_, T>(reader).ok())
}

pub(crate) fn local_prompt_title(text: &str) -> Option<String> {
    let text = if let Some(start) = text.find("<USER_REQUEST>") {
        let request = &text[start + "<USER_REQUEST>".len()..];
        &request[..request.find("</USER_REQUEST>").unwrap_or(request.len())]
    } else {
        text
    };
    if [
        "<local-command-caveat>",
        "<command-name>",
        "<task-notification>",
    ]
    .iter()
    .any(|tag| text.contains(tag))
    {
        return None;
    }

    let mut in_skip_block = false;
    let skip_tags_start = [
        "<INSTRUCTIONS>",
        "<instructions>",
        "<environment_context>",
        "<permissions instructions>",
        "<system-reminder>",
    ];
    let skip_tags_end = [
        "</INSTRUCTIONS>",
        "</instructions>",
        "</environment_context>",
        "</permissions instructions>",
        "</system-reminder>",
    ];

    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }

        if skip_tags_start.iter().any(|tag| line.contains(tag)) {
            in_skip_block = true;
        }
        if in_skip_block {
            if skip_tags_end.iter().any(|tag| line.contains(tag)) {
                in_skip_block = false;
            }
            continue;
        }

        let clean = line
            .trim_start_matches(['#', ' ', '>', '-', '*', ':'])
            .trim();
        if clean.is_empty() || clean.starts_with('<') || clean.starts_with("<!--") {
            continue;
        }
        if clean.starts_with("AGENTS.md instructions")
            || clean.starts_with("Instructions for")
            || clean.starts_with("You are ")
        {
            continue;
        }
        let lower = clean.to_lowercase();
        if lower == "untitled" || lower == "未命名会话" || lower == "none" || lower == "null" {
            continue;
        }
        return metadata_label(clean, MAX_SESSION_TITLE_CHARS);
    }
    None
}

macro_rules! serde_ignore_scalar_primitives {
    () => {
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }

        fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_i128<E: serde::de::Error>(self, _: i128) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_u128<E: serde::de::Error>(self, _: u128) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }
    };
}
pub(crate) use serde_ignore_scalar_primitives;

macro_rules! serde_ignore_containers {
    () => {
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(Self::Value::default())
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            while map
                .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                .is_some()
            {}
            Ok(Self::Value::default())
        }
    };
}

macro_rules! serde_ignore_non_map {
    () => {
        $crate::collectors::serde_ignore_scalar_primitives!();

        fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(Self::Value::default())
        }
    };
}
pub(crate) use serde_ignore_non_map;

macro_rules! serde_ignore_non_seq {
    () => {
        $crate::collectors::serde_ignore_scalar_primitives!();

        fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
            Ok(Self::Value::default())
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            while map
                .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                .is_some()
            {}
            Ok(Self::Value::default())
        }
    };
}
pub(crate) use serde_ignore_non_seq;

/// Lenient borrowed optional JSON string that returns `None` (and drains nested
/// sequences/maps zero-allocation) when a field is `null` or a non-string type.
#[derive(Debug, Clone, Default)]
pub(crate) struct JsonOptStr<'a>(pub Option<std::borrow::Cow<'a, str>>);

impl<'a> JsonOptStr<'a> {
    pub(crate) fn as_deref(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl<'de: 'a, 'a> serde::Deserialize<'de> for JsonOptStr<'a> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OptStrVisitor<'a>(std::marker::PhantomData<&'a ()>);

        impl<'de: 'a, 'a> serde::de::Visitor<'de> for OptStrVisitor<'a> {
            type Value = JsonOptStr<'a>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("optional string or ignored JSON value")
            }

            fn visit_borrowed_str<E: serde::de::Error>(
                self,
                v: &'de str,
            ) -> Result<Self::Value, E> {
                Ok(JsonOptStr(Some(std::borrow::Cow::Borrowed(v))))
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(JsonOptStr(Some(std::borrow::Cow::Owned(v.to_owned()))))
            }

            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(JsonOptStr(Some(std::borrow::Cow::Owned(v))))
            }

            serde_ignore_scalar_primitives!();
            serde_ignore_containers!();
        }

        deserializer.deserialize_any(OptStrVisitor(std::marker::PhantomData))
    }
}

/// Lenient optional non-negative JSON integer (`serde_json::Value::as_u64` equivalent).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JsonOptU64(pub Option<u64>);

impl<'de> serde::Deserialize<'de> for JsonOptU64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OptU64Visitor;

        impl<'de> serde::de::Visitor<'de> for OptU64Visitor {
            type Value = JsonOptU64;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("optional u64 or ignored JSON value")
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(JsonOptU64(Some(v)))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(JsonOptU64(u64::try_from(v).ok()))
            }

            fn visit_u128<E: serde::de::Error>(self, v: u128) -> Result<Self::Value, E> {
                Ok(JsonOptU64(u64::try_from(v).ok()))
            }

            fn visit_i128<E: serde::de::Error>(self, v: i128) -> Result<Self::Value, E> {
                Ok(JsonOptU64(u64::try_from(v).ok()))
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonOptU64(None))
            }

            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonOptU64(None))
            }

            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                deserializer.deserialize_any(self)
            }

            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(JsonOptU64(None))
            }

            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(JsonOptU64(None))
            }

            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(JsonOptU64(None))
            }

            serde_ignore_containers!();
        }

        deserializer.deserialize_any(OptU64Visitor)
    }
}

/// Lenient optional signed JSON integer (`serde_json::Value::as_i64` equivalent).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JsonOptI64(pub Option<i64>);

impl<'de> serde::Deserialize<'de> for JsonOptI64 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OptI64Visitor;

        impl<'de> serde::de::Visitor<'de> for OptI64Visitor {
            type Value = JsonOptI64;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("optional i64 or ignored JSON value")
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(JsonOptI64(Some(v)))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(JsonOptI64(i64::try_from(v).ok()))
            }

            fn visit_i128<E: serde::de::Error>(self, v: i128) -> Result<Self::Value, E> {
                Ok(JsonOptI64(i64::try_from(v).ok()))
            }

            fn visit_u128<E: serde::de::Error>(self, v: u128) -> Result<Self::Value, E> {
                Ok(JsonOptI64(i64::try_from(v).ok()))
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonOptI64(None))
            }

            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonOptI64(None))
            }

            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                deserializer.deserialize_any(self)
            }

            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(JsonOptI64(None))
            }

            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(JsonOptI64(None))
            }

            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(JsonOptI64(None))
            }

            serde_ignore_containers!();
        }

        deserializer.deserialize_any(OptI64Visitor)
    }
}

/// Lenient optional JSON boolean (`serde_json::Value::as_bool` equivalent).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JsonOptBool(pub Option<bool>);

impl<'de> serde::Deserialize<'de> for JsonOptBool {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OptBoolVisitor;

        impl<'de> serde::de::Visitor<'de> for OptBoolVisitor {
            type Value = JsonOptBool;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("optional bool or ignored JSON value")
            }

            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(JsonOptBool(Some(v)))
            }

            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(JsonOptBool(None))
            }

            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(JsonOptBool(None))
            }

            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(JsonOptBool(None))
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonOptBool(None))
            }

            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(JsonOptBool(None))
            }

            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                deserializer.deserialize_any(self)
            }

            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(JsonOptBool(None))
            }

            serde_ignore_containers!();
        }

        deserializer.deserialize_any(OptBoolVisitor)
    }
}

/// Lenient optional JSON object (`serde_json::Value::as_object` equivalent).
#[derive(Debug, Clone)]
pub(crate) struct JsonOptMap<T>(pub Option<T>);

impl<T> Default for JsonOptMap<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for JsonOptMap<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OptMapVisitor<T>(std::marker::PhantomData<fn() -> T>);

        impl<'de, T: serde::Deserialize<'de>> serde::de::Visitor<'de> for OptMapVisitor<T> {
            type Value = JsonOptMap<T>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("optional map or ignored JSON value")
            }

            serde_ignore_non_map!();

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                T::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(|v| JsonOptMap(Some(v)))
            }
        }

        deserializer.deserialize_any(OptMapVisitor(std::marker::PhantomData))
    }
}
