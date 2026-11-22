//! Magpie-grade Live Model Catalog Engine.
//!
//! Features:
//! - Discovers active providers from CC Switch (`~/.cc-switch/cc-switch.db`),
//!   environment variables (`ANTHROPIC_BASE_URL`, `OPENAI_BASE_URL`), and local profiles.
//! - Queries `/v1/models` and `/models` endpoints with authentic headers:
//!   `Authorization: Bearer <key>`, `x-api-key: <key>`, `anthropic-version: 2023-06-01`,
//!   `Accept: application/json`, `User-Agent: agenthub-runner`.
//! - Supports both OpenAI (`data: []`) and Anthropic (`models: []`) list payloads,
//!   including cursor pagination (`has_more`, `last_id`, `after_id`).
//! - Filters out non-LLM artifacts (TTS, ASR, image generation, audio, embeddings, rerank).
//! - Extracts rich metadata: display names, context window length (e.g. 200K, 1M),
//!   max output tokens, input/output modalities (Vision, Tools, Text, Reasoning).
//! - Persists models to disk (`~/.agenthub/models_cache.json`) with in-memory TTL caching.

pub use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{OnceLock, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub use serde::{Deserialize, Serialize};
pub use serde_json::{Value, json};

pub mod types;
pub use types::*;

pub mod disk_cache;
pub use disk_cache::*;

pub mod discovery;
pub use discovery::*;

pub mod static_catalogs;
pub use static_catalogs::*;

#[cfg(test)]
mod tests;
