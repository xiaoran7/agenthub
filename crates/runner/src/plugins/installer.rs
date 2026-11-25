use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use chrono::Utc;
use sha2::{Digest, Sha256};
use url::Url;

#[cfg(test)]
pub(crate) use super::security::is_forbidden_ip;
pub use super::security::{
    is_forbidden_plugin_env_key, is_trusted_remote_registry_url, url_matches_registry_prefix,
    validate_manifest_identifiers, validate_remote_url, validate_runtime_spec,
    validate_trusted_remote_registry_url,
};

use super::{
    security::build_http_agent,
    store::PluginStore,
    types::{
        InstalledPluginRecord, MAX_PLUGIN_FILE_BYTES, MAX_REGISTRY_BYTES, PluginManifest,
        RegistryDocument, RegistryPluginEntry, validate_plugin_id, validate_relative_path,
    },
};

pub const MAX_PLUGIN_FILES: usize = 256;
pub const MAX_PLUGIN_TOTAL_BYTES: usize = 64 * 1024 * 1024;

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
    fetch_bytes_with_policy_opts(source, max_bytes, enforce_remote_https, false)
}

fn fetch_bytes_with_policy_opts(
    source: &str,
    max_bytes: usize,
    enforce_remote_https: bool,
    enforce_remote_trust: bool,
) -> Result<Vec<u8>, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err("empty_source_url".into());
    }
    if is_local_source(trimmed) {
        if enforce_remote_https || enforce_remote_trust {
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
        let validated = validate_remote_url(&current, enforce_remote_https)?;
        if enforce_remote_trust && !is_trusted_remote_registry_url(&validated) {
            return Err("untrusted_remote_registry_origin".into());
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
    let reg_is_http =
        has_scheme_prefix(reg_trimmed, "http://") || has_scheme_prefix(reg_trimmed, "https://");
    let require_https = has_scheme_prefix(reg_trimmed, "https://");
    if has_scheme_prefix(trimmed, "file://") {
        if reg_is_http {
            return Err("forbidden_local_child_url".into());
        }
        return Ok(trimmed.to_string());
    }
    if has_scheme_prefix(trimmed, "http://") || has_scheme_prefix(trimmed, "https://") {
        if reg_is_http {
            return Ok(validate_remote_url(trimmed, require_https)?.to_string());
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
    let joined_url = base
        .join(trimmed)
        .map_err(|e| format!("invalid_child_url:{e}"))?;
    if reg_is_http && !matches!(joined_url.scheme(), "http" | "https") {
        return Err("forbidden_local_child_url".into());
    }
    let joined = joined_url.to_string();
    if reg_is_http {
        return Ok(validate_remote_url(&joined, require_https)?.to_string());
    }
    Ok(joined)
}

pub fn fetch_registry_document(registry_url: &str) -> Result<RegistryDocument, String> {
    let enforce_https = has_scheme_prefix(registry_url.trim(), "https://");
    let bytes =
        fetch_bytes_with_policy_opts(registry_url, MAX_REGISTRY_BYTES, enforce_https, false)?;
    parse_registry_document(&bytes)
}

pub fn fetch_remote_registry_document(registry_url: &str) -> Result<RegistryDocument, String> {
    validate_trusted_remote_registry_url(registry_url)?;
    let bytes = fetch_bytes_with_policy_opts(registry_url, MAX_REGISTRY_BYTES, true, true)?;
    parse_registry_document(&bytes)
}

fn parse_registry_document(bytes: &[u8]) -> Result<RegistryDocument, String> {
    let doc: RegistryDocument =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid_registry_json:{e}"))?;
    if doc.schema_version != 1 {
        return Err(format!(
            "unsupported_registry_schema:{}",
            doc.schema_version
        ));
    }
    Ok(doc)
}

pub(crate) fn stage_from_registry(
    store: &PluginStore,
    registry_url: &str,
    entry: &RegistryPluginEntry,
) -> Result<(tempfile::TempDir, PluginManifest), String> {
    stage_from_registry_with_policy(store, registry_url, entry, false)
}

pub(crate) fn stage_from_registry_with_policy(
    store: &PluginStore,
    registry_url: &str,
    entry: &RegistryPluginEntry,
    enforce_remote_trust: bool,
) -> Result<(tempfile::TempDir, PluginManifest), String> {
    validate_plugin_id(&entry.id)?;
    if entry.files.is_empty() {
        return Err(format!("plugin_registry_missing_files:{}", entry.id));
    }
    if entry.files.len() > MAX_PLUGIN_FILES {
        return Err(format!("plugin_too_many_files:{}", entry.files.len()));
    }
    let parsed_reg = if enforce_remote_trust {
        Some(validate_trusted_remote_registry_url(registry_url)?)
    } else {
        Url::parse(registry_url.trim()).ok()
    };
    let enforce_child_trust = enforce_remote_trust
        || parsed_reg
            .as_ref()
            .is_some_and(|u| u.scheme() == "https" && is_trusted_remote_registry_url(u));

    store.ensure_dirs()?;
    let staging = tempfile::tempdir_in(store.staging_dir())
        .map_err(|e| format!("create_staging_dir_failed:{e}"))?;

    let mut seen_paths: HashSet<String> = HashSet::with_capacity(entry.files.len());
    let mut validated_files: Vec<(&crate::plugins::types::RegistryFileEntry, PathBuf)> =
        Vec::with_capacity(entry.files.len());

    // Fail-fast pass: validate all paths, duplicates, sizes, and child trust BEFORE any I/O.
    for file in &entry.files {
        let rel_path = validate_relative_path(&file.path)?;
        let norm_key = rel_path.to_string_lossy().to_ascii_lowercase();
        if !seen_paths.insert(norm_key) {
            return Err(format!("duplicate_plugin_file_path:{}", file.path));
        }
        if let Some(expected_size) = file.size
            && expected_size > MAX_PLUGIN_FILE_BYTES as u64
        {
            return Err(format!("download_exceeded_limit:{MAX_PLUGIN_FILE_BYTES}"));
        }
        if enforce_child_trust {
            let raw_child = file.url.trim();
            if has_scheme_prefix(raw_child, "file://") {
                return Err("forbidden_local_child_url".into());
            }
            if super::security::has_unsafe_path_encoding(raw_child)
                || super::security::has_raw_dot_segments(raw_child)
            {
                return Err(format!("untrusted_plugin_child_url:{}", file.path));
            }
            let pre_child = if has_scheme_prefix(raw_child, "http://")
                || has_scheme_prefix(raw_child, "https://")
            {
                Url::parse(raw_child).ok()
            } else {
                parsed_reg.as_ref().and_then(|reg| reg.join(raw_child).ok())
            };
            let Some(child_url) = pre_child else {
                return Err(format!("untrusted_plugin_child_url:{}", file.path));
            };
            let within_registry_prefix = parsed_reg
                .as_ref()
                .is_some_and(|reg| url_matches_registry_prefix(&child_url, reg));
            if !within_registry_prefix && !is_trusted_remote_registry_url(&child_url) {
                return Err(format!("untrusted_plugin_child_url:{}", file.path));
            }
        }
        validated_files.push((file, rel_path));
    }

    let mut total_bytes: usize = 0;

    for (file, rel_path) in validated_files {
        let url = resolve_child_url(registry_url, &file.url)?;
        let bytes = if enforce_child_trust {
            fetch_bytes_with_policy_opts(&url, MAX_PLUGIN_FILE_BYTES, true, true)?
        } else {
            fetch_bytes(&url, MAX_PLUGIN_FILE_BYTES)?
        };
        total_bytes = total_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| format!("plugin_total_size_exceeded:{MAX_PLUGIN_TOTAL_BYTES}"))?;
        if total_bytes > MAX_PLUGIN_TOTAL_BYTES {
            return Err(format!(
                "plugin_total_size_exceeded:{MAX_PLUGIN_TOTAL_BYTES}"
            ));
        }
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
    Ok((staging, manifest))
}

pub fn install_from_registry(
    store: &PluginStore,
    registry_url: &str,
    entry: &RegistryPluginEntry,
    enable_after: bool,
) -> Result<InstalledPluginRecord, String> {
    let (staging, manifest) = stage_from_registry(store, registry_url, entry)?;
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
    let mut file_count = 0usize;
    let mut total_bytes = 0usize;
    copy_dir_safe(&dir_path, staging.path(), &mut file_count, &mut total_bytes)?;
    let manifest = validate_staged_manifest(staging.path(), expected_id)?;
    commit_staged_dir(
        store,
        staging.keep(),
        &manifest,
        Some(source_dir),
        enable_after,
    )
}

fn copy_dir_safe(
    src: &Path,
    dst: &Path,
    file_count: &mut usize,
    total_bytes: &mut usize,
) -> Result<(), String> {
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
            copy_dir_safe(&path, &target, file_count, total_bytes)?;
        } else if ft.is_file() {
            *file_count += 1;
            if *file_count > MAX_PLUGIN_FILES {
                return Err(format!("plugin_too_many_files:{MAX_PLUGIN_FILES}"));
            }
            let len = entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
            if len > MAX_PLUGIN_FILE_BYTES {
                return Err(format!("local_file_too_large:{}", path.display()));
            }
            *total_bytes = total_bytes
                .checked_add(len)
                .ok_or_else(|| format!("plugin_total_size_exceeded:{MAX_PLUGIN_TOTAL_BYTES}"))?;
            if *total_bytes > MAX_PLUGIN_TOTAL_BYTES {
                return Err(format!(
                    "plugin_total_size_exceeded:{MAX_PLUGIN_TOTAL_BYTES}"
                ));
            }
            fs::copy(&path, &target).map_err(|e| format!("copy_failed:{e}"))?;
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
    validate_manifest_identifiers(&manifest)?;
    validate_runtime_spec(&manifest.runtime, dir)?;
    Ok(manifest)
}

pub(crate) fn commit_staged_dir(
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
