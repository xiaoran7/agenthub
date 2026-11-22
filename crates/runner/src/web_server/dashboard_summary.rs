use super::*;

pub(crate) struct DashboardSessionStats {
    pub(crate) total_sessions: usize,
    pub(crate) claude_count: usize,
    pub(crate) claude_desktop_count: usize,
    pub(crate) antigravity_count: usize,
    pub(crate) codex_count: usize,
    pub(crate) deepseek_harness_count: usize,
    pub(crate) provider_counts: std::collections::BTreeMap<String, usize>,
    pub(crate) discovered_sessions: Vec<Value>,
    pub(crate) workspace_map: HashMap<String, usize>,
    pub(crate) usage_summary: Value,
}

struct ModelAgg {
    name: String,
    provider: String,
    calls: usize,
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_read_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
}

struct AgentAgg {
    id: String,
    name: String,
    calls: usize,
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_read_tokens: u64,
    total_tokens: u64,
    cost_usd: f64,
}

fn default_model_for_provider(provider: &str) -> &str {
    match provider {
        "claude_code" | "claude" | "claude_desktop" => "claude-sonnet-4-6",
        "codex" => "gpt-6.1-sol",
        "antigravity" => "gemini-4-carbon-high",
        "deepseek_harness" | "deepseek" => "deepseek-reasoner",
        _ => "general-agent-model",
    }
}

fn agent_identity_for_provider(
    provider: &str,
    plugin_names: &HashMap<String, String>,
) -> (String, String) {
    match provider {
        "claude_code" | "claude" => ("claude_code".into(), "Claude Code".into()),
        "codex" => ("codex".into(), "Codex App-Server".into()),
        "claude_desktop" => ("claude_desktop".into(), "Claude Desktop".into()),
        "antigravity" => ("antigravity".into(), "Antigravity".into()),
        "deepseek_harness" | "deepseek" => ("deepseek_harness".into(), "DeepSeek Harness".into()),
        other if !other.trim().is_empty() => {
            if let Some(name) = plugin_names.get(other) {
                (other.to_string(), name.clone())
            } else {
                (other.to_string(), other.to_string())
            }
        }
        _ => ("other".into(), "Other Agent".into()),
    }
}

fn share_and_hit_rate(
    total_tokens: u64,
    prompt_tokens: u64,
    cache_read_tokens: u64,
    overall_tokens: u64,
) -> (u32, u32) {
    let share_pct = if overall_tokens > 0 {
        ((total_tokens as f64 / overall_tokens as f64) * 100.0)
            .round()
            .min(100.0) as u32
    } else {
        0
    };
    let hit_rate = if prompt_tokens > 0 {
        ((cache_read_tokens as f64 / prompt_tokens as f64) * 100.0)
            .round()
            .min(100.0) as u32
    } else {
        0
    };
    (share_pct, hit_rate)
}

pub(crate) fn aggregate_dashboard_sessions(
    discover_res: &crate::daemon_protocol::DaemonResponse,
) -> DashboardSessionStats {
    let mut claude_count = 0usize;
    let mut claude_desktop_count = 0usize;
    let mut antigravity_count = 0usize;
    let mut codex_count = 0usize;
    let mut deepseek_harness_count = 0usize;
    let mut provider_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let plugin_names = crate::plugins::installed_plugin_names();
    let mut workspace_map: HashMap<String, usize> = HashMap::new();
    let mut discovered_sessions: Vec<Value> = Vec::new();

    let mut total_requests: u64 = 0;
    let mut total_prompt_tokens: u64 = 0;
    let mut total_completion_tokens: u64 = 0;
    let mut total_cache_read_tokens: u64 = 0;
    let mut total_cost_usd: f64 = 0.0;
    let mut model_aggs: HashMap<String, ModelAgg> = HashMap::new();
    let mut agent_aggs: HashMap<String, AgentAgg> = HashMap::new();

    if let crate::daemon_protocol::DaemonResponse::Discovery(val) = discover_res
        && let Some(candidates) = val.get("candidates").and_then(Value::as_array)
    {
        let mut seen_session_ids = std::collections::HashSet::new();
        for c in candidates {
            let sid = c
                .get("provider_session_id")
                .and_then(Value::as_str)
                .or_else(|| c.get("id").and_then(Value::as_str))
                .or_else(|| c.get("session_id").and_then(Value::as_str))
                .unwrap_or("");
            if !sid.is_empty() && !seen_session_ids.insert(sid.to_string()) {
                continue;
            }
            discovered_sessions.push(c.clone());
            let provider = c.get("provider").and_then(Value::as_str).unwrap_or("");
            let raw_pt = c.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
            let ct = c
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let cr = c
                .get("cache_read_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let tt = c
                .get("total_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(raw_pt + ct);
            let pt = if tt > raw_pt.saturating_add(ct) {
                tt.saturating_sub(ct)
            } else {
                raw_pt
            };
            let req_count = c
                .get("request_count")
                .and_then(Value::as_u64)
                .or_else(|| c.get("calls").and_then(Value::as_u64))
                .unwrap_or(1);

            total_requests = total_requests.saturating_add(req_count);
            total_prompt_tokens = total_prompt_tokens.saturating_add(pt);
            total_completion_tokens = total_completion_tokens.saturating_add(ct);
            total_cache_read_tokens = total_cache_read_tokens.saturating_add(cr);

            let model_name = c
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| default_model_for_provider(provider))
                .to_string();

            let cost = crate::usage_ledger::calc_cost(&model_name, pt, ct, cr);
            total_cost_usd += cost;

            let model_entry = model_aggs
                .entry(model_name.clone())
                .or_insert_with(|| ModelAgg {
                    name: model_name,
                    provider: provider.to_string(),
                    calls: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                });
            model_entry.calls += req_count as usize;
            model_entry.prompt_tokens = model_entry.prompt_tokens.saturating_add(pt);
            model_entry.completion_tokens = model_entry.completion_tokens.saturating_add(ct);
            model_entry.cache_read_tokens = model_entry.cache_read_tokens.saturating_add(cr);
            model_entry.total_tokens = model_entry.total_tokens.saturating_add(tt);
            model_entry.cost_usd += cost;

            let (agent_id, agent_display_name) =
                agent_identity_for_provider(provider, &plugin_names);
            let agent_entry = agent_aggs
                .entry(agent_id.clone())
                .or_insert_with(|| AgentAgg {
                    id: agent_id.clone(),
                    name: agent_display_name,
                    calls: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                });
            agent_entry.calls += req_count as usize;
            agent_entry.prompt_tokens = agent_entry.prompt_tokens.saturating_add(pt);
            agent_entry.completion_tokens = agent_entry.completion_tokens.saturating_add(ct);
            agent_entry.cache_read_tokens = agent_entry.cache_read_tokens.saturating_add(cr);
            agent_entry.total_tokens = agent_entry.total_tokens.saturating_add(tt);
            agent_entry.cost_usd += cost;

            match agent_id.as_str() {
                "claude_code" => claude_count += 1,
                "claude_desktop" => claude_desktop_count += 1,
                "antigravity" => antigravity_count += 1,
                "codex" => codex_count += 1,
                "deepseek_harness" => deepseek_harness_count += 1,
                _ => {}
            }
            if !agent_id.is_empty() && agent_id != "other" {
                *provider_counts.entry(agent_id).or_insert(0) += 1;
            }
            if let Some(ws) = c.get("workspace_path").and_then(Value::as_str) {
                *workspace_map.entry(ws.to_string()).or_insert(0) += 1;
            }
        }
    }

    let total_sessions = discovered_sessions.len();
    let overall_tokens = total_prompt_tokens.saturating_add(total_completion_tokens);
    let (_, overall_cache_hit_rate) = share_and_hit_rate(
        overall_tokens,
        total_prompt_tokens,
        total_cache_read_tokens,
        overall_tokens,
    );

    let mut models_breakdown_vec: Vec<ModelAgg> = model_aggs.into_values().collect();
    // ties keep the name's order so dashboard lists stay stable across polls
    models_breakdown_vec.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.name.cmp(&b.name))
    });
    let models_breakdown: Vec<Value> = models_breakdown_vec
        .into_iter()
        .map(|m| {
            let (share_pct, hit_rate) = share_and_hit_rate(
                m.total_tokens,
                m.prompt_tokens,
                m.cache_read_tokens,
                overall_tokens,
            );
            json!({
                "id": m.name,
                "name": m.name,
                "provider": m.provider,
                "calls": m.calls,
                "prompt_tokens": m.prompt_tokens,
                "completion_tokens": m.completion_tokens,
                "cache_read_tokens": m.cache_read_tokens,
                "total_tokens": m.total_tokens,
                "cache_hit_rate_pct": hit_rate,
                "share_pct": share_pct,
                "estimated_cost_usd": (m.cost_usd * 100.0).round() / 100.0
            })
        })
        .collect();

    let mut agents_breakdown_vec: Vec<AgentAgg> = agent_aggs.into_values().collect();
    agents_breakdown_vec.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.id.cmp(&b.id))
    });
    let agents_breakdown: Vec<Value> = agents_breakdown_vec
        .into_iter()
        .map(|a| {
            let (share_pct, hit_rate) = share_and_hit_rate(
                a.total_tokens,
                a.prompt_tokens,
                a.cache_read_tokens,
                overall_tokens,
            );
            json!({
                "id": a.id,
                "name": a.name,
                "calls": a.calls,
                "prompt_tokens": a.prompt_tokens,
                "completion_tokens": a.completion_tokens,
                "cache_read_tokens": a.cache_read_tokens,
                "total_tokens": a.total_tokens,
                "cache_hit_rate_pct": hit_rate,
                "share_pct": share_pct,
                "estimated_cost_usd": (a.cost_usd * 100.0).round() / 100.0
            })
        })
        .collect();

    let usage_summary = json!({
        "calls": total_requests,
        "sessions": total_sessions,
        "input": total_prompt_tokens,
        "output": total_completion_tokens,
        "cache_read": total_cache_read_tokens,
        "total_tokens": overall_tokens,
        "cache_hit_rate_pct": overall_cache_hit_rate,
        "cost_usd": (total_cost_usd * 100.0).round() / 100.0,
        "models": models_breakdown,
        "agents": agents_breakdown
    });

    DashboardSessionStats {
        total_sessions,
        claude_count,
        claude_desktop_count,
        antigravity_count,
        codex_count,
        deepseek_harness_count,
        provider_counts,
        discovered_sessions,
        workspace_map,
        usage_summary,
    }
}

pub(crate) fn build_dashboard_models_json(handler: &Arc<dyn DaemonRequestHandler>) -> Vec<Value> {
    let catalog_models = crate::model_catalog::fetch_catalog_models(false);
    if !catalog_models.is_empty() {
        return catalog_models
            .iter()
            .map(|m| {
                json!({
                    "id": m.id,
                    "name": m.name,
                    "display_name": m.display_name,
                    "context_length": m.context_length,
                    "context_window_str": m.context_window_str,
                    "max_output_tokens": m.max_output_tokens,
                    "input_modalities": m.input_modalities,
                    "output_modalities": m.output_modalities,
                    "supports_reasoning": m.supports_reasoning,
                    "supports_tools": m.supports_tools,
                    "supports_vision": m.supports_vision,
                    "upstream_provider": m.upstream_provider,
                })
            })
            .collect();
    }
    let mut models = Vec::new();
    if let crate::daemon_protocol::DaemonResponse::Models(ref val) =
        handler.handle(DaemonRequest::GetModels)
        && let Some(data) = val.get("data").and_then(Value::as_array)
    {
        models.extend(data.iter().cloned());
    }
    models
}

pub(crate) fn build_dashboard_workspaces_json(
    status_res: &crate::daemon_protocol::DaemonResponse,
    workspace_map: &HashMap<String, usize>,
) -> Vec<Value> {
    let discovered_workspaces = crate::collectors::workspace::discover_all_workspaces();
    if !discovered_workspaces.is_empty() {
        return discovered_workspaces
            .into_iter()
            .map(|w| {
                json!({
                    "name": w.name,
                    "path": w.path,
                    "sessions_count": w.session_count,
                    "last_active_at_ms": w.last_active_at_ms,
                    "last_active_str": w.last_active_str,
                    "agents": w.agents,
                    "is_git": w.is_git,
                })
            })
            .collect();
    }
    let mut registered_workspaces = Vec::new();
    if let crate::daemon_protocol::DaemonResponse::Status { workspaces, .. } = status_res {
        for ws in workspaces {
            let s = ws.to_string_lossy().to_string();
            let count = workspace_map.get(&s).copied().unwrap_or(0);
            registered_workspaces.push((s, count));
        }
    }
    for (ws, count) in workspace_map {
        if !registered_workspaces.iter().any(|(w, _)| w == ws) {
            registered_workspaces.push((ws.clone(), *count));
        }
    }
    registered_workspaces
        .into_iter()
        .map(|(path, count)| {
            let name = PathBuf::from(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            json!({
                "name": name,
                "path": path,
                "sessions_count": count,
                "last_active_at_ms": 0,
                "last_active_str": "历史会话",
                "agents": ["Local Agent"],
                "is_git": std::path::Path::new(&path).join(".git").exists(),
            })
        })
        .collect()
}

pub(crate) fn build_dashboard_agents_json(stats: &DashboardSessionStats) -> (Value, usize) {
    let [
        (claude_installed, claude_path),
        (codex_installed, codex_path),
        (claude_desktop_installed, claude_desktop_path),
        (antigravity_installed, antigravity_path),
        (deepseek_installed, deepseek_path),
    ] = cached_agent_installations();

    let mut active_agents_count = 0usize;
    for installed in [
        claude_installed,
        claude_desktop_installed || stats.claude_desktop_count > 0,
        antigravity_installed || stats.antigravity_count > 0,
        codex_installed,
        deepseek_installed,
    ] {
        if installed {
            active_agents_count += 1;
        }
    }

    // Coding Assistant and Claude Desktop are two different products that share
    // a transcript store and are never folded together.
    let mut agents = vec![
        json!({
            "id": "claude_code",
            "name": "Coding Assistant",
            "installed": claude_installed,
            "path": claude_path,
            "sessions_count": stats.claude_count,
            "command": "claude",
            "affinity": true,
            "is_plugin": false,
            "description": "官方终端助手，支持 x-session-affinity 会话亲和性保护与 Prompt 缓存"
        }),
        json!({
            "id": "claude_desktop",
            "name": "Claude Desktop",
            "installed": claude_desktop_installed || stats.claude_desktop_count > 0,
            "path": claude_desktop_path,
            "sessions_count": stats.claude_desktop_count,
            "command": "Claude",
            "affinity": false,
            "is_plugin": false,
            "description": "Claude 桌面客户端的 Code / Cowork 会话，与 Coding Assistant 是两款独立产品"
        }),
        json!({
            "id": "antigravity",
            "name": "Antigravity",
            "installed": antigravity_installed || stats.antigravity_count > 0,
            "path": antigravity_path,
            "sessions_count": stats.antigravity_count,
            "command": "antigravity",
            "affinity": false,
            "is_plugin": false,
            "description": "Google Antigravity 智能代理开发环境，支持多 Subagent 树与 Gemini 1M 上下文"
        }),
        json!({
            "id": "codex",
            "name": "Codex",
            "installed": codex_installed,
            "path": codex_path,
            "sessions_count": stats.codex_count,
            "command": "codex app-server",
            "affinity": false,
            "is_plugin": false,
            "description": "OpenAI Codex 智能代码辅助与工作区守护代理"
        }),
        json!({
            "id": "deepseek_harness",
            "name": "DeepSeek Harness",
            "installed": deepseek_installed,
            "path": deepseek_path,
            "sessions_count": stats.deepseek_harness_count,
            "command": "deepseek",
            "affinity": false,
            "is_plugin": false,
            "description": "DeepSeek 原生 Harness 预留接口，等待本地运行时协议接入"
        }),
    ];

    for plugin in crate::plugins::list_local_plugins_view_cached()
        .0
        .into_iter()
        .filter(|p| p.installed && p.enabled)
    {
        let sess_count = stats.provider_counts.get(&plugin.id).copied().unwrap_or(0);
        let available = plugin.available.unwrap_or(false);
        if available || sess_count > 0 {
            active_agents_count += 1;
        }
        let ver = plugin
            .installed_version
            .as_deref()
            .filter(|v| !v.is_empty())
            .unwrap_or(&plugin.version);
        let path = plugin
            .binary_path
            .clone()
            .or_else(|| Some(format!("plugins/installed/{}", plugin.id)));
        agents.push(json!({
            "id": plugin.id,
            "name": plugin.name,
            "installed": available,
            "available": available,
            "path": path,
            "sessions_count": sess_count,
            "command": plugin.id,
            "affinity": false,
            "is_plugin": true,
            "plugin_version": ver,
            "runtime_status": plugin.runtime_status,
            "icon_url": format!("/api/plugins/icon/{}", plugin.id),
            "capabilities": plugin.capabilities,
            "description": plugin.description,
        }));
    }

    (Value::Array(agents), active_agents_count)
}
