use std::{
    env,
    path::{Path, PathBuf},
};

use super::*;

pub fn discover_claude_binary() -> PathBuf {
    claude_binary_candidates()
        .into_iter()
        .next()
        .map(|candidate| candidate.path)
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// Every Claude Code binary worth trying, best first.
///
/// Order is decided by probed version, not by which file was found first: an
/// older CLI reports a transcript written by a newer one as missing, so the
/// newest local build is the one most likely to resume a session at all.
pub fn claude_binary_candidates() -> Vec<crate::agent_binary::BinaryCandidate> {
    crate::agent_binary::claude_candidates().into_ranked(BINARY_PROBE_TIMEOUT)
}

pub fn discover_antigravity_binary() -> PathBuf {
    antigravity_binary_candidates()
        .into_iter()
        .next()
        .map(|candidate| candidate.path)
        .unwrap_or_else(|| PathBuf::from("agy"))
}

/// Discovers candidate Antigravity CLI executables (`agy` preferred, then `antigravity`).
pub fn antigravity_binary_candidates() -> Vec<crate::agent_binary::BinaryCandidate> {
    let mut set = crate::agent_binary::CandidateSet::new();
    if let Some(path) = env::var_os("ANTIGRAVITY_BINARY").filter(|value| !value.is_empty()) {
        set.push_override(path);
        return set.into_ranked(BINARY_PROBE_TIMEOUT);
    }

    if let Some(home) = home_dir() {
        let local_bin = home.join(".local").join("bin");
        for stem in ["agy", "antigravity", "antigravity-cli"] {
            set.push_suffixed(
                &local_bin,
                stem,
                crate::agent_binary::CandidateOrigin::UserInstall,
            );
        }
        #[cfg(windows)]
        {
            set.push_suffixed(
                &home.join("AppData").join("Local").join("agy").join("bin"),
                "agy",
                crate::agent_binary::CandidateOrigin::ManagedInstall,
            );
            set.push_suffixed(
                &home
                    .join("AppData")
                    .join("Local")
                    .join("Programs")
                    .join("antigravity")
                    .join("bin"),
                "antigravity",
                crate::agent_binary::CandidateOrigin::ManagedInstall,
            );
        }
    }

    #[cfg(windows)]
    if let Some(local_app) = env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        let local_root = PathBuf::from(local_app);
        set.push_suffixed(
            &local_root.join("agy").join("bin"),
            "agy",
            crate::agent_binary::CandidateOrigin::ManagedInstall,
        );
        set.push_suffixed(
            &local_root.join("Programs").join("antigravity").join("bin"),
            "antigravity",
            crate::agent_binary::CandidateOrigin::ManagedInstall,
        );
    }

    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            for stem in ["agy", "antigravity", "antigravity-cli"] {
                set.push_suffixed(&dir, stem, crate::agent_binary::CandidateOrigin::PathLookup);
            }
        }
    }
    if set.is_empty() {
        set.push(
            PathBuf::from("agy"),
            crate::agent_binary::CandidateOrigin::PathLookup,
        );
        set.push(
            PathBuf::from("antigravity"),
            crate::agent_binary::CandidateOrigin::PathLookup,
        );
    }
    set.into_ranked(BINARY_PROBE_TIMEOUT)
}

fn is_electron_antigravity_gui_binary(path: &Path) -> bool {
    let fname = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if fname == "antigravity.exe" {
        let p_lower = path.to_string_lossy().to_ascii_lowercase();
        if p_lower.contains(r"programsntigravity") || p_lower.contains("programs/antigravity") {
            return true;
        }
    }
    false
}

/// Non-spawning check whether the resolved Antigravity CLI executable (`agy`) exists locally or in PATH.
pub fn is_antigravity_cli_available(binary: &Path) -> bool {
    if binary.is_file() {
        return !is_electron_antigravity_gui_binary(binary);
    }
    if binary.parent().is_some_and(|p| !p.as_os_str().is_empty()) {
        return false;
    }
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    let name = binary.to_string_lossy();
    env::split_paths(&path_var).any(|dir| {
        #[cfg(windows)]
        let exts = ["", ".exe", ".cmd", ".bat"];
        #[cfg(not(windows))]
        let exts = [""];
        exts.iter().any(|ext| {
            let candidate = if ext.is_empty() || name.ends_with(ext) {
                dir.join(name.as_ref())
            } else {
                dir.join(format!("{name}{ext}"))
            };
            candidate.is_file() && !is_electron_antigravity_gui_binary(&candidate)
        })
    })
}

/// True when Antigravity turns can be executed via either the standalone `agy` CLI
/// or a live Antigravity 2.0 Desktop `language_server` daemon.
pub fn is_antigravity_runtime_available(binary: &Path) -> bool {
    is_antigravity_cli_available(binary)
        || crate::collectors::antigravity::discover_live_antigravity_daemon().is_some()
        || discover_antigravity_agentapi_binary().is_some()
}

/// Locates Antigravity Desktop's bundled `agentapi` helper (`~/.gemini/antigravity/bin/agentapi[.bat]`).
pub fn discover_antigravity_agentapi_binary() -> Option<PathBuf> {
    if let Some(path) = env::var_os("ANTIGRAVITY_AGENTAPI_BINARY").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    #[cfg(windows)]
    let names = ["agentapi.bat", "agentapi.cmd", "agentapi.exe", "agentapi"];
    #[cfg(not(windows))]
    let names = ["agentapi"];
    if let Ok(Some(ag_home)) = super::discover_antigravity_home() {
        let bin = ag_home.join("bin");
        for name in names {
            let p = bin.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    if let Some(home) = home_dir() {
        let bin = home.join(".gemini").join("antigravity").join("bin");
        for name in names {
            let p = bin.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn resolve_node_executable() -> PathBuf {
    if let Some(path) = env::var_os("NODE_BINARY").filter(|v| !v.is_empty()) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return dunce::canonicalize(&candidate).unwrap_or(candidate);
        }
    }
    #[cfg(windows)]
    {
        for candidate in [
            PathBuf::from(r"D:\nodejs\node.exe"),
            PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
        ] {
            if candidate.is_file() {
                return dunce::canonicalize(&candidate).unwrap_or(candidate);
            }
        }
    }
    PathBuf::from("node")
}

pub(crate) fn resolve_codex_app_server_binary(binary: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        resolve_windows_codex_binary(binary).unwrap_or_else(|| binary.to_path_buf())
    }
    #[cfg(not(windows))]
    {
        binary.to_path_buf()
    }
}

#[cfg(windows)]
fn resolve_windows_codex_binary(binary: &Path) -> Option<PathBuf> {
    if binary
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty())
    {
        return resolve_windows_codex_candidate(binary);
    }

    let path = env::var_os("PATH")?;
    let mut executable_fallback = None;
    for directory in env::split_paths(&path) {
        for extension in ["cmd", "ps1"] {
            let candidate = directory.join(format!("{}.{}", binary.display(), extension));
            if let Some(native) = resolve_windows_codex_candidate(&candidate) {
                return Some(native);
            }
        }
        let candidate = directory.join(format!("{}.exe", binary.display()));
        if executable_fallback.is_none() && candidate.is_file() {
            executable_fallback = candidate.canonicalize().ok().or(Some(candidate));
        }
    }
    executable_fallback
}

#[cfg(windows)]
pub(crate) fn resolve_windows_codex_candidate(candidate: &Path) -> Option<PathBuf> {
    if candidate
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        && candidate.is_file()
    {
        return candidate
            .canonicalize()
            .ok()
            .or_else(|| Some(candidate.into()));
    }
    if !candidate.is_file()
        || !candidate
            .file_stem()
            .is_some_and(|stem| stem.eq_ignore_ascii_case("codex"))
    {
        return None;
    }

    let parent = candidate.parent()?;
    let package_root = if parent
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(".bin"))
    {
        parent.parent()?.join("@openai").join("codex")
    } else {
        parent.join("node_modules").join("@openai").join("codex")
    };
    #[cfg(target_arch = "x86_64")]
    let (platform_package, target) = ("codex-win32-x64", "x86_64-pc-windows-msvc");
    #[cfg(target_arch = "aarch64")]
    let (platform_package, target) = ("codex-win32-arm64", "aarch64-pc-windows-msvc");
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    return None;

    for native in [
        package_root
            .join("node_modules")
            .join("@openai")
            .join(platform_package)
            .join("vendor")
            .join(target)
            .join("bin")
            .join("codex.exe"),
        package_root
            .join("vendor")
            .join(target)
            .join("bin")
            .join("codex.exe"),
    ] {
        if native.is_file() {
            return native.canonicalize().ok().or(Some(native));
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct CollectorBinaries {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

impl Default for CollectorBinaries {
    fn default() -> Self {
        Self {
            claude: PathBuf::from("claude"),
            codex: PathBuf::from("codex"),
        }
    }
}
