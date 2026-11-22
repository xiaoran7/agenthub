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
    let html = render_index_html(4321, "test-csrf");
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
    let json = app_layout_json(4321, "test-csrf");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("layout json");
    assert_eq!(parsed["webPort"], 4321);
    assert_eq!(parsed["csrfToken"], "test-csrf");
    // one size for the daemon's frame and the page's tokens, read here from
    // the daemon's own constant so the two cannot drift apart
    let tray = include_str!("../tray/windows.rs");
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
        "ledSubTabs",
        "ledColsBtn",
        "ledColPickerWrap",
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
    assert!(!INDEX_HTML.contains("id=\"usageEvery\""));
    assert!(!INDEX_HTML.contains("id=\"pollSegs\""));
    for fn_name in [
        "initUsage",
        "loadUsage",
        "drawLedgerTrend",
        "drawLedgerHeat",
        "renderLedgerRequests",
        "toggleLedgerColsMenu",
        "toggleLedgerFilterMenu",
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
        "antigravity",
        "deepseek_harness",
    ] {
        assert!(DASHBOARD_JS.contains(&format!("{id}:")), "{id}");
    }
    assert!(INDEX_HTML.contains("id=\"setTab-plugins\""));
    assert!(INDEX_HTML.contains("id=\"setPage-plugins\""));
    assert!(DASHBOARD_JS.contains("function refreshPluginCatalog"));
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

fn extract_csrf_from_root(root_html: &str) -> String {
    let marker = "window.AppLayout = ";
    let start = root_html.find(marker).expect("AppLayout in html") + marker.len();
    let end = root_html[start..].find(";</script>").expect("end script") + start;
    let parsed: serde_json::Value =
        serde_json::from_str(&root_html[start..end]).expect("valid AppLayout json");
    parsed["csrfToken"]
        .as_str()
        .expect("csrfToken present")
        .to_string()
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
    let mut root_resp = ureq::get(&root_url)
        .call()
        .expect("should connect to root html");
    assert_eq!(root_resp.status().as_u16(), 200);
    let root_html = root_resp.body_mut().read_to_string().unwrap();
    let csrf = extract_csrf_from_root(&root_html);

    // 测试 GET /api/capabilities
    let cap_url = format!("http://127.0.0.1:{}/api/capabilities", bound_port);
    let cap_resp = ureq::get(&cap_url)
        .call()
        .expect("should connect to /api/capabilities");
    assert_eq!(cap_resp.status().as_u16(), 200);

    // 测试 POST /api/capabilities/sync (需携带 CSRF + JSON Content-Type)
    let sync_url = format!("http://127.0.0.1:{}/api/capabilities/sync", bound_port);
    let sync_resp = ureq::post(&sync_url)
        .header("X-AgentHub-Csrf", &csrf)
        .content_type("application/json")
        .send("{}")
        .expect("should connect to /api/capabilities/sync");
    assert_eq!(sync_resp.status().as_u16(), 200);

    stopped.store(true, Ordering::Relaxed);
}

/// The change stream is the console's only long connection. It must
/// answer with `text/event-stream`, send its opening frame at once, and
/// deliver a `change` frame when the daemon publishes — without waiting
/// for a poll.
#[test]
fn the_change_stream_answers_with_events_and_follows_publishes() {
    use std::io::{BufRead, BufReader, Write as _};

    let stopped = Arc::new(AtomicBool::new(false));
    let handler = Arc::new(MockHandler);
    let config = WebServerConfig {
        port: 14400,
        open_browser: false,
        identity_path: crate::device_name::default_identity_path(),
    };
    let port = serve(config, handler, stopped.clone()).expect("server should bind");

    let mut stream =
        std::net::TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect to the stream");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"GET /api/events HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .unwrap();

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut headers = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() {
            break;
        }
        headers.push_str(&line);
    }
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "stream must answer 200: {headers}"
    );
    assert!(
        headers
            .to_lowercase()
            .contains("content-type: text/event-stream"),
        "stream must be an event stream: {headers}"
    );

    // The opening `ready` frame names the version the page should start
    // from, so a page that only just connected has nothing to re-read.
    let mut opening = String::new();
    for _ in 0..6 {
        line.clear();
        reader.read_line(&mut line).unwrap();
        opening.push_str(&line);
        if line.starts_with("data:") {
            break;
        }
    }
    assert!(
        opening.contains("event: ready"),
        "first frame is ready: {opening}"
    );

    crate::live::publish();
    let mut change = String::new();
    for _ in 0..8 {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => panic!("stream closed before a change frame: {error}"),
        }
        change.push_str(&line);
        if line.starts_with("data:") && change.contains("event: change") {
            break;
        }
    }
    assert!(
        change.contains("event: change"),
        "a publish must reach the stream at once: {change}"
    );

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

    let agent = ureq::Agent::config_builder()
        .max_idle_connections(0)
        .build()
        .new_agent();

    let foreign = agent
        .post(&url)
        .header("Origin", "https://evil.example")
        .send_empty();
    let refused = match foreign {
        Err(ureq::Error::StatusCode(code)) => code,
        Ok(resp) => resp.status().as_u16(),
        Err(other) => panic!("unexpected transport error: {other}"),
    };
    assert_eq!(refused, 403, "another page may not drive the daemon");

    // a sandboxed frame or a file:// page sends the literal "null"
    let nulled = agent.post(&url).header("Origin", "null").send_empty();
    let code = match nulled {
        Err(ureq::Error::StatusCode(code)) => code,
        Ok(resp) => resp.status().as_u16(),
        Err(other) => panic!("unexpected transport error: {other}"),
    };
    assert_eq!(code, 403, "\"null\" is not this console");

    let root_html = agent
        .get(&format!("http://127.0.0.1:{port}/"))
        .call()
        .unwrap()
        .body_mut()
        .read_to_string()
        .unwrap();
    let csrf = extract_csrf_from_root(&root_html);

    // the console's own origin, and a caller that sends no origin at all (with CSRF + JSON)
    for origin in [Some(format!("http://127.0.0.1:{port}")), None] {
        let mut req = agent
            .post(&url)
            .header("X-AgentHub-Csrf", &csrf)
            .content_type("application/json");
        if let Some(origin) = origin {
            req = req.header("Origin", &origin);
        }
        let resp = req.send("{}").expect("the console and the tray are served");
        assert_eq!(resp.status().as_u16(), 200);
    }

    // Missing CSRF token or wrong Content-Type must be rejected before routing
    let missing_csrf = agent
        .post(&url)
        .header("Origin", &format!("http://127.0.0.1:{port}"))
        .content_type("application/json")
        .send("{}");
    assert!(matches!(missing_csrf, Err(ureq::Error::StatusCode(403))));

    let wrong_ct = agent
        .post(&url)
        .header("Origin", &format!("http://127.0.0.1:{port}"))
        .header("X-AgentHub-Csrf", &csrf)
        .content_type("text/plain")
        .send("{}");
    assert!(matches!(wrong_ct, Err(ureq::Error::StatusCode(415))));

    // DNS rebinding Host or cross-site Sec-Fetch-Site must be blocked
    let rebind_host = agent
        .get(&format!("http://127.0.0.1:{port}/api/status"))
        .header("Host", "evil.example:4200")
        .call();
    assert!(matches!(rebind_host, Err(ureq::Error::StatusCode(403))));

    let cross_fetch = agent
        .get(&format!("http://127.0.0.1:{port}/api/status"))
        .header("Sec-Fetch-Site", "cross-site")
        .call();
    assert!(matches!(cross_fetch, Err(ureq::Error::StatusCode(403))));

    // no response hands a foreign page a grant to skip the check next time
    let resp = agent
        .get(&format!("http://127.0.0.1:{port}/api/status"))
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

#[test]
fn cached_agent_installations_exposes_antigravity_slot() {
    let installs = crate::web_server::device_info::cached_agent_installations();
    assert_eq!(installs.len(), 5);
}

#[test]
fn plugin_routes_expose_local_state_and_validate_inputs() {
    let stopped = Arc::new(AtomicBool::new(false));
    let handler = Arc::new(MockHandler);
    let config = WebServerConfig {
        port: 14321,
        open_browser: false,
        identity_path: crate::device_name::default_identity_path(),
    };
    let port = serve(config, handler, stopped.clone()).expect("server should bind");
    let agent = ureq::Agent::new_with_defaults();

    let list_resp: serde_json::Value = agent
        .get(&format!("http://127.0.0.1:{port}/api/plugins"))
        .call()
        .expect("/api/plugins should respond")
        .body_mut()
        .read_json()
        .expect("json response");
    assert_eq!(list_resp["ok"], true);
    assert!(list_resp["plugins"].is_array());
    assert!(list_resp["registry_url"].is_string());

    let missing_icon = agent
        .get(&format!(
            "http://127.0.0.1:{port}/api/plugins/icon/nonexistent-plugin"
        ))
        .call();
    assert!(matches!(missing_icon, Err(ureq::Error::StatusCode(404))));

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind header check");
    let check_port = listener.local_addr().unwrap().port();
    let h = thread::spawn(move || {
        let (mut srv, _) = listener.accept().unwrap();
        super::http_io::send_sandboxed_response(
            &mut srv,
            200,
            "OK",
            "image/svg+xml",
            b"<svg/>",
            None,
        )
        .unwrap();
    });
    let mut cli = TcpStream::connect(("127.0.0.1", check_port)).unwrap();
    let mut raw_http = String::new();
    cli.read_to_string(&mut raw_http).unwrap();
    h.join().unwrap();
    assert!(
        raw_http.contains("Content-Security-Policy: default-src 'none'; sandbox"),
        "expected sandbox CSP header, got: {raw_http}"
    );

    stopped.store(true, Ordering::Relaxed);
}

#[test]
fn oversized_content_length_returns_413_without_oom() {
    let stopped = Arc::new(AtomicBool::new(false));
    let handler = Arc::new(MockHandler);
    let config = WebServerConfig {
        port: 14320,
        open_browser: false,
        identity_path: crate::device_name::default_identity_path(),
    };
    let port = serve(config, handler, stopped.clone()).expect("server should bind");

    let mut conn = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    conn.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    conn.write_all(
        b"POST /api/capabilities/sync HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 99999999999\r\n\r\n",
    )
    .unwrap();
    conn.flush().unwrap();

    let mut resp = String::new();
    conn.read_to_string(&mut resp).unwrap();
    assert!(
        resp.starts_with("HTTP/1.1 413"),
        "expected 413 Payload Too Large, got: {resp}"
    );

    let mut long_hdr = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    long_hdr
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let huge = "A".repeat(MAX_HTTP_LINE_BYTES + 128);
    write!(
        long_hdr,
        "GET /api/status HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Huge: {huge}\r\n\r\n"
    )
    .unwrap();
    long_hdr.flush().unwrap();
    let mut resp2 = String::new();
    long_hdr.read_to_string(&mut resp2).unwrap();
    assert!(
        resp2.starts_with("HTTP/1.1 431"),
        "expected 431 Request Header Fields Too Large, got: {resp2}"
    );

    stopped.store(true, Ordering::Relaxed);
}
