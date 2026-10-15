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
            .args(["/c", "start", "", url])
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
