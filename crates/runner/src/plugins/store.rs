use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::types::{
    DEFAULT_PLUGIN_REGISTRY_URL, InstalledPluginRecord, PluginStateFile, RegistryDocument,
    validate_plugin_id, validate_relative_path,
};

static PLUGIN_STATE_GEN: AtomicU64 = AtomicU64::new(1);

pub fn current_state_generation() -> u64 {
    PLUGIN_STATE_GEN.load(Ordering::Acquire)
}

pub fn bump_state_generation() -> u64 {
    PLUGIN_STATE_GEN.fetch_add(1, Ordering::AcqRel) + 1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryCacheFile {
    pub fetched_at_ms: i64,
    pub registry_url: String,
    pub document: RegistryDocument,
}

#[derive(Debug, Clone)]
pub struct PluginStore {
    root: PathBuf,
}

impl PluginStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn resolve_default_root() -> PathBuf {
        if let Some(dir) = std::env::var_os("AGENTHUB_PLUGINS_DIR").filter(|v| !v.is_empty()) {
            return PathBuf::from(dir);
        }
        #[cfg(test)]
        {
            std::env::temp_dir().join(format!("agenthub-test-plugins-{}", std::process::id()))
        }
        #[cfg(not(test))]
        {
            crate::collectors::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".agenthub")
                .join("plugins")
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn installed_dir(&self) -> PathBuf {
        self.root.join("installed")
    }

    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn registry_cache_path(&self) -> PathBuf {
        self.root.join("registry-cache.json")
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        fs::create_dir_all(self.installed_dir()).map_err(|e| format!("create_plugins_dir:{e}"))?;
        fs::create_dir_all(self.staging_dir()).map_err(|e| format!("create_staging_dir:{e}"))?;
        Ok(())
    }

    pub fn effective_registry_url(&self, state: &PluginStateFile) -> String {
        if let Ok(env_url) = std::env::var("AGENTHUB_PLUGIN_REGISTRY_URL") {
            let trimmed = env_url.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        state
            .registry_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_PLUGIN_REGISTRY_URL)
            .to_string()
    }

    pub fn load_state(&self) -> PluginStateFile {
        let path = self.state_path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return PluginStateFile::default();
        };
        let mut parsed: PluginStateFile = serde_json::from_str(&raw).unwrap_or_default();
        // Prune records whose directory disappeared from disk so state.json stays truthful.
        parsed.installed.retain(|id, rec| {
            validate_plugin_id(id).is_ok() && Path::new(&rec.install_dir).is_dir()
        });
        parsed
    }

    pub fn save_state(&self, state: &PluginStateFile) -> Result<(), String> {
        self.ensure_dirs()?;
        let path = self.state_path();
        let tmp = self.root.join(format!(
            "state.{}.{}.tmp",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let payload =
            serde_json::to_vec_pretty(state).map_err(|e| format!("serialize_plugin_state:{e}"))?;
        fs::write(&tmp, payload).map_err(|e| format!("write_plugin_state_tmp:{e}"))?;
        if let Err(err) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&path);
            fs::rename(&tmp, &path).map_err(|e| {
                let _ = fs::remove_file(&tmp);
                format!("rename_plugin_state:{err}/{e}")
            })?;
        }
        Ok(())
    }

    pub fn load_cached_registry(
        &self,
        expected_url: &str,
        ttl_ms: i64,
    ) -> Option<RegistryDocument> {
        let raw = fs::read_to_string(self.registry_cache_path()).ok()?;
        let cache: RegistryCacheFile = serde_json::from_str(&raw).ok()?;
        if cache.registry_url != expected_url {
            return None;
        }
        let age = Utc::now()
            .timestamp_millis()
            .saturating_sub(cache.fetched_at_ms);
        if age > ttl_ms {
            return None;
        }
        Some(cache.document)
    }

    pub fn load_stale_cached_registry(&self, expected_url: &str) -> Option<RegistryDocument> {
        let raw = fs::read_to_string(self.registry_cache_path()).ok()?;
        let cache: RegistryCacheFile = serde_json::from_str(&raw).ok()?;
        if cache.registry_url == expected_url {
            Some(cache.document)
        } else {
            None
        }
    }

    pub fn save_cached_registry(
        &self,
        registry_url: &str,
        document: &RegistryDocument,
    ) -> Result<(), String> {
        self.ensure_dirs()?;
        let cache = RegistryCacheFile {
            fetched_at_ms: Utc::now().timestamp_millis(),
            registry_url: registry_url.to_string(),
            document: document.clone(),
        };
        let payload =
            serde_json::to_vec_pretty(&cache).map_err(|e| format!("serialize_cache:{e}"))?;
        fs::write(self.registry_cache_path(), payload).map_err(|e| format!("write_cache:{e}"))
    }

    pub fn read_icon(&self, record: &InstalledPluginRecord) -> Option<(Vec<u8>, &'static str)> {
        let icon_rel = record.manifest.icon.as_deref()?;
        let rel_path = validate_relative_path(icon_rel).ok()?;
        let full = Path::new(&record.install_dir).join(&rel_path);
        if !full.is_file() {
            return None;
        }
        let bytes = fs::read(&full).ok()?;
        if bytes.len() > 512 * 1024 {
            return None;
        }
        let ext = full
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "svg" => {
                if !is_safe_svg(&bytes) {
                    return None;
                }
                "image/svg+xml"
            }
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => "application/octet-stream",
        };
        Some((bytes, mime))
    }
}

fn is_safe_svg(bytes: &[u8]) -> bool {
    let Ok(raw) = std::str::from_utf8(bytes) else {
        return false;
    };
    let lower = raw.to_ascii_lowercase();
    if lower.contains("<script")
        || lower.contains("<foreignobject")
        || lower.contains("javascript:")
        || lower.contains("vbscript:")
        || lower.contains("data:text/html")
    {
        return false;
    }
    // Reject inline event handler attributes such as onload=, onclick=, onerror=.
    let b = lower.as_bytes();
    for i in 0..b.len().saturating_sub(3) {
        if (b[i].is_ascii_whitespace() || b[i] == b'<' || b[i] == b'"' || b[i] == b'\'')
            && b[i + 1] == b'o'
            && b[i + 2] == b'n'
            && b[i + 3].is_ascii_alphabetic()
        {
            let mut j = i + 3;
            while j < b.len() && b[j].is_ascii_alphanumeric() {
                j += 1;
            }
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < b.len() && b[j] == b'=' {
                return false;
            }
        }
    }
    true
}
