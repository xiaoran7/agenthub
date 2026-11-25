use std::{borrow::Cow, fmt};

use serde::{
    Deserialize,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;

use super::*;

#[derive(Default, Deserialize)]
pub(crate) struct CtxWindowSnake {
    pub(crate) context_window_size: Option<u64>,
}

#[derive(Default, Deserialize)]
pub(crate) struct CtxWindowCamel {
    #[serde(rename = "contextWindowSize")]
    pub(crate) context_window_size: Option<u64>,
}

pub(crate) enum PromptContent<'a> {
    Str(Cow<'a, str>),
    Extracted(Option<String>),
    Ignored,
}

impl<'a> PromptContent<'a> {
    pub(crate) fn as_extracted(&self) -> Option<String> {
        match self {
            Self::Str(text) => {
                let trimmed = text.trim();
                if is_synthetic_claude_prompt(trimmed) {
                    None
                } else {
                    clean_claude_prompt_text(trimmed)
                }
            }
            Self::Extracted(text) => text.clone(),
            Self::Ignored => None,
        }
    }

    pub(crate) fn as_raw_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text.as_ref()),
            _ => None,
        }
    }
}

#[derive(Default, Deserialize)]
struct PromptBlock<'a> {
    #[serde(borrow, rename = "type")]
    kind: Option<Cow<'a, str>>,
    #[serde(borrow)]
    text: Option<Cow<'a, str>>,
}

struct PromptContentSeed {
    parse_array: bool,
}

impl<'de> DeserializeSeed<'de> for PromptContentSeed {
    type Value = PromptContent<'de>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(PromptContentVisitor {
            parse_array: self.parse_array,
        })
    }
}

struct PromptContentVisitor {
    parse_array: bool,
}

impl<'de> Visitor<'de> for PromptContentVisitor {
    type Value = PromptContent<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("prompt content string or blocks")
    }

    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(PromptContent::Str(Cow::Borrowed(v)))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(PromptContent::Str(Cow::Owned(v.to_owned())))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        if !self.parse_array {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            return Ok(PromptContent::Ignored);
        }
        let mut texts = Vec::new();
        let mut has_image = false;
        let mut is_tool_result = false;
        while !is_tool_result {
            let Some(block) = seq.next_element::<Option<PromptBlock<'_>>>()? else {
                break;
            };
            let Some(block) = block else { continue };
            match block.kind.as_deref().unwrap_or_default() {
                "tool_result" => is_tool_result = true,
                "image" => has_image = true,
                "text" => {
                    if let Some(t) = block.text.as_deref() {
                        let trimmed = t.trim();
                        if !trimmed.is_empty()
                            && !is_synthetic_claude_prompt(trimmed)
                            && let Some(cleaned) = clean_claude_prompt_text(trimmed)
                        {
                            texts.push(cleaned);
                        }
                    }
                }
                _ => {}
            }
        }
        if is_tool_result {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            return Ok(PromptContent::Extracted(None));
        }
        let extracted = if !texts.is_empty() {
            Some(texts.join(" "))
        } else if has_image {
            Some("[image]".to_owned())
        } else {
            None
        };
        Ok(PromptContent::Extracted(extracted))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(PromptContent::Ignored)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(PromptContent::Ignored)
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
        Ok(PromptContent::Ignored)
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
        Ok(PromptContent::Ignored)
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
        Ok(PromptContent::Ignored)
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
        Ok(PromptContent::Ignored)
    }
}

pub(crate) struct ClaudeContextMessage<'a> {
    pub(crate) id: Option<Cow<'a, str>>,
    pub(crate) model: Option<Cow<'a, str>>,
    pub(crate) effort: Option<Cow<'a, str>>,
    pub(crate) stop_reason: Option<Cow<'a, str>>,
    pub(crate) usage: Option<ClaudeUsageFields>,
    pub(crate) content: Option<PromptContent<'a>>,
}

struct ClaudeContextMessageSeed {
    parse_array: bool,
}

impl<'de> DeserializeSeed<'de> for ClaudeContextMessageSeed {
    type Value = Option<ClaudeContextMessage<'de>>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(ClaudeContextMessageVisitor {
            parse_array: self.parse_array,
        })
    }
}

struct ClaudeContextMessageVisitor {
    parse_array: bool,
}

impl<'de> Visitor<'de> for ClaudeContextMessageVisitor {
    type Value = Option<ClaudeContextMessage<'de>>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("message object or null")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut msg = ClaudeContextMessage {
            id: None,
            model: None,
            effort: None,
            stop_reason: None,
            usage: None,
            content: None,
        };
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            match key.as_ref() {
                "id" => msg.id = map.next_value::<JsonOptStr<'de>>()?.0,
                "model" => msg.model = map.next_value()?,
                "effort" => msg.effort = map.next_value()?,
                "stop_reason" => msg.stop_reason = map.next_value()?,
                "usage" => msg.usage = map.next_value()?,
                "content" => {
                    msg.content = Some(map.next_value_seed(PromptContentSeed {
                        parse_array: self.parse_array,
                    })?);
                }
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(Some(msg))
    }
}

#[derive(Default)]
pub(crate) struct ClaudeContextLine<'a> {
    pub(crate) session_id: Option<Cow<'a, str>>,
    pub(crate) record_type: Option<Cow<'a, str>>,
    pub(crate) custom_title: Option<Cow<'a, str>>,
    pub(crate) title: Option<Cow<'a, str>>,
    pub(crate) name: Option<Cow<'a, str>>,
    pub(crate) ai_title: Option<Cow<'a, str>>,
    pub(crate) mode: Option<Cow<'a, str>>,
    pub(crate) permission_mode: Option<Cow<'a, str>>,
    pub(crate) model: Option<Cow<'a, str>>,
    pub(crate) effort: Option<Cow<'a, str>>,
    pub(crate) stop_reason: Option<Cow<'a, str>>,
    pub(crate) content: Option<PromptContent<'a>>,
    pub(crate) is_meta: Option<bool>,
    pub(crate) is_compact_summary: Option<bool>,
    pub(crate) message: Option<ClaudeContextMessage<'a>>,
    pub(crate) context_window: Option<CtxWindowSnake>,
    pub(crate) context_window_camel: Option<CtxWindowCamel>,
}

pub(crate) struct ClaudeContextLineSeed {
    pub(crate) parse_array: bool,
}

impl<'de> DeserializeSeed<'de> for ClaudeContextLineSeed {
    type Value = ClaudeContextLine<'de>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(ClaudeContextLineVisitor {
            parse_array: self.parse_array,
        })
    }
}

struct ClaudeContextLineVisitor {
    parse_array: bool,
}

impl<'de> Visitor<'de> for ClaudeContextLineVisitor {
    type Value = ClaudeContextLine<'de>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("claude context json line")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut rec = ClaudeContextLine::default();
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            match key.as_ref() {
                "sessionId" => rec.session_id = map.next_value()?,
                "type" => rec.record_type = map.next_value()?,
                "customTitle" => rec.custom_title = map.next_value()?,
                "title" => rec.title = map.next_value()?,
                "name" => rec.name = map.next_value()?,
                "aiTitle" => rec.ai_title = map.next_value()?,
                "mode" => rec.mode = map.next_value()?,
                "permissionMode" => rec.permission_mode = map.next_value()?,
                "model" => rec.model = map.next_value()?,
                "effort" => rec.effort = map.next_value()?,
                "stop_reason" => rec.stop_reason = map.next_value()?,
                "isMeta" => rec.is_meta = map.next_value()?,
                "isCompactSummary" => rec.is_compact_summary = map.next_value()?,
                "context_window" => rec.context_window = map.next_value()?,
                "contextWindow" => rec.context_window_camel = map.next_value()?,
                "content" => {
                    rec.content = Some(map.next_value_seed(PromptContentSeed {
                        parse_array: self.parse_array,
                    })?);
                }
                "message" => {
                    rec.message = map.next_value_seed(ClaudeContextMessageSeed {
                        parse_array: self.parse_array,
                    })?;
                }
                _ => {
                    let _ = map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(rec)
    }
}

pub(crate) fn is_synthetic_claude_prompt(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    if t.starts_with("[Request interrupted") || t.starts_with("Base directory for this skill:") {
        return true;
    }
    t.starts_with("<command-name")
        || t.starts_with("</command-name")
        || t.starts_with("<command-message")
        || t.starts_with("<command-args")
        || t.starts_with("<local-command-stdout")
        || t.starts_with("<local-command-caveat")
        || t.starts_with("<bash-input")
        || t.starts_with("<bash-stdout")
        || t.starts_with("<bash-stderr")
        || t.starts_with("<system-reminder")
}

pub(crate) fn clean_claude_prompt_text(text: &str) -> Option<String> {
    let mut cleaned = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find("[Image: source:") {
        cleaned.push_str(&remaining[..start]);
        if let Some(end) = remaining[start..].find(']') {
            remaining = &remaining[start + end + 1..];
        } else {
            remaining = "";
            break;
        }
    }
    cleaned.push_str(remaining);
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
    }
}

pub(crate) fn extract_claude_prompt_text(value: &Value) -> Option<String> {
    if value.get("isMeta").and_then(Value::as_bool) == Some(true)
        || value.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let content = value
        .pointer("/message/content")
        .or_else(|| value.get("content"))?;
    match content {
        Value::String(text) => {
            let trimmed = text.trim();
            if is_synthetic_claude_prompt(trimmed) {
                None
            } else {
                clean_claude_prompt_text(trimmed)
            }
        }
        Value::Array(blocks) => {
            // In real transcripts, over 85% of role=user lines are tool_result messages.
            // If any block is a tool_result, this is a tool return to the model, NOT an actual user prompt.
            if blocks
                .iter()
                .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            {
                return None;
            }
            let mut texts = Vec::new();
            let mut has_image = false;
            for block in blocks {
                let kind = block
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if kind == "image" {
                    has_image = true;
                } else if kind == "text"
                    && let Some(t) = block.get("text").and_then(Value::as_str)
                {
                    let trimmed = t.trim();
                    if !trimmed.is_empty()
                        && !is_synthetic_claude_prompt(trimmed)
                        && let Some(cleaned) = clean_claude_prompt_text(trimmed)
                    {
                        texts.push(cleaned);
                    }
                }
            }
            if !texts.is_empty() {
                Some(texts.join(" "))
            } else if has_image {
                Some("[image]".to_owned())
            } else {
                None
            }
        }
        _ => None,
    }
}
