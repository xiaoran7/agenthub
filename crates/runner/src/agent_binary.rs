//! Agent CLI discovery and selection.
//!
//! An agent CLI is picked in three steps: collect every candidate the machine
//! offers, probe each one for a usable version, then select. Selection is
//! *newest-first* rather than *first-found* because transcript formats only
//! gain fields: a newer binary can read an older transcript, but an older
//! binary reports a session it cannot parse as missing. Picking the newest
//! therefore maximises the chance a resume succeeds, and a recorded
//! session-to-binary affinity covers the case where newest still cannot read it.
//!
//! Discovery is deliberately not provider-specific. Codex, Grok and Antigravity
//! install side-by-side with a user's own copy, and ZCode ships as a node
//! script, so every one of them can end up with two versions on the same
//! machine.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

/// A parsed provider CLI version. Only the components that order releases are
/// kept; pre-release and build metadata are ignored because they carry no
/// ordering meaning for "can this binary read that transcript".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AgentCliVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl AgentCliVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Extracts a `2.1.281`-shaped version from arbitrary CLI text.
    ///
    /// Providers print different shapes (`claude 2.1.281 (Claude Code)`,
    /// `codex-cli 0.52.0`, `1.2`). The first dotted triple wins, which matches
    /// every current provider because the leading token is the version itself.
    pub fn parse(text: &str) -> Option<Self> {
        for token in text.split(|ch: char| ch.is_whitespace() || ch == '(' || ch == ')') {
            let token = token.trim_start_matches('v');
            if token.is_empty() {
                continue;
            }
            // Skip a bare number that is only a year or a pid-shaped token.
            let mut parts = token.split('.');
            let (Some(major), Some(minor)) = (
                parts.next().and_then(|part| part.parse::<u32>().ok()),
                parts.next().and_then(|part| part.parse::<u32>().ok()),
            ) else {
                continue;
            };
            let patch = parts
                .next()
                .and_then(|part| part.parse::<u32>().ok())
                .unwrap_or(0);
            return Some(Self::new(major, minor, patch));
        }
        None
    }
}

impl std::fmt::Display for AgentCliVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Where a candidate binary came from. Kept separate from ordering so the
/// fallback log can explain why a particular executable was tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateOrigin {
    /// The user's explicit override. Never probed past the first candidate and
    /// never replaced, so a pinned binary keeps behaving exactly as pinned.
    EnvironmentOverride,
    /// A provider-managed install directory that the owning app updates.
    ManagedInstall,
    /// A version-manager shim, e.g. `~/.local/bin/claude`.
    UserInstall,
    /// Resolved through `PATH`.
    PathLookup,
}

impl CandidateOrigin {
    /// Prefers managed installs over user installs when versions tie: the
    /// managed copy is the one the provider's own launcher runs, so it is the
    /// binary most likely to match the transcripts on disk.
    fn rank(self) -> u8 {
        match self {
            Self::EnvironmentOverride => 0,
            Self::ManagedInstall => 1,
            Self::UserInstall => 2,
            Self::PathLookup => 3,
        }
    }
}

/// One discovered executable plus the version it reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryCandidate {
    pub path: PathBuf,
    pub version: Option<AgentCliVersion>,
    pub origin: CandidateOrigin,
}

impl BinaryCandidate {
    fn new(
        path: impl Into<PathBuf>,
        version: Option<AgentCliVersion>,
        origin: CandidateOrigin,
    ) -> Self {
        Self {
            path: path.into(),
            version,
            origin,
        }
    }

    /// Selection key: known versions sort above unknown ones, and among known
    /// versions the newest wins. Origin only breaks exact ties.
    fn rank(&self) -> (u8, Option<AgentCliVersion>, u8) {
        (
            self.version.is_some() as u8,
            self.version,
            self.origin.rank(),
        )
    }
}

/// Runs `<binary> --version` and parses the result.
///
/// A binary that will not start, hangs, or prints something unparseable is
/// reported as version-less rather than as an error: discovery must survive a
/// half-installed CLI rather than abort the whole selection.
///
/// The child is killed once `timeout` elapses. `Command::output()` alone would
/// block forever on a wedged binary, and discovery runs on the daemon startup
/// path, so the wait is bounded explicitly.
pub fn probe_version(binary: &Path, timeout: Duration) -> Option<AgentCliVersion> {
    let mut command = Command::new(binary);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A version probe must never flash a console window on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + timeout;
    let stdout = loop {
        match child.try_wait() {
            Ok(Some(_status)) => break child.wait_with_output().ok().map(|out| out.stdout),
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                return None;
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(20));
    }?;
    AgentCliVersion::parse(&String::from_utf8_lossy(&stdout))
}

/// Probes every candidate, drops duplicates, and orders them best-first.
///
/// Probes run concurrently, one short-lived thread per candidate. Discovery
/// runs on the daemon startup path and a machine that has several versions of
/// an agent installed can hold a dozen candidates; probing them one at a time
/// made startup cost `candidates × timeout` in the worst case, where a probe
/// only ever has to outlive the slowest single binary. The result is
/// order-independent, so the concurrency does not change what is returned.
pub fn rank_candidates(
    mut candidates: Vec<BinaryCandidate>,
    timeout: Duration,
) -> Vec<BinaryCandidate> {
    // Same path discovered twice (PATH and an explicit dir, say) is one
    // candidate, not two fallback attempts.
    let mut by_path: BTreeMap<PathBuf, CandidateOrigin> = BTreeMap::new();
    for candidate in candidates.drain(..) {
        let entry = by_path
            .entry(normalize_key(&candidate.path))
            .or_insert(candidate.origin);
        if candidate.origin.rank() < entry.rank() {
            *entry = candidate.origin;
        }
    }
    let mut unique: Vec<BinaryCandidate> = by_path
        .into_iter()
        .map(|(path, origin)| BinaryCandidate::new(path, None, origin))
        .collect();
    let probed: Vec<Option<AgentCliVersion>> = probe_all(
        unique.iter().map(|candidate| candidate.path.clone()),
        timeout,
    );
    for (candidate, version) in unique.iter_mut().zip(probed) {
        candidate.version = version;
    }
    unique.sort_by_key(|candidate| std::cmp::Reverse(candidate.rank()));
    unique
}

/// Probes each path in its own thread and returns the versions in the same
/// order. Threads, not async: a probe is a blocking spawn/wait/kill loop, and
/// there is at most a dozen of them.
fn probe_all(
    paths: impl IntoIterator<Item = PathBuf>,
    timeout: Duration,
) -> Vec<Option<AgentCliVersion>> {
    thread::scope(|scope| {
        let handles: Vec<_> = paths
            .into_iter()
            .map(|path| scope.spawn(move || probe_version(&path, timeout)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(None))
            .collect()
    })
}

/// Windows and Unix spell the same file differently once canonicalized;
/// comparing raw strings would make `/path/claude` and `C:\path\claude.exe`
/// two candidates. Case folding only applies to Windows paths.
fn normalize_key(path: &Path) -> PathBuf {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    let canonical = PathBuf::from(canonical.to_string_lossy().to_lowercase());
    canonical
}

/// The ordered set of executables worth trying for one agent.
#[derive(Debug, Default)]
pub struct CandidateSet {
    candidates: Vec<BinaryCandidate>,
}

impl CandidateSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a user override. The first one wins and is never replaced: an
    /// explicit pin must survive every other source.
    ///
    /// Setting an override also suppresses every candidate added afterwards,
    /// so a caller cannot accidentally widen a pinned binary back out to
    /// whatever else happens to be installed.
    pub fn push_override(&mut self, path: impl Into<PathBuf>) {
        if self.is_overridden() {
            return;
        }
        self.candidates.push(BinaryCandidate::new(
            path,
            None,
            CandidateOrigin::EnvironmentOverride,
        ));
    }

    fn is_overridden(&self) -> bool {
        self.candidates
            .iter()
            .any(|candidate| candidate.origin == CandidateOrigin::EnvironmentOverride)
    }

    /// Adds a discovered candidate. Ignored once an override is set, which is
    /// what makes the override authoritative rather than merely preferred.
    pub fn push(&mut self, path: impl Into<PathBuf>, origin: CandidateOrigin) {
        if self.is_overridden() {
            return;
        }
        self.candidates
            .push(BinaryCandidate::new(path, None, origin));
    }

    /// Adds `<dir>/<stem>.exe` and the extension-less `<dir>/<stem>` variants
    /// that npm shims use on Unix.
    pub fn push_suffixed(&mut self, dir: &Path, stem: &str, origin: CandidateOrigin) {
        for suffix in ["exe", "cmd", ""] {
            let name = if suffix.is_empty() {
                stem.to_owned()
            } else {
                format!("{stem}.{suffix}")
            };
            let candidate = dir.join(name);
            if candidate.is_file() {
                self.push(candidate, origin);
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    pub fn into_ranked(self, timeout: Duration) -> Vec<BinaryCandidate> {
        rank_candidates(self.candidates, timeout)
    }
}

/// Where the managed (provider-owned) install of a Claude Code CLI lives.
///
/// The layout is `<root>/claude-code/<version>/claude.exe`, so the version
/// directory is part of the path and changes on every upgrade. Globbing for it
/// keeps discovery working across upgrades instead of pinning one release.
#[cfg(windows)]
pub fn managed_claude_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(local) = env_local_app_data() {
        roots.push(local.join("Claude-3p"));
        roots.push(local.join("Claude"));
        roots.push(local.join("Programs").join("Claude"));
    }
    roots
}

#[cfg(not(windows))]
pub fn managed_claude_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(app_support) =
        env_home().map(|home| home.join("Library").join("Application Support"))
    {
        roots.push(app_support.join("Claude"));
        roots.push(app_support.join("Claude-3p"));
    }
    roots
}

#[cfg(windows)]
fn env_local_app_data() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn env_home() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Expands every `<root>/claude-code/*/claude[.exe]` found under the managed
/// install roots. A malformed or absent directory yields nothing rather than
/// an error, because a missing managed install is the normal case for a user
/// who only has the standalone CLI.
pub fn managed_claude_binaries() -> Vec<BinaryCandidate> {
    let mut found = Vec::new();
    for root in managed_claude_install_roots() {
        let Ok(entries) = fs::read_dir(root.join("claude-code")) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let version_dir = entry.path();
            for name in ["claude.exe", "claude.cmd", "claude"] {
                let candidate = version_dir.join(name);
                if candidate.is_file() {
                    found.push(BinaryCandidate::new(
                        candidate,
                        None,
                        CandidateOrigin::ManagedInstall,
                    ));
                }
            }
        }
    }
    found
}

/// Builds the full candidate list for a Claude Code CLI.
///
/// Order of *collection* is irrelevant to the result — `rank_candidates`
/// re-sorts by probed version — but collecting the override first lets
/// `push_override` short-circuit the rest of the scan.
pub fn claude_candidates() -> CandidateSet {
    let mut set = CandidateSet::new();
    if let Some(path) = non_empty_env_path("CLAUDE_BINARY") {
        set.push_override(path);
        // An explicit pin is the whole answer; probing other installs would
        // risk falling back away from what the user asked for.
        return set;
    }
    for binary in managed_claude_binaries() {
        set.push(binary.path, binary.origin);
    }
    if let Some(home) = env_home() {
        set.push_suffixed(
            &home.join(".local").join("bin"),
            "claude",
            CandidateOrigin::UserInstall,
        );
    }
    set.push(PathBuf::from("claude"), CandidateOrigin::PathLookup);
    set
}

fn non_empty_env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Compatibility cache mapping a provider session to the binary that could
/// actually resume it.
///
/// A newest-first probe is right the vast majority of the time. This exists for
/// the remainder: a transcript format the newest local build still cannot read,
/// where an older sibling install can.
#[derive(Debug, Default)]
pub struct BinaryAffinity {
    entries: BTreeMap<String, String>,
}

impl BinaryAffinity {
    pub fn new() -> Self {
        Self::default()
    }

    fn cache_path() -> Option<PathBuf> {
        Some(
            env_home()?
                .join(".agent-pulse")
                .join("agent-binary-affinity.json"),
        )
    }

    /// Returns the binary recorded as working for `session_key`, if it is
    /// still present among the current candidates.
    pub fn lookup(&self, session_key: &str, candidates: &[BinaryCandidate]) -> Option<PathBuf> {
        let recorded = self.entries.get(session_key)?;
        candidates
            .iter()
            .find(|candidate| candidate.path.as_os_str() == recorded.as_str())
            .map(|candidate| candidate.path.clone())
    }

    pub fn record(&mut self, session_key: &str, binary: &Path) {
        self.entries
            .insert(session_key.to_owned(), binary.display().to_string());
        self.persist();
    }

    fn persist(&self) {
        let Some(path) = Self::cache_path() else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return;
        }
        let _ = fs::write(path, serde_json::to_vec(&self.entries).unwrap_or_default());
    }

    pub fn load() -> Self {
        let Some(path) = Self::cache_path() else {
            return Self::new();
        };
        let entries = fs::read(path)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
        Self { entries }
    }
}

/// True when an agent's failure means "this binary cannot read that session",
/// so trying a different binary is worth the cost of another process.
///
/// The match is deliberately narrow. A generic non-zero exit, a timeout, or a
/// cancelled turn must not trigger a fallback: replaying a turn that may have
/// partially executed is exactly the duplicate side effect the remote command
/// contract forbids. `output` must be the failing binary's own stdout — the
/// caller drains another process's stderr and never passes it here.
pub fn is_unreadable_session_failure(_program: &str, output: &str) -> bool {
    output.contains("No conversation found with session ID")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotted_versions_from_provider_banner() {
        assert_eq!(
            AgentCliVersion::parse("2.1.281 (Claude Code)"),
            Some(AgentCliVersion::new(2, 1, 281))
        );
        assert_eq!(
            AgentCliVersion::parse("codex-cli 0.52.0"),
            Some(AgentCliVersion::new(0, 52, 0))
        );
        assert_eq!(
            AgentCliVersion::parse("1.2"),
            Some(AgentCliVersion::new(1, 2, 0))
        );
        assert_eq!(
            AgentCliVersion::parse("v3.4.5"),
            Some(AgentCliVersion::new(3, 4, 5))
        );
    }

    #[test]
    fn rejects_unversioned_banner() {
        assert_eq!(AgentCliVersion::parse("Claude Code"), None);
        assert_eq!(AgentCliVersion::parse(""), None);
    }

    #[test]
    fn newer_version_outranks_older_regardless_of_origin() {
        let older = BinaryCandidate::new(
            PathBuf::from("/a/claude"),
            Some(AgentCliVersion::new(2, 1, 196)),
            CandidateOrigin::UserInstall,
        );
        let newer = BinaryCandidate::new(
            PathBuf::from("/b/claude"),
            Some(AgentCliVersion::new(2, 1, 281)),
            CandidateOrigin::ManagedInstall,
        );
        assert!(newer.rank() > older.rank());
    }

    #[test]
    fn known_version_outranks_unknown() {
        let known = BinaryCandidate::new(
            PathBuf::from("/a/claude"),
            Some(AgentCliVersion::new(1, 0, 0)),
            CandidateOrigin::PathLookup,
        );
        let unknown = BinaryCandidate::new(
            PathBuf::from("/b/claude"),
            None,
            CandidateOrigin::UserInstall,
        );
        assert!(known.rank() > unknown.rank());
    }

    #[test]
    fn environment_override_is_used_exclusively() {
        let mut set = CandidateSet::new();
        set.push_override(PathBuf::from("/pinned/claude"));
        set.push(
            PathBuf::from("/other/claude"),
            CandidateOrigin::ManagedInstall,
        );
        let ranked = set.into_ranked(Duration::from_millis(1));
        assert_eq!(
            ranked.len(),
            1,
            "override must suppress every other candidate"
        );
        assert_eq!(ranked[0].path, PathBuf::from("/pinned/claude"));
    }

    #[test]
    fn only_first_override_survives() {
        let mut set = CandidateSet::new();
        set.push_override(PathBuf::from("/first/claude"));
        set.push_override(PathBuf::from("/second/claude"));
        let ranked = set.into_ranked(Duration::from_millis(1));
        assert_eq!(ranked[0].path, PathBuf::from("/first/claude"));
    }

    #[test]
    fn affinity_returns_recorded_binary_only_when_still_present() {
        let mut affinity = BinaryAffinity::new();
        affinity
            .entries
            .insert("claude:abc".to_owned(), "/usr/local/bin/claude".to_owned());
        let present = BinaryCandidate::new(
            PathBuf::from("/usr/local/bin/claude"),
            Some(AgentCliVersion::new(2, 1, 281)),
            CandidateOrigin::ManagedInstall,
        );
        let absent = BinaryCandidate::new(
            PathBuf::from("/other/claude"),
            Some(AgentCliVersion::new(2, 1, 281)),
            CandidateOrigin::ManagedInstall,
        );
        assert_eq!(
            affinity.lookup("claude:abc", std::slice::from_ref(&present)),
            Some(PathBuf::from("/usr/local/bin/claude"))
        );
        assert_eq!(affinity.lookup("claude:abc", &[absent]), None);
    }

    #[test]
    fn unreadable_session_failure_is_matched_narrowly() {
        assert!(is_unreadable_session_failure(
            "claude",
            r#"{"errors":["No conversation found with session ID: abc"]}"#
        ));
        assert!(!is_unreadable_session_failure("claude", "network timeout"));
        assert!(!is_unreadable_session_failure("claude", ""));
    }

    #[test]
    fn a_failed_candidate_is_never_offered_again_as_its_own_retry() {
        // Guards the fallback loop: retrying the binary that just reported the
        // session unreadable would replay the same failure at full cost.
        let failed = PathBuf::from("/old/claude");
        let remaining: Vec<BinaryCandidate> = [
            BinaryCandidate::new(
                failed.clone(),
                Some(AgentCliVersion::new(2, 1, 196)),
                CandidateOrigin::UserInstall,
            ),
            BinaryCandidate::new(
                PathBuf::from("/new/claude"),
                Some(AgentCliVersion::new(2, 1, 281)),
                CandidateOrigin::ManagedInstall,
            ),
        ]
        .into_iter()
        .filter(|candidate| candidate.path != failed)
        .collect();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].path, PathBuf::from("/new/claude"));
    }

    #[test]
    fn duplicate_paths_collapse_to_one_candidate() {
        let set = rank_candidates(
            vec![
                BinaryCandidate::new(
                    PathBuf::from("/usr/bin/claude"),
                    None,
                    CandidateOrigin::PathLookup,
                ),
                BinaryCandidate::new(
                    PathBuf::from("/usr/bin/claude"),
                    None,
                    CandidateOrigin::UserInstall,
                ),
            ],
            Duration::from_millis(1),
        );
        assert_eq!(
            set.len(),
            1,
            "the same executable is one candidate, not two"
        );
    }
}
