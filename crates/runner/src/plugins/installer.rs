use std::{
    fs,
    io::Read,
    net::{IpAddr, ToSocketAddrs},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use chrono::Utc;
use sha2::{Digest, Sha256};
use url::{Host, Url};

use super::{
    store::PluginStore,
    types::{
        InstalledPluginRecord, MAX_PLUGIN_FILE_BYTES, MAX_REGISTRY_BYTES, PluginManifest,
        PluginRuntimeSpec, RegistryDocument, RegistryPluginEntry, validate_plugin_id,
        validate_relative_path,
    },
};

fn build_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("AgentHub-Runner/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .new_agent()
}

fn allow_insecure_plugin_urls() -> bool {
    std::env::var("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "yes"))
}

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let oct = v4.octets();
            let is_cgnat = oct[0] == 100 && (oct[1] & 0b1100_0000) == 64;
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || is_cgnat
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_forbidden_ip(IpAddr::V4(mapped));
            }
            let seg0 = v6.segments()[0];
            let is_ula = (seg0 & 0xfe00) == 0xfc00;
            let is_link_local = (seg0 & 0xffc0) == 0xfe80;
            v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || is_ula || is_link_local
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
                if let Ok(addrs) = (domain, port).to_socket_addrs() {
                    for addr in addrs {
                        if is_forbidden_ip(addr.ip()) {
                            return Err("forbidden_private_plugin_ip".into());
                        }
                    }
                }
            }
        }
    }
    Ok(url)
}

pub fn has_scheme_prefix(s: &str, scheme_prefix: &str) -> bool {
    s.get(..scheme_prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(scheme_prefix))
}

fn file_url_to_path(source: &str) -> Result<PathBuf, String> {
    let trimmed = source.trim();
    if has_scheme_prefix(trimmed, "file://") {
        let parsed = Url::parse(trimmed).map_err(|e| format!("invalid_file_url:{e}"))?;
        if let Ok(path) = parsed.to_file_path() {
            return Ok(path);
        }
        return Ok(PathBuf::from(&trimmed["file://".len()..]));
    }
    Ok(PathBuf::from(trimmed))
}

pub fn is_local_source(source: &str) -> bool {
    let trimmed = source.trim();
    has_scheme_prefix(trimmed, "file://")
        || (!has_scheme_prefix(trimmed, "http://") && !has_scheme_prefix(trimmed, "https://"))
}

pub fn fetch_bytes(source: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let require_remote_https = has_scheme_prefix(source.trim(), "https://");
    fetch_bytes_with_policy(source, max_bytes, require_remote_https)
}

pub fn fetch_bytes_with_policy(
    source: &str,
    max_bytes: usize,
    enforce_remote_https: bool,
) -> Result<Vec<u8>, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err("empty_source_url".into());
    }
    if is_local_source(trimmed) {
        if enforce_remote_https {
            return Err("local_source_forbidden_for_remote_policy".into());
        }
        let path = file_url_to_path(trimmed)?;
        let meta = fs::metadata(&path)
            .map_err(|e| format!("read_local_failed:{}: {e}", path.display()))?;
        if meta.len() as usize > max_bytes {
            return Err(format!("local_file_too_large:{}", path.display()));
        }
        return fs::read(&path).map_err(|e| format!("read_local_failed:{}: {e}", path.display()));
    }

    let agent = build_http_agent();
    let mut current = trimmed.to_string();
    for hop in 0..=5 {
        if enforce_remote_https {
            validate_remote_url(&current, true)?;
        } else {
            let parsed = Url::parse(&current).map_err(|e| format!("invalid_source_url:{e}"))?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(format!("unsupported_source_scheme:{}", parsed.scheme()));
            }
        }
        let mut response = agent
            .get(&current)
            .call()
            .map_err(|e| format!("download_failed:{current}: {e}"))?;
        let status = response.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if hop == 5 {
                return Err(format!("download_too_many_redirects:{trimmed}"));
            }
            let loc = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| format!("download_redirect_missing_location:{current}"))?;
            let base = Url::parse(&current).map_err(|e| format!("invalid_source_url:{e}"))?;
            current = base
                .join(loc.trim())
                .map_err(|e| format!("invalid_redirect_url:{e}"))?
                .to_string();
            continue;
        }
        if status >= 400 {
            return Err(format!("download_http_status:{status}:{current}"));
        }
        let mut buf = Vec::new();
        let mut reader = response.body_mut().as_reader().take((max_bytes + 1) as u64);
        reader
            .read_to_end(&mut buf)
            .map_err(|e| format!("download_read_failed:{current}: {e}"))?;
        if buf.len() > max_bytes {
            return Err(format!("download_exceeded_limit:{max_bytes}"));
        }
        return Ok(buf);
    }
    Err(format!("download_too_many_redirects:{trimmed}"))
}

pub fn resolve_child_url(registry_url: &str, file_url: &str) -> Result<String, String> {
    let reg_trimmed = registry_url.trim();
    let trimmed = file_url.trim();
    if has_scheme_prefix(trimmed, "http://")
        || has_scheme_prefix(trimmed, "https://")
        || has_scheme_prefix(trimmed, "file://")
    {
        if has_scheme_prefix(reg_trimmed, "https://") {
            return Ok(validate_remote_url(trimmed, true)?.to_string());
        }
        return Ok(trimmed.to_string());
    }
    if is_local_source(reg_trimmed) {
        let reg_path = file_url_to_path(reg_trimmed)?;
        let base_dir = reg_path
            .parent()
            .ok_or_else(|| "registry_path_missing_parent".to_string())?;
        let rel = validate_relative_path(trimmed)?;
        let target = base_dir.join(rel);
        if has_scheme_prefix(reg_trimmed, "file://") {
            return Url::from_file_path(&target)
                .map(|u| u.to_string())
                .map_err(|_| format!("invalid_file_path:{}", target.display()));
        }
        return Ok(target.to_string_lossy().to_string());
    }
    let base = Url::parse(reg_trimmed).map_err(|e| format!("invalid_registry_url:{e}"))?;
    let joined = base
        .join(trimmed)
        .map(|u| u.to_string())
        .map_err(|e| format!("invalid_child_url:{e}"))?;
    if has_scheme_prefix(reg_trimmed, "https://") {
        return Ok(validate_remote_url(&joined, true)?.to_string());
    }
    Ok(joined)
}

pub fn fetch_registry_document(registry_url: &str) -> Result<RegistryDocument, String> {
    let enforce_https = has_scheme_prefix(registry_url.trim(), "https://");
    let bytes = fetch_bytes_with_policy(registry_url, MAX_REGISTRY_BYTES, enforce_https)?;
    let doc: RegistryDocument =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid_registry_json:{e}"))?;
    if doc.schema_version != 1 {
        return Err(format!(
            "unsupported_registry_schema:{}",
            doc.schema_version
        ));
    }
    Ok(doc)
}

pub fn fetch_remote_registry_document(registry_url: &str) -> Result<RegistryDocument, String> {
    validate_remote_url(registry_url, true)?;
    fetch_registry_document(registry_url)
}

pub fn install_from_registry(
    store: &PluginStore,
    registry_url: &str,
    entry: &RegistryPluginEntry,
    enable_after: bool,
) -> Result<InstalledPluginRecord, String> {
    validate_plugin_id(&entry.id)?;
    if entry.files.is_empty() {
        return Err(format!("plugin_registry_missing_files:{}", entry.id));
    }
    store.ensure_dirs()?;
    let staging = tempfile::tempdir_in(store.staging_dir())
        .map_err(|e| format!("create_staging_dir_failed:{e}"))?;

    for file in &entry.files {
        let rel_path = validate_relative_path(&file.path)?;
        let url = resolve_child_url(registry_url, &file.url)?;
        let bytes = fetch_bytes(&url, MAX_PLUGIN_FILE_BYTES)?;
        if let Some(expected_size) = file.size
            && bytes.len() as u64 != expected_size
        {
            return Err(format!(
                "plugin_file_size_mismatch:{}: expected {}, got {}",
                file.path,
                expected_size,
                bytes.len()
            ));
        }
        let actual_sha = hex::encode(Sha256::digest(&bytes));
        if !actual_sha.eq_ignore_ascii_case(file.sha256.trim()) {
            return Err(format!(
                "plugin_sha256_mismatch:{}: expected {}, got {}",
                file.path, file.sha256, actual_sha
            ));
        }
        let dest = staging.path().join(&rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create_subdir_failed:{e}"))?;
        }
        fs::write(&dest, &bytes).map_err(|e| format!("write_staged_file_failed:{e}"))?;
    }

    let manifest = validate_staged_manifest(staging.path(), Some(&entry.id))?;
    commit_staged_dir(
        store,
        staging.keep(),
        &manifest,
        Some(registry_url),
        enable_after,
    )
}

pub fn install_from_local_directory(
    store: &PluginStore,
    source_dir: &str,
    expected_id: Option<&str>,
    enable_after: bool,
) -> Result<InstalledPluginRecord, String> {
    let dir_path = file_url_to_path(source_dir)?;
    if !dir_path.is_dir() {
        return Err(format!("plugin_source_not_dir:{}", dir_path.display()));
    }
    let _ = validate_staged_manifest(&dir_path, expected_id)?;
    store.ensure_dirs()?;
    let staging = tempfile::tempdir_in(store.staging_dir())
        .map_err(|e| format!("create_staging_dir_failed:{e}"))?;
    copy_dir_safe(&dir_path, staging.path())?;
    let manifest = validate_staged_manifest(staging.path(), expected_id)?;
    commit_staged_dir(
        store,
        staging.keep(),
        &manifest,
        Some(source_dir),
        enable_after,
    )
}

fn copy_dir_safe(src: &Path, dst: &Path) -> Result<(), String> {
    let entries = fs::read_dir(src).map_err(|e| format!("read_dir_failed:{e}"))?;
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".git" || name_str == "node_modules" {
            continue;
        }
        let path = entry.path();
        let target = dst.join(&name);
        if ft.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("mkdir_failed:{e}"))?;
            copy_dir_safe(&path, &target)?;
        } else if ft.is_file() {
            fs::copy(&path, &target).map_err(|e| format!("copy_failed:{e}"))?;
        }
    }
    Ok(())
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
        let rel = validate_relative_path(cmd)?;
        if !install_dir.join(rel).is_file() {
            return Err(format!("forbidden_runtime_command:{cmd}"));
        }
    }
    Ok(())
}

fn validate_staged_manifest(
    dir: &Path,
    expected_id: Option<&str>,
) -> Result<PluginManifest, String> {
    let manifest_path = dir.join("plugin.json");
    let raw = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("missing_plugin_manifest:{}: {e}", manifest_path.display()))?;
    let manifest: PluginManifest =
        serde_json::from_str(&raw).map_err(|e| format!("invalid_plugin_manifest:{e}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported_plugin_schema:{}",
            manifest.schema_version
        ));
    }
    validate_plugin_id(&manifest.id)?;
    if let Some(expected) = expected_id
        && manifest.id != expected
    {
        return Err(format!(
            "plugin_id_mismatch: expected {expected}, got {}",
            manifest.id
        ));
    }
    validate_runtime_spec(&manifest.runtime, dir)?;
    Ok(manifest)
}

fn commit_staged_dir(
    store: &PluginStore,
    staged_path: PathBuf,
    manifest: &PluginManifest,
    source_url: Option<&str>,
    enable_after: bool,
) -> Result<InstalledPluginRecord, String> {
    let target = store.installed_dir().join(&manifest.id);
    let backup = store.staging_dir().join(format!(
        "backup-{}-{}",
        manifest.id,
        Utc::now().timestamp_millis()
    ));
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    let had_existing = target.exists();
    if had_existing && let Err(err) = fs::rename(&target, &backup) {
        let _ = fs::remove_dir_all(&staged_path);
        return Err(format!("backup_existing_plugin:{err}"));
    }
    if let Err(err) = fs::rename(&staged_path, &target) {
        if had_existing && backup.exists() {
            let _ = fs::rename(&backup, &target);
        }
        let _ = fs::remove_dir_all(&staged_path);
        return Err(format!("activate_plugin_dir:{err}"));
    }
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }

    let final_dir = dunce::canonicalize(&target).unwrap_or(target);
    Ok(InstalledPluginRecord {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        enabled: enable_after,
        install_dir: final_dir.to_string_lossy().to_string(),
        installed_at: Utc::now().to_rfc3339(),
        source_url: source_url.map(str::to_string),
        runtime_status: if enable_after {
            "ready".to_string()
        } else {
            "disabled".to_string()
        },
        error: None,
        manifest: manifest.clone(),
    })
}

fn is_retryable_remove_err(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::PermissionDenied
        || matches!(err.raw_os_error(), Some(32) | Some(145))
}

pub fn remove_installed_dir(store: &PluginStore, id: &str) -> Result<(), String> {
    validate_plugin_id(id)?;
    let target = store.installed_dir().join(id);
    if !target.exists() {
        return Ok(());
    }
    for attempt in 0..8 {
        match fs::remove_dir_all(&target) {
            Ok(()) => return Ok(()),
            Err(err) if attempt < 7 && is_retryable_remove_err(&err) => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(format!("remove_plugin_dir:{err}")),
        }
    }
    Ok(())
}
