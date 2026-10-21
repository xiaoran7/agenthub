//! Human-facing name for the machine this Runner runs on.
//!
//! The device id stays a backend concern: it is a key, not a label. The
//! console shows this name instead, so the sidebar reads like a list of
//! computers rather than a list of identifiers.

use std::path::{Path, PathBuf};

use serde_json::json;

/// Longest name the Gateway will accept. Keeps a hostile hostname from
/// bloating every snapshot the console polls.
const MAX_DEVICE_NAME_LEN: usize = 64;

/// Where the console's device label comes from.
///
/// The label is resolved again on every publish rather than captured once at
/// startup, so a rename takes effect without restarting the Runner.
#[derive(Clone, Debug, Default)]
pub struct DeviceNameSource {
    /// A name given for this run; it wins over anything stored on disk.
    configured: Option<String>,
    /// The identity file the stored name lives beside.
    identity_path: Option<PathBuf>,
}

impl DeviceNameSource {
    pub fn new(configured: Option<String>, identity_path: Option<PathBuf>) -> Self {
        Self {
            configured: configured.map(|name| name.trim().to_owned()),
            identity_path,
        }
    }

    /// The name to publish right now.
    pub fn resolve(&self) -> String {
        resolve(self.configured.as_deref(), self.identity_path.as_deref())
    }
}

/// The name to show for this computer.
///
/// A user-chosen name always wins — first the one stored on disk, then the
/// one given for this run. The stored name comes first because it is what
/// the rename endpoint writes, and a rename has to take effect on the
/// running process. Otherwise the operating system's computer name is used,
/// and only if that is unavailable does the caller fall back to a generic
/// label.
pub fn resolve(configured: Option<&str>, identity_path: Option<&Path>) -> String {
    if let Some(path) = identity_path
        && let Some(name) = read_configured_name(path)
    {
        return truncate(&name);
    }
    if let Some(name) = configured.map(str::trim).filter(|n| !n.is_empty()) {
        return truncate(name);
    }
    system_hostname()
        .map(|name| truncate(&name))
        .unwrap_or_else(|| fallback_device_name().to_owned())
}

/// The name to show when even the hostname is unavailable.
pub fn fallback_device_name() -> &'static str {
    "这台电脑"
}

fn truncate(name: &str) -> String {
    name.chars().take(MAX_DEVICE_NAME_LEN).collect()
}

#[cfg(windows)]
fn system_hostname() -> Option<String> {
    use windows_sys::Win32::System::SystemInformation::{
        COMPUTER_NAME_FORMAT, ComputerNameDnsHostname, GetComputerNameExW,
    };
    const COMPUTER_NAME_FORMAT_VALUE: COMPUTER_NAME_FORMAT = ComputerNameDnsHostname;
    // GetComputerNameExW writes the name plus a NUL terminator.
    let mut buffer = [0u16; 256];
    let mut size = buffer.len() as u32;
    let ok =
        unsafe { GetComputerNameExW(COMPUTER_NAME_FORMAT_VALUE, buffer.as_mut_ptr(), &mut size) };
    if ok == 0 {
        return None;
    }
    // `size` includes the terminator, so the name is the prefix before it.
    let name = String::from_utf16_lossy(&buffer[..size.saturating_sub(1) as usize]);
    let name = name.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

#[cfg(unix)]
fn system_hostname() -> Option<String> {
    // `uname -n` is the portable POSIX answer; libc's gethostname is not
    // available in the unix dependency set of this crate.
    let output = std::process::Command::new("uname")
        .arg("-n")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if name.is_empty() { None } else { Some(name) }
}

#[cfg(not(any(windows, unix)))]
fn system_hostname() -> Option<String> {
    None
}

/// Reads the persisted user-chosen name for this machine, if any.
///
/// The name is stored beside the device identity so it survives restarts
/// without the user retyping it.
pub fn read_configured_name(identity_path: &Path) -> Option<String> {
    let path = config_path(identity_path);
    let content = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let name = value.get("device_name")?.as_str()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

/// Persists the user-chosen name for this machine.
pub fn write_configured_name(identity_path: &Path, name: &str) -> std::io::Result<()> {
    let path = config_path(identity_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let value = json!({ "device_name": name.trim() });
    std::fs::write(
        path,
        serde_json::to_string_pretty(&value).unwrap_or_default(),
    )
}

/// The identity file the Runner uses when no path is given on the command
/// line.
///
/// The device name, the identity and the binding all live together, so a
/// rename through any of those entry points lands on the same file.
pub fn default_identity_path() -> PathBuf {
    home_dir()
        .map(|home| home.join(".agenthub").join("identity.json"))
        .unwrap_or_else(|| PathBuf::from("identity.json"))
}

/// The home directory the Runner keeps its state in.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The configuration file that holds the user-chosen name.
///
/// It lives next to the identity file so a machine's identity and its label
/// stay together.
fn config_path(identity_path: &Path) -> std::path::PathBuf {
    identity_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("device.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_name_wins_over_the_hostname() {
        assert_eq!(resolve(Some("  我的台式机  "), None).as_str(), "我的台式机");
    }

    #[test]
    fn a_stored_name_wins_over_a_configured_name() {
        // The rename endpoint writes the stored name, so it has to win for a
        // rename to take effect on an already-running Runner.
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity.json");
        write_configured_name(&identity, "书房主机").unwrap();
        assert_eq!(
            resolve(Some("命令行名字"), Some(&identity)).as_str(),
            "书房主机"
        );
    }

    #[test]
    fn a_blank_configured_name_falls_back_to_the_stored_one() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity.json");
        write_configured_name(&identity, "书房主机").unwrap();
        assert_eq!(resolve(Some("   "), Some(&identity)).as_str(), "书房主机");
    }

    #[test]
    fn a_stored_name_is_used_when_nothing_else_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity.json");
        write_configured_name(&identity, "客厅机器").unwrap();
        assert_eq!(resolve(None, Some(&identity)).as_str(), "客厅机器");
    }

    #[test]
    fn a_long_name_is_truncated() {
        let long = "电".repeat(200);
        let name = resolve(Some(&long), None);
        assert_eq!(name.chars().count(), MAX_DEVICE_NAME_LEN);
    }

    #[test]
    fn the_configured_name_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity.json");
        assert_eq!(read_configured_name(&identity), None);
        write_configured_name(&identity, "书房主机").unwrap();
        assert_eq!(read_configured_name(&identity).as_deref(), Some("书房主机"));
    }

    #[test]
    fn a_blank_stored_name_reads_back_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity.json");
        write_configured_name(&identity, "   ").unwrap();
        assert_eq!(read_configured_name(&identity), None);
    }

    #[test]
    fn the_source_picks_up_a_rename_without_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let identity = dir.path().join("identity.json");
        let source = DeviceNameSource::new(None, Some(identity.clone()));
        let before = source.resolve();
        write_configured_name(&identity, "书房主机").unwrap();
        assert_ne!(source.resolve(), before);
        assert_eq!(source.resolve().as_str(), "书房主机");
    }
}
