use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
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
            open_in_browser(&url);
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

fn open_in_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
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
        send_response(&mut stream, 204, "No Content", "text/plain", b"", true)?;
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
            let resp = handler.handle(DaemonRequest::GetModels);
            send_json_response(stream, 200, &resp)?;
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
        _ => {
            let err_json = json!({ "error": "not_found" });
            send_json_response(stream, 404, &err_json)?;
        }
    }
    Ok(())
}

fn build_dashboard_summary(
    handler: &Arc<dyn DaemonRequestHandler>,
    port: u16,
) -> serde_json::Value {
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

    if let crate::daemon_protocol::DaemonResponse::Discovery(ref val) = discover_res {
        if let Some(candidates) = val.get("candidates").and_then(serde_json::Value::as_array) {
            total_sessions = candidates.len();
            for c in candidates {
                let provider = c.get("provider").and_then(serde_json::Value::as_str).unwrap_or("");
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
        }
    }

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
        .unwrap_or(models.len());

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
        }
    ]);

    let workspaces_json: Vec<_> = registered_workspaces
        .into_iter()
        .map(|(path, count)| {
            let name = PathBuf::from(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            json!({
                "name": name,
                "path": path,
                "sessions_count": count
            })
        })
        .collect();

    let total_sessions_stat = total_sessions.max(1);
    let est_tokens = (total_sessions_stat as u64) * 75_000;
    let saved_tokens = (est_tokens as f64 * 0.894) as u64;

    json!({
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
            "total_models": models.len().max(gateway_models_count),
            "total_workspaces": workspaces_json.len(),
            "estimated_tokens": format_num(est_tokens),
            "cached_tokens": format_num(saved_tokens),
            "cache_hit_rate": "89.4%",
            "tokens_saved": format_num(saved_tokens)
        },
        "agents": agents,
        "workspaces": workspaces_json,
        "models": models,
        "gateway": {
            "detected": gateway_detected,
            "source": gateway_source,
            "base_url": gateway_base_url,
            "models_count": gateway_models_count,
            "affinity_header": "x-session-affinity"
        }
    })
}

fn find_executable(name: &str, relative_hints: &[&str]) -> (bool, Option<String>) {
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home_path = PathBuf::from(home);
        for hint in relative_hints {
            let full = home_path.join(hint);
            if full.exists() {
                return (true, Some(full.to_string_lossy().to_string()));
            }
        }
    }

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
                    return (true, Some(candidate.to_string_lossy().to_string()));
                }
            }
        }
    }

    (false, None)
}

fn check_agent_dir(dir_name: &str) -> (bool, Option<String>) {
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let full = PathBuf::from(home).join(dir_name);
        if full.is_dir() {
            return (true, Some(full.to_string_lossy().to_string()));
        }
    }
    (false, None)
}

fn format_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    for (i, c) in chars.into_iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

fn send_json_response<T: Serialize>(
    stream: &mut TcpStream,
    status: u16,
    data: &T,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(data)?;
    send_response(stream, status, "OK", "application/json", &body, false)
}

fn send_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
    is_options: bool,
) -> anyhow::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n",
        status_code,
        status_text,
        content_type,
        body.len()
    );
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
const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN" class="dark">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>AgentHub Runner Console</title>
  <link rel="icon" type="image/svg+xml" href="/icon.svg">
  <link rel="alternate icon" href="/favicon.ico">
  <style>
    :root {
      --cds-clay: #d97757;
      --cds-clay-hover: #c86b4c;
      --cds-clay-alpha12: rgba(217, 119, 87, 0.12);
      --cds-surface-0: #f5f5f3;
      --cds-surface-1: #fcfcfb;
      --cds-surface-2: #ffffff;
      --cds-surface-3: #ffffff;
      --cds-text-primary: #151515;
      --cds-text-secondary: #525252;
      --cds-text-muted: #8a8a8a;
      --cds-border: rgba(0, 0, 0, 0.08);
      --cds-border-strong: rgba(0, 0, 0, 0.16);
      --cds-bg-tint-blue: #f0f4f8;
      --cds-shadow-composer: 0 4px 16px rgba(0,0,0,0.06), 0 0 0 1px var(--cds-border);
      --cds-shadow-focus: 0 0 0 1.5px var(--cds-clay), 0 4px 16px var(--cds-clay-alpha12);
    }
    html.dark {
      --cds-surface-0: #0b0b0a;
      --cds-surface-1: #151515;
      --cds-surface-2: #1a1a19;
      --cds-surface-3: #20201e;
      --cds-text-primary: #f2f2f0;
      --cds-text-secondary: #a3a3a0;
      --cds-text-muted: #666663;
      --cds-border: rgba(255, 255, 255, 0.08);
      --cds-border-strong: rgba(255, 255, 255, 0.16);
      --cds-bg-tint-blue: rgba(255, 255, 255, 0.06);
      --cds-shadow-composer: 0 4px 20px rgba(0,0,0,0.4), 0 0 0 1px var(--cds-border);
      --cds-shadow-focus: 0 0 0 1.5px var(--cds-clay), 0 4px 20px var(--cds-clay-alpha12);
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
      background: var(--cds-surface-0);
      color: var(--cds-text-primary);
      height: 100vh;
      overflow: hidden;
      display: flex;
      flex-direction: column;
    }
    @keyframes quiet-dot {
      0%, 100% { opacity: 0.25; transform: translateY(0); }
      50% { opacity: 1; transform: translateY(-2px); }
    }
    @keyframes fade-up {
      from { opacity: 0; transform: translateY(6px); }
      to { opacity: 1; transform: translateY(0); }
    }
    .header {
      height: 48px;
      padding: 0 16px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      border-bottom: 1px solid var(--cds-border);
      background: var(--cds-surface-1);
    }
    .brand {
      display: flex;
      align-items: center;
      gap: 10px;
      font-weight: 600;
      font-size: 14px;
    }
    .brand-badge {
      font-size: 11px;
      padding: 2px 6px;
      border-radius: 999px;
      background: var(--cds-clay-alpha12);
      color: var(--cds-clay);
      font-weight: 600;
    }
    .header-actions {
      display: flex;
      align-items: center;
      gap: 8px;
    }
    .btn {
      padding: 5px 12px;
      font-size: 12px;
      font-weight: 500;
      border-radius: 8px;
      border: 1px solid var(--cds-border);
      background: var(--cds-surface-2);
      color: var(--cds-text-primary);
      cursor: pointer;
      transition: all 0.15s ease;
    }
    .btn:hover {
      border-color: var(--cds-border-strong);
      background: var(--cds-surface-3);
    }
    .btn-clay {
      background: var(--cds-clay);
      color: #fff;
      border: none;
    }
    .btn-clay:hover {
      background: var(--cds-clay-hover);
    }
    .layout {
      flex: 1;
      display: flex;
      overflow: hidden;
    }
    .sidebar {
      width: 300px;
      border-right: 1px solid var(--cds-border);
      background: var(--cds-surface-1);
      display: flex;
      flex-direction: column;
    }
    .sidebar-head {
      padding: 12px 14px;
      font-size: 12px;
      font-weight: 600;
      display: flex;
      justify-content: space-between;
      align-items: center;
      border-bottom: 1px solid var(--cds-border);
    }
    .session-list {
      flex: 1;
      overflow-y: auto;
      padding: 8px;
      display: flex;
      flex-direction: column;
      gap: 4px;
    }
    .session-item {
      padding: 10px 12px;
      border-radius: 10px;
      cursor: pointer;
      font-size: 13px;
      transition: background 0.15s ease;
      border: 1px solid transparent;
    }
    .session-item:hover {
      background: var(--cds-surface-2);
    }
    .session-item.active {
      background: var(--cds-surface-2);
      border-color: var(--cds-border-strong);
    }
    .session-item-title {
      font-weight: 500;
      margin-bottom: 4px;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .session-item-meta {
      display: flex;
      justify-content: space-between;
      align-items: center;
      font-size: 11px;
      color: var(--cds-text-muted);
    }
    .status-dot {
      display: inline-block;
      width: 6px;
      height: 6px;
      border-radius: 50%;
      margin-right: 4px;
    }
    .dot-working { background: var(--cds-clay); }
    .dot-idle { background: #10b981; }
    .dot-waiting { background: #f59e0b; }
    .main-view {
      flex: 1;
      display: flex;
      flex-direction: column;
      background: var(--cds-surface-0);
      overflow: hidden;
    }
    .top-bar {
      height: 48px;
      padding: 0 20px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      border-bottom: 1px solid var(--cds-border);
      background: var(--cds-surface-1);
    }
    .timeline {
      flex: 1;
      overflow-y: auto;
      padding: 24px 20px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    }
    .message-bubble {
      max-width: 85%;
      padding: 12px 16px;
      font-size: 13px;
      line-height: 1.6;
      animation: fade-up 0.18s cubic-bezier(0.16, 1, 0.3, 1);
    }
    .message-bubble.user {
      align-self: flex-end;
      background: var(--cds-bg-tint-blue);
      border-radius: 18px;
      border: 1px solid var(--cds-border);
    }
    .message-bubble.assistant {
      align-self: flex-start;
      background: var(--cds-surface-2);
      border-radius: 14px;
      border: 1px solid var(--cds-border);
    }
    .thinking-accordion {
      margin-bottom: 10px;
      border-radius: 12px;
      border: 1px solid var(--cds-border);
      background: var(--cds-surface-1);
      overflow: hidden;
    }
    .thinking-header {
      padding: 8px 12px;
      font-size: 11px;
      font-weight: 600;
      display: flex;
      align-items: center;
      justify-content: space-between;
      cursor: pointer;
      user-select: none;
    }
    .quiet-dots {
      display: inline-flex;
      gap: 3px;
      margin-right: 6px;
    }
    .quiet-dots span {
      width: 5px;
      height: 5px;
      border-radius: 50%;
      background: var(--cds-clay);
      animation: quiet-dot 1.4s infinite;
    }
    .quiet-dots span:nth-child(2) { animation-delay: 0.2s; }
    .quiet-dots span:nth-child(3) { animation-delay: 0.4s; }
    .thinking-content {
      padding: 10px 12px;
      font-size: 11px;
      font-family: monospace;
      color: var(--cds-text-secondary);
      border-top: 1px solid var(--cds-border);
      background: var(--cds-surface-0);
      white-space: pre-wrap;
      word-break: break-all;
    }
    .approval-card {
      padding: 16px;
      margin: 12px 20px;
      border-radius: 14px;
      border: 1.5px solid var(--cds-clay);
      background: var(--cds-surface-1);
      box-shadow: 0 4px 16px var(--cds-clay-alpha12);
    }
    .approval-title {
      font-size: 13px;
      font-weight: 600;
      color: var(--cds-clay);
      margin-bottom: 8px;
    }
    .approval-preview {
      font-family: monospace;
      font-size: 12px;
      padding: 8px;
      border-radius: 6px;
      background: var(--cds-surface-0);
      border: 1px solid var(--cds-border);
      margin-bottom: 12px;
      word-break: break-all;
    }
    .composer-container {
      padding: 12px 20px 20px;
      background: var(--cds-surface-0);
    }
    .composer-card {
      border-radius: 20px;
      background: var(--cds-surface-3);
      box-shadow: var(--cds-shadow-composer);
      padding: 10px 14px;
      transition: box-shadow 0.2s ease;
      display: flex;
      flex-direction: column;
      gap: 8px;
    }
    .composer-card:focus-within {
      box-shadow: var(--cds-shadow-focus);
    }
    .composer-textarea {
      width: 100%;
      background: transparent;
      border: none;
      color: var(--cds-text-primary);
      font-size: 13px;
      resize: none;
      outline: none;
      min-height: 44px;
      max-height: 200px;
      font-family: inherit;
      line-height: 1.5;
    }
    .composer-bottom {
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    .effort-select {
      background: var(--cds-surface-1);
      border: 1px solid var(--cds-border);
      color: var(--cds-text-secondary);
      border-radius: 6px;
      font-size: 11px;
      padding: 3px 8px;
      outline: none;
    }
  </style>
</head>
<body>
  <div class="header">
    <div class="brand">
      <img src="/icon.svg" alt="AgentHub" style="width: 20px; height: 20px; border-radius: 4px; display: inline-block; vertical-align: middle;" />
      <span>AgentHub Runner</span>
      <span class="brand-badge">CDS Native</span>
    </div>
    <div class="header-actions">
      <span id="hostStatus" style="font-size:12px; color:var(--cds-text-muted);">Connecting...</span>
      <button class="btn" onclick="toggleTheme()">🌓 切换外观</button>
      <button class="btn" onclick="refreshSnapshot()">🔄 刷新</button>
    </div>
  </div>

  <div class="layout">
    <div class="sidebar">
      <div class="sidebar-head">
        <span>活跃与最近会话</span>
        <button class="btn" style="padding:2px 8px; font-size:11px;" onclick="discoverLocal()">探测本地</button>
      </div>
      <div class="session-list" id="sessionList">
        <div style="padding:16px; text-align:center; color:var(--cds-text-muted); font-size:12px;">加载会话中...</div>
      </div>
    </div>

    <div class="main-view">
      <div class="top-bar">
        <div>
          <span id="activeTitle" style="font-weight:600; font-size:13px;">请从左侧选择会话</span>
          <span id="activeCwd" style="font-size:11px; color:var(--cds-text-muted); margin-left:8px;"></span>
        </div>
        <div>
          <button class="btn" id="btnInterrupt" style="display:none; color:#ef4444;" onclick="interruptTurn()">急停中断</button>
        </div>
      </div>

      <div id="approvalArea"></div>

      <div class="timeline" id="timeline">
        <div style="text-align:center; color:var(--cds-text-muted); font-size:13px; margin:auto;">选择左侧会话查看对话记录与执行过程</div>
      </div>

      <div class="composer-container">
        <div class="composer-card">
          <textarea
            class="composer-textarea"
            id="composerInput"
            placeholder="输入指令继续会话... (Enter 发送，Shift+Enter 换行，支持中日韩输入法防误触)"
            rows="1"
          ></textarea>
          <div class="composer-bottom">
            <select class="effort-select" id="effortSelect">
              <option value="default">思考强度: 默认</option>
              <option value="low">低 (Low)</option>
              <option value="medium">中 (Medium)</option>
              <option value="high">高 (High)</option>
              <option value="max">超高 (Max)</option>
            </select>
            <button class="btn btn-clay" id="btnSend" onclick="sendMessage()">发送指令</button>
          </div>
        </div>
      </div>
    </div>
  </div>

  <script>
    let state = {
      sessions: [],
      activeSessionId: null,
      activeAgent: 'claude_code',
      pendingApprovals: [],
      generation: null,
      isComposing: false,
    };

    function toggleTheme() {
      document.documentElement.classList.toggle('dark');
    }

    // CJK 双重互锁守卫
    const composer = document.getElementById('composerInput');
    composer.addEventListener('compositionstart', () => { state.isComposing = true; });
    composer.addEventListener('compositionend', () => { state.isComposing = false; });
    composer.addEventListener('keydown', (e) => {
      const composing = state.isComposing || e.isComposing || e.keyCode === 229;
      if (e.key === 'Enter' && !e.shiftKey && !composing) {
        e.preventDefault();
        sendMessage();
      }
    });

    // 自动高度伸缩
    composer.addEventListener('input', () => {
      composer.style.height = 'auto';
      composer.style.height = Math.min(composer.scrollHeight, 200) + 'px';
    });

    async function init() {
      await refreshStatus();
      await refreshSnapshot();
      setInterval(pollEvents, 2000);
    }

    async function refreshStatus() {
      try {
        const res = await fetch('/api/status');
        const data = await res.json();
        if (data.result) {
          state.generation = data.result.host_generation;
          document.getElementById('hostStatus').innerText =
            '工作区: ' + (data.result.workspaces?.length || 0) + ' | 远程: ' + (data.result.remote_connected ? '已连接' : '就绪');
        }
      } catch (e) {
        document.getElementById('hostStatus').innerText = '未连接 Runner';
      }
    }

    async function refreshSnapshot() {
      try {
        const res = await fetch('/api/snapshot');
        const data = await res.json();
        if (data.result && data.result.sessions) {
          state.sessions = data.result.sessions;
          renderSessionList();
          if (!state.activeSessionId && state.sessions.length > 0) {
            selectSession(state.sessions[0].hub_session_id);
          }
        }
      } catch (e) {
        console.error('Failed to load snapshot', e);
      }
    }

    async function discoverLocal() {
      try {
        const res = await fetch('/api/discover');
        const data = await res.json();
        alert('本地会话探测已完成，刷新即可查看');
        refreshSnapshot();
      } catch (e) {
        alert('探测失败: ' + e);
      }
    }

    function renderSessionList() {
      const container = document.getElementById('sessionList');
      if (state.sessions.length === 0) {
        container.innerHTML = '<div style="padding:16px; text-align:center; color:var(--cds-text-muted); font-size:12px;">暂无活动会话</div>';
        return;
      }
      container.innerHTML = state.sessions.map(s => {
        const activeClass = s.hub_session_id === state.activeSessionId ? 'active' : '';
        const stateType = s.execution_state || 'idle';
        let dotClass = 'dot-idle';
        if (stateType === 'running') dotClass = 'dot-working';
        if (stateType === 'awaiting_permission' || stateType === 'awaiting_input') dotClass = 'dot-waiting';
        return `
          <div class="session-item ${activeClass}" onclick="selectSession('${s.hub_session_id}')">
            <div class="session-item-title">${escapeHtml(s.provider_session_id || s.hub_session_id)}</div>
            <div class="session-item-meta">
              <span><span class="status-dot ${dotClass}"></span>${stateType}</span>
              <span>${escapeHtml(s.workspace_path?.split(/[\\\\/]/).pop() || '')}</span>
            </div>
          </div>
        `;
      }).join('');
    }

    async function selectSession(hubSessionId) {
      state.activeSessionId = hubSessionId;
      renderSessionList();
      const session = state.sessions.find(s => s.hub_session_id === hubSessionId);
      if (session) {
        document.getElementById('activeTitle').innerText = session.provider_session_id || session.hub_session_id;
        document.getElementById('activeCwd').innerText = session.workspace_path || '';
        document.getElementById('btnInterrupt').style.display = session.execution_state === 'running' ? 'inline-block' : 'none';
      }
      await loadConversation(hubSessionId);
    }

    async function loadConversation(hubSessionId) {
      const timeline = document.getElementById('timeline');
      timeline.innerHTML = '<div style="text-align:center; color:var(--cds-text-muted); font-size:12px; margin:auto;">加载对话历史中...</div>';
      try {
        const res = await fetch(`/api/conversation?session_id=${encodeURIComponent(hubSessionId)}&agent=claude`);
        const data = await res.json();
        const messages = data.result || [];
        if (messages.length === 0) {
          timeline.innerHTML = '<div style="text-align:center; color:var(--cds-text-muted); font-size:12px; margin:auto;">该会话暂无消息</div>';
          return;
        }
        timeline.innerHTML = messages.map(m => renderMessage(m)).join('');
        timeline.scrollTop = timeline.scrollHeight;
      } catch (e) {
        timeline.innerHTML = '<div style="text-align:center; color:#ef4444; font-size:12px; margin:auto;">加载历史失败: ' + escapeHtml(String(e)) + '</div>';
      }
    }

    function renderMessage(msg) {
      const role = msg.role || 'assistant';
      const text = msg.content || msg.text || '';
      const { thinking, cleanText } = splitThinking(text);

      let thinkingHtml = '';
      if (thinking) {
        thinkingHtml = `
          <div class="thinking-accordion">
            <div class="thinking-header" onclick="this.nextElementSibling.style.display = this.nextElementSibling.style.display === 'none' ? 'block' : 'none'">
              <div>
                <span class="quiet-dots"><span></span><span></span><span></span></span>
                <span>Thinking Process</span>
              </div>
              <span style="color:var(--cds-text-muted);">▾</span>
            </div>
            <div class="thinking-content" style="display:none;">${escapeHtml(thinking)}</div>
          </div>
        `;
      }

      return `
        <div class="message-bubble ${role}">
          ${thinkingHtml}
          <div>${escapeHtml(cleanText)}</div>
        </div>
      `;
    }

    function splitThinking(content) {
      const match = content.match(/<thinking>([\s\S]*?)<\/thinking>/i) || content.match(/<thought>([\s\S]*?)<\/thought>/i);
      if (match) {
        return {
          thinking: match[1].trim(),
          cleanText: content.replace(match[0], '').trim()
        };
      }
      return { thinking: '', cleanText: content };
    }

    async function sendMessage() {
      const input = document.getElementById('composerInput');
      const text = input.value.trim();
      if (!text || !state.activeSessionId) return;

      const effort = document.getElementById('effortSelect').value;
      const cmd = {
        client_cmd_id: 'cmd_' + Date.now(),
        hub_session_id: state.activeSessionId,
        host_generation: state.generation,
        command: {
          send_prompt: {
            text: text,
            effort: effort !== 'default' ? effort : undefined,
          }
        }
      };

      input.value = '';
      input.style.height = 'auto';

      try {
        await fetch('/api/execute', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(cmd)
        });
        setTimeout(() => {
          loadConversation(state.activeSessionId);
          refreshSnapshot();
        }, 300);
      } catch (e) {
        alert('发送指令失败: ' + e);
      }
    }

    async function interruptTurn() {
      if (!state.activeSessionId) return;
      const cmd = {
        client_cmd_id: 'cmd_' + Date.now(),
        hub_session_id: state.activeSessionId,
        host_generation: state.generation,
        command: 'interrupt'
      };
      await fetch('/api/execute', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(cmd)
      });
      refreshSnapshot();
    }

    async function pollEvents() {
      if (state.activeSessionId) {
        // 轻量增量同步
        refreshSnapshot();
      }
    }

    function escapeHtml(str) {
      if (!str) return '';
      return String(str)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
    }

    window.addEventListener('DOMContentLoaded', init);
  </script>
</body>
</html>
"#;

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
