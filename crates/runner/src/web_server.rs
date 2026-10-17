use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    daemon_ipc::DaemonRequestHandler, daemon_protocol::DaemonRequest, domain::AgentType,
    remote_host::RemoteCommand,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub struct WebServerConfig {
    pub port: u16,
    pub open_browser: bool,
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            port: 4200,
            open_browser: true,
        }
    }
}

/// 启动内嵌轻量 Web 控制台服务
pub fn serve(
    config: WebServerConfig,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
) -> anyhow::Result<u16> {
    // 尝试在指定端口绑定，如果被占用则递增端口
    let mut bound_listener = None;
    let mut actual_port = config.port;

    for port_offset in 0..10 {
        let test_port = config.port + port_offset;
        let addr = format!("127.0.0.1:{}", test_port);
        match TcpListener::bind(&addr) {
            Ok(l) => {
                actual_port = test_port;
                bound_listener = Some(l);
                break;
            }
            Err(_) => continue,
        }
    }

    let listener = bound_listener
        .ok_or_else(|| anyhow::anyhow!("failed_to_bind_web_server_port_{}", config.port))?;

    // 设置非阻塞以便定期检测 stopped 信号
    listener.set_nonblocking(true)?;

    println!(
        "[AgentHub] Runtime Web UI running at http://127.0.0.1:{}",
        actual_port
    );

    if config.open_browser {
        let url = format!("http://127.0.0.1:{}", actual_port);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            crate::tray::open_desktop_window(&url);
        });
    }

    let web_url = format!("http://127.0.0.1:{}", actual_port);
    crate::tray::start_tray(web_url, stopped.clone());

    let server_stopped = stopped.clone();
    let server_handler = handler.clone();

    thread::Builder::new()
        .name("agenthub-web-server".into())
        .spawn(move || {
            while !server_stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let h = server_handler.clone();
                        let s = server_stopped.clone();
                        thread::spawn(move || {
                            let _ = handle_connection(stream, h, s, actual_port);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        })?;

    Ok(actual_port)
}

fn handle_connection(
    mut stream: TcpStream,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
    port: u16,
) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let full_path = parts[1];

    let mut headers = HashMap::new();
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers.insert(key, val);
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    // CORS preflight
    if method == "OPTIONS" {
        send_response(&mut stream, 204, "No Content", "text/plain", b"", true, None)?;
        return Ok(());
    }

    let (path, query) = parse_path_and_query(full_path);

    if path == "/favicon.ico" {
        send_response(
            &mut stream,
            200,
            "OK",
            "image/x-icon",
            FAVICON_ICO,
            false,
            Some("public, max-age=86400"),
        )?;
        return Ok(());
    }

    if path == "/icon.svg" {
        send_response(
            &mut stream,
            200,
            "OK",
            "image/svg+xml",
            ICON_SVG.as_bytes(),
            false,
            Some("public, max-age=86400"),
        )?;
        return Ok(());
    }

    if path == "/manifest.json" {
        send_response(
            &mut stream,
            200,
            "OK",
            "application/manifest+json",
            MANIFEST_JSON.as_bytes(),
            false,
            Some("public, max-age=86400"),
        )?;
        return Ok(());
    }

    // REST API endpoints
    if path.starts_with("/api/") {
        handle_api(&mut stream, method, path, &query, &body, handler, stopped, port)?;
        return Ok(());
    }

    // 默认提供嵌入式 CDS Web 界面
    send_response(
        &mut stream,
        200,
        "OK",
        "text/html; charset=utf-8",
        INDEX_HTML.as_bytes(),
        false,
        Some("no-cache"),
    )?;

    Ok(())
}

fn parse_path_and_query(full_path: &str) -> (&str, HashMap<String, String>) {
    let mut parts = full_path.splitn(2, '?');
    let path = parts.next().unwrap_or("/");
    let mut query = HashMap::new();
    if let Some(q) = parts.next() {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                query.insert(k.to_string(), v.to_string());
            } else if !pair.is_empty() {
                query.insert(pair.to_string(), String::new());
            }
        }
    }
    (path, query)
}

fn handle_api(
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    body: &[u8],
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
    port: u16,
) -> anyhow::Result<()> {
    match (method, path) {
        ("GET", "/api/dashboard") => {
            let data = build_dashboard_summary(&handler, port);
            send_json_response(stream, 200, &data)?;
        }
        ("POST", "/api/shutdown") => {
            let resp = json!({
                "status": "shutting_down",
                "message": "AgentHub Runner 正在退出，托盘图标与服务已安全终止。"
            });
            send_json_response(stream, 200, &resp)?;
            stopped.store(true, Ordering::SeqCst);
            thread::spawn(|| {
                thread::sleep(Duration::from_millis(250));
                std::process::exit(0);
            });
        }
        ("GET", "/api/status") => {
            let resp = handler.handle(DaemonRequest::GetStatus);
            send_json_response(stream, 200, &resp)?;
        }
        ("GET", "/api/snapshot") => {
            let resp = handler.handle(DaemonRequest::GetSnapshot);
            send_json_response(stream, 200, &resp)?;
        }
        ("GET", "/api/events") => {
            let after_cursor = query
                .get("after")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let limit = query
                .get("limit")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(100);
            let resp = handler.handle(DaemonRequest::GetEvents {
                after_cursor,
                limit,
            });
            send_json_response(stream, 200, &resp)?;
        }
        ("GET", "/api/models") => {
            let catalog_models = crate::model_catalog::fetch_catalog_models(false);
            if !catalog_models.is_empty() {
                let resp = crate::model_catalog::catalog_models_to_json(&catalog_models);
                send_json_response(stream, 200, &resp)?;
            } else {
                let resp = handler.handle(DaemonRequest::GetModels);
                send_json_response(stream, 200, &resp)?;
            }
        }
        ("POST", "/api/models/refresh") => {
            let catalog_models = crate::model_catalog::fetch_catalog_models(true);
            let resp = crate::model_catalog::catalog_models_to_json(&catalog_models);
            send_json_response(stream, 200, &resp)?;
        }
        ("GET", "/api/workspaces") => {
            let ws = crate::collectors::workspace::discover_all_workspaces();
            send_json_response(stream, 200, &json!({ "workspaces": ws }))?;
        }
        ("GET", "/api/discover") => {
            let resp = handler.handle(DaemonRequest::Discover);
            send_json_response(stream, 200, &resp)?;
        }
        ("GET", "/api/conversation") => {
            let session_id = query.get("session_id").cloned().unwrap_or_default();
            let agent_str = query.get("agent").map(String::as_str).unwrap_or("claude");
            let agent_type = match agent_str {
                "codex" => AgentType::Codex,
                "antigravity" => AgentType::Antigravity,
                "grok" => AgentType::GrokBuild,
                "zcode" => AgentType::ZCode,
                _ => AgentType::ClaudeCode,
            };
            let resp = handler.handle(DaemonRequest::GetSessionConversation {
                agent_type,
                session_id,
            });
            send_json_response(stream, 200, &resp)?;
        }
        ("POST", "/api/execute") => match serde_json::from_slice::<RemoteCommand>(body) {
            Ok(command) => {
                let resp = handler.handle(DaemonRequest::Execute { command });
                send_json_response(stream, 200, &resp)?;
            }
            Err(err) => {
                let err_json = json!({ "error": format!("invalid_command_payload: {}", err) });
                send_json_response(stream, 400, &err_json)?;
            }
        },
        ("POST", "/api/authorize-workspace") => {
            #[derive(Deserialize)]
            struct AuthWorkspaceReq {
                workspace: PathBuf,
            }
            match serde_json::from_slice::<AuthWorkspaceReq>(body) {
                Ok(req) => {
                    let resp = handler.handle(DaemonRequest::AuthorizeWorkspace {
                        workspace: req.workspace,
                    });
                    send_json_response(stream, 200, &resp)?;
                }
                Err(err) => {
                    let err_json =
                        json!({ "error": format!("invalid_workspace_payload: {}", err) });
                    send_json_response(stream, 400, &err_json)?;
                }
            }
        }
        ("POST", "/api/device/bind") => {
            #[derive(Deserialize)]
            struct BindReq {
                gateway: String,
                binding_code: String,
            }
            match serde_json::from_slice::<BindReq>(body) {
                Ok(req) => {
                    let home = std::env::var_os("USERPROFILE")
                        .or_else(|| std::env::var_os("HOME"))
                        .map(PathBuf::from);
                    let Some(home) = home else {
                        let err_json = json!({ "error": "home_dir_unavailable" });
                        send_json_response(stream, 500, &err_json)?;
                        return Ok(());
                    };
                    let identity_path = home.join(".agenthub").join("identity.json");
                    let identity = match crate::device::DeviceIdentity::load(&identity_path) {
                        Ok(id) => id,
                        Err(err) => {
                            let err_json = json!({ "error": format!("load_identity_failed: {}", err) });
                            send_json_response(stream, 500, &err_json)?;
                            return Ok(());
                        }
                    };

                    let gateway_url = req.gateway.trim().trim_end_matches('/');
                    let client = match crate::gateway::GatewayClient::new(
                        gateway_url,
                        Duration::from_secs(10),
                    ) {
                        Ok(c) => c,
                        Err(err) => {
                            let err_json = json!({ "error": format!("invalid_gateway_url: {}", err) });
                            send_json_response(stream, 400, &err_json)?;
                            return Ok(());
                        }
                    };

                    match client.bind(&identity, req.binding_code.trim()) {
                        Ok(binding) => {
                            let binding_info = json!({
                                "device_id": binding.device_id,
                                "registered": binding.registered,
                                "gateway": gateway_url,
                                "paired_at": chrono::Utc::now().to_rfc3339()
                            });
                            let binding_path = home.join(".agenthub").join("binding.json");
                            let _ = std::fs::write(&binding_path, serde_json::to_string_pretty(&binding_info).unwrap_or_default());
                            let resp = json!({
                                "ok": true,
                                "message": "设备已成功完成配对与网关注册！",
                                "binding": binding_info
                            });
                            send_json_response(stream, 200, &resp)?;
                        }
                        Err(err) => {
                            let err_json = json!({ "error": format!("bind_failed: {}", err) });
                            send_json_response(stream, 400, &err_json)?;
                        }
                    }
                }
                Err(err) => {
                    let err_json = json!({ "error": format!("invalid_bind_payload: {}", err) });
                    send_json_response(stream, 400, &err_json)?;
                }
            }
        }
        _ => {
            let err_json = json!({ "error": "not_found" });
            send_json_response(stream, 404, &err_json)?;
        }
    }
    Ok(())
}

fn dashboard_summary_cache() -> &'static RwLock<Option<(Instant, serde_json::Value)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, serde_json::Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn build_dashboard_summary(
    handler: &Arc<dyn DaemonRequestHandler>,
    port: u16,
) -> serde_json::Value {
    if let Ok(guard) = dashboard_summary_cache().read() {
        if let Some((cached_at, ref val)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(2) {
                let mut v = val.clone();
                v["port"] = json!(port);
                return v;
            }
        }
    }

    let discover_res = handler.handle(DaemonRequest::Discover);
    let models_res = handler.handle(DaemonRequest::GetModels);
    let status_res = handler.handle(DaemonRequest::GetStatus);

    let mut total_sessions = 0;
    let mut claude_count = 0;
    let mut codex_count = 0;
    let mut antigravity_count = 0;
    let mut grok_count = 0;
    let mut zcode_count = 0;
    let mut workspace_map: HashMap<String, usize> = HashMap::new();
    let mut discovered_sessions: Vec<serde_json::Value> = Vec::new();

    let mut total_prompt_tokens: u64 = 0;
    let mut total_completion_tokens: u64 = 0;
    let mut total_cache_read_tokens: u64 = 0;
    let mut total_cost_usd: f64 = 0.0;

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

    fn calc_cost(model_name: &str, prompt: u64, completion: u64, cache_read: u64) -> f64 {
        let m = model_name.to_ascii_lowercase();
        let (input_rate, output_rate, cache_read_rate) = if m.contains("opus") {
            (15.0, 75.0, 1.5)
        } else if m.contains("sonnet") || m.contains("fable") || m.contains("mythos") {
            (3.0, 15.0, 0.3)
        } else if m.contains("haiku") || m.contains("flash") || m.contains("mini") {
            (0.25, 1.25, 0.025)
        } else if m.contains("deepseek") {
            (0.14, 0.28, 0.014)
        } else if m.contains("codex") || m.contains("gpt-5") || m.contains("gpt-4") {
            (2.5, 10.0, 1.25)
        } else {
            (3.0, 15.0, 0.3)
        };
        let regular_prompt = prompt.saturating_sub(cache_read);
        (regular_prompt as f64 * input_rate
            + completion as f64 * output_rate
            + cache_read as f64 * cache_read_rate)
            / 1_000_000.0
    }

    let mut model_aggs: HashMap<String, ModelAgg> = HashMap::new();
    let mut agent_aggs: HashMap<String, AgentAgg> = HashMap::new();

    if let crate::daemon_protocol::DaemonResponse::Discovery(ref val) = discover_res {
        if let Some(candidates) = val.get("candidates").and_then(serde_json::Value::as_array) {
            let mut seen_session_ids = std::collections::HashSet::new();
            for c in candidates {
                let sid = c
                    .get("provider_session_id")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| c.get("id").and_then(serde_json::Value::as_str))
                    .or_else(|| c.get("session_id").and_then(serde_json::Value::as_str))
                    .unwrap_or("");
                if !sid.is_empty() && !seen_session_ids.insert(sid.to_string()) {
                    continue;
                }
                discovered_sessions.push(c.clone());
                let provider = c.get("provider").and_then(serde_json::Value::as_str).unwrap_or("");
                let pt = c.get("prompt_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let ct = c.get("completion_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let cr = c.get("cache_read_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0);
                let tt = c.get("total_tokens").and_then(serde_json::Value::as_u64).unwrap_or(pt + ct);

                total_prompt_tokens = total_prompt_tokens.saturating_add(pt);
                total_completion_tokens = total_completion_tokens.saturating_add(ct);
                total_cache_read_tokens = total_cache_read_tokens.saturating_add(cr);

                let raw_model = c.get("model").and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty());
                let model_name = match raw_model {
                    Some(m) => m.to_string(),
                    None => match provider {
                        "claude_code" | "claude" => "claude-sonnet-4-6".to_string(),
                        "codex" => "gpt-5-codex".to_string(),
                        "antigravity" => "antigravity-deep-reasoning".to_string(),
                        "grok_build" | "grok" => "grok-3-developer".to_string(),
                        "zcode" => "zcode-pro-turbo".to_string(),
                        _ => "general-agent-model".to_string(),
                    },
                };

                let cost = calc_cost(&model_name, pt, ct, cr);
                total_cost_usd += cost;

                // Model aggregation
                let model_entry = model_aggs.entry(model_name.clone()).or_insert_with(|| ModelAgg {
                    name: model_name.clone(),
                    provider: provider.to_string(),
                    calls: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                });
                model_entry.calls += 1;
                model_entry.prompt_tokens = model_entry.prompt_tokens.saturating_add(pt);
                model_entry.completion_tokens = model_entry.completion_tokens.saturating_add(ct);
                model_entry.cache_read_tokens = model_entry.cache_read_tokens.saturating_add(cr);
                model_entry.total_tokens = model_entry.total_tokens.saturating_add(tt);
                model_entry.cost_usd += cost;

                // Agent aggregation
                let (agent_id, agent_display_name) = match provider {
                    "claude_code" | "claude" => ("claude_code", "Claude Code"),
                    "codex" => ("codex", "Codex App-Server"),
                    "antigravity" => ("antigravity", "Antigravity"),
                    "grok_build" | "grok" => ("grok_build", "Grok Build"),
                    "zcode" => ("zcode", "ZCode Agent"),
                    _ => ("other", "Other Agent"),
                };
                let agent_entry = agent_aggs.entry(agent_id.to_string()).or_insert_with(|| AgentAgg {
                    id: agent_id.to_string(),
                    name: agent_display_name.to_string(),
                    calls: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cache_read_tokens: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                });
                agent_entry.calls += 1;
                agent_entry.prompt_tokens = agent_entry.prompt_tokens.saturating_add(pt);
                agent_entry.completion_tokens = agent_entry.completion_tokens.saturating_add(ct);
                agent_entry.cache_read_tokens = agent_entry.cache_read_tokens.saturating_add(cr);
                agent_entry.total_tokens = agent_entry.total_tokens.saturating_add(tt);
                agent_entry.cost_usd += cost;

                match provider {
                    "claude_code" | "claude" => claude_count += 1,
                    "codex" => codex_count += 1,
                    "antigravity" => antigravity_count += 1,
                    "grok_build" | "grok" => grok_count += 1,
                    "zcode" => zcode_count += 1,
                    _ => {}
                }
                if let Some(ws) = c.get("workspace_path").and_then(serde_json::Value::as_str) {
                    *workspace_map.entry(ws.to_string()).or_insert(0) += 1;
                }
            }
            total_sessions = discovered_sessions.len();
        }
    }

    let overall_tokens = total_prompt_tokens.saturating_add(total_completion_tokens);
    let overall_cache_hit_rate = if total_prompt_tokens > 0 {
        ((total_cache_read_tokens as f64 / total_prompt_tokens as f64) * 100.0).round().clamp(0.0, 100.0) as u32
    } else {
        0
    };

    let mut models_breakdown_vec: Vec<ModelAgg> = model_aggs.into_values().collect();
    models_breakdown_vec.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    let models_breakdown: Vec<serde_json::Value> = models_breakdown_vec
        .into_iter()
        .map(|m| {
            let share_pct = if overall_tokens > 0 {
                ((m.total_tokens as f64 / overall_tokens as f64) * 100.0).round().min(100.0) as u32
            } else {
                0
            };
            let hit_rate = if m.prompt_tokens > 0 {
                ((m.cache_read_tokens as f64 / m.prompt_tokens as f64) * 100.0).round().min(100.0) as u32
            } else {
                0
            };
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
    agents_breakdown_vec.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    let agents_breakdown: Vec<serde_json::Value> = agents_breakdown_vec
        .into_iter()
        .map(|a| {
            let share_pct = if overall_tokens > 0 {
                ((a.total_tokens as f64 / overall_tokens as f64) * 100.0).round().min(100.0) as u32
            } else {
                0
            };
            let hit_rate = if a.prompt_tokens > 0 {
                ((a.cache_read_tokens as f64 / a.prompt_tokens as f64) * 100.0).round().min(100.0) as u32
            } else {
                0
            };
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
        "calls": total_sessions,
        "input": total_prompt_tokens,
        "output": total_completion_tokens,
        "cache_read": total_cache_read_tokens,
        "total_tokens": overall_tokens,
        "cache_hit_rate_pct": overall_cache_hit_rate,
        "cost_usd": (total_cost_usd * 100.0).round() / 100.0,
        "models": models_breakdown,
        "agents": agents_breakdown
    });

    let mut registered_workspaces = Vec::new();
    if let crate::daemon_protocol::DaemonResponse::Status { ref workspaces, .. } = status_res {
        for ws in workspaces {
            let s = ws.to_string_lossy().to_string();
            let count = workspace_map.get(&s).copied().unwrap_or(0);
            registered_workspaces.push((s, count));
        }
    }
    for (ws, count) in &workspace_map {
        if !registered_workspaces.iter().any(|(w, _)| w == ws) {
            registered_workspaces.push((ws.clone(), *count));
        }
    }

    let mut models = Vec::new();
    if let crate::daemon_protocol::DaemonResponse::Models(ref val) = models_res {
        if let Some(data) = val.get("data").and_then(serde_json::Value::as_array) {
            for m in data {
                models.push(m.clone());
            }
        }
    }

    let catalog_models = crate::model_catalog::fetch_catalog_models(false);
    let models_json: Vec<serde_json::Value> = if !catalog_models.is_empty() {
        catalog_models
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
            .collect()
    } else {
        models
    };

    let gateway_profile = crate::claude_gateway::discover_claude_gateway();
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

    let (claude_installed, claude_path) = find_executable(
        "claude",
        &[r"AppData\Roaming\npm\claude.cmd", r".local\bin\claude"],
    );
    let (codex_installed, codex_path) = find_executable(
        "codex",
        &[r"AppData\Roaming\npm\codex.cmd", r".local\bin\codex"],
    );
    let (antigravity_installed, antigravity_path) = check_agent_dir(".antigravity");
    let (grok_installed, grok_path) = check_agent_dir(".grok");
    let (zcode_installed, zcode_path) = find_executable("zcode", &[]);
    let (gemini_installed, gemini_path) = find_executable(
        "gemini",
        &[r"AppData\Roaming\npm\gemini.cmd", r".local\bin\gemini"],
    );
    let (cursor_installed, cursor_path) = find_executable("cursor", &[]);
    let (cline_installed, cline_path) = check_agent_dir(".cline");

    let mut active_agents_count = 0;
    if claude_installed {
        active_agents_count += 1;
    }
    if codex_installed {
        active_agents_count += 1;
    }
    if antigravity_installed {
        active_agents_count += 1;
    }
    if grok_installed {
        active_agents_count += 1;
    }
    if zcode_installed {
        active_agents_count += 1;
    }
    if gemini_installed {
        active_agents_count += 1;
    }
    if cursor_installed {
        active_agents_count += 1;
    }
    if cline_installed {
        active_agents_count += 1;
    }

    let agents = json!([
        {
            "id": "claude_code",
            "name": "Claude Code CLI",
            "installed": claude_installed,
            "path": claude_path,
            "sessions_count": claude_count,
            "command": "claude",
            "affinity": true,
            "description": "官方终端助手，支持 x-session-affinity 会话亲和性保护与 Prompt 缓存"
        },
        {
            "id": "codex",
            "name": "Codex App-Server",
            "installed": codex_installed,
            "path": codex_path,
            "sessions_count": codex_count,
            "command": "codex app-server",
            "affinity": false,
            "description": "OpenAI Codex 智能代码辅助与工作区守护代理"
        },
        {
            "id": "antigravity",
            "name": "Antigravity",
            "installed": antigravity_installed,
            "path": antigravity_path,
            "sessions_count": antigravity_count,
            "command": "antigravity",
            "affinity": false,
            "description": "反重力代码推理引擎与头脑风暴运行时"
        },
        {
            "id": "grok_build",
            "name": "Grok Build",
            "installed": grok_installed,
            "path": grok_path,
            "sessions_count": grok_count,
            "command": "grok-build",
            "affinity": false,
            "description": "X.AI Grok 构建协同代理与会话记录器"
        },
        {
            "id": "zcode",
            "name": "ZCode Agent",
            "installed": zcode_installed,
            "path": zcode_path,
            "sessions_count": zcode_count,
            "command": "zcode",
            "affinity": false,
            "description": "系统级代码助手与快速原型生成器"
        },
        {
            "id": "gemini_cli",
            "name": "Gemini CLI",
            "installed": gemini_installed,
            "path": gemini_path,
            "sessions_count": 0,
            "command": "gemini",
            "affinity": false,
            "description": "Google Gemini 官方命令行编程助手"
        },
        {
            "id": "cursor",
            "name": "Cursor Agent",
            "installed": cursor_installed,
            "path": cursor_path,
            "sessions_count": 0,
            "command": "cursor",
            "affinity": false,
            "description": "Cursor 编辑器核心代码代理与终端执行引擎"
        },
        {
            "id": "cline",
            "name": "Cline",
            "installed": cline_installed,
            "path": cline_path,
            "sessions_count": 0,
            "command": "cline",
            "affinity": false,
            "description": "开源自主代码代理与工具编排助手"
        }
    ]);

    let discovered_workspaces = crate::collectors::workspace::discover_all_workspaces();
    let workspaces_json: Vec<_> = if !discovered_workspaces.is_empty() {
        discovered_workspaces
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
            .collect()
    } else {
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
    };

    let total_agents_count = agents.as_array().map(|a| a.len()).unwrap_or(0);

    let summary = json!({
        "device_id": "local-dev",
        "port": port,
        "stats": {
            "total_sessions": total_sessions,
            "claude_sessions": claude_count,
            "codex_sessions": codex_count,
            "antigravity_sessions": antigravity_count,
            "grok_sessions": grok_count,
            "zcode_sessions": zcode_count,
            "active_agents": active_agents_count,
            "total_agents": total_agents_count,
            "total_models": models_json.len().max(gateway_models_count),
            "total_workspaces": workspaces_json.len(),
        },
        "agents": agents,
        "workspaces": workspaces_json,
        "models": models_json,
        "gateway": {
            "detected": gateway_detected,
            "source": gateway_source,
            "base_url": gateway_base_url,
            "models_count": gateway_models_count,
            "affinity_header": "x-session-affinity"
        },
        "sessions": discovered_sessions,
        "usage": usage_summary,
        "device": get_device_info()
    });

    if let Ok(mut guard) = dashboard_summary_cache().write() {
        *guard = Some((Instant::now(), summary.clone()));
    }
    summary
}

fn device_info_cache() -> &'static RwLock<Option<(Instant, serde_json::Value)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, serde_json::Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn get_device_info() -> serde_json::Value {
    if let Ok(guard) = device_info_cache().read() {
        if let Some((cached_at, ref val)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(5) {
                return val.clone();
            }
        }
    }

    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let Some(home) = home else {
        return json!({
            "device_id": "unknown",
            "public_key": "",
            "paired": false,
            "gateway_url": ""
        });
    };

    let agenthub_dir = home.join(".agenthub");
    let identity_path = agenthub_dir.join("identity.json");
    let binding_path = agenthub_dir.join("binding.json");

    let mut device_id = "unknown".to_string();
    let mut public_key = "".to_string();

    if let Ok(identity) = crate::device::DeviceIdentity::load(&identity_path) {
        device_id = identity.device_id().to_string();
        public_key = identity.public_key_hex();
    } else if let Ok(content) = std::fs::read_to_string(&identity_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(id) = val.get("device_id").and_then(|v| v.as_str()) {
                device_id = id.to_string();
            }
            if let Some(pk) = val.get("public_key").and_then(|v| v.as_str()) {
                public_key = pk.to_string();
            }
        }
    }

    let mut paired = false;
    let mut gateway_url = String::new();
    let mut paired_at = String::new();
    if let Ok(binding_content) = std::fs::read_to_string(&binding_path) {
        if let Ok(binding_val) = serde_json::from_str::<serde_json::Value>(&binding_content) {
            paired = binding_val.get("registered").and_then(|v| v.as_bool()).unwrap_or(false);
            gateway_url = binding_val.get("gateway").and_then(|v| v.as_str()).unwrap_or("").to_string();
            paired_at = binding_val.get("paired_at").and_then(|v| v.as_str()).unwrap_or("").to_string();
        }
    }

    let res = json!({
        "device_id": device_id,
        "public_key": public_key,
        "paired": paired,
        "gateway_url": gateway_url,
        "paired_at": paired_at
    });

    if let Ok(mut guard) = device_info_cache().write() {
        *guard = Some((Instant::now(), res.clone()));
    }
    res
}

fn executable_cache() -> &'static RwLock<HashMap<String, (Instant, (bool, Option<String>))>> {
    static CACHE: OnceLock<RwLock<HashMap<String, (Instant, (bool, Option<String>))>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn find_executable(name: &str, relative_hints: &[&str]) -> (bool, Option<String>) {
    if let Ok(guard) = executable_cache().read() {
        if let Some((cached_at, res)) = guard.get(name) {
            if cached_at.elapsed() < Duration::from_secs(30) {
                return res.clone();
            }
        }
    }

    let mut found = (false, None);
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home_path = PathBuf::from(home);
        for hint in relative_hints {
            let full = home_path.join(hint);
            if full.exists() {
                found = (true, Some(full.to_string_lossy().to_string()));
                break;
            }
        }
    }

    if !found.0 {
        if let Some(path_var) = std::env::var_os("PATH") {
            let paths = std::env::split_paths(&path_var);
            #[cfg(target_os = "windows")]
            let exts = [".cmd", ".exe", ".bat", ""];
            #[cfg(not(target_os = "windows"))]
            let exts = [""];

            for p in paths {
                for ext in &exts {
                    let candidate = p.join(format!("{}{}", name, ext));
                    if candidate.is_file() {
                        found = (true, Some(candidate.to_string_lossy().to_string()));
                        break;
                    }
                }
                if found.0 {
                    break;
                }
            }
        }
    }

    if let Ok(mut guard) = executable_cache().write() {
        guard.insert(name.to_string(), (Instant::now(), found.clone()));
    }
    found
}

fn agent_dir_cache() -> &'static RwLock<HashMap<String, (Instant, (bool, Option<String>))>> {
    static CACHE: OnceLock<RwLock<HashMap<String, (Instant, (bool, Option<String>))>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn check_agent_dir(dir_name: &str) -> (bool, Option<String>) {
    if let Ok(guard) = agent_dir_cache().read() {
        if let Some((cached_at, res)) = guard.get(dir_name) {
            if cached_at.elapsed() < Duration::from_secs(30) {
                return res.clone();
            }
        }
    }

    let mut found = (false, None);
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let full = PathBuf::from(home).join(dir_name);
        if full.is_dir() {
            found = (true, Some(full.to_string_lossy().to_string()));
        }
    }

    if let Ok(mut guard) = agent_dir_cache().write() {
        guard.insert(dir_name.to_string(), (Instant::now(), found.clone()));
    }
    found
}

fn send_json_response<T: Serialize>(
    stream: &mut TcpStream,
    status: u16,
    data: &T,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(data)?;
    send_response(
        stream,
        status,
        "OK",
        "application/json",
        &body,
        false,
        Some("no-cache, no-store, must-revalidate"),
    )
}

fn send_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
    is_options: bool,
    cache_control: Option<&str>,
) -> anyhow::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n",
        status_code,
        status_text,
        content_type,
        body.len()
    );
    if let Some(cc) = cache_control {
        header.push_str(&format!("Cache-Control: {}\r\n", cc));
    }
    if is_options {
        header.push_str("Access-Control-Max-Age: 86400\r\n");
    }
    header.push_str("\r\n");

    stream.write_all(header.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()?;
    Ok(())
}

/// 嵌入式自包含静态图标资源
const FAVICON_ICO: &[u8] = include_bytes!("../resources/icon.ico");
const ICON_SVG: &str = include_str!("../resources/icon.svg");

/// 嵌入式自包含 CDS (Claude Design System) 单页前端
const INDEX_HTML: &str = include_str!("../resources/dashboard.html");

/// 嵌入式 PWA Web App Manifest，用于驱动 Edge 桌面应用模式的主题色与窗口特征
const MANIFEST_JSON: &str = r##"{
  "name": "AgentHub Runner",
  "short_name": "AgentHub",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#f4f4f6",
  "theme_color": "#f4f4f6",
  "icons": [
    {
      "src": "/icon.svg",
      "sizes": "any",
      "type": "image/svg+xml"
    }
  ]
}"##;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon_protocol::DaemonResponse;

    struct MockHandler;

    impl DaemonRequestHandler for MockHandler {
        fn handle(&self, request: DaemonRequest) -> DaemonResponse {
            match request {
                DaemonRequest::GetStatus => DaemonResponse::Status {
                    host_generation: "test-gen-123".into(),
                    workspaces: vec![PathBuf::from("/test/workspace")],
                    remote_enabled: true,
                    remote_connected: false,
                },
                _ => DaemonResponse::Error {
                    code: "not_implemented".into(),
                },
            }
        }
    }

    #[test]
    fn test_parse_path_and_query() {
        let (path, query) = parse_path_and_query("/api/events?after=10&limit=50");
        assert_eq!(path, "/api/events");
        assert_eq!(query.get("after").map(String::as_str), Some("10"));
        assert_eq!(query.get("limit").map(String::as_str), Some("50"));

        let (path_plain, query_empty) = parse_path_and_query("/api/status");
        assert_eq!(path_plain, "/api/status");
        assert!(query_empty.is_empty());
    }

    #[test]
    fn test_web_server_serves_html_and_api() {
        let stopped = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(MockHandler);

        let config = WebServerConfig {
            port: 14200,
            open_browser: false,
        };

        let bound_port = serve(config, handler, stopped.clone()).expect("server should bind");

        // 测试请求 /api/status
        let url = format!("http://127.0.0.1:{}/api/status", bound_port);
        let resp = ureq::get(&url)
            .call()
            .expect("should connect to web server");
        assert_eq!(resp.status().as_u16(), 200);

        // 测试请求 / 返回内置 HTML
        let root_url = format!("http://127.0.0.1:{}/", bound_port);
        let root_resp = ureq::get(&root_url)
            .call()
            .expect("should connect to root html");
        assert_eq!(root_resp.status().as_u16(), 200);

        stopped.store(true, Ordering::Relaxed);
    }
}
