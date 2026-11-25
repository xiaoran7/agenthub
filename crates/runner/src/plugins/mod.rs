pub mod installer;
pub mod runtime;
pub mod security;
pub mod store;
pub mod types;

#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use chrono::Utc;
use serde_json::{Value, json};

pub use types::{
    DEFAULT_PLUGIN_REGISTRY_URL, InstalledPluginRecord, NATIVE_CORE_AGENTS, PluginCapabilities,
    PluginCommandOutcome, PluginManifest, PluginRuntimeSpec, PluginStateFile, PluginView,
    RegistryDocument, RegistryFileEntry, RegistryPluginEntry, RemotePluginCommand,
    is_reserved_native_agent, validate_plugin_id,
};

use self::{
    installer::{
        commit_staged_dir, fetch_registry_document, fetch_remote_registry_document,
        has_scheme_prefix, install_from_local_directory, remove_installed_dir,
        stage_from_registry_with_policy, validate_trusted_remote_registry_url,
    },
    runtime::{
        cached_plugin_metadata, invalidate_runtime_cache, query_plugin_history,
        query_plugin_metadata, spawn_plugin_server,
    },
    store::{PluginStore, bump_state_generation, current_state_generation},
};

const REGISTRY_CACHE_TTL_MS: i64 = 5 * 60 * 1000;

pub struct PluginManager {
    store: PluginStore,
    lock: Mutex<()>,
}

impl PluginManager {
    pub fn with_dir(root: PathBuf) -> Self {
        Self {
            store: PluginStore::new(root),
            lock: Mutex::new(()),
        }
    }

    pub fn registry_url(&self) -> String {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.store.load_state();
        self.store.effective_registry_url(&state)
    }

    pub fn set_registry_url(&self, url: Option<&str>) -> Result<String, String> {
        self.set_registry_url_internal(url, false)
    }

    pub fn set_remote_registry_url(&self, url: Option<&str>) -> Result<String, String> {
        self.set_registry_url_internal(url, true)
    }

    fn set_registry_url_internal(
        &self,
        url: Option<&str>,
        enforce_remote_policy: bool,
    ) -> Result<String, String> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.store.load_state_for_write()?;
        let mut cleaned = url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(u) = cleaned.take() {
            if enforce_remote_policy {
                let normalized = validate_trusted_remote_registry_url(&u)?;
                cleaned = Some(normalized.to_string());
            } else if has_scheme_prefix(&u, "https://") || has_scheme_prefix(&u, "http://") {
                let parsed = url::Url::parse(&u).map_err(|_| "invalid_registry_url".to_string())?;
                cleaned = Some(parsed.to_string());
            } else if has_scheme_prefix(&u, "file://") || Path::new(&u).exists() {
                cleaned = Some(u);
            } else {
                return Err("invalid_registry_url".into());
            }
        }
        state.registry_url = cleaned;
        self.store.save_state(&state)?;
        bump_state_generation();
        Ok(self.store.effective_registry_url(&state))
    }

    pub fn refresh_registry(&self) -> Result<(RegistryDocument, String), String> {
        self.refresh_registry_internal(false)
    }

    pub fn refresh_remote_registry(&self) -> Result<(RegistryDocument, String), String> {
        self.refresh_registry_internal(true)
    }

    fn refresh_registry_internal(
        &self,
        enforce_remote_policy: bool,
    ) -> Result<(RegistryDocument, String), String> {
        let url = {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            let state = self.store.load_state();
            self.store.effective_registry_url(&state)
        };
        let doc = if enforce_remote_policy {
            fetch_remote_registry_document(&url)?
        } else {
            fetch_registry_document(&url)?
        };
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let _ = self.store.save_cached_registry(&url, &doc);
        Ok((doc, url))
    }

    fn load_registry_for_view(
        &self,
        state: &PluginStateFile,
        force_refresh: bool,
    ) -> (Option<RegistryDocument>, String, Option<String>) {
        let url = self.store.effective_registry_url(state);
        if !force_refresh
            && let Some(doc) = self.store.load_cached_registry(&url, REGISTRY_CACHE_TTL_MS)
        {
            return (Some(doc), url, None);
        }
        match fetch_registry_document(&url) {
            Ok(doc) => {
                let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
                let _ = self.store.save_cached_registry(&url, &doc);
                (Some(doc), url, None)
            }
            Err(err) => {
                let stale = self.store.load_stale_cached_registry(&url);
                (stale, url, Some(err))
            }
        }
    }

    pub fn installed_plugin_names(&self) -> HashMap<String, String> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.store
            .load_state()
            .installed
            .into_iter()
            .map(|(id, rec)| (id, rec.manifest.name))
            .collect()
    }

    pub fn list_local_plugins_view(&self) -> (Vec<PluginView>, String) {
        let state = {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            self.store.load_state()
        };
        let reg_url = self.store.effective_registry_url(&state);
        let stale = self.store.load_stale_cached_registry(&reg_url);
        (self.build_plugins_view(state, stale, false), reg_url)
    }

    pub fn list_plugins_view(
        &self,
        force_refresh: bool,
    ) -> (Vec<PluginView>, String, Option<String>) {
        let state = {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            self.store.load_state()
        };
        let (reg_opt, reg_url, reg_err) = self.load_registry_for_view(&state, force_refresh);
        (
            self.build_plugins_view(state, reg_opt, true),
            reg_url,
            reg_err,
        )
    }

    fn build_plugins_view(
        &self,
        state: PluginStateFile,
        reg_opt: Option<RegistryDocument>,
        probe_live: bool,
    ) -> Vec<PluginView> {
        let mut by_id: BTreeMap<String, PluginView> = BTreeMap::new();
        let now = Utc::now().to_rfc3339();

        if let Some(reg) = reg_opt {
            for entry in reg.plugins {
                if validate_plugin_id(&entry.id).is_err() {
                    continue;
                }
                let display_name = entry
                    .display_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&entry.name)
                    .to_string();
                let prefix = entry.effective_session_prefix();
                let session_prefixes = entry.effective_session_prefixes();
                by_id.insert(
                    entry.id.clone(),
                    PluginView {
                        id: entry.id,
                        name: entry.name,
                        display_name,
                        category: entry.category,
                        description: entry.description,
                        version: entry.version,
                        installed_version: None,
                        installed: false,
                        enabled: false,
                        runtime_status: "not_installed".to_string(),
                        error: None,
                        session_prefix: prefix,
                        session_prefixes,
                        aliases: entry.aliases,
                        icon: entry.icon,
                        capabilities: entry.capabilities,
                        available: None,
                        binary_path: None,
                        source_url: None,
                        updated_at: now.clone(),
                    },
                );
            }
        }

        for (id, rec) in state.installed {
            let meta = if rec.enabled {
                if probe_live {
                    Some(query_plugin_metadata(&rec))
                } else {
                    cached_plugin_metadata(&rec)
                }
            } else {
                None
            };
            let (status, err, available, binary_path) = if !rec.enabled {
                ("disabled".to_string(), None, None, None)
            } else if let Some(ref m) = meta {
                (
                    "ready".to_string(),
                    m.error.clone(),
                    m.available,
                    m.binary_path.clone(),
                )
            } else {
                (
                    if rec.runtime_status.is_empty() {
                        "ready".to_string()
                    } else {
                        rec.runtime_status.clone()
                    },
                    rec.error.clone(),
                    None,
                    None,
                )
            };

            let remote_version = by_id
                .get(&id)
                .map(|v| v.version.clone())
                .unwrap_or_else(|| rec.version.clone());

            by_id.insert(
                id.clone(),
                PluginView {
                    id: rec.id.clone(),
                    name: rec.manifest.name.clone(),
                    display_name: rec.manifest.effective_display_name(),
                    category: rec.manifest.category.clone(),
                    description: rec.manifest.description.clone(),
                    version: remote_version,
                    installed_version: Some(rec.version.clone()),
                    installed: true,
                    enabled: rec.enabled,
                    runtime_status: status,
                    error: err,
                    session_prefix: rec.manifest.effective_session_prefix(),
                    session_prefixes: rec.manifest.effective_session_prefixes(),
                    aliases: rec.manifest.aliases.clone(),
                    icon: rec.manifest.icon.clone(),
                    capabilities: rec.manifest.capabilities.clone(),
                    available,
                    binary_path,
                    source_url: rec.source_url.clone(),
                    updated_at: rec.installed_at.clone(),
                },
            );
        }

        by_id.into_values().collect()
    }

    pub fn list_installed_enabled(&self) -> Vec<InstalledPluginRecord> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.store.load_state();
        state
            .installed
            .into_values()
            .filter(|rec| rec.enabled)
            .collect()
    }

    pub fn get_installed(&self, id: &str) -> Option<InstalledPluginRecord> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.store.load_state();
        state.installed.get(id).cloned()
    }

    pub fn get_enabled_plugin(&self, provider_or_id: &str) -> Option<InstalledPluginRecord> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.store.load_state();
        state
            .installed
            .into_values()
            .find(|rec| rec.enabled && rec.manifest.matches_provider(provider_or_id))
    }

    pub fn find_enabled_plugin_for_session(
        &self,
        session_id: &str,
    ) -> Option<InstalledPluginRecord> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.store.load_state();
        state
            .installed
            .into_values()
            .find(|rec| rec.enabled && rec.manifest.matches_session_id(session_id))
    }

    pub fn is_plugin_enabled(&self, id: &str) -> bool {
        self.get_enabled_plugin(id).is_some()
    }

    pub fn install_plugin(
        &self,
        plugin_id: &str,
        source_url: Option<&str>,
        enable_after: bool,
    ) -> Result<InstalledPluginRecord, String> {
        self.install_plugin_internal(plugin_id, source_url, enable_after, false)
    }

    fn install_plugin_internal(
        &self,
        plugin_id: &str,
        source_url: Option<&str>,
        enable_after: bool,
        enforce_remote_policy: bool,
    ) -> Result<InstalledPluginRecord, String> {
        if let Some(src) = source_url.map(str::trim).filter(|s| !s.is_empty()) {
            if enforce_remote_policy {
                return Err("remote_source_url_unsupported".into());
            }
            let expected_id = (!plugin_id.trim().is_empty()).then_some(plugin_id.trim());
            let record = {
                let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
                let mut state = self.store.load_state_for_write()?;
                let record =
                    install_from_local_directory(&self.store, src, expected_id, enable_after)?;
                state.installed.insert(record.id.clone(), record.clone());
                self.store.save_state(&state)?;
                invalidate_runtime_cache(Some(&record.id));
                bump_state_generation();
                record
            };
            if enable_after {
                let _ = query_plugin_metadata(&record);
            }
            return Ok(record);
        }

        validate_plugin_id(plugin_id)?;
        let reg_url = self.registry_url();
        let doc = if enforce_remote_policy {
            fetch_remote_registry_document(&reg_url)?
        } else {
            fetch_registry_document(&reg_url)?
        };
        {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            let _ = self.store.save_cached_registry(&reg_url, &doc);
        }
        let entry = doc
            .plugins
            .iter()
            .find(|p| p.id == plugin_id)
            .ok_or_else(|| format!("plugin_not_found_in_registry:{plugin_id}"))?;
        let (staging, manifest) =
            stage_from_registry_with_policy(&self.store, &reg_url, entry, enforce_remote_policy)?;

        let record = {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut state = self.store.load_state_for_write()?;
            let record = commit_staged_dir(
                &self.store,
                staging.keep(),
                &manifest,
                Some(&reg_url),
                enable_after,
            )?;
            state.installed.insert(record.id.clone(), record.clone());
            self.store.save_state(&state)?;
            invalidate_runtime_cache(Some(&record.id));
            bump_state_generation();
            record
        };
        if enable_after {
            let _ = query_plugin_metadata(&record);
        }
        Ok(record)
    }

    pub fn toggle_plugin(
        &self,
        plugin_id: &str,
        enabled: Option<bool>,
    ) -> Result<InstalledPluginRecord, String> {
        validate_plugin_id(plugin_id)?;
        let out = {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut state = self.store.load_state_for_write()?;
            let rec = state
                .installed
                .get_mut(plugin_id)
                .ok_or_else(|| format!("plugin_not_installed:{plugin_id}"))?;
            let next = enabled.unwrap_or(!rec.enabled);
            rec.enabled = next;
            rec.runtime_status = if next {
                "ready".to_string()
            } else {
                "disabled".to_string()
            };
            rec.error = None;
            let out = rec.clone();
            self.store.save_state(&state)?;
            invalidate_runtime_cache(Some(plugin_id));
            bump_state_generation();
            out
        };
        if out.enabled {
            let _ = query_plugin_metadata(&out);
        }
        Ok(out)
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) -> Result<(), String> {
        validate_plugin_id(plugin_id)?;
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = self.store.load_state_for_write()?;
        state.installed.remove(plugin_id);
        self.store.save_state(&state)?;
        invalidate_runtime_cache(Some(plugin_id));
        bump_state_generation();
        remove_installed_dir(&self.store, plugin_id)?;
        Ok(())
    }

    pub fn handle_remote_command(&self, cmd: &RemotePluginCommand) -> PluginCommandOutcome {
        let mut err: Option<String> = None;
        let mut refreshed_doc: Option<RegistryDocument> = None;
        let mut fetch_catalog = false;
        let mut probe_live = false;
        match cmd.action.as_str() {
            "list" => {}
            "catalog" | "refresh_catalog" => {
                fetch_catalog = true;
                match self.refresh_remote_registry() {
                    Ok((doc, _)) => refreshed_doc = Some(doc),
                    Err(e) => err = Some(e),
                }
            }
            "set_registry" => {
                fetch_catalog = true;
                if let Err(e) = self.set_remote_registry_url(cmd.registry_url.as_deref()) {
                    err = Some(e);
                } else {
                    match self.refresh_remote_registry() {
                        Ok((doc, _)) => refreshed_doc = Some(doc),
                        Err(e) => err = Some(e),
                    }
                }
            }
            "install" => {
                if cmd
                    .source_url
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
                {
                    err = Some("remote_source_url_unsupported".into());
                } else {
                    let pid = cmd.plugin_id.as_deref().unwrap_or("");
                    probe_live = true;
                    if let Err(e) = self.install_plugin_internal(pid, None, true, true) {
                        err = Some(e);
                    }
                }
            }
            "uninstall" => {
                let pid = cmd.plugin_id.as_deref().unwrap_or("");
                if let Err(e) = self.uninstall_plugin(pid) {
                    err = Some(e);
                }
            }
            "toggle" | "enable" | "disable" => {
                let pid = cmd.plugin_id.as_deref().unwrap_or("");
                let target = match cmd.action.as_str() {
                    "enable" => Some(true),
                    "disable" => Some(false),
                    _ => cmd.enabled,
                };
                probe_live = true;
                if let Err(e) = self.toggle_plugin(pid, target) {
                    err = Some(e);
                }
            }
            other => {
                err = Some(format!("unsupported_plugin_action:{other}"));
            }
        }
        let state = {
            let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            self.store.load_state()
        };
        let registry_url = self.store.effective_registry_url(&state);
        let reg_opt = if refreshed_doc.is_some() {
            refreshed_doc
        } else if fetch_catalog && err.is_none() {
            let (doc, _, fetch_err) = self.load_registry_for_view(&state, true);
            if err.is_none() {
                err = fetch_err;
            }
            doc
        } else {
            self.store.load_stale_cached_registry(&registry_url)
        };
        let plugins = self.build_plugins_view(state, reg_opt, probe_live);
        PluginCommandOutcome {
            ok: err.is_none(),
            plugins,
            registry_url,
            error: err,
        }
    }

    pub fn discover_all_sessions(&self) -> Vec<Value> {
        let enabled = self.list_installed_enabled();
        let mut sessions = Vec::new();
        for rec in enabled {
            if !rec.manifest.capabilities.discover {
                continue;
            }
            let snap = query_plugin_metadata(&rec);
            sessions.extend(snap.sessions);
        }
        sessions
    }

    pub fn collect_all_models(&self) -> Vec<(String, Vec<Value>)> {
        let enabled = self.list_installed_enabled();
        let mut out = Vec::new();
        for rec in enabled {
            if !rec.manifest.capabilities.models {
                continue;
            }
            let snap = query_plugin_metadata(&rec);
            if !snap.models.is_empty() {
                out.push((rec.id, snap.models));
            }
        }
        out
    }

    pub fn load_conversation(
        &self,
        plugin_id: &str,
        session_id: &str,
        workspace: Option<&Path>,
    ) -> Vec<Value> {
        let Some(rec) = self.get_enabled_plugin(plugin_id) else {
            return Vec::new();
        };
        query_plugin_history(&rec, session_id, workspace)
    }

    pub fn read_icon(&self, plugin_id: &str) -> Option<(Vec<u8>, &'static str)> {
        let rec = self.get_installed(plugin_id)?;
        self.store.read_icon(&rec)
    }

    pub fn query_metadata_for(
        &self,
        rec: &InstalledPluginRecord,
    ) -> runtime::PluginMetadataSnapshot {
        query_plugin_metadata(rec)
    }

    pub fn spawn_acp_server(
        &self,
        plugin_id: &str,
        timeout: std::time::Duration,
    ) -> Result<(InstalledPluginRecord, crate::collectors::AcpAppServer), String> {
        let rec = self
            .get_enabled_plugin(plugin_id)
            .ok_or_else(|| "plugin_disabled".to_string())?;
        let server = spawn_plugin_server(&rec, timeout)?;
        Ok((rec, server))
    }
}

pub fn global_manager() -> &'static PluginManager {
    static MANAGER: OnceLock<PluginManager> = OnceLock::new();
    MANAGER.get_or_init(|| PluginManager::with_dir(PluginStore::resolve_default_root()))
}

pub fn state_generation() -> u64 {
    current_state_generation()
}

pub fn is_plugin_enabled(plugin_id: &str) -> bool {
    global_manager().is_plugin_enabled(plugin_id)
}

pub fn list_installed_enabled() -> Vec<InstalledPluginRecord> {
    global_manager().list_installed_enabled()
}

pub fn get_enabled_plugin(provider_or_id: &str) -> Option<InstalledPluginRecord> {
    global_manager().get_enabled_plugin(provider_or_id)
}

pub fn find_enabled_plugin_for_session(session_id: &str) -> Option<InstalledPluginRecord> {
    global_manager().find_enabled_plugin_for_session(session_id)
}

pub fn list_local_plugins_view() -> (Vec<PluginView>, String) {
    global_manager().list_local_plugins_view()
}

pub fn list_local_plugins_view_cached() -> (Vec<PluginView>, String) {
    global_manager().list_local_plugins_view()
}

pub fn installed_plugin_names() -> HashMap<String, String> {
    global_manager().installed_plugin_names()
}

pub fn list_plugins_view(force_refresh: bool) -> (Vec<PluginView>, String, Option<String>) {
    global_manager().list_plugins_view(force_refresh)
}

pub fn ready_plugins_payload() -> Vec<Value> {
    let (views, _) = global_manager().list_local_plugins_view();
    views
        .into_iter()
        .filter_map(|v| serde_json::to_value(v).ok())
        .collect()
}

pub fn discover_all_plugin_sessions() -> Vec<Value> {
    global_manager().discover_all_sessions()
}

pub fn collect_plugin_models() -> Vec<(String, Vec<Value>)> {
    global_manager().collect_all_models()
}

pub fn load_plugin_conversation(
    plugin_id: &str,
    session_id: &str,
    workspace: Option<&Path>,
) -> Vec<Value> {
    global_manager().load_conversation(plugin_id, session_id, workspace)
}

pub fn read_plugin_icon(plugin_id: &str) -> Option<(Vec<u8>, &'static str)> {
    global_manager().read_icon(plugin_id)
}

pub fn install_plugin(
    plugin_id: &str,
    source_url: Option<&str>,
    enable_after: bool,
) -> Result<InstalledPluginRecord, String> {
    global_manager().install_plugin(plugin_id, source_url, enable_after)
}

pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    global_manager().uninstall_plugin(plugin_id)
}

pub fn toggle_plugin(
    plugin_id: &str,
    enabled: Option<bool>,
) -> Result<InstalledPluginRecord, String> {
    global_manager().toggle_plugin(plugin_id, enabled)
}

pub fn set_registry_url(url: Option<&str>) -> Result<String, String> {
    global_manager().set_registry_url(url)
}

pub fn handle_remote_plugin_command(cmd: &RemotePluginCommand) -> PluginCommandOutcome {
    global_manager().handle_remote_command(cmd)
}

pub fn downloading_plugin_state_frame(
    plugins: &[PluginView],
    plugin_id: &str,
    request_id: Option<&str>,
    registry_url: &str,
) -> Value {
    let patched: Vec<PluginView> = plugins
        .iter()
        .cloned()
        .map(|mut p| {
            if p.id == plugin_id {
                p.runtime_status = "downloading".to_string();
                p.error = None;
            }
            p
        })
        .collect();
    json!({
        "type": "plugin_state",
        "request_id": request_id,
        "registry_url": registry_url,
        "plugins": patched,
    })
}
