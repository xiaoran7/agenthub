use super::*;

#[derive(Clone)]
pub(crate) struct CachedDashboardEntry {
    pub(crate) cached_at: Instant,
    pub(crate) scan_gen: u64,
    pub(crate) plugin_gen: u64,
    pub(crate) port: u16,
    pub(crate) sessions_loading: bool,
    pub(crate) has_sessions: bool,
    pub(crate) json_bytes: Arc<Vec<u8>>,
    pub(crate) etag: String,
}

impl CachedDashboardEntry {
    fn from_summary(
        summary: &Value,
        scan_gen: u64,
        plugin_gen: u64,
        port: u16,
        sessions_loading: bool,
        has_sessions: bool,
    ) -> Self {
        let bytes = serde_json::to_vec(summary).unwrap_or_default();
        let etag = format!("\"dash-{:016x}\"", fnv1a64(&bytes));
        Self {
            cached_at: Instant::now(),
            scan_gen,
            plugin_gen,
            port,
            sessions_loading,
            has_sessions,
            json_bytes: Arc::new(bytes),
            etag,
        }
    }
}

pub(crate) fn dashboard_summary_cache() -> &'static RwLock<Option<CachedDashboardEntry>> {
    static CACHE: OnceLock<RwLock<Option<CachedDashboardEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub(crate) fn invalidate_dashboard_summary_cache() {
    if let Ok(mut guard) = dashboard_summary_cache().write() {
        *guard = None;
    }
}

/// Returns pre-serialized `(json_bytes, etag)` for `/api/dashboard` without cloning multi-MB JSON values.
pub(crate) fn build_cached_dashboard_payload(
    handler: &Arc<dyn DaemonRequestHandler>,
    port: u16,
    identity_path: &Path,
) -> (Arc<Vec<u8>>, String) {
    build_cached_dashboard_payload_with_store_ready(
        handler,
        port,
        identity_path,
        crate::remote_host::is_local_store_ready(),
    )
}

pub(crate) fn build_cached_dashboard_payload_with_store_ready(
    handler: &Arc<dyn DaemonRequestHandler>,
    port: u16,
    identity_path: &Path,
    local_store_ready: bool,
) -> (Arc<Vec<u8>>, String) {
    let current_scan_gen = crate::remote_host::local_store_scan_generation();
    let current_plugin_gen = crate::plugins::state_generation();
    if let Ok(guard) = dashboard_summary_cache().read()
        && let Some(ref entry) = *guard
    {
        let ttl = if entry.sessions_loading {
            Duration::from_millis(250)
        } else if entry.has_sessions {
            Duration::from_millis(1200)
        } else {
            Duration::from_millis(500)
        };
        if entry.scan_gen == current_scan_gen
            && entry.plugin_gen == current_plugin_gen
            && entry.port == port
            && entry.cached_at.elapsed() < ttl
        {
            return (Arc::clone(&entry.json_bytes), entry.etag.clone());
        }
    }

    let discover_res = handler.handle(DaemonRequest::Discover);
    let is_ready = matches!(
        discover_res,
        crate::daemon_protocol::DaemonResponse::Discovery(_)
    );
    let is_starting = matches!(
        &discover_res,
        crate::daemon_protocol::DaemonResponse::Error { code } if code == "daemon_starting"
    );
    let status_res = handler.handle(DaemonRequest::GetStatus);
    let (remote_enabled, remote_connected) = match &status_res {
        crate::daemon_protocol::DaemonResponse::Status {
            remote_enabled,
            remote_connected,
            ..
        } => (*remote_enabled, *remote_connected),
        _ => (false, false),
    };

    let stats = aggregate_dashboard_sessions(&discover_res);
    let models_json = build_dashboard_models_json(handler);
    let workspaces_json = build_dashboard_workspaces_json(&status_res, &stats.workspace_map);
    let (agents, active_agents_count) = build_dashboard_agents_json(&stats);

    let gateway_profile = crate::claude_gateway::discover_claude_code_gateway()
        .or_else(crate::claude_gateway::discover_claude_desktop_gateway);
    let gateway_detected = gateway_profile.is_some();
    let gateway_source = gateway_profile
        .as_ref()
        .map(|p| p.source.clone())
        .unwrap_or_else(|| "none".into());
    let gateway_base_url = gateway_profile
        .as_ref()
        .map(|p| p.base_url.clone())
        .unwrap_or_else(|| "https://api.anthropic.com".into());
    let gateway_models_count = gateway_profile
        .as_ref()
        .map(|p| p.models.len())
        .unwrap_or(models_json.len());

    let total_agents_count = agents.as_array().map(|a| a.len()).unwrap_or(0);
    let capabilities = crate::collectors::capabilities::discover_capabilities(None);

    let mut device_info = get_device_info(identity_path);
    if let Some(obj) = device_info.as_object_mut() {
        obj.insert("remote_enabled".to_string(), json!(remote_enabled));
        obj.insert("connected".to_string(), json!(remote_connected));
    }
    let sessions_loading = is_starting || !local_store_ready;
    let total_sessions = stats.total_sessions;

    let (local_plugins, plugin_registry_url) = crate::plugins::list_local_plugins_view_cached();
    let summary = json!({
        "ready": is_ready,
        "status": if is_starting { "starting" } else if is_ready { "ready" } else { "idle" },
        "sessions_loading": sessions_loading,
        "device_id": "local-dev",
        "port": port,
        "plugins": {
            "registry_url": plugin_registry_url,
            "items": local_plugins,
        },
        "stats": {
            "total_sessions": total_sessions,
            "claude_sessions": stats.claude_count,
            "claude_desktop_sessions": stats.claude_desktop_count,
            "antigravity_sessions": stats.antigravity_count,
            "codex_sessions": stats.codex_count,
            "deepseek_harness_sessions": stats.deepseek_harness_count,
            "provider_counts": stats.provider_counts,
            "active_agents": active_agents_count,
            "total_agents": total_agents_count,
            "total_models": models_json.len().max(gateway_models_count),
            "total_workspaces": workspaces_json.len(),
            "total_skills": capabilities.total_skills,
            "total_mcp_servers": capabilities.total_mcp_servers,
            "total_hooks": capabilities.total_hooks,
        },
        "agents": agents,
        "workspaces": workspaces_json,
        "models": models_json,
        "capabilities": capabilities,
        "gateway": {
            "detected": gateway_detected,
            "source": gateway_source,
            "base_url": gateway_base_url,
            "models_count": gateway_models_count,
            "affinity_header": "x-session-affinity"
        },
        "sessions": stats.discovered_sessions,
        "usage": stats.usage_summary,
        "device": device_info
    });

    if total_sessions == 0
        && (sessions_loading || is_starting)
        && let Ok(guard) = dashboard_summary_cache().read()
        && let Some(ref prev) = *guard
        && prev.has_sessions
        && let Ok(mut v) = serde_json::from_slice::<Value>(&prev.json_bytes)
    {
        v["port"] = json!(port);
        v["ready"] = json!(is_ready);
        v["status"] = json!(if is_starting { "starting" } else { "ready" });
        v["sessions_loading"] = json!(sessions_loading);
        if let Some(dev) = summary.get("device") {
            v["device"] = dev.clone();
        }
        let patched = serde_json::to_vec(&v).unwrap_or_default();
        let etag = format!("\"dash-{:016x}\"", fnv1a64(&patched));
        return (Arc::new(patched), etag);
    }

    let final_scan_gen = crate::remote_host::local_store_scan_generation();
    let final_plugin_gen = crate::plugins::state_generation();
    let entry = CachedDashboardEntry::from_summary(
        &summary,
        final_scan_gen,
        final_plugin_gen,
        port,
        sessions_loading,
        total_sessions > 0,
    );
    let out = (Arc::clone(&entry.json_bytes), entry.etag.clone());

    if is_ready && (total_sessions > 0 || !sessions_loading) {
        if let Ok(mut guard) = dashboard_summary_cache().write() {
            *guard = Some(entry);
        }
    } else if is_ready && total_sessions == 0 {
        let prev_had_sessions = dashboard_summary_cache()
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|e| e.has_sessions))
            .unwrap_or(false);
        if !prev_had_sessions && let Ok(mut guard) = dashboard_summary_cache().write() {
            *guard = Some(entry);
        }
    }
    out
}
