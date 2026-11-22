use super::*;

pub(crate) fn is_loopback_or_private_v4(v4: std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
}

pub(crate) fn is_loopback_or_private_host(host: Option<&str>) -> bool {
    let Some(host_str) = host else {
        return false;
    };
    let trimmed = host_str.trim();
    let unwrapped = trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed);
    if unwrapped.is_empty() {
        return false;
    }
    if let Ok(ip) = unwrapped.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => is_loopback_or_private_v4(v4),
            std::net::IpAddr::V6(v6) => {
                let seg0 = v6.segments()[0];
                let is_ula = (seg0 & 0xfe00) == 0xfc00;
                let is_link_local = (seg0 & 0xffc0) == 0xfe80;
                v6.is_loopback()
                    || is_ula
                    || is_link_local
                    || v6.to_ipv4_mapped().is_some_and(is_loopback_or_private_v4)
            }
        };
    }
    let lower = unwrapped.to_ascii_lowercase();
    lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".ts.net")
        || lower.ends_with(".local")
        || (!lower.contains('.') && !lower.contains(':'))
}

pub(crate) fn validate_server(server: &str) -> Result<String> {
    let mut normalized = server.trim().to_string();
    if let Some(stripped) = normalized.strip_prefix("http://") {
        normalized = format!("ws://{stripped}");
    } else if let Some(stripped) = normalized.strip_prefix("https://") {
        normalized = format!("wss://{stripped}");
    }
    let mut url = url::Url::parse(&normalized)?;
    if !matches!(url.scheme(), "ws" | "wss")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("invalid_remote_url");
    }
    if url.path() == "/" || url.path().is_empty() {
        url.set_path("/v3/remote/ws");
    }
    if url.scheme() == "ws" && !is_loopback_or_private_host(url.host_str()) {
        bail!("remote_tls_required");
    }
    if url.path() != "/v3/remote/ws" {
        bail!("remote_url_path_required");
    }
    Ok(url.to_string())
}

pub(crate) type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

pub(crate) fn connect_with_timeout(url_str: &str, timeout: Duration) -> Result<Socket> {
    use std::net::ToSocketAddrs;
    let parsed = url::Url::parse(url_str)?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("missing_remote_host"))?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<_> = (host, port).to_socket_addrs()?.collect();
    if addrs.is_empty() {
        bail!("remote_dns_empty");
    }
    let mut last_err = None;
    let mut tcp = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let stream = match tcp {
        Some(s) => s,
        None => {
            return Err(last_err
                .map(Into::into)
                .unwrap_or_else(|| anyhow::anyhow!("remote_connect_failed")));
        }
    };
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let ws_config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(16 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024));
    let (socket, _) = tungstenite::client_tls_with_config(url_str, stream, Some(ws_config), None)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(socket)
}

pub(crate) fn set_timeouts(socket: &mut Socket, read: Duration) -> Result<()> {
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => &mut stream.sock,
        _ => bail!("unsupported_remote_tls"),
    };
    stream.set_read_timeout(Some(read))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    Ok(())
}

pub(crate) fn send(socket: &mut Socket, value: Value) -> Result<()> {
    socket.send(Message::Text(value.to_string().into()))?;
    Ok(())
}

pub(crate) fn receive(socket: &mut Socket) -> Result<Value> {
    loop {
        match socket.read()? {
            Message::Text(text) => return Ok(serde_json::from_str(text.as_str())?),
            Message::Close(_) => bail!("remote_closed"),
            _ => {}
        }
    }
}
