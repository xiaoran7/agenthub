use super::*;

pub(crate) const SECURITY_HEADERS: &str = "X-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: frame-ancestors 'none'; base-uri 'self'\r\n";
pub(crate) const SANDBOXED_MEDIA_HEADERS: &str = "X-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; sandbox\r\n";

pub(crate) fn send_not_modified(stream: &mut TcpStream, etag: &str) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 304 Not Modified\r\nETag: {etag}\r\nCache-Control: no-cache, must-revalidate\r\n{SECURITY_HEADERS}Content-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(header.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

pub(crate) fn send_json_bytes_with_etag(
    stream: &mut TcpStream,
    etag: &str,
    body: &[u8],
) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: {etag}\r\nCache-Control: no-cache, must-revalidate\r\n{SECURITY_HEADERS}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

pub(crate) fn send_json_response_etag<T: Serialize>(
    stream: &mut TcpStream,
    req_headers: &HashMap<String, String>,
    data: &T,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(data)?;
    let etag = format!("\"dash-{:016x}\"", fnv1a64(&body));
    if req_headers
        .get("if-none-match")
        .is_some_and(|v| v.trim() == etag)
    {
        return send_not_modified(stream, &etag);
    }
    send_json_bytes_with_etag(stream, &etag, &body)
}

pub(crate) fn send_json_response_with_etag<T: Serialize>(
    stream: &mut TcpStream,
    req_headers: &HashMap<String, String>,
    etag: &str,
    data: &T,
) -> anyhow::Result<()> {
    if req_headers
        .get("if-none-match")
        .is_some_and(|v| v.trim() == etag)
    {
        return send_not_modified(stream, etag);
    }
    let body = serde_json::to_vec(data)?;
    send_json_bytes_with_etag(stream, etag, &body)
}

pub(crate) fn send_json_response<T: Serialize>(
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
pub(crate) fn origin_allowed(origin: Option<&str>, host: Option<&str>) -> bool {
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

/// Strictly validates HTTP `Host` against loopback (`127.0.0.1`, `localhost`,
/// `[::1]`), bare or bound to `port`, blocking DNS rebinding attacks.
pub(crate) fn host_allowed(host: Option<&str>, port: u16) -> bool {
    let Some(raw) = host.map(str::trim) else {
        return false;
    };
    if raw.is_empty() || raw.contains('@') || raw.contains('/') || raw.contains('\\') {
        return false;
    }
    let port_suffix = format!(":{port}");
    let normalized = raw.to_ascii_lowercase();
    normalized == "127.0.0.1"
        || normalized == "localhost"
        || normalized == "[::1]"
        || normalized == format!("127.0.0.1{port_suffix}")
        || normalized == format!("localhost{port_suffix}")
        || normalized == format!("[::1]{port_suffix}")
}

/// Validates browser `Sec-Fetch-Site` headers when supplied.
pub(crate) fn sec_fetch_site_allowed(
    method: &str,
    path: &str,
    sec_fetch_site: Option<&str>,
) -> bool {
    let Some(val) = sec_fetch_site.map(str::trim) else {
        return true;
    };
    if val.eq_ignore_ascii_case("same-origin") {
        return true;
    }
    let is_safe = matches!(method, "GET" | "HEAD");
    is_safe && !path.starts_with("/api/") && val.eq_ignore_ascii_case("none")
}

pub(crate) fn is_json_content_type(content_type: Option<&str>) -> bool {
    let Some(raw) = content_type else {
        return false;
    };
    raw.split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub(crate) fn read_bounded_body<R: BufRead>(
    reader: &mut R,
    expected_len: usize,
    deadline: Instant,
) -> std::io::Result<Vec<u8>> {
    if expected_len == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(expected_len.min(64 * 1024));
    let mut scratch = [0u8; 16 * 1024];
    let mut remaining = expected_len;
    while remaining > 0 {
        if Instant::now() > deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "request body deadline exceeded",
            ));
        }
        let want = remaining.min(scratch.len());
        let n = reader.read(&mut scratch[..want])?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated request body",
            ));
        }
        out.extend_from_slice(&scratch[..n]);
        remaining -= n;
    }
    Ok(out)
}

pub(crate) fn send_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
    cache_control: Option<&str>,
) -> anyhow::Result<()> {
    send_response_with_headers(
        stream,
        status_code,
        status_text,
        content_type,
        body,
        cache_control,
        SECURITY_HEADERS,
    )
}

pub(crate) fn send_sandboxed_response(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
    cache_control: Option<&str>,
) -> anyhow::Result<()> {
    send_response_with_headers(
        stream,
        status_code,
        status_text,
        content_type,
        body,
        cache_control,
        SANDBOXED_MEDIA_HEADERS,
    )
}

fn send_response_with_headers(
    stream: &mut TcpStream,
    status_code: u16,
    status_text: &str,
    content_type: &str,
    body: &[u8],
    cache_control: Option<&str>,
    security_headers: &str,
) -> anyhow::Result<()> {
    let mut header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{security_headers}Connection: close\r\n",
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
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

pub(crate) const MAX_HTTP_LINE_BYTES: usize = 8 * 1024;
pub(crate) const MAX_HTTP_HEADERS: usize = 64;
pub(crate) const MAX_HTTP_BODY_BYTES: usize = 16 * 1024 * 1024;

pub(crate) enum BoundedLine {
    Eof,
    Line(String),
    TooLong,
}

pub(crate) fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<BoundedLine> {
    let mut buf = Vec::with_capacity(256);
    let n = reader
        .by_ref()
        .take((max_bytes + 1) as u64)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(BoundedLine::Eof);
    }
    if buf.len() > max_bytes && !buf.ends_with(b"\n") {
        return Ok(BoundedLine::TooLong);
    }
    Ok(BoundedLine::Line(
        String::from_utf8_lossy(&buf).into_owned(),
    ))
}
