use std::{
    net::{IpAddr, ToSocketAddrs},
    path::Path,
    time::Duration,
};

use ureq::{
    Agent, Error as UreqError,
    config::Config as UreqConfig,
    http::Uri,
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::{DefaultConnector, NextTimeout},
    },
};
use url::{Host, Url};

use super::types::{
    DEFAULT_PLUGIN_REGISTRY_URL, PluginManifest, PluginRuntimeSpec, is_reserved_native_agent,
    validate_relative_path,
};

pub(crate) fn allow_insecure_plugin_urls() -> bool {
    std::env::var("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "yes"))
}

pub(crate) fn allow_native_plugins() -> bool {
    std::env::var("AGENTHUB_ALLOW_NATIVE_PLUGINS")
        .ok()
        .is_some_and(|v| v.trim() == "1")
}

#[derive(Debug)]
struct SafePluginResolver {
    allow_private: bool,
    inner: DefaultResolver,
}

impl SafePluginResolver {
    fn new(allow_private: bool) -> Self {
        Self {
            allow_private,
            inner: DefaultResolver::default(),
        }
    }
}

impl Resolver for SafePluginResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &UreqConfig,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, UreqError> {
        if !self.allow_private
            && let Some(host) = uri.host()
        {
            let unwrapped = host
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(host);
            let lower = unwrapped.trim_end_matches('.').to_ascii_lowercase();
            if lower == "localhost"
                || lower.ends_with(".localhost")
                || lower.ends_with(".local")
                || lower.ends_with(".internal")
            {
                return Err(UreqError::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "forbidden_private_plugin_host",
                )));
            }
        }
        let resolved = self.inner.resolve(uri, config, timeout)?;
        if !self.allow_private {
            for addr in resolved.iter() {
                if is_forbidden_ip(addr.ip()) {
                    return Err(UreqError::Io(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "forbidden_private_plugin_ip",
                    )));
                }
            }
        }
        Ok(resolved)
    }
}

pub(crate) fn build_http_agent() -> Agent {
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("AgentHub-Runner/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .max_redirects(0)
        .build();
    Agent::with_parts(
        config,
        DefaultConnector::default(),
        SafePluginResolver::new(allow_insecure_plugin_urls()),
    )
}

pub(crate) fn has_unsafe_path_encoding(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains('\\') || lower.contains("%2e") || lower.contains("%2f") || lower.contains("%5c")
}

pub(crate) fn has_raw_dot_segments(raw_url: &str) -> bool {
    let without_frag = raw_url.split('#').next().unwrap_or(raw_url);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    let raw_path = match without_query.find("://") {
        Some(idx) => {
            let after_scheme = &without_query[idx + 3..];
            match after_scheme.find('/') {
                Some(path_start) => &after_scheme[path_start..],
                None => return false,
            }
        }
        None => without_query,
    };
    raw_path
        .split('/')
        .any(|segment| segment == "." || segment == "..")
}

fn is_valid_https_registry_url(url: &Url) -> bool {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.origin().ascii_serialization() == "null"
    {
        return false;
    }
    let path = url.path();
    !has_unsafe_path_encoding(path)
        && !path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

fn official_trusted_prefix() -> Option<(String, String)> {
    let default_url = Url::parse(DEFAULT_PLUGIN_REGISTRY_URL).ok()?;
    if !is_valid_https_registry_url(&default_url) {
        return None;
    }
    let origin = default_url.origin().ascii_serialization();
    let path = default_url.path();
    let prefix = if default_url
        .host_str()
        .is_some_and(|h| h.eq_ignore_ascii_case("raw.githubusercontent.com"))
    {
        let mut segments = path.split('/').filter(|s| !s.is_empty());
        if let (Some(owner), Some(repo)) = (segments.next(), segments.next()) {
            format!("/{owner}/{repo}/")
        } else {
            let parent = match path.rfind('/') {
                Some(idx) => &path[..=idx],
                None => "/",
            };
            parent.to_string()
        }
    } else {
        let parent = match path.rfind('/') {
            Some(idx) => &path[..=idx],
            None => "/",
        };
        parent.to_string()
    };
    Some((origin, prefix))
}

fn normalized_allowed_prefix(allowed: &Url) -> Option<(String, String, Option<String>)> {
    if !is_valid_https_registry_url(allowed) {
        return None;
    }
    let origin = allowed.origin().ascii_serialization();
    let path = allowed.path();
    if path.ends_with('/') {
        Some((origin, path.to_string(), None))
    } else if path.to_ascii_lowercase().ends_with(".json") {
        let parent = match path.rfind('/') {
            Some(idx) => &path[..=idx],
            None => "/",
        };
        Some((origin, parent.to_string(), Some(path.to_string())))
    } else {
        let dir = if path.is_empty() {
            "/".to_string()
        } else {
            format!("{path}/")
        };
        Some((origin, dir, Some(path.to_string())))
    }
}

pub fn url_matches_registry_prefix(candidate: &Url, allowed_registry: &Url) -> bool {
    if !is_valid_https_registry_url(candidate) {
        return false;
    }
    let Some((allowed_origin, dir_prefix, exact_path)) =
        normalized_allowed_prefix(allowed_registry)
    else {
        return false;
    };
    let candidate_origin = candidate.origin().ascii_serialization();
    if !candidate_origin.eq_ignore_ascii_case(&allowed_origin) {
        return false;
    }
    let candidate_path = candidate.path();
    candidate_path.starts_with(&dir_prefix)
        || exact_path
            .as_deref()
            .is_some_and(|exact| candidate_path == exact)
}

pub fn is_trusted_remote_registry_url(url: &Url) -> bool {
    if !is_valid_https_registry_url(url) {
        return false;
    }
    let origin = url.origin().ascii_serialization();
    if let Some((official_origin, official_prefix)) = official_trusted_prefix()
        && origin.eq_ignore_ascii_case(&official_origin)
        && url.path().starts_with(&official_prefix)
    {
        return true;
    }
    if let Ok(extra) = std::env::var("AGENTHUB_ALLOWED_PLUGIN_REGISTRIES") {
        for raw in extra.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if has_raw_dot_segments(raw) || has_unsafe_path_encoding(raw) {
                continue;
            }
            if let Ok(parsed) = Url::parse(raw)
                && url_matches_registry_prefix(url, &parsed)
            {
                return true;
            }
        }
    }
    false
}

pub fn validate_trusted_remote_registry_url(raw: &str) -> Result<Url, String> {
    let trimmed = raw.trim();
    if has_raw_dot_segments(trimmed) || has_unsafe_path_encoding(trimmed) {
        return Err("untrusted_remote_registry_origin".into());
    }
    if !super::has_scheme_prefix(trimmed, "https://") {
        return Err("untrusted_remote_registry_origin".into());
    }
    let parsed = Url::parse(trimmed).map_err(|e| format!("invalid_remote_url:{e}"))?;
    if !is_trusted_remote_registry_url(&parsed) {
        return Err("untrusted_remote_registry_origin".into());
    }
    validate_remote_url(trimmed, true)
}

pub(crate) fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let oct = v4.octets();
            let is_this_network = oct[0] == 0;
            let is_cgnat = oct[0] == 100 && (oct[1] & 0b1100_0000) == 64;
            let is_ietf_protocol = oct[0] == 192 && oct[1] == 0 && oct[2] == 0;
            let is_benchmarking = oct[0] == 198 && (oct[1] & 0xfe) == 18;
            let is_reserved_future = oct[0] >= 240;
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || is_this_network
                || is_cgnat
                || is_ietf_protocol
                || is_benchmarking
                || is_reserved_future
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_forbidden_ip(IpAddr::V4(mapped));
            }
            let segs = v6.segments();
            let seg0 = segs[0];
            let is_ula = (seg0 & 0xfe00) == 0xfc00;
            let is_link_local = (seg0 & 0xffc0) == 0xfe80;
            let is_site_local = (seg0 & 0xffc0) == 0xfec0;
            let is_ipv4_compatible = segs[0] == 0
                && segs[1] == 0
                && segs[2] == 0
                && segs[3] == 0
                && segs[4] == 0
                && segs[5] == 0;
            let is_ipv4_translated = segs[0] == 0
                && segs[1] == 0
                && segs[2] == 0
                && segs[3] == 0
                && segs[4] == 0xffff
                && segs[5] == 0;
            let is_discard = segs[0] == 0x0100 && segs[1] == 0 && segs[2] == 0 && segs[3] == 0;
            let is_teredo = segs[0] == 0x2001 && segs[1] == 0x0000;
            let is_documentation = segs[0] == 0x2001 && segs[1] == 0x0db8;
            let is_nat64 = segs[0] == 0x0064
                && segs[1] == 0xff9b
                && ((segs[2] == 0 && segs[3] == 0 && segs[4] == 0 && segs[5] == 0) || segs[2] == 1);
            let is_6to4 = seg0 == 0x2002;
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || is_ula
                || is_link_local
                || is_site_local
                || is_ipv4_compatible
                || is_ipv4_translated
                || is_discard
                || is_teredo
                || is_documentation
                || is_nat64
                || is_6to4
        }
    }
}

pub fn validate_remote_url(raw: &str, require_https: bool) -> Result<Url, String> {
    let trimmed = raw.trim();
    let url = Url::parse(trimmed).map_err(|e| format!("invalid_remote_url:{e}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("remote_url_credentials_forbidden".into());
    }
    let dev_allow = allow_insecure_plugin_urls();
    if require_https && !dev_allow {
        if url.scheme() != "https" {
            return Err(format!("insecure_plugin_url_scheme:{}", url.scheme()));
        }
    } else if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("unsupported_plugin_url_scheme:{}", url.scheme()));
    }
    let Some(host) = url.host() else {
        return Err("remote_url_missing_host".into());
    };
    if !dev_allow {
        match host {
            Host::Ipv4(v4) => {
                if is_forbidden_ip(IpAddr::V4(v4)) {
                    return Err("forbidden_private_plugin_host".into());
                }
            }
            Host::Ipv6(v6) => {
                if is_forbidden_ip(IpAddr::V6(v6)) {
                    return Err("forbidden_private_plugin_host".into());
                }
            }
            Host::Domain(domain) => {
                let lower = domain.trim_end_matches('.').to_ascii_lowercase();
                if lower == "localhost"
                    || lower.ends_with(".localhost")
                    || lower.ends_with(".local")
                    || lower.ends_with(".internal")
                {
                    return Err("forbidden_private_plugin_host".into());
                }
                let port = url.port_or_known_default().unwrap_or(443);
                let addrs: Vec<_> = (domain, port)
                    .to_socket_addrs()
                    .map_err(|_| "unresolvable_plugin_host".to_string())?
                    .collect();
                if addrs.is_empty() {
                    return Err("unresolvable_plugin_host".into());
                }
                for addr in addrs {
                    if is_forbidden_ip(addr.ip()) {
                        return Err("forbidden_private_plugin_ip".into());
                    }
                }
            }
        }
    }
    Ok(url)
}

pub fn is_forbidden_plugin_env_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "NODE_OPTIONS"
            | "LD_PRELOAD"
            | "LD_LIBRARY_PATH"
            | "DYLD_INSERT_LIBRARIES"
            | "DYLD_LIBRARY_PATH"
            | "PATH"
            | "PATHEXT"
            | "COMSPEC"
            | "SHELL"
            | "IFS"
            | "BASH_ENV"
            | "ENV"
            | "PYTHONSTARTUP"
            | "PYTHONPATH"
            | "RUBYOPT"
            | "PERL5OPT"
    )
}

fn is_forbidden_node_arg(arg: &str) -> bool {
    let t = arg.trim();
    matches!(
        t,
        "-e" | "--eval"
            | "-p"
            | "--print"
            | "-r"
            | "--require"
            | "--import"
            | "--loader"
            | "--experimental-loader"
            | "--experimental-vm-modules"
    ) || t.starts_with("--eval=")
        || t.starts_with("--print=")
        || t.starts_with("--require=")
        || t.starts_with("--import=")
        || t.starts_with("--loader=")
        || t.starts_with("--experimental-loader=")
}

pub fn validate_runtime_spec(spec: &PluginRuntimeSpec, install_dir: &Path) -> Result<(), String> {
    if spec.protocol != "acp" {
        return Err(format!("unsupported_plugin_protocol:{}", spec.protocol));
    }
    let cmd = spec.command.trim();
    if cmd.is_empty() {
        return Err("missing_runtime_command".into());
    }
    for key in spec.env.keys() {
        if is_forbidden_plugin_env_key(key) {
            return Err(format!("forbidden_plugin_env_key:{key}"));
        }
    }
    if cmd.eq_ignore_ascii_case("node") {
        let first_arg = spec
            .args
            .first()
            .ok_or_else(|| "missing_node_entrypoint".to_string())?;
        let lower = first_arg.trim().to_ascii_lowercase();
        if !lower.ends_with(".mjs") && !lower.ends_with(".js") {
            return Err(format!("invalid_node_entrypoint:{first_arg}"));
        }
        let rel = validate_relative_path(first_arg)?;
        if !install_dir.join(rel).is_file() {
            return Err(format!("missing_plugin_entrypoint:{first_arg}"));
        }
        for arg in &spec.args {
            if is_forbidden_node_arg(arg) {
                return Err(format!("forbidden_node_runtime_arg:{arg}"));
            }
        }
    } else {
        if !allow_native_plugins() {
            return Err(format!("forbidden_runtime_command:{cmd}"));
        }
        let rel = validate_relative_path(cmd)?;
        if !install_dir.join(rel).is_file() {
            return Err(format!("forbidden_runtime_command:{cmd}"));
        }
    }
    Ok(())
}

fn validate_plugin_identifier_token(value: &str, err_prefix: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.len() < 4 || trimmed.len() > 64 {
        return Err(format!("{err_prefix}:{value}"));
    }
    let lower = trimmed.to_ascii_lowercase();
    let bare = lower.trim_end_matches('_');
    if is_reserved_native_agent(&lower)
        || is_reserved_native_agent(bare)
        || lower.starts_with("codex")
        || lower.starts_with("claude_")
        || lower.starts_with("antigravity_")
        || lower.starts_with("acp_")
        || lower.starts_with("deepseek_harness")
    {
        return Err(format!("reserved_{err_prefix}:{trimmed}"));
    }
    Ok(())
}

pub fn validate_manifest_identifiers(manifest: &PluginManifest) -> Result<(), String> {
    if let Some(ref prefix) = manifest.session_prefix {
        validate_plugin_identifier_token(prefix, "invalid_session_prefix")?;
    }
    for prefix in &manifest.session_prefixes {
        validate_plugin_identifier_token(prefix, "invalid_session_prefix")?;
    }
    for alias in &manifest.aliases {
        validate_plugin_identifier_token(alias, "invalid_plugin_alias")?;
    }
    Ok(())
}
