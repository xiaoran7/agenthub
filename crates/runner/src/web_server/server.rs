use super::*;

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
    let active_connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let active_sse_connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut csrf_bytes = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut csrf_bytes);
    let csrf_token: Arc<str> = Arc::from(hex::encode(csrf_bytes));

    thread::Builder::new()
        .name("agenthub-web-server".into())
        .spawn(move || {
            const MAX_ACTIVE_WEB_CONNECTIONS: usize = 32;

            while !server_stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let prev = active_connections.fetch_add(1, Ordering::AcqRel);
                        if prev >= MAX_ACTIVE_WEB_CONNECTIONS {
                            active_connections.fetch_sub(1, Ordering::AcqRel);
                            let _ = stream.set_nonblocking(false);
                            let _ = send_response(
                                &mut stream,
                                503,
                                "Service Unavailable",
                                "text/plain; charset=utf-8",
                                b"too many concurrent connections",
                                Some("no-store"),
                            );
                            continue;
                        }
                        let permit = ConnectionPermit(active_connections.clone());
                        let sse_counter = active_sse_connections.clone();
                        let h = server_handler.clone();
                        let s = server_stopped.clone();
                        let id = identity_path.clone();
                        let csrf = csrf_token.clone();
                        thread::spawn(move || {
                            let _ = handle_connection(
                                stream,
                                h,
                                s,
                                actual_port,
                                id,
                                &csrf,
                                Some(permit),
                                sse_counter,
                            );
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

pub(crate) struct ConnectionPermit(pub(crate) Arc<std::sync::atomic::AtomicUsize>);
impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_connection(
    mut stream: TcpStream,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
    port: u16,
    identity_path: PathBuf,
    csrf_token: &str,
    mut web_permit: Option<ConnectionPermit>,
    sse_counter: Arc<std::sync::atomic::AtomicUsize>,
) -> anyhow::Result<()> {
    const MAX_ACTIVE_SSE_CONNECTIONS: usize = 16;
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let header_deadline = Instant::now() + Duration::from_secs(10);

    let mut reader = BufReader::new(stream.try_clone()?);
    let request_line = match read_bounded_line(&mut reader, MAX_HTTP_LINE_BYTES)? {
        BoundedLine::Eof => return Ok(()),
        BoundedLine::TooLong => {
            send_response(
                &mut stream,
                414,
                "URI Too Long",
                "text/plain; charset=utf-8",
                b"request line too long",
                None,
            )?;
            return Ok(());
        }
        BoundedLine::Line(line) => line,
    };

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let full_path = parts[1];

    let mut headers = HashMap::new();
    let mut content_length: Option<usize> = None;
    let mut header_count: usize = 0;
    loop {
        if Instant::now() > header_deadline {
            return Ok(());
        }
        let line = match read_bounded_line(&mut reader, MAX_HTTP_LINE_BYTES)? {
            BoundedLine::Eof => break,
            BoundedLine::TooLong => {
                send_response(
                    &mut stream,
                    431,
                    "Request Header Fields Too Large",
                    "text/plain; charset=utf-8",
                    b"header line too long",
                    None,
                )?;
                return Ok(());
            }
            BoundedLine::Line(line) => line,
        };
        if line.trim().is_empty() {
            break;
        }
        header_count += 1;
        if header_count > MAX_HTTP_HEADERS {
            send_response(
                &mut stream,
                431,
                "Request Header Fields Too Large",
                "text/plain; charset=utf-8",
                b"too many headers",
                None,
            )?;
            return Ok(());
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                match val.parse::<usize>() {
                    Ok(len) if len <= MAX_HTTP_BODY_BYTES => {
                        if content_length.is_some_and(|prev| prev != len) {
                            send_response(
                                &mut stream,
                                400,
                                "Bad Request",
                                "text/plain; charset=utf-8",
                                b"conflicting content-length",
                                None,
                            )?;
                            return Ok(());
                        }
                        content_length = Some(len);
                    }
                    Ok(_) => {
                        send_response(
                            &mut stream,
                            413,
                            "Payload Too Large",
                            "text/plain; charset=utf-8",
                            b"payload too large",
                            None,
                        )?;
                        return Ok(());
                    }
                    Err(_) => {
                        send_response(
                            &mut stream,
                            400,
                            "Bad Request",
                            "text/plain; charset=utf-8",
                            b"invalid content-length",
                            None,
                        )?;
                        return Ok(());
                    }
                }
            }
            headers.insert(key, val);
        }
    }

    let (path, query) = parse_path_and_query(full_path);

    // Reject DNS rebinding (`Host` must be loopback on this port), cross-origin
    // browser requests, and unexpected `Sec-Fetch-Site` before touching any body.
    if !host_allowed(headers.get("host").map(String::as_str), port) {
        send_response(
            &mut stream,
            403,
            "Forbidden",
            "text/plain; charset=utf-8",
            b"host not allowed",
            Some("no-store"),
        )?;
        return Ok(());
    }

    let origin = headers.get("origin").map(String::as_str);
    if !origin_allowed(origin, headers.get("host").map(String::as_str))
        || !sec_fetch_site_allowed(
            method,
            path,
            headers.get("sec-fetch-site").map(String::as_str),
        )
    {
        send_response(
            &mut stream,
            403,
            "Forbidden",
            "text/plain; charset=utf-8",
            b"origin not allowed",
            Some("no-store"),
        )?;
        return Ok(());
    }

    if headers
        .get("transfer-encoding")
        .is_some_and(|te| !te.trim().is_empty())
    {
        send_response(
            &mut stream,
            501,
            "Not Implemented",
            "text/plain; charset=utf-8",
            b"chunked transfer-encoding not supported",
            Some("no-store"),
        )?;
        return Ok(());
    }

    // The console is same-origin, so it never sends a preflight. One that
    // arrives came from a page that was already turned away above; it gets a
    // bare 204 with no grant, which is what stops the browser sending the
    // request it was asking about.
    if method == "OPTIONS" {
        send_response(&mut stream, 204, "No Content", "text/plain", b"", None)?;
        return Ok(());
    }

    // Mutating `/api/*` endpoints require a JSON content-type and the per-daemon
    // CSRF header (`X-AgentHub-Csrf`) stamped into `window.AppLayout`.
    if path.starts_with("/api/") && !matches!(method, "GET" | "HEAD") {
        let supplied_csrf = headers
            .get("x-agenthub-csrf")
            .map(String::as_str)
            .unwrap_or("");
        if !constant_time_eq(supplied_csrf, csrf_token) {
            send_json_response(&mut stream, 403, &json!({ "error": "csrf_required" }))?;
            return Ok(());
        }
        if !is_json_content_type(headers.get("content-type").map(String::as_str)) {
            send_json_response(
                &mut stream,
                415,
                &json!({ "error": "unsupported_media_type" }),
            )?;
            return Ok(());
        }
    }

    let body_len = content_length.unwrap_or(0);
    let body = match read_bounded_body(
        &mut reader,
        body_len,
        Instant::now() + Duration::from_secs(30),
    ) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
            send_response(
                &mut stream,
                408,
                "Request Timeout",
                "text/plain; charset=utf-8",
                b"request body timed out",
                Some("no-store"),
            )?;
            return Ok(());
        }
        Err(_) => return Ok(()),
    };

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
        let manifest = render_manifest_json();
        send_response(
            &mut stream,
            200,
            "OK",
            "application/manifest+json",
            manifest.as_bytes(),
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

    // The console's one long connection. It replaces the poll that made the
    // page ask every two seconds: the daemon answers only when a scan pass
    // found a change, and every row of the snapshot therefore lands in the
    // browser at the same moment instead of at each provider's own cache age.
    if path == "/api/events" {
        drop(web_permit.take());
        let prev_sse = sse_counter.fetch_add(1, Ordering::AcqRel);
        if prev_sse >= MAX_ACTIVE_SSE_CONNECTIONS {
            sse_counter.fetch_sub(1, Ordering::AcqRel);
            send_response(
                &mut stream,
                503,
                "Service Unavailable",
                "text/plain; charset=utf-8",
                b"too many concurrent event streams",
                Some("no-store"),
            )?;
            return Ok(());
        }
        let _sse_permit = ConnectionPermit(sse_counter);
        serve_change_stream(
            &mut stream,
            headers.get("last-event-id").map(String::as_str),
            &stopped,
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
            headers: &headers,
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
    let page = render_index_html(port, csrf_token);
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
