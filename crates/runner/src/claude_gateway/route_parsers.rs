use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CodexTomlRoute {
    pub active_provider_key: Option<String>,
    pub active_provider_name: Option<String>,
    pub active_base_url: Option<String>,
    pub model: Option<String>,
}

pub(crate) fn read_json_url_from_paths(path: &Path, pointers: &[&str]) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let val: Value =
        serde_json::from_str(content.strip_prefix('\u{feff}').unwrap_or(&content)).ok()?;
    for ptr in pointers {
        if let Some(s) = val
            .pointer(ptr)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(s.to_string());
        }
    }
    None
}

pub(crate) fn discover_local_gateway_label(
    base_url: &str,
    home: Option<&Path>,
    appdata: Option<&Path>,
) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(app) = appdata {
        candidates.push(app.join("magpie").join("gateway.json"));
    }
    if let Some(h) = home {
        candidates.push(h.join(".config").join("magpie").join("gateway.json"));
    }
    for path in candidates {
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(val) = serde_json::from_str::<Value>(raw.strip_prefix('\u{feff}').unwrap_or(&raw))
        else {
            continue;
        };
        let Some(addr) = val
            .get("addr")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let normalized = if addr.contains("://") {
            addr.to_string()
        } else {
            format!("http://{addr}")
        };
        if urls_share_origin(base_url, &normalized) {
            return Some("Magpie".to_string());
        }
        if let Some(port) = addr
            .strip_prefix("0.0.0.0:")
            .or_else(|| addr.strip_prefix("[::]:"))
            && urls_share_origin(base_url, &format!("http://127.0.0.1:{port}"))
        {
            return Some("Magpie".to_string());
        }
    }
    None
}

pub(crate) fn extract_dotenv_value(text: &str, keys: &[&str]) -> Option<String> {
    let clean = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parsed = HashMap::new();
    for line in clean.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let body = trimmed
            .strip_prefix("export")
            .filter(|rest| rest.starts_with(char::is_whitespace))
            .map(str::trim_start)
            .unwrap_or(trimmed);
        let Some((lhs, rhs)) = body.split_once('=') else {
            continue;
        };
        let key = lhs.trim();
        if key.is_empty() {
            continue;
        }
        let val = parse_env_or_toml_value(rhs);
        if !val.is_empty() {
            parsed.insert(key, val);
        }
    }
    keys.iter().find_map(|k| parsed.get(*k).cloned())
}

pub(crate) fn parse_codex_toml_route(toml_text: &str) -> CodexTomlRoute {
    #[derive(Default)]
    struct ProviderEntry {
        name: Option<String>,
        base_url: Option<String>,
    }
    enum Section {
        Root,
        ModelProvider(String),
        Other,
    }

    let clean = toml_text.strip_prefix('\u{feff}').unwrap_or(toml_text);
    let mut section = Section::Root;
    let mut root_model_provider: Option<String> = None;
    let mut root_base_url: Option<String> = None;
    let mut model: Option<String> = None;
    let mut providers: HashMap<String, ProviderEntry> = HashMap::new();

    for raw_line in clean.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("[[") && line.ends_with("]]") {
            section = Section::Other;
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let header = line[1..line.len() - 1].trim();
            section = parse_model_provider_section(header)
                .map(Section::ModelProvider)
                .unwrap_or(Section::Other);
            continue;
        }
        let Some((lhs, rhs)) = line.split_once('=') else {
            continue;
        };
        let key = lhs.trim();
        let val = parse_env_or_toml_value(rhs);
        if val.is_empty() {
            continue;
        }
        match &section {
            Section::Root => match key {
                "model_provider" => root_model_provider = Some(val),
                "base_url" => root_base_url = Some(val),
                "model" => model = Some(val),
                _ => {}
            },
            Section::ModelProvider(pid) => {
                let entry = providers.entry(pid.clone()).or_default();
                match key {
                    "name" => entry.name = Some(val),
                    "base_url" => entry.base_url = Some(val),
                    _ => {}
                }
            }
            Section::Other => {}
        }
    }

    if let Some(active_key) = root_model_provider {
        let entry = providers.get(&active_key).or_else(|| {
            providers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&active_key))
                .map(|(_, v)| v)
        });
        CodexTomlRoute {
            active_provider_name: entry.and_then(|e| e.name.clone()),
            active_base_url: entry.and_then(|e| e.base_url.clone()).or(root_base_url),
            active_provider_key: Some(active_key),
            model,
        }
    } else {
        CodexTomlRoute {
            active_provider_key: None,
            active_provider_name: None,
            active_base_url: root_base_url,
            model,
        }
    }
}

fn parse_model_provider_section(header: &str) -> Option<String> {
    let (head, tail) = header.split_once('.')?;
    if head.trim() != "model_providers" {
        return None;
    }
    let key_part = tail.trim();
    if let Some(rest) = key_part.strip_prefix('"') {
        let (inner, after) = rest.split_once('"')?;
        if after.trim().is_empty() && !inner.is_empty() {
            return Some(inner.to_string());
        }
        return None;
    }
    if let Some(rest) = key_part.strip_prefix('\'') {
        let (inner, after) = rest.split_once('\'')?;
        if after.trim().is_empty() && !inner.is_empty() {
            return Some(inner.to_string());
        }
        return None;
    }
    if !key_part.is_empty() && !key_part.contains('.') {
        Some(key_part.to_string())
    } else {
        None
    }
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_double {
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
            continue;
        }
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            continue;
        }
        match ch {
            '"' => in_double = true,
            '\'' => in_single = true,
            '#' => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn parse_env_or_toml_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix('"')
        && let Some((inner, _)) = rest.split_once('"')
    {
        return inner.trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix('\'')
        && let Some((inner, _)) = rest.split_once('\'')
    {
        return inner.trim().to_string();
    }
    trimmed.split('#').next().unwrap_or("").trim().to_string()
}
