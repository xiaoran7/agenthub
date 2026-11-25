use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LiveCatalogModel {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub context_length: usize,
    pub context_window_str: String,
    pub max_output_tokens: Option<usize>,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub supports_reasoning: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub upstream_provider: String,
}

pub(crate) const DISK_MODEL_CACHE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskModelCache {
    pub fetched_at_ms: i64,
    pub base_url: String,
    pub provider_name: String,
    pub models: Vec<LiveCatalogModel>,
    #[serde(default)]
    pub schema_version: u32,
}

pub(crate) fn memory_cache()
-> &'static RwLock<Option<(Instant, Option<String>, Vec<LiveCatalogModel>)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, Option<String>, Vec<LiveCatalogModel>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

#[cfg(not(test))]
pub(crate) fn agent_catalogs_cache() -> &'static RwLock<Option<(Instant, Vec<(String, Vec<Value>)>)>>
{
    static CACHE: OnceLock<RwLock<Option<(Instant, Vec<(String, Vec<Value>)>)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

/// Formats raw token count into human-readable shorthand (e.g. 200000 -> "200K", 1000000 -> "1M").
pub fn format_context_tokens(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        format!("{tokens}")
    }
}

/// Infer context window length when upstream omits explicit `context_length`.
/// Returns 0 and empty string if not explicitly declared or identifiable from the identifier,
/// to avoid fabricating speculative context figures.
pub fn infer_context_window(model_id: &str) -> (usize, String) {
    let id = model_id.to_ascii_lowercase();

    let tokens = if id.contains("1m") {
        1_000_000
    } else if id.contains("2m") {
        2_000_000
    } else {
        0
    };

    let s = if tokens > 0 {
        format_context_tokens(tokens)
    } else {
        String::new()
    };
    (tokens, s)
}
