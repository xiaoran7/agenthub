use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
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
    /// The identity file the device name lives beside.
    ///
    /// The rename endpoint has to write the same file the Runner reads, so the
    /// path is carried in rather than assumed to be the default one.
    pub identity_path: PathBuf,
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            port: 4200,
            open_browser: true,
            identity_path: crate::device_name::default_identity_path(),
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
    let identity_path = config.identity_path.clone();

    thread::Builder::new()
        .name("agenthub-web-server".into())
        .spawn(move || {
            while !server_stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let h = server_handler.clone();
                        let s = server_stopped.clone();
                        let id = identity_path.clone();
                        thread::spawn(move || {
                            let _ = handle_connection(stream, h, s, actual_port, id);
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
    identity_path: PathBuf,
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

    // A browser page on another origin may not drive this daemon. Only the
    // console the daemon serves itself may read its state or ask it to do
    // anything: the server holds the reader's transcripts and can move or
    // delete them, and a drive-by POST from any tab would otherwise be enough.
    // A request with no `Origin` at all is not a browser — the tray, a CLI, a
    // probe — and is served.
    let origin = headers.get("origin").map(String::as_str);
    if !origin_allowed(origin, headers.get("host").map(String::as_str)) {
        send_response(
            &mut stream,
            403,
            "Forbidden",
            "text/plain; charset=utf-8",
            b"origin not allowed",
            None,
        )?;
        return Ok(());
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    // The console is same-origin, so it never sends a preflight. One that
    // arrives came from a page that was already turned away above; it gets a
    // bare 204 with no grant, which is what stops the browser sending the
    // request it was asking about.
    if method == "OPTIONS" {
        send_response(&mut stream, 204, "No Content", "text/plain", b"", None)?;
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
            Some("no-cache, must-revalidate"),
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
            Some("no-cache, must-revalidate"),
        )?;
        return Ok(());
    }

    if path == "/icon.png" || path == "/icon-256.png" {
        send_response(
            &mut stream,
            200,
            "OK",
            "image/png",
            ICON_256_PNG,
            Some("no-cache, must-revalidate"),
        )?;
        return Ok(());
    }

    // Every physical size Windows/Chromium may ask for, served as a native
    // PNG so the high-DPI taskbar never has to up- or down-sample a frame.
    // 28 and 56 are the exact 175%-DPI metrics (SM_CXSMICON / SM_CXICON).
    for (name, bytes) in [
        ("/icon-16.png", ICON_16_PNG),
        ("/icon-20.png", ICON_20_PNG),
        ("/icon-24.png", ICON_24_PNG),
        ("/icon-28.png", ICON_28_PNG),
        ("/icon-30.png", ICON_30_PNG),
        ("/icon-32.png", ICON_32_PNG),
        ("/icon-36.png", ICON_36_PNG),
        ("/icon-40.png", ICON_40_PNG),
        ("/icon-42.png", ICON_42_PNG),
        ("/icon-48.png", ICON_48_PNG),
        ("/icon-56.png", ICON_56_PNG),
        ("/icon-64.png", ICON_64_PNG),
        ("/icon-72.png", ICON_72_PNG),
        ("/icon-96.png", ICON_96_PNG),
        ("/icon-128.png", ICON_128_PNG),
        ("/icon-192.png", ICON_192_PNG),
        ("/icon-512.png", ICON_512_PNG),
    ] {
        if path == name {
            send_response(
                &mut stream,
                200,
                "OK",
                "image/png",
                bytes,
                Some("no-cache, must-revalidate"),
            )?;
            return Ok(());
        }
    }

    if path == "/manifest.json" {
        send_response(
            &mut stream,
            200,
            "OK",
            "application/manifest+json",
            MANIFEST_JSON.as_bytes(),
            Some("no-cache, must-revalidate"),
        )?;
        return Ok(());
    }

    // the vendor marks the usage page draws beside a model or an agent, taken
    // verbatim from magpie's icon set (MIT, see console_icons.rs)
    if let Some(icon) = crate::console_icons::find(path) {
        let cache = if dev_assets() {
            "no-store"
        } else {
            "public, max-age=31536000, immutable"
        };
        send_response(&mut stream, 200, "OK", icon.mime, icon.bytes, Some(cache))?;
        return Ok(());
    }

    if path == "/dashboard.css" {
        // the sheet carries no build stamp itself; the page asks for a versioned
        // URL, so a release can never pair a new page with an old sheet
        // a debug build reads these from disk on every request, so no copy
        // may outlive the file it came from
        let cache = match query.get("v") {
            _ if dev_assets() => "no-store",
            Some(v) if v == ASSETS_VERSION => "no-cache, must-revalidate",
            _ => "no-cache",
        };
        let sheet = asset_text(DASHBOARD_CSS, "dashboard.css");
        send_response(
            &mut stream,
            200,
            "OK",
            "text/css; charset=utf-8",
            sheet.as_bytes(),
            Some(cache),
        )?;
        return Ok(());
    }

    if path == "/dashboard.js" {
        // the script is versioned the same way as the sheet: the page and the
        // code it runs are always from one build
        // a debug build reads these from disk on every request, so no copy
        // may outlive the file it came from
        let cache = match query.get("v") {
            _ if dev_assets() => "no-store",
            Some(v) if v == ASSETS_VERSION => "no-cache, must-revalidate",
            _ => "no-cache",
        };
        let script = asset_text(DASHBOARD_JS, "dashboard.js");
        send_response(
            &mut stream,
            200,
            "OK",
            "text/javascript; charset=utf-8",
            script.as_bytes(),
            Some(cache),
        )?;
        return Ok(());
    }

    // the debug build's watcher: answer as soon as anything under resources/
    // changes, or after twenty seconds, so the page's next ask hears it
    if path == "/api/dev/wait" && dev_assets() {
        let since = query.get("since").cloned().unwrap_or_default();
        let mut stamp = assets_stamp();
        for _ in 0..50 {
            if !since.is_empty() && stamp != since {
                break;
            }
            thread::sleep(Duration::from_millis(400));
            stamp = assets_stamp();
        }
        let body = json!({ "stamp": stamp }).to_string();
        send_response(
            &mut stream,
            200,
            "OK",
            "application/json; charset=utf-8",
            body.as_bytes(),
            Some("no-store"),
        )?;
        return Ok(());
    }

    // REST API endpoints
    if path.starts_with("/api/") {
        let req = ApiRequest {
            stream: &mut stream,
            method,
            path,
            query: &query,
            body: &body,
            handler,
            stopped,
            port,
            identity_path,
        };
        req.route()?;
        return Ok(());
    }

    // 默认提供嵌入式 CDS Web 界面: the page gets the layout tokens, the
    // versioned asset URL and the port it is actually served on stamped in, so
    // the console never carries a copy of a number the daemon already knows
    let page = render_index_html(port);
    // under `cargo run` the page also watches the sheet and the script it
    // was read from: an edit lands in the open window without a rebuild
    let page = if dev_assets() {
        page.replace(
            "</body>",
            &format!("<script>{DEV_RELOAD_JS}</script></body>"),
        )
    } else {
        page
    };
    send_response(
        &mut stream,
        200,
        "OK",
        "text/html; charset=utf-8",
        page.as_bytes(),
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

/// One REST call as it arrived: the socket to answer on, what was asked, and
/// the daemon and port that answer it.
struct ApiRequest<'a> {
    stream: &'a mut TcpStream,
    method: &'a str,
    path: &'a str,
    query: &'a HashMap<String, String>,
    body: &'a [u8],
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
    port: u16,
    /// The identity file the device name lives beside.
    identity_path: PathBuf,
}

impl ApiRequest<'_> {
    fn route(self) -> anyhow::Result<()> {
        let ApiRequest {
            stream,
            method,
            path,
            query,
            body,
            handler,
            stopped,
            port,
            identity_path,
        } = self;
        let _ = body;
        match (method, path) {
            ("GET", "/api/dashboard") => {
                let data = build_dashboard_summary(&handler, port, &identity_path);
                send_json_response(stream, 200, &data)?;
            }
            // the usage page's per-request ledger: one record per model call,
            // aggregated into the trend, the rankings, the heatmap and the
            // request list. `period` is today | 7d | 30d | all.
            ("GET", "/api/usage/ledger") => {
                let period =
                    crate::usage_ledger::Period::parse(query.get("period").map(String::as_str));
                let limit = query
                    .get("limit")
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(200)
                    .clamp(1, 1000);
                let records = crate::usage_ledger::claude_records();
                let digest = crate::usage_ledger::digest(&records, period, limit);
                send_json_response(stream, 200, &digest)?;
            }
            // Deleting a session moves its files to the runner's trash: the
            // conversation's transcript, its subagents' folder and what
            // Coding Assistant keeps by its id. A session written to in the
            // last minute is refused, as its agent may still be running it.
            ("POST", "/api/sessions/delete") => {
                let ids = serde_json::from_slice::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("ids").and_then(|ids| ids.as_array().cloned()))
                    .unwrap_or_default();
                let mut deleted: Vec<String> = Vec::new();
                let mut refused: Vec<serde_json::Value> = Vec::new();
                for id in ids.iter().filter_map(|id| id.as_str()).take(200) {
                    let id = id.strip_prefix("claude_").unwrap_or(id);
                    match crate::collectors::delete_claude_session(id) {
                        Ok(_trash) => deleted.push(format!("claude_{id}")),
                        Err(error) => refused.push(json!({
                            "id": format!("claude_{id}"),
                            "error": error.to_string(),
                        })),
                    }
                }
                // the page refreshes the moment this answers, and both caches
                // it reads from would otherwise still list what just went to
                // the trash: a second delete of the same row would be told
                // "no such session" although the first one worked
                if !deleted.is_empty() {
                    if let Ok(mut guard) = dashboard_summary_cache().write() {
                        *guard = None;
                    }
                    crate::usage_ledger::forget_transcripts();
                }
                send_json_response(
                    stream,
                    200,
                    &json!({ "deleted": deleted, "refused": refused }),
                )?;
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
            ("POST", "/api/open-data-dir") => {
                crate::tray::open_data_dir();
                let resp = json!({
                    "status": "ok",
                    "message": "已在系统文件管理器中打开本地数据存储目录"
                });
                send_json_response(stream, 200, &resp)?;
            }
            // the window's caption is painted the page's colour by the theme
            // extension the desktop window opens with; Edge reads that theme as
            // it starts, so the page leaves its choice here for the next window
            ("POST", "/api/window-theme") => {
                let dark = serde_json::from_slice::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| v.get("dark").and_then(|d| d.as_bool()));
                let saved = dark.and_then(crate::tray::remember_window_theme);
                let resp = match (dark, saved) {
                    (None, _) => json!({ "ok": false, "error": "dark_must_be_a_boolean" }),
                    (Some(dark), Some(())) => json!({ "ok": true, "dark": dark }),
                    (Some(_), None) => json!({ "ok": false, "error": "home_dir_unavailable" }),
                };
                send_json_response(stream, 200, &resp)?;
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
            ("GET", "/api/capabilities") => {
                let caps = crate::collectors::capabilities::discover_capabilities(None);
                send_json_response(stream, 200, &json!({ "ok": true, "capabilities": caps }))?;
            }
            ("POST", "/api/capabilities/mcp/register") => {
                #[derive(Deserialize)]
                struct RegisterMcpReq {
                    name: String,
                    command: Option<String>,
                    #[serde(default)]
                    args: Vec<String>,
                    url: Option<String>,
                    #[serde(default)]
                    headers: HashMap<String, String>,
                }
                match serde_json::from_slice::<RegisterMcpReq>(body) {
                    Ok(req) => {
                        match crate::collectors::capabilities::register_agenthub_mcp_server(
                            &req.name,
                            req.command,
                            req.args,
                            req.url,
                            req.headers,
                        ) {
                            Ok(srv) => send_json_response(
                                stream,
                                200,
                                &json!({ "ok": true, "server": srv }),
                            )?,
                            Err(err) => send_json_response(
                                stream,
                                400,
                                &json!({ "ok": false, "error": err }),
                            )?,
                        }
                    }
                    Err(err) => {
                        let err_json =
                            json!({ "error": format!("invalid_mcp_register_payload: {}", err) });
                        send_json_response(stream, 400, &err_json)?;
                    }
                }
            }
            ("POST", "/api/capabilities/sync") => {
                // Invalidate dashboard summary cache to ensure fresh capabilities view
                if let Ok(mut guard) = dashboard_summary_cache().write() {
                    *guard = None;
                }
                let caps = crate::collectors::capabilities::discover_capabilities(None);
                let resp = json!({
                    "ok": true,
                    "message": "已完成智能体扩展生态 (Skills/MCP/Hooks) 探测与统一对齐",
                    "capabilities": caps
                });
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
                                let err_json =
                                    json!({ "error": format!("load_identity_failed: {}", err) });
                                send_json_response(stream, 500, &err_json)?;
                                return Ok(());
                            }
                        };

                        let mut gateway_url = req.gateway.trim().trim_end_matches('/').to_string();
                        if !gateway_url.starts_with("http://")
                            && !gateway_url.starts_with("https://")
                        {
                            gateway_url = format!("https://{}", gateway_url);
                        }
                        let client = match crate::gateway::GatewayClient::new(
                            &gateway_url,
                            Duration::from_secs(10),
                        ) {
                            Ok(c) => c,
                            Err(err) => {
                                let err_json =
                                    json!({ "error": format!("invalid_gateway_url: {}", err) });
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
                                let _ = std::fs::write(
                                    &binding_path,
                                    serde_json::to_string_pretty(&binding_info).unwrap_or_default(),
                                );
                                let _ = handler.handle(DaemonRequest::SetRemoteEnabled {
                                    server_url: Some(gateway_url),
                                });
                                if let Ok(mut guard) = dashboard_summary_cache().write() {
                                    *guard = None;
                                }
                                if let Ok(mut guard) = device_info_cache().write() {
                                    *guard = None;
                                }
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
            ("POST", "/api/device/name") => {
                #[derive(Deserialize)]
                struct NameReq {
                    name: String,
                }
                match serde_json::from_slice::<NameReq>(body) {
                    Ok(req) => {
                        // Written beside the identity the Runner actually uses,
                        // so the running process picks the name up on its next
                        // publish instead of reading a different file.
                        match crate::device_name::write_configured_name(
                            &identity_path,
                            req.name.trim(),
                        ) {
                            Ok(()) => {
                                let name = crate::device_name::resolve(None, Some(&identity_path));
                                send_json_response(
                                    stream,
                                    200,
                                    &json!({ "ok": true, "device_name": name }),
                                )?;
                            }
                            Err(err) => {
                                let err_json =
                                    json!({ "error": format!("write_device_name_failed: {err}") });
                                send_json_response(stream, 500, &err_json)?;
                            }
                        }
                    }
                    Err(err) => {
                        let err_json = json!({ "error": format!("invalid_name_payload: {err}") });
                        send_json_response(stream, 400, &err_json)?;
                    }
                }
            }
            ("POST", "/api/device/unbind") => {
                let home = std::env::var_os("USERPROFILE")
                    .or_else(|| std::env::var_os("HOME"))
                    .map(PathBuf::from);
                if let Some(home) = home {
                    let binding_path = home.join(".agenthub").join("binding.json");
                    let _ = std::fs::remove_file(&binding_path);
                }
                let _ = handler.handle(DaemonRequest::SetRemoteEnabled { server_url: None });
                if let Ok(mut guard) = dashboard_summary_cache().write() {
                    *guard = None;
                }
                if let Ok(mut guard) = device_info_cache().write() {
                    *guard = None;
                }
                let resp = json!({
                    "ok": true,
                    "message": "已成功解除与网关的绑定关系"
                });
                send_json_response(stream, 200, &resp)?;
            }
            _ => {
                let err_json = json!({ "error": "not_found" });
                send_json_response(stream, 404, &err_json)?;
            }
        }
        Ok(())
    }
}

fn dashboard_summary_cache() -> &'static RwLock<Option<(Instant, serde_json::Value)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, serde_json::Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn build_dashboard_summary(
    handler: &Arc<dyn DaemonRequestHandler>,
    port: u16,
    identity_path: &Path,
) -> serde_json::Value {
    if let Ok(guard) = dashboard_summary_cache().read() {
        if let Some((cached_at, ref val)) = *guard {
            let has_sessions = val
                .get("sessions")
                .and_then(serde_json::Value::as_array)
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            let ttl = if has_sessions {
                Duration::from_secs(4)
            } else {
                Duration::from_millis(500)
            };
            if cached_at.elapsed() < ttl {
                let mut v = val.clone();
                v["port"] = json!(port);
                return v;
            }
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
    let models_res = handler.handle(DaemonRequest::GetModels);
    let status_res = handler.handle(DaemonRequest::GetStatus);

    let mut total_sessions = 0;
    let mut claude_count = 0;
    let mut claude_desktop_count = 0;
    let mut codex_count = 0;
    let mut grok_count = 0;
    let mut gemini_count = 0;
    let mut workspace_map: HashMap<String, usize> = HashMap::new();
    let mut discovered_sessions: Vec<serde_json::Value> = Vec::new();

    let mut total_requests: u64 = 0;
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
        crate::usage_ledger::calc_cost(model_name, prompt, completion, cache_read)
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
                let provider = c
                    .get("provider")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let raw_pt = c
                    .get("prompt_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let ct = c
                    .get("completion_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let cr = c
                    .get("cache_read_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let tt = c
                    .get("total_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(raw_pt + ct);
                let pt = if tt > raw_pt.saturating_add(ct) {
                    tt.saturating_sub(ct)
                } else {
                    raw_pt
                };
                let req_count = c
                    .get("request_count")
                    .and_then(serde_json::Value::as_u64)
                    .or_else(|| c.get("calls").and_then(serde_json::Value::as_u64))
                    .unwrap_or(1);

                total_requests = total_requests.saturating_add(req_count);
                total_prompt_tokens = total_prompt_tokens.saturating_add(pt);
                total_completion_tokens = total_completion_tokens.saturating_add(ct);
                total_cache_read_tokens = total_cache_read_tokens.saturating_add(cr);

                let raw_model = c
                    .get("model")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
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
                let model_entry =
                    model_aggs
                        .entry(model_name.clone())
                        .or_insert_with(|| ModelAgg {
                            name: model_name.clone(),
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

                // Agent aggregation
                let (agent_id, agent_display_name) = match provider {
                    "claude_code" | "claude" => ("claude_code", "Claude Code"),
                    "codex" => ("codex", "Codex App-Server"),
                    "claude_desktop" => ("claude_desktop", "Claude Desktop"),
                    "grok_build" | "grok" => ("grok_build", "Grok"),
                    "gemini_cli" | "gemini" => ("gemini_cli", "Gemini CLI"),
                    "zcode" => ("zcode", "ZCode Agent"),
                    _ => ("other", "Other Agent"),
                };
                let agent_entry =
                    agent_aggs
                        .entry(agent_id.to_string())
                        .or_insert_with(|| AgentAgg {
                            id: agent_id.to_string(),
                            name: agent_display_name.to_string(),
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

                match provider {
                    "claude_code" | "claude" => claude_count += 1,
                    "claude_desktop" => claude_desktop_count += 1,
                    "codex" => codex_count += 1,
                    "grok_build" | "grok" => grok_count += 1,
                    "gemini_cli" | "gemini" => gemini_count += 1,
                    // antigravity, zcode, cursor and cline still have their
                    // transcripts collected and listed under 会话, but they
                    // are not part of the console's five-agent matrix
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
        ((total_cache_read_tokens as f64 / total_prompt_tokens as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u32
    } else {
        0
    };

    let mut models_breakdown_vec: Vec<ModelAgg> = model_aggs.into_values().collect();
    // ties keep the name's order: a hash map hands its values out in an order
    // that changes between snapshots, and a list that reshuffles under the
    // reader every four seconds reads as noise, not as a live figure
    models_breakdown_vec.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.name.cmp(&b.name))
    });
    let models_breakdown: Vec<serde_json::Value> = models_breakdown_vec
        .into_iter()
        .map(|m| {
            let share_pct = if overall_tokens > 0 {
                ((m.total_tokens as f64 / overall_tokens as f64) * 100.0)
                    .round()
                    .min(100.0) as u32
            } else {
                0
            };
            let hit_rate = if m.prompt_tokens > 0 {
                ((m.cache_read_tokens as f64 / m.prompt_tokens as f64) * 100.0)
                    .round()
                    .min(100.0) as u32
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
    agents_breakdown_vec.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.id.cmp(&b.id))
    });
    let agents_breakdown: Vec<serde_json::Value> = agents_breakdown_vec
        .into_iter()
        .map(|a| {
            let share_pct = if overall_tokens > 0 {
                ((a.total_tokens as f64 / overall_tokens as f64) * 100.0)
                    .round()
                    .min(100.0) as u32
            } else {
                0
            };
            let hit_rate = if a.prompt_tokens > 0 {
                ((a.cache_read_tokens as f64 / a.prompt_tokens as f64) * 100.0)
                    .round()
                    .min(100.0) as u32
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
    let (grok_installed, grok_path) = check_agent_dir(".grok");
    let (gemini_installed, gemini_path) = find_executable(
        "gemini",
        &[r"AppData\Roaming\npm\gemini.cmd", r".local\bin\gemini"],
    );
    // Claude Desktop has no CLI to probe: its data folder is the evidence.
    let (claude_desktop_installed, claude_desktop_path) =
        check_agent_dir("AppData\\Local\\AIClaude");

    let mut active_agents_count = 0;
    for installed in [
        claude_installed,
        claude_desktop_installed,
        codex_installed,
        grok_installed,
        gemini_installed,
    ] {
        if installed {
            active_agents_count += 1;
        }
    }

    // The console's agent matrix is exactly the five official runtimes it
    // shows. Coding Assistant and Claude Desktop are two different products
    // that happen to share a transcript store, and are never folded together.
    let agents = json!([
        {
            "id": "claude_code",
            "name": "Coding Assistant",
            "installed": claude_installed,
            "path": claude_path,
            "sessions_count": claude_count,
            "command": "claude",
            "affinity": true,
            "description": "官方终端助手，支持 x-session-affinity 会话亲和性保护与 Prompt 缓存"
        },
        {
            "id": "claude_desktop",
            "name": "Claude Desktop",
            "installed": claude_desktop_installed || claude_desktop_count > 0,
            "path": claude_desktop_path,
            "sessions_count": claude_desktop_count,
            "command": "Claude",
            "affinity": false,
            "description": "Claude 桌面客户端的 Code / Cowork 会话，与 Coding Assistant 是两款独立产品"
        },
        {
            "id": "codex",
            "name": "Codex",
            "installed": codex_installed,
            "path": codex_path,
            "sessions_count": codex_count,
            "command": "codex app-server",
            "affinity": false,
            "description": "OpenAI Codex 智能代码辅助与工作区守护代理"
        },
        {
            "id": "grok_build",
            "name": "Grok",
            "installed": grok_installed,
            "path": grok_path,
            "sessions_count": grok_count,
            "command": "grok",
            "affinity": false,
            "description": "xAI Grok 构建协同代理与会话记录器"
        },
        {
            "id": "gemini_cli",
            "name": "Gemini CLI",
            "installed": gemini_installed,
            "path": gemini_path,
            "sessions_count": gemini_count,
            "command": "gemini",
            "affinity": false,
            "description": "Google Gemini 官方命令行编程助手"
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
    let capabilities = crate::collectors::capabilities::discover_capabilities(None);

    let summary = json!({
        "ready": is_ready,
        "status": if is_starting { "starting" } else if is_ready { "ready" } else { "idle" },
        "device_id": "local-dev",
        "port": port,
        "stats": {
            "total_sessions": total_sessions,
            "claude_sessions": claude_count,
            "claude_desktop_sessions": claude_desktop_count,
            "codex_sessions": codex_count,
            "grok_sessions": grok_count,
            "gemini_sessions": gemini_count,
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
        "sessions": discovered_sessions,
        "usage": usage_summary,
        "device": get_device_info(identity_path)
    });

    if total_sessions == 0 {
        if let Ok(guard) = dashboard_summary_cache().read() {
            if let Some((_, ref prev_val)) = *guard {
                if let Some(prev_sessions) = prev_val
                    .get("sessions")
                    .and_then(serde_json::Value::as_array)
                {
                    if !prev_sessions.is_empty() {
                        let mut v = prev_val.clone();
                        v["port"] = json!(port);
                        v["ready"] = json!(is_ready);
                        v["status"] = json!(if is_starting { "starting" } else { "ready" });
                        return v;
                    }
                }
            }
        }
    }

    if is_ready && total_sessions > 0 {
        if let Ok(mut guard) = dashboard_summary_cache().write() {
            *guard = Some((Instant::now(), summary.clone()));
        }
    } else if is_ready && total_sessions == 0 {
        let prev_had_sessions = dashboard_summary_cache()
            .read()
            .ok()
            .and_then(|g| {
                g.as_ref().map(|(_, v)| {
                    v.get("sessions")
                        .and_then(serde_json::Value::as_array)
                        .map(|a| !a.is_empty())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if !prev_had_sessions {
            if let Ok(mut guard) = dashboard_summary_cache().write() {
                *guard = Some((Instant::now(), summary.clone()));
            }
        }
    }
    summary
}

fn device_info_cache() -> &'static RwLock<Option<(Instant, serde_json::Value)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, serde_json::Value)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn get_device_info(identity_path: &Path) -> serde_json::Value {
    if let Ok(guard) = device_info_cache().read() {
        if let Some((cached_at, ref val)) = *guard {
            if cached_at.elapsed() < Duration::from_secs(5) {
                return val.clone();
            }
        }
    }

    let Some(home) = crate::device_name::home_dir() else {
        return json!({
            "device_id": "unknown",
            "public_key": "",
            "paired": false,
            "gateway_url": "",
            "device_name": crate::device_name::fallback_device_name(),
        });
    };

    // The binding lives beside the identity, wherever the Runner was told to
    // keep it.
    let agenthub_dir = identity_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".agenthub"));
    let binding_path = agenthub_dir.join("binding.json");

    let mut device_id = "unknown".to_string();
    let mut public_key = "".to_string();

    if let Ok(identity) = crate::device::DeviceIdentity::load(identity_path) {
        device_id = identity.device_id().to_string();
        public_key = identity.public_key_hex();
    } else if let Ok(content) = std::fs::read_to_string(identity_path) {
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
            paired = binding_val
                .get("registered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            gateway_url = binding_val
                .get("gateway")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            paired_at = binding_val
                .get("paired_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
    }

    let res = json!({
        "device_id": device_id,
        "public_key": public_key,
        "paired": paired,
        "gateway_url": gateway_url,
        "paired_at": paired_at,
        "device_name": crate::device_name::resolve(None, Some(identity_path)),
    });

    if let Ok(mut guard) = device_info_cache().write() {
        *guard = Some((Instant::now(), res.clone()));
    }
    res
}

fn executable_cache() -> &'static RwLock<HashMap<String, (Instant, (bool, Option<String>))>> {
    static CACHE: OnceLock<RwLock<HashMap<String, (Instant, (bool, Option<String>))>>> =
        OnceLock::new();
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
    static CACHE: OnceLock<RwLock<HashMap<String, (Instant, (bool, Option<String>))>>> =
        OnceLock::new();
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
        Some("no-cache, no-store, must-revalidate"),
    )
}

/// May a request from `origin` use this daemon? No origin is not a browser (a
/// CLI, the tray, a probe) and is served. A page is served only from this very
/// origin — the console the daemon serves itself — so the console's own reads
/// and writes work and a page on any other origin is turned away.
fn origin_allowed(origin: Option<&str>, host: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Some(host) = host else {
        return false;
    };
    origin
        .trim_end_matches('/')
        .eq_ignore_ascii_case(&format!("http://{host}"))
}

fn send_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
    cache_control: Option<&str>,
) -> anyhow::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status_code,
        status_text,
        content_type,
        body.len()
    );
    if let Some(cc) = cache_control {
        header.push_str(&format!("Cache-Control: {}\r\n", cc));
    }
    header.push_str("\r\n");

    stream.write_all(header.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()?;
    Ok(())
}

/// 嵌入式自包含静态图标资源（包含全尺寸 ICO 与高清晰度多分辨率 PNG 簇）
const FAVICON_ICO: &[u8] = include_bytes!("../resources/icon.ico");
const ICON_SVG: &str = include_str!("../resources/icon.svg");
const ICON_16_PNG: &[u8] = include_bytes!("../resources/icon-16.png");
const ICON_20_PNG: &[u8] = include_bytes!("../resources/icon-20.png");
const ICON_24_PNG: &[u8] = include_bytes!("../resources/icon-24.png");
const ICON_28_PNG: &[u8] = include_bytes!("../resources/icon-28.png");
const ICON_30_PNG: &[u8] = include_bytes!("../resources/icon-30.png");
const ICON_32_PNG: &[u8] = include_bytes!("../resources/icon-32.png");
const ICON_36_PNG: &[u8] = include_bytes!("../resources/icon-36.png");
const ICON_40_PNG: &[u8] = include_bytes!("../resources/icon-40.png");
const ICON_42_PNG: &[u8] = include_bytes!("../resources/icon-42.png");
const ICON_48_PNG: &[u8] = include_bytes!("../resources/icon-48.png");
const ICON_56_PNG: &[u8] = include_bytes!("../resources/icon-56.png");
const ICON_64_PNG: &[u8] = include_bytes!("../resources/icon-64.png");
const ICON_72_PNG: &[u8] = include_bytes!("../resources/icon-72.png");
const ICON_96_PNG: &[u8] = include_bytes!("../resources/icon-96.png");
const ICON_128_PNG: &[u8] = include_bytes!("../resources/icon-128.png");
const ICON_192_PNG: &[u8] = include_bytes!("../resources/icon-192.png");
const ICON_256_PNG: &[u8] = include_bytes!("../resources/icon-256.png");
const ICON_512_PNG: &[u8] = include_bytes!("../resources/icon-512.png");

/// 嵌入式自包含 CDS (Claude Design System) 单页前端
const INDEX_HTML: &str = include_str!("../resources/dashboard.html");
const DASHBOARD_CSS: &str = include_str!("../resources/dashboard.css");
/// the console's behaviour, kept in its own file so `node --check` and any
/// formatter can read it without parsing the page
const DASHBOARD_JS: &str = include_str!("../resources/dashboard.js");

/// A debug build (`cargo run`) reads the page, the sheet and the script from
/// `resources/` on every request and watches them, as magpie's `make dev`
/// does: an edit is in the open window at the next poll, with no rebuild and
/// no restart. A release build serves the copies embedded above and carries
/// no watcher at all.
const DEV_RELOAD_JS: &str = r#"(async () => {
  let seen = "";
  for (;;) {
    try {
      const r = await fetch("/api/dev/wait?since=" + seen, { cache: "no-store" });
      const j = await r.json();
      if (seen && j.stamp !== seen) { location.reload(); return; }
      seen = j.stamp;
    } catch (e) { await new Promise((r) => setTimeout(r, 1000)); }
  }
})();"#;

fn dev_assets() -> bool {
    // a debug build reads the console from disk; the tests read the copies
    // embedded in the test binary, so what they assert is what ships
    cfg!(all(debug_assertions, not(test)))
}

/// The console's three files, as this build will serve them: from memory in a
/// release, from disk in a debug build.
fn asset_text(embedded: &'static str, name: &str) -> String {
    if dev_assets() {
        if let Ok(text) = std::fs::read_to_string(assets_dir().join(name)) {
            return text;
        }
    }
    embedded.to_owned()
}

fn assets_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")
}

/// The newest modification time under `resources/`, as one string: a change
/// to any of the console's files changes it, and the page reloads.
fn assets_stamp() -> String {
    let mut newest = std::time::SystemTime::UNIX_EPOCH;
    if let Ok(entries) = std::fs::read_dir(assets_dir()) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    if let Ok(modified) = meta.modified() {
                        if modified > newest {
                            newest = modified;
                        }
                    }
                }
            }
        }
    }
    newest
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Bumped with every release; the page links the sheet and script under this
/// name.
const ASSETS_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), ".1");

/// The console's layout tokens, one place for both sides of the wire: the page
/// keeps the same numbers as its fallback, and the daemon stamps this object in
/// front of them so the window size, the poll cadence and the gateway port are
/// declared once.
fn app_layout_json(port: u16) -> String {
    let mut layout = json!({
        "version": ASSETS_VERSION,
        "webPort": port,
        "gatewayPort": 3425,
        "window": {
            "width": 820, "height": 600, "minWidth": 640, "minHeight": 460,
            "sideMinWidth": 168, "sideWidth": 212, "sideMaxWidth": 300
        },
        // the rail opens with the window beside at least a 560px column of
        // text; the button in its head folds it into the header, and the choice
        // outlives the window. On a first visit a narrow window starts folded
        "sidebarAt": 768, "sidebarNarrow": 200, "contentMax": 1100,
        "inset": 16, "radius": 14, "radiusSm": 10, "topBar": 46,
        "dashboardPollMs": 4000, "firstProbeRetries": 2, "firstProbeRetryMs": 1500,
        "emptyGraceMs": 15000, "emptyStrikes": 3,
        "themeBg": { "light": "#f4f5f8", "dark": "#1a1a1e" },
        "storage": {
            "theme": "agenthub_theme", "view": "agenthub_view",
            "layout": "agenthub_layout", "sideWidth": "agenthub_side_w",
            "closedGroups": "agenthub_closed_groups",
            "customGateway": "agenthub_custom_gateway", "sideNav": "agenthub_side_nav",
            "settingsTab": "agenthub_settings_tab", "pollMs": "agenthub_poll_ms",
            "motion": "agenthub_motion",
            "usageTab": "agenthub_usage_tab", "usagePeriod": "agenthub_usage_period",
            "usageEvery": "agenthub_usage_every", "ledMetric": "agenthub_led_metric",
            "ledSplit": "agenthub_led_split", "ledHeat": "agenthub_led_heat"
        }
    });
    if dev_assets() {
        layout["devAssets"] = json!(true);
    }
    layout.to_string()
}

/// The page as served: the layout object first, then the stylesheet and script
/// URLs at this build's version.
fn render_index_html(port: u16) -> String {
    let layout = format!(
        r#"<script>window.AppLayout = {};</script>"#,
        app_layout_json(port)
    );
    // in a release the page is the copy embedded in the binary; a debug build
    // reads `resources/dashboard.html` so an edit shows without a rebuild
    let page = asset_text(INDEX_HTML, "dashboard.html");
    // the page's fallback object opens its own script; the daemon's goes just
    // before it. The page ships with CRLF line endings, so anchor on text only.
    let marker = "// ---- the console's layout tokens";
    let at = page.find(marker).unwrap_or(0);
    let head = page[..at].rfind("<script>").unwrap_or(at);
    let html = format!("{}  {layout}\n  {}", &page[..head], &page[head..]);
    // a debug build asks for the sheet and the script under a name that
    // changes with the files themselves, so a reload after an edit can never
    // be answered from a copy of the one before it
    let version = if dev_assets() {
        format!("{ASSETS_VERSION}-{}", assets_stamp())
    } else {
        ASSETS_VERSION.to_owned()
    };
    html.replace("__ASSETS_VERSION__", &version)
}

/// 嵌入式 PWA Web App Manifest，用于驱动 Edge 桌面应用模式的主题色与窗口特征
const MANIFEST_JSON: &str = r##"{
  "name": "AgentHub Runner",
  "short_name": "AgentHub",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#18181b",
  "theme_color": "#18181b",
  "icons": [
    {
      "src": "/icon-28.png?v=2.5.7",
      "sizes": "28x28",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-30.png?v=2.5.7",
      "sizes": "30x30",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-36.png?v=2.5.7",
      "sizes": "36x36",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-42.png?v=2.5.7",
      "sizes": "42x42",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-48.png?v=2.5.7",
      "sizes": "48x48",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-56.png?v=2.5.7",
      "sizes": "56x56",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-72.png?v=2.5.7",
      "sizes": "72x72",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-96.png?v=2.5.7",
      "sizes": "96x96",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-128.png?v=2.5.7",
      "sizes": "128x128",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-192.png?v=2.5.7",
      "sizes": "192x192",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-256.png?v=2.5.7",
      "sizes": "256x256",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon-512.png?v=2.5.7",
      "sizes": "512x512",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon.png?v=2.5.7",
      "sizes": "256x256",
      "type": "image/png",
      "purpose": "any"
    },
    {
      "src": "/icon.svg?v=2.5.7",
      "sizes": "any",
      "type": "image/svg+xml",
      "purpose": "any"
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
    fn index_html_stamps_layout_once_and_versions_assets() {
        let html = render_index_html(4321);
        let stamped = html
            .find("window.AppLayout = {")
            .filter(|&i| {
                // the daemon's object is JSON, keys unordered: any quoted key will do
                html[i + "window.AppLayout = {".len()..].starts_with('"')
            })
            .expect("stamped layout");
        let fallback = html
            .find("window.AppLayout = Object.assign(")
            .expect("fallback");
        assert!(stamped < fallback);
        assert!(html.contains(r#""webPort":4321"#));
        assert!(!html.contains("__ASSETS_VERSION__"));
        assert!(html.contains(&format!("/dashboard.css?v={ASSETS_VERSION}")));
        assert!(html.contains(&format!("/dashboard.js?v={ASSETS_VERSION}")));
        // a real JSON object, not a fragment the page has to repair
        let json = app_layout_json(4321);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("layout json");
        assert_eq!(parsed["webPort"], 4321);
        // one size for the daemon's frame and the page's tokens, read here from
        // the daemon's own constant so the two cannot drift apart
        let tray = include_str!("tray.rs");
        let default = tray
            .split("const WINDOW_DEFAULT: (u32, u32) = (")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("WINDOW_DEFAULT");
        let mut sides = default.split(',').map(|n| n.trim().parse::<u64>().unwrap());
        assert_eq!(parsed["window"]["width"], sides.next().unwrap());
        assert_eq!(parsed["window"]["height"], sides.next().unwrap());
    }

    #[test]
    fn dashboard_css_is_served_from_the_embedded_sheet() {
        assert!(DASHBOARD_CSS.contains("--fs-body"));
        assert!(!DASHBOARD_CSS.contains("</style>"));
    }

    #[test]
    fn dashboard_js_is_its_own_embedded_file() {
        // the page loads it, never inlines it: a `</script>` in the source
        // would end the tag early if the two were ever put back together
        assert!(!INDEX_HTML.contains("function renderSessions"));
        assert!(DASHBOARD_JS.contains("function renderSessions"));
        assert!(!DASHBOARD_JS.contains("</script>"));
        // one pricing authority: the daemon. The page must not carry rates.
        assert!(!DASHBOARD_JS.contains("per 1M tokens"));
        assert!(!DASHBOARD_JS.contains("fmtCostFromTokens"));
    }

    #[test]
    fn dashboard_assets_include_capabilities_ecosystem() {
        // the capability ecosystem is a Settings tab now, not a rail page:
        // skills, MCP servers and hooks are configuration, read where the
        // rest of the configuration is
        assert!(!INDEX_HTML.contains("id=\"view-capabilities\""));
        assert!(!INDEX_HTML.contains("data-view=\"capabilities\""));
        assert!(INDEX_HTML.contains("id=\"setPage-capabilities\""));
        assert!(INDEX_HTML.contains("id=\"capSubTabs\""));
        assert!(INDEX_HTML.contains("id=\"registerMcpModal\""));
        // the ecosystem reads as flat console rows, the same .row idiom as
        // every other page, not as a second card design
        assert!(DASHBOARD_CSS.contains(".cap-row"));
        assert!(DASHBOARD_JS.contains("function renderCapabilities"));
        assert!(DASHBOARD_JS.contains("function submitRegisterMcp"));
    }

    #[test]
    fn dashboard_navigation_keeps_four_pages_and_settings() {
        // 会话 / 智能体 / 用量 / 工作区, and Settings behind the gear. The
        // models and gateway pages are gone: the ledger is the only place a
        // model is read, and routing is the upstream CPA's business
        for view in ["sessions", "agents", "usage", "workspaces"] {
            assert!(
                INDEX_HTML.contains(&format!("data-view=\"{view}\"")),
                "{view}"
            );
        }
        for gone in ["models", "gateway", "capabilities"] {
            assert!(
                !INDEX_HTML.contains(&format!("data-view=\"{gone}\"")),
                "{gone}"
            );
        }
        assert!(INDEX_HTML.contains("id=\"setTab-capabilities\""));
        assert!(INDEX_HTML.contains("id=\"setTab-gateway\""));
        // the footer no longer repeats the address Settings already shows
        assert!(!INDEX_HTML.contains("id=\"footPort\""));
        assert!(!INDEX_HTML.contains("id=\"footEndpoint\""));
    }

    #[test]
    fn dashboard_usage_page_reads_the_ledger() {
        for id in [
            "usageTabs",
            "usagePeriods",
            "usageEvery",
            "usageKpi",
            "ledChart",
            "ledRank",
            "ledHeat",
            "ledWrap",
            "usageSessions",
            "usageContext",
        ] {
            assert!(INDEX_HTML.contains(&format!("id=\"{id}\"")), "{id}");
        }
        for fn_name in [
            "initUsage",
            "loadUsage",
            "drawLedgerTrend",
            "drawLedgerHeat",
            "renderLedgerRequests",
            "exportLedgerCsv",
        ] {
            assert!(
                DASHBOARD_JS.contains(&format!("function {fn_name}")),
                "{fn_name}"
            );
        }
        assert!(DASHBOARD_JS.contains("/api/usage/ledger?period="));
        // the marks are the served files, never an inline redraw
        assert!(DASHBOARD_JS.contains("/icons/${file}"));
    }

    #[test]
    fn dashboard_agents_are_the_five_products() {
        for id in [
            "claude_code",
            "claude_desktop",
            "codex",
            "grok_build",
            "gemini_cli",
        ] {
            assert!(DASHBOARD_JS.contains(&format!("{id}:")), "{id}");
        }
    }

    #[test]
    fn only_the_consoles_own_origin_may_drive_the_daemon() {
        // a CLI, the tray, a probe: no Origin at all
        assert!(origin_allowed(None, Some("127.0.0.1:4200")));
        assert!(origin_allowed(None, None));
        // the console the daemon serves itself
        assert!(origin_allowed(
            Some("http://127.0.0.1:4200"),
            Some("127.0.0.1:4200")
        ));
        assert!(origin_allowed(
            Some("http://localhost:4200/"),
            Some("localhost:4200")
        ));
        // any other page, including one on a loopback port of its own
        assert!(!origin_allowed(
            Some("http://evil.example"),
            Some("127.0.0.1:4200")
        ));
        assert!(!origin_allowed(
            Some("http://127.0.0.1:3000"),
            Some("127.0.0.1:4200")
        ));
        assert!(!origin_allowed(
            Some("https://127.0.0.1:4200"),
            Some("127.0.0.1:4200")
        ));
        // an Origin without a Host cannot be matched to this daemon
        assert!(!origin_allowed(Some("http://127.0.0.1:4200"), None));
        // "null" is what a sandboxed frame and a file:// page send
        assert!(!origin_allowed(Some("null"), Some("127.0.0.1:4200")));
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
            identity_path: crate::device_name::default_identity_path(),
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

        // 测试 GET /api/capabilities
        let cap_url = format!("http://127.0.0.1:{}/api/capabilities", bound_port);
        let cap_resp = ureq::get(&cap_url)
            .call()
            .expect("should connect to /api/capabilities");
        assert_eq!(cap_resp.status().as_u16(), 200);

        // 测试 POST /api/capabilities/sync
        let sync_url = format!("http://127.0.0.1:{}/api/capabilities/sync", bound_port);
        let sync_resp = ureq::post(&sync_url)
            .send_empty()
            .expect("should connect to /api/capabilities/sync");
        assert_eq!(sync_resp.status().as_u16(), 200);

        stopped.store(true, Ordering::Relaxed);
    }

    /// A page on another origin must not be able to drive the daemon, and the
    /// gate has to sit before the body is read: a drive-by `POST` is refused
    /// whatever it asks for. Requests with no `Origin` (the tray, a CLI) are
    /// still served, and so is the console's own origin.
    #[test]
    fn a_foreign_origin_cannot_post_to_the_daemon() {
        let stopped = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(MockHandler);
        let config = WebServerConfig {
            port: 14300,
            open_browser: false,
            identity_path: crate::device_name::default_identity_path(),
        };
        let port = serve(config, handler, stopped.clone()).expect("server should bind");
        let url = format!("http://127.0.0.1:{}/api/capabilities/sync", port);

        let foreign = ureq::post(&url)
            .header("Origin", "https://evil.example")
            .send_empty();
        let refused = match foreign {
            Err(ureq::Error::StatusCode(code)) => code,
            Ok(resp) => resp.status().as_u16(),
            Err(other) => panic!("unexpected transport error: {other}"),
        };
        assert_eq!(refused, 403, "another page may not drive the daemon");

        // a sandboxed frame or a file:// page sends the literal "null"
        let nulled = ureq::post(&url).header("Origin", "null").send_empty();
        let code = match nulled {
            Err(ureq::Error::StatusCode(code)) => code,
            Ok(resp) => resp.status().as_u16(),
            Err(other) => panic!("unexpected transport error: {other}"),
        };
        assert_eq!(code, 403, "\"null\" is not this console");

        // the console's own origin, and a caller that sends no origin at all
        for origin in [Some(format!("http://127.0.0.1:{port}")), None] {
            let mut req = ureq::post(&url);
            if let Some(origin) = origin {
                req = req.header("Origin", &origin);
            }
            let resp = req
                .send_empty()
                .expect("the console and the tray are served");
            assert_eq!(resp.status().as_u16(), 200);
        }

        // no response hands a foreign page a grant to skip the check next time
        let resp = ureq::get(&format!("http://127.0.0.1:{port}/api/status"))
            .header("Origin", "https://evil.example")
            .call();
        if let Ok(resp) = resp {
            assert!(
                resp.headers().get("access-control-allow-origin").is_none(),
                "no CORS grant may be sent"
            );
        }

        stopped.store(true, Ordering::Relaxed);
    }
}
