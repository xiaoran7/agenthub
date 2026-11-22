use std::{
    path::Path,
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::{config::is_model_placeholder, config::read_json_bom_safe, discover_antigravity_homes};
use crate::collectors::file_modified_millis;

pub(crate) const DAEMON_QUERY_TIMEOUT: Duration = Duration::from_millis(500);
const DAEMON_POSITIVE_TTL: Duration = Duration::from_secs(30);
const DAEMON_EMPTY_TTL: Duration = Duration::from_secs(10);
pub(crate) const MAX_DAEMON_CANDIDATES: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityLiveModel {
    pub model_id: String,
    pub label: String,
    pub alias: Option<String>,
    pub supports_images: bool,
    pub supports_thinking: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AntigravityLiveCatalog {
    pub models: Vec<AntigravityLiveModel>,
    pub default_model_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityDaemonEndpoint {
    pub http_port: u16,
    pub csrf_token: String,
}

impl AntigravityDaemonEndpoint {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.http_port)
    }
}

fn live_models_cache() -> &'static RwLock<Option<(Instant, Option<AntigravityLiveCatalog>)>> {
    static CACHE: OnceLock<RwLock<Option<(Instant, Option<AntigravityLiveCatalog>)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

pub fn invalidate_antigravity_models_cache() {
    if let Ok(mut guard) = live_models_cache().write() {
        *guard = None;
    }
}

fn is_disabled_model(model_id: &str, alias: Option<&str>, entry: &Value) -> bool {
    if entry.get("disabled").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    let id_lower = model_id.to_ascii_lowercase();
    if id_lower.ends_with("-disabled") {
        return true;
    }
    if let Some(a) = alias
        && a.to_ascii_lowercase().ends_with("-disabled")
    {
        return true;
    }
    false
}

fn infer_antigravity_thinking(model_id: &str, label: &str, entry: &Value) -> bool {
    if let Some(b) = entry
        .get("supportsThinking")
        .or_else(|| entry.get("thinking"))
        .and_then(Value::as_bool)
    {
        return b;
    }
    let id_l = model_id.to_ascii_lowercase();
    let lbl_l = label.to_ascii_lowercase();
    id_l.contains("thinking")
        || id_l.contains("high")
        || id_l.contains("medium")
        || id_l.contains("low")
        || id_l.contains("pro")
        || id_l.contains("opus")
        || id_l.contains("sonnet")
        || id_l.contains("carbon")
        || id_l.contains("hydrogen")
        || id_l.contains("argon")
        || lbl_l.contains("thinking")
        || lbl_l.contains("high")
}

/// Parses `GetCascadeModelConfigData` JSON from a running Antigravity `language_server`.
pub fn parse_cascade_model_configs(val: &Value) -> AntigravityLiveCatalog {
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(arr) = val.get("clientModelConfigs").and_then(Value::as_array) {
        for entry in arr {
            let Some(model_id) = entry
                .get("modelId")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let alias = entry
                .pointer("/modelOrAlias/model")
                .or_else(|| entry.pointer("/modelOrAlias/alias"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if is_disabled_model(model_id, alias.as_deref(), entry) {
                continue;
            }
            if !seen.insert(model_id.to_ascii_lowercase()) {
                continue;
            }
            let label = entry
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(model_id)
                .to_owned();
            let supports_images = entry
                .get("supportsImages")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let supports_thinking = infer_antigravity_thinking(model_id, &label, entry);
            models.push(AntigravityLiveModel {
                model_id: model_id.to_owned(),
                label,
                alias,
                supports_images,
                supports_thinking,
            });
        }
    }

    let default_ref = val
        .pointer("/defaultOverrideModelConfig/modelOrAlias/model")
        .or_else(|| val.pointer("/defaultOverrideModelConfig/modelId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let default_model_id = default_ref.and_then(|target| {
        models
            .iter()
            .find(|m| {
                m.model_id.eq_ignore_ascii_case(target)
                    || m.alias
                        .as_deref()
                        .is_some_and(|a| a.eq_ignore_ascii_case(target))
            })
            .map(|m| m.model_id.clone())
    });

    AntigravityLiveCatalog {
        models,
        default_model_id,
    }
}

/// Normalizes a human display label (e.g. `"Gemini 4 Carbon (High)"` or
/// `"Claude Opus 4.6 (Thinking)"`) into a canonical slug when the daemon is offline.
pub fn slugify_antigravity_model_label(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_model_placeholder(trimmed) {
        return None;
    }
    if !trimmed.contains(' ') {
        return Some(trimmed.to_owned());
    }
    let mut out = String::with_capacity(trimmed.len());
    let mut prev_dash = false;
    let chars: Vec<char> = trimmed.chars().collect();
    for (idx, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if ch == '.' {
            let prev_digit = idx > 0 && chars[idx - 1].is_ascii_digit();
            let next_digit = idx + 1 < chars.len() && chars[idx + 1].is_ascii_digit();
            if prev_digit && next_digit && trimmed.to_ascii_lowercase().starts_with("claude") {
                out.push('-');
                prev_dash = true;
            } else if prev_digit && next_digit {
                out.push('.');
                prev_dash = false;
            } else if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    (!out.is_empty()).then_some(out)
}

/// Resolves an Antigravity model reference (`MODEL_PLACEHOLDER_M*`, human display label
/// from `<USER_SETTINGS_CHANGE>`, or model ID) against a catalog or offline slug rules.
pub fn resolve_model_in_catalog(
    raw: &str,
    catalog: Option<&AntigravityLiveCatalog>,
) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('`').trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(cat) = catalog {
        if let Some(hit) = cat.models.iter().find(|m| {
            m.model_id.eq_ignore_ascii_case(trimmed)
                || m.label.eq_ignore_ascii_case(trimmed)
                || m.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(trimmed))
        }) {
            return Some(hit.model_id.clone());
        }
    }
    if is_model_placeholder(trimmed) {
        return None;
    }
    slugify_antigravity_model_label(trimmed)
}

/// Resolves any Antigravity model reference using the live daemon catalog if available.
pub fn resolve_antigravity_model_reference(raw: &str) -> Option<String> {
    let live = discover_live_antigravity_models();
    resolve_model_in_catalog(raw, live.as_ref())
}

/// Maps a `model_id` (or label) back to the `MODEL_PLACEHOLDER_*` enum string expected by
/// `language_server` ConnectRPC `requestedModel`, falling back to `model_id` itself.
pub fn resolve_antigravity_rpc_model_enum(model_id: &str) -> Option<String> {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(live) = discover_live_antigravity_models() {
        if let Some(hit) = live.models.iter().find(|m| {
            m.model_id.eq_ignore_ascii_case(trimmed)
                || m.label.eq_ignore_ascii_case(trimmed)
                || m.alias
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(trimmed))
        }) {
            return hit.alias.clone().or_else(|| Some(hit.model_id.clone()));
        }
    }
    Some(trimmed.to_owned())
}

pub(crate) fn read_daemon_endpoints(ag_home: &Path) -> Vec<AntigravityDaemonEndpoint> {
    let daemon_dir = ag_home.join("daemon");
    let Ok(entries) = std::fs::read_dir(&daemon_dir) else {
        return Vec::new();
    };
    let mut hubs: Vec<(i64, std::path::PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if fname.starts_with("hub_") && fname.ends_with(".json") && path.is_file() {
            let mtime = file_modified_millis(&path).unwrap_or(0);
            hubs.push((mtime, path));
        }
    }
    hubs.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));

    let mut out = Vec::new();
    for (_, hub_path) in hubs {
        let Some(val) = read_json_bom_safe(&hub_path) else {
            continue;
        };
        let Some(http_port) = val
            .get("httpPort")
            .and_then(Value::as_u64)
            .and_then(|v| u16::try_from(v).ok())
            .filter(|&p| p > 0)
        else {
            continue;
        };
        let Some(csrf_token) = val
            .get("csrfToken")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
        else {
            continue;
        };
        out.push(AntigravityDaemonEndpoint {
            http_port,
            csrf_token,
        });
    }
    out
}

/// Queries an Antigravity `language_server` base URL (e.g. `http://127.0.0.1:<httpPort>`)
/// for its live `GetCascadeModelConfigData` catalog.
pub fn query_daemon_models_from_url(
    base_url: &str,
    csrf_token: &str,
    timeout: Duration,
) -> Option<AntigravityLiveCatalog> {
    let url = format!(
        "{}/exa.language_server_pb.LanguageServerService/GetCascadeModelConfigData",
        base_url.trim_end_matches('/')
    );
    let agent = ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .new_agent();

    let mut resp = agent
        .post(&url)
        .header("X-Codeium-Csrf-Token", csrf_token)
        .header("Connect-Protocol-Version", "1")
        .header("Content-Type", "application/json")
        .send_json(json!({}))
        .ok()?;
    if resp.status().as_u16() != 200 {
        return None;
    }
    let body: Value = resp.body_mut().read_json().ok()?;
    let parsed = parse_cascade_model_configs(&body);
    (!parsed.models.is_empty()).then_some(parsed)
}

/// Queries the local running Antigravity daemon for real available models, cached briefly
/// so frequent model catalog refreshes never stall the runner loop.
pub fn discover_live_antigravity_models() -> Option<AntigravityLiveCatalog> {
    if let Ok(guard) = live_models_cache().read()
        && let Some((ts, cached)) = guard.as_ref()
    {
        let ttl = if cached.is_some() {
            DAEMON_POSITIVE_TTL
        } else {
            DAEMON_EMPTY_TTL
        };
        if ts.elapsed() < ttl {
            return cached.clone();
        }
    }

    let mut found = None;
    'outer: for ag_home in discover_antigravity_homes() {
        for endpoint in read_daemon_endpoints(&ag_home)
            .into_iter()
            .take(MAX_DAEMON_CANDIDATES)
        {
            let base_url = endpoint.base_url();
            if let Some(catalog) =
                query_daemon_models_from_url(&base_url, &endpoint.csrf_token, DAEMON_QUERY_TIMEOUT)
            {
                found = Some(catalog);
                break 'outer;
            }
        }
    }

    if let Ok(mut guard) = live_models_cache().write() {
        *guard = Some((Instant::now(), found.clone()));
    }
    found
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use super::*;

    #[test]
    fn parse_cascade_model_configs_filters_disabled_and_resolves_aliases_and_labels() {
        let payload = json!({
            "clientModelConfigs": [
                {
                    "label": "Gemini 4 Carbon (High)",
                    "modelId": "gemini-4-carbon-high",
                    "supportsImages": true,
                    "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M427" }
                },
                {
                    "label": "Gemini 4 Carbon Disabled",
                    "modelId": "gemini-4-carbon-disabled",
                    "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M99" }
                },
                {
                    "label": "Claude Opus 4.6 (Thinking)",
                    "modelId": "claude-opus-4-6-thinking",
                    "supportsImages": true,
                    "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M26" }
                }
            ],
            "defaultOverrideModelConfig": {
                "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M427" }
            }
        });

        let catalog = parse_cascade_model_configs(&payload);
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(catalog.models[0].model_id, "gemini-4-carbon-high");
        assert!(catalog.models[0].supports_images);
        assert!(catalog.models[0].supports_thinking);
        assert_eq!(
            catalog.default_model_id.as_deref(),
            Some("gemini-4-carbon-high")
        );
        assert_eq!(
            resolve_model_in_catalog("MODEL_PLACEHOLDER_M427", Some(&catalog)).as_deref(),
            Some("gemini-4-carbon-high")
        );
        assert_eq!(
            resolve_model_in_catalog("Gemini 4 Carbon (High)", Some(&catalog)).as_deref(),
            Some("gemini-4-carbon-high")
        );
        assert_eq!(
            resolve_model_in_catalog("Claude Opus 4.6 (Thinking)", None).as_deref(),
            Some("claude-opus-4-6-thinking")
        );
    }

    #[test]
    fn query_daemon_models_from_url_sends_csrf_header_and_parses_response() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut req_bytes = Vec::with_capacity(512);
            let mut buf = [0_u8; 1024];
            loop {
                let n = stream.read(&mut buf).expect("read request");
                if n == 0 {
                    break;
                }
                req_bytes.extend_from_slice(&buf[..n]);
                if let Some(hdr_end) = req_bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let hdr_str =
                        String::from_utf8_lossy(&req_bytes[..hdr_end]).to_ascii_lowercase();
                    let body_len = hdr_str
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if req_bytes.len() >= hdr_end + 4 + body_len {
                        break;
                    }
                }
            }
            let req = String::from_utf8_lossy(&req_bytes).to_string();
            assert!(
                req.to_ascii_lowercase()
                    .contains("x-codeium-csrf-token: test-secret-csrf")
            );
            let body = json!({
                "clientModelConfigs": [
                    {
                        "label": "Gemini 3 Flash",
                        "modelId": "gemini-3-flash",
                        "modelOrAlias": { "model": "MODEL_PLACEHOLDER_M47" }
                    }
                ]
            })
            .to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(resp.as_bytes()).expect("write response");
            stream.flush().expect("flush response");
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        let catalog = query_daemon_models_from_url(
            &format!("http://127.0.0.1:{port}"),
            "test-secret-csrf",
            Duration::from_secs(2),
        )
        .expect("daemon catalog");
        handle.join().expect("server thread");
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].model_id, "gemini-3-flash");
    }
}
