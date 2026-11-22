use super::*;

/// Invalidates in-memory model catalog caches so a manual refresh re-reads immediately.
pub fn invalidate_model_catalog_cache() {
    if let Ok(mut guard) = memory_cache().write() {
        *guard = None;
    }
    #[cfg(not(test))]
    if let Ok(mut guard) = agent_catalogs_cache().write() {
        *guard = None;
    }
}

/// Returns the cache file path: `~/.agenthub/models_cache.json`.
fn disk_cache_path() -> Option<PathBuf> {
    crate::collectors::home_dir().map(|h| h.join(".agenthub").join("models_cache.json"))
}

/// Reads the cached live models from disk if valid and non-empty.
pub fn load_disk_cache() -> Option<DiskModelCache> {
    let p = disk_cache_path()?;
    if !p.is_file() {
        return None;
    }
    let data = fs::read_to_string(&p).ok()?;
    let cache = serde_json::from_str::<DiskModelCache>(&data).ok()?;
    (cache.schema_version == DISK_MODEL_CACHE_VERSION).then_some(cache)
}

/// Saves live models to disk cache.
pub fn save_disk_cache(base_url: &str, provider_name: &str, models: &[LiveCatalogModel]) {
    let Some(p) = disk_cache_path() else {
        return;
    };
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let cache = DiskModelCache {
        fetched_at_ms: now_ms,
        base_url: base_url.to_string(),
        provider_name: provider_name.to_string(),
        models: models.to_vec(),
        schema_version: DISK_MODEL_CACHE_VERSION,
    };

    if let Ok(json_str) = serde_json::to_string_pretty(&cache) {
        let _ = fs::write(p, json_str);
    }
}
