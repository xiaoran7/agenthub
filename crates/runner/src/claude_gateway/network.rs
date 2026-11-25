use super::*;

pub(crate) fn parse_http_url(url: &str) -> Option<(url::Url, String, u16)> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if host.is_empty() {
        return None;
    }
    let port = parsed.port_or_known_default()?;
    Some((parsed, host, port))
}

pub(crate) fn parse_url_host_port(url: &str) -> Option<(String, u16)> {
    let (_, host, port) = parse_http_url(url)?;
    Some((host, port))
}

pub(crate) fn is_loopback_host(host: &str) -> bool {
    let lower = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") {
        return true;
    }
    if let Ok(ip) = lower.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => v4.is_loopback(),
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
            }
        };
    }
    false
}

pub(crate) fn is_private_or_tailnet_v4(v4: std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    v4.is_private() || v4.is_link_local() || (octets[0] == 100 && (64..=127).contains(&octets[1]))
}

pub(crate) fn is_private_or_tailnet_host(host: &str) -> bool {
    let lower = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    if let Ok(ip) = lower.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_private_or_tailnet_v4(v4),
            IpAddr::V6(v6) => {
                let seg0 = v6.segments()[0];
                let is_ula = (seg0 & 0xfe00) == 0xfc00;
                let is_link_local = (seg0 & 0xffc0) == 0xfe80;
                is_ula || is_link_local || v6.to_ipv4_mapped().is_some_and(is_private_or_tailnet_v4)
            }
        };
    }
    lower.ends_with(".ts.net")
        || lower.ends_with(".local")
        || (!lower.contains('.') && !lower.contains(':'))
}

/// Determines if an endpoint points to CC Switch (`:15721` or `/claude-desktop` proxy path).
pub fn is_cc_switch_endpoint(url: &str) -> bool {
    let Some((parsed, _, port)) = parse_http_url(url) else {
        return false;
    };
    if port == 15721 {
        return true;
    }
    let path = parsed.path().to_ascii_lowercase();
    path == "/claude-desktop" || path.starts_with("/claude-desktop/")
}

pub(crate) fn host_label_from_url(url: &str) -> Option<String> {
    let (host, port) = parse_url_host_port(url)?;
    if is_loopback_host(&host) {
        Some(format!("Local :{port}"))
    } else {
        Some(host)
    }
}

/// Verifies that local loopback (`80ms`) and Tailnet/private LAN (`300ms`) endpoints are
/// accepting TCP connections before injecting them or issuing HTTP probes.
/// Public WAN endpoints return `true` immediately without blocking on WAN latency.
pub(crate) fn is_gateway_url_reachable(url: &str) -> bool {
    let Some((host, port)) = parse_url_host_port(url) else {
        return false;
    };
    let timeout = if is_loopback_host(&host) {
        Duration::from_millis(80)
    } else if is_private_or_tailnet_host(&host) {
        Duration::from_millis(300)
    } else {
        return true;
    };
    let addr_str = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    if let Ok(addrs) = addr_str.to_socket_addrs() {
        for addr in addrs {
            if TcpStream::connect_timeout(&addr, timeout).is_ok() {
                return true;
            }
        }
    } else if let Ok(ip) = host.parse::<IpAddr>() {
        let sock = SocketAddr::new(ip, port);
        if TcpStream::connect_timeout(&sock, timeout).is_ok() {
            return true;
        }
    }
    false
}

/// Dynamically queries live models from an upstream OpenAI/Anthropic compatible gateway.
pub fn fetch_live_upstream_models(base_url: &str, token: &str) -> Vec<ClaudeGatewayModel> {
    let base_url = base_url.trim().trim_end_matches('/');
    let token = token.trim();
    if base_url.is_empty()
        || !is_token_compatible_with_endpoint(base_url, token)
        || !is_gateway_url_reachable(base_url)
    {
        return Vec::new();
    }

    let cache_key = (base_url.to_string(), token_fingerprint(token));
    if let Ok(guard) = UPSTREAM_MODELS_CACHE.read()
        && let Some(map) = guard.as_ref()
        && let Some((cached_at, ttl, cached_models)) = map.get(&cache_key)
        && cached_at.elapsed() < *ttl
    {
        return cached_models.clone();
    }

    let models_url = if base_url.ends_with("/v1") {
        format!("{base_url}/models")
    } else {
        format!("{base_url}/v1/models")
    };

    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_millis(1800)))
        .max_redirects(0)
        .build()
        .new_agent();

    let Ok(mut resp) = agent
        .get(&models_url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("x-api-key", token)
        .call()
    else {
        if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write() {
            let map = guard.get_or_insert_with(HashMap::new);
            map.insert(
                cache_key,
                (Instant::now(), Duration::from_secs(15), Vec::new()),
            );
        }
        return Vec::new();
    };

    let status = resp.status().as_u16();
    if matches!(status, 401 | 403) {
        if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write()
            && let Some(map) = guard.as_mut()
        {
            map.remove(&cache_key);
        }
        return Vec::new();
    }
    if !(200..300).contains(&status) {
        if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write() {
            let map = guard.get_or_insert_with(HashMap::new);
            map.insert(
                cache_key,
                (Instant::now(), Duration::from_secs(10), Vec::new()),
            );
        }
        return Vec::new();
    }

    let Ok(val) = resp.body_mut().read_json::<Value>() else {
        if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write() {
            let map = guard.get_or_insert_with(HashMap::new);
            map.insert(
                cache_key,
                (Instant::now(), Duration::from_secs(10), Vec::new()),
            );
        }
        return Vec::new();
    };

    let mut fetched = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let items = val
        .get("data")
        .or_else(|| val.get("models"))
        .and_then(Value::as_array);

    if let Some(items) = items {
        for item in items {
            let id = item
                .get("id")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if id.is_empty() {
                continue;
            }
            let id_lower = id.to_ascii_lowercase();
            // Filter non-LLM models (image generation, audio/tts/asr, embedding, rerank)
            if id_lower.contains("image")
                || id_lower.contains("audio")
                || id_lower.contains("tts")
                || id_lower.contains("asr")
                || id_lower.contains("embed")
                || id_lower.contains("rerank")
                || id_lower.contains("whisper")
                || id_lower.contains("moderation")
            {
                continue;
            }
            if !seen.insert(id_lower.clone()) {
                continue;
            }
            let label = item
                .get("displayName")
                .or_else(|| item.get("display_name"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let supports_1m =
                id_lower.contains("1m") || id_lower.contains("high") || id_lower.contains("gemini");
            fetched.push(ClaudeGatewayModel {
                name: id.to_string(),
                label,
                supports_1m,
            });
        }
    }

    if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            cache_key,
            (Instant::now(), Duration::from_secs(120), fetched.clone()),
        );
    }

    fetched
}

#[cfg(test)]
pub(crate) fn merge_gateway_models(
    target: &mut Vec<ClaudeGatewayModel>,
    incoming: Vec<ClaudeGatewayModel>,
) {
    let mut seen: std::collections::HashSet<String> =
        target.iter().map(|m| m.name.to_ascii_lowercase()).collect();
    for m in target.iter() {
        if let Some(lbl) = &m.label {
            seen.insert(lbl.to_ascii_lowercase());
        }
    }
    for m in incoming {
        if seen.insert(m.name.to_ascii_lowercase()) {
            target.push(m);
        }
    }
}
