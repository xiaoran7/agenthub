use std::path::Path;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::domain::ProjectRef;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum PrivacyError {
    #[error("fingerprint key cannot be empty")]
    EmptyFingerprintKey,
}

#[derive(Clone)]
pub struct ProjectRedactor {
    fingerprint_key: Vec<u8>,
}

impl ProjectRedactor {
    pub fn new(fingerprint_key: impl AsRef<[u8]>) -> Result<Self, PrivacyError> {
        let fingerprint_key = fingerprint_key.as_ref();
        if fingerprint_key.is_empty() {
            return Err(PrivacyError::EmptyFingerprintKey);
        }
        Ok(Self {
            fingerprint_key: fingerprint_key.to_vec(),
        })
    }

    pub fn redact(&self, raw_cwd: &str) -> ProjectRef {
        let normalized = normalize_path_for_fingerprint(raw_cwd);
        let display_name = project_display_name(raw_cwd);
        let mut mac =
            HmacSha256::new_from_slice(&self.fingerprint_key).expect("HMAC accepts any key length");
        mac.update(normalized.as_bytes());

        ProjectRef {
            display_name,
            fingerprint: hex::encode(mac.finalize().into_bytes()),
        }
    }
}

pub fn workspace_id_for_path(raw_cwd: &str) -> String {
    let normalized = normalize_path_for_fingerprint(raw_cwd);
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"agenthub_workspace_v1:");
    hasher.update(normalized.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let display = project_display_name(raw_cwd);
    let safe_display: String = display
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(24)
        .collect();
    format!("ws-{safe_display}-{}", &hash[..16])
}

fn normalize_path_for_fingerprint(raw_cwd: &str) -> String {
    let mut normalized = raw_cwd
        .trim()
        .trim_end_matches(['/', '\\'])
        .replace('\\', "/");
    if normalized.starts_with("//?/") {
        normalized = normalized[4..].to_string();
    }
    #[cfg(windows)]
    {
        normalized.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

pub fn project_display_name(raw_cwd: &str) -> String {
    let trimmed = raw_cwd.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return "unknown-project".into();
    }
    // A session started directly in the home directory would otherwise use the
    // account name as its display name; the username is not project metadata.
    if is_current_user_home(trimmed) {
        return "home".into();
    }

    Path::new(trimmed)
        .file_name()
        .and_then(|part| part.to_str())
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            trimmed
                .rsplit(['/', '\\'])
                .find(|part| !part.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown-project".into())
}

/// Recursively traverses a JSON Value and replaces any SessionBinding.workspace_path,
/// candidate workspace_path, cwd, workdir, or physical path fields with safe workspace_id.
pub fn sanitize_workspace_paths_in_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let is_path_key = key == "workspace_path"
                    || key == "cwd"
                    || key == "workdir"
                    || key == "working_dir"
                    || key == "working_directory"
                    || key == "project_path"
                    || key == "root_path";
                if is_path_key
                    && let Some(path_str) = val.as_str()
                    && !path_str.starts_with("ws-")
                {
                    *val = serde_json::Value::String(workspace_id_for_path(path_str));
                }
                sanitize_workspace_paths_in_json(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                sanitize_workspace_paths_in_json(item);
            }
        }
        _ => {}
    }
}

fn is_current_user_home(trimmed_cwd: &str) -> bool {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    home.is_some_and(|home| matches_home(trimmed_cwd, home.to_string_lossy().as_ref()))
}

fn matches_home(trimmed_cwd: &str, home: &str) -> bool {
    let home = home.trim().trim_end_matches(['/', '\\']);
    !home.is_empty()
        && normalize_path_for_fingerprint(trimmed_cwd) == normalize_path_for_fingerprint(home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_full_path_and_keeps_project_name() {
        let redactor = ProjectRedactor::new(b"device-secret").unwrap();
        let project = redactor.redact(r"C:\Users\alice\source\payments-api");

        assert_eq!(project.display_name, "payments-api");
        assert_eq!(project.fingerprint.len(), 64);
        assert!(!project.fingerprint.contains("alice"));
    }

    #[test]
    fn fingerprints_are_stable_for_the_same_path() {
        let redactor = ProjectRedactor::new(b"device-secret").unwrap();
        assert_eq!(
            redactor.redact("/work/app").fingerprint,
            redactor.redact("/work/app").fingerprint
        );
    }

    #[test]
    fn home_directory_is_not_used_as_a_display_name() {
        assert!(matches_home(r"C:\Users\alice", r"C:\Users\alice"));
        #[cfg(windows)]
        assert!(matches_home(r"C:\Users\Alice", r"c:/users/alice"));
        assert!(!matches_home(r"C:\Users\alice\project", r"C:\Users\alice"));
        assert!(!matches_home(r"C:\Users\alice", ""));
    }

    #[test]
    fn workspace_id_is_stable_and_does_not_reveal_full_path() {
        let ws1 = workspace_id_for_path(r"C:\Users\alice\code\my-app");
        let ws2 = workspace_id_for_path(r"c:\users\alice\code\my-app\");
        assert_eq!(ws1, ws2);
        assert!(ws1.starts_with("ws-my-app-"));
        assert!(!ws1.contains("alice"));
        assert!(!ws1.contains("code"));
    }

    #[test]
    fn sanitize_workspace_paths_in_json_replaces_cwd_and_nested_paths() {
        let mut sample = serde_json::json!({
            "type": "data",
            "provider_response": {
                "thread": {
                    "id": "thread-1",
                    "cwd": "C:\\Users\\alice\\projects\\super-app"
                }
            },
            "events": [
                {
                    "raw": {
                        "params": {
                            "cwd": "/home/bob/secret-repo",
                            "turn": {
                                "id": "turn-1"
                            }
                        }
                    },
                    "binding": {
                        "workspace_path": "D:\\Workspaces\\team-portal"
                    }
                }
            ]
        });
        sanitize_workspace_paths_in_json(&mut sample);
        let serialized = sample.to_string();
        assert!(!serialized.contains("alice"));
        assert!(!serialized.contains("bob"));
        assert!(!serialized.contains("Users"));
        assert!(!serialized.contains("home"));
        assert!(!serialized.contains("D:\\\\Workspaces"));
        assert_eq!(
            sample["provider_response"]["thread"]["cwd"]
                .as_str()
                .unwrap(),
            workspace_id_for_path("C:\\Users\\alice\\projects\\super-app")
        );
        assert_eq!(
            sample["events"][0]["raw"]["params"]["cwd"]
                .as_str()
                .unwrap(),
            workspace_id_for_path("/home/bob/secret-repo")
        );
        assert_eq!(
            sample["events"][0]["binding"]["workspace_path"]
                .as_str()
                .unwrap(),
            workspace_id_for_path("D:\\Workspaces\\team-portal")
        );
    }
}
