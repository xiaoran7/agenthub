use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_PLUGIN_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/xiaoran7/agenthub-plugins/main/registry.json";
pub const MAX_REGISTRY_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PLUGIN_FILE_BYTES: usize = 16 * 1024 * 1024;

pub const NATIVE_CORE_AGENTS: [&str; 5] = [
    "antigravity",
    "codex",
    "claude_desktop",
    "claude_code",
    "deepseek_harness",
];

pub fn is_reserved_native_agent(id: &str) -> bool {
    matches!(
        id.trim().to_ascii_lowercase().as_str(),
        "antigravity"
            | "codex"
            | "claude_desktop"
            | "claude_code"
            | "claude"
            | "deepseek_harness"
            | "acp"
    )
}

pub fn validate_plugin_id(id: &str) -> Result<(), String> {
    let trimmed = id.trim();
    if trimmed.len() < 2 || trimmed.len() > 64 {
        return Err("invalid_plugin_id_length".into());
    }
    let bytes = trimmed.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return Err("invalid_plugin_id_prefix".into());
    }
    if !bytes[1..]
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
    {
        return Err("invalid_plugin_id_chars".into());
    }
    if is_reserved_native_agent(trimmed) {
        return Err(format!("plugin_id_reserved_native:{trimmed}"));
    }
    Ok(())
}

pub fn validate_relative_path(rel: &str) -> Result<PathBuf, String> {
    let trimmed = rel.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.contains('\\')
        || trimmed.contains(':')
        || trimmed.contains('\0')
    {
        return Err(format!("unsafe_plugin_file_path:{rel}"));
    }
    let mut out = PathBuf::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("unsafe_plugin_file_segment:{rel}"));
        }
        out.push(part);
    }
    Ok(out)
}

fn default_true() -> bool {
    true
}

fn default_category() -> String {
    "agent".to_string()
}

fn default_protocol() -> String {
    "acp".to_string()
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginCapabilities {
    #[serde(default = "default_true")]
    pub discover: bool,
    #[serde(default = "default_true")]
    pub launch: bool,
    #[serde(default = "default_true")]
    pub resume: bool,
    #[serde(default = "default_true")]
    pub models: bool,
    #[serde(default = "default_true")]
    pub history: bool,
    #[serde(default)]
    pub approvals: bool,
}

impl Default for PluginCapabilities {
    fn default() -> Self {
        Self {
            discover: true,
            launch: true,
            resume: true,
            models: true,
            history: true,
            approvals: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginRuntimeSpec {
    #[serde(default = "default_protocol")]
    pub protocol: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub binary_hints: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub version: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub min_runner_version: Option<String>,
    #[serde(default)]
    pub session_prefix: Option<String>,
    #[serde(default)]
    pub session_prefixes: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    pub runtime: PluginRuntimeSpec,
}

fn resolve_primary_prefix(id: &str, session_prefix: Option<&str>) -> String {
    session_prefix
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{id}_"))
}

fn dedup_session_prefixes(primary: String, extra: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(1 + extra.len());
    out.push(primary);
    for item in extra {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !out.iter().any(|existing| existing == trimmed) {
            out.push(trimmed.to_owned());
        }
    }
    out
}

impl PluginManifest {
    pub fn effective_display_name(&self) -> String {
        self.display_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.name)
            .to_string()
    }

    pub fn effective_session_prefix(&self) -> String {
        resolve_primary_prefix(&self.id, self.session_prefix.as_deref())
    }

    pub fn effective_session_prefixes(&self) -> Vec<String> {
        dedup_session_prefixes(self.effective_session_prefix(), &self.session_prefixes)
    }

    pub fn matches_provider(&self, provider: &str) -> bool {
        let norm = provider.trim().to_ascii_lowercase();
        if norm.is_empty() {
            return false;
        }
        self.id.eq_ignore_ascii_case(&norm)
            || self
                .aliases
                .iter()
                .any(|alias| alias.trim().eq_ignore_ascii_case(&norm))
    }

    pub fn matches_session_id(&self, session_id: &str) -> bool {
        let prefix = self.effective_session_prefix();
        if session_id.starts_with(&prefix) {
            return true;
        }
        self.session_prefixes
            .iter()
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .any(|p| session_id.starts_with(p))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryFileEntry {
    pub path: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryPluginEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub version: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub min_runner_version: Option<String>,
    #[serde(default)]
    pub session_prefix: Option<String>,
    #[serde(default)]
    pub session_prefixes: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub runtime: Option<PluginRuntimeSpec>,
    #[serde(default)]
    pub files: Vec<RegistryFileEntry>,
}

impl RegistryPluginEntry {
    pub fn effective_session_prefix(&self) -> String {
        resolve_primary_prefix(&self.id, self.session_prefix.as_deref())
    }

    pub fn effective_session_prefixes(&self) -> Vec<String> {
        dedup_session_prefixes(self.effective_session_prefix(), &self.session_prefixes)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryDocument {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub plugins: Vec<RegistryPluginEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledPluginRecord {
    pub id: String,
    pub version: String,
    pub enabled: bool,
    pub install_dir: String,
    pub installed_at: String,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub runtime_status: String,
    #[serde(default)]
    pub error: Option<String>,
    pub manifest: PluginManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginStateFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub registry_url: Option<String>,
    #[serde(default)]
    pub installed: BTreeMap<String, InstalledPluginRecord>,
}

impl Default for PluginStateFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            registry_url: None,
            installed: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub category: String,
    pub description: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    pub installed: bool,
    pub enabled: bool,
    pub runtime_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub session_prefix: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_prefixes: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub capabilities: PluginCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemotePluginCommand {
    #[serde(default)]
    pub request_id: Option<String>,
    pub action: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub registry_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginCommandOutcome {
    pub ok: bool,
    pub plugins: Vec<PluginView>,
    pub registry_url: String,
    pub error: Option<String>,
}
