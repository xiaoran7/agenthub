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

pub fn is_absolute_filesystem_path(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.starts_with("ws-") {
        return false;
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
    {
        return true;
    }
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        return true;
    }
    if trimmed.starts_with('/') && trimmed[1..].contains('/') {
        return true;
    }
    false
}

/// Recursively traverses a JSON Value and enforces that no absolute physical filesystem paths
/// leak to the remote Gateway. Any string matching an absolute filesystem path or explicit path field
/// is securely transformed into an irreversible workspace_id.
pub fn sanitize_workspace_paths_in_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                let normalized_key = key
                    .chars()
                    .filter(|character| character.is_ascii_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                let is_sensitive_key = normalized_key == "env"
                    || normalized_key.contains("environment")
                    || normalized_key.contains("password")
                    || normalized_key.contains("passwd")
                    || normalized_key.contains("cookie")
                    || normalized_key.contains("authorization")
                    || normalized_key.contains("apikey")
                    || normalized_key == "token"
                    || normalized_key.ends_with("token")
                    || normalized_key.contains("accesstoken")
                    || normalized_key.contains("refreshtoken")
                    || normalized_key.contains("privkey")
                    || normalized_key.contains("privatekey")
                    || normalized_key == "secret"
                    || normalized_key.ends_with("secret");
                if is_sensitive_key {
                    *val = serde_json::Value::String("[redacted]".into());
                    continue;
                }
                let is_path_key = key == "workspace_path"
                    || key == "cwd"
                    || key == "workdir"
                    || key == "working_dir"
                    || key == "working_directory"
                    || key == "project_path"
                    || key == "root_path"
                    || key == "path"
                    || key == "file_path"
                    || key == "filePath";
                if let Some(path_str) = val.as_str() {
                    if (is_path_key || is_absolute_filesystem_path(path_str))
                        && !path_str.starts_with("ws-")
                        && !path_str.trim().is_empty()
                    {
                        *val = serde_json::Value::String(workspace_id_for_path(path_str));
                    } else if contains_absolute_filesystem_path(path_str) {
                        *val = serde_json::Value::String("[local_path_redacted]".into());
                    }
                }
                sanitize_workspace_paths_in_json(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                if let Some(path_str) = item.as_str() {
                    if is_absolute_filesystem_path(path_str) && !path_str.starts_with("ws-") {
                        *item = serde_json::Value::String(workspace_id_for_path(path_str));
                    } else if contains_absolute_filesystem_path(path_str) {
                        *item = serde_json::Value::String("[local_path_redacted]".into());
                    }
                } else {
                    sanitize_workspace_paths_in_json(item);
                }
            }
        }
        _ => {}
    }
}

fn contains_absolute_filesystem_path(value: &str) -> bool {
    value
        .split(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}'
                )
        })
        .map(|token| token.trim_start_matches([':', '=', ',', ';']))
        .any(is_absolute_filesystem_path)
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

    #[test]
    fn sanitize_workspace_paths_in_json_redacts_embedded_paths_and_credentials() {
        let mut sample = serde_json::json!({
            "command": "cat C:\\Users\\alice\\repo\\.env",
            "stderr": "failed at /home/alice/private/config.json",
            "url": "https://example.com/private/resource",
            "Authorization": "Bearer usable-secret",
            "access_token": "usable-token",
            "environment": {"SAFE": "value"}
        });

        sanitize_workspace_paths_in_json(&mut sample);

        assert_eq!(sample["command"], "[local_path_redacted]");
        assert_eq!(sample["stderr"], "[local_path_redacted]");
        assert_eq!(sample["url"], "https://example.com/private/resource");
        assert_eq!(sample["Authorization"], "[redacted]");
        assert_eq!(sample["access_token"], "[redacted]");
        assert_eq!(sample["environment"], "[redacted]");
        let serialized = sample.to_string();
        for forbidden in ["alice", "usable-secret", "usable-token"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn sanitize_workspace_paths_in_json_cleans_tool_events_and_arbitrary_paths() {
        let mut sample = serde_json::json!({
            "type": "event",
            "event": {
                "kind": "item/fileChange/requestApproval",
                "raw": {
                    "method": "item/fileChange/requestApproval",
                    "params": {
                        "filePath": "C:\\Users\\admin\\repo\\src\\main.rs",
                        "diff": "--- a/src/main.rs\n+++ b/src/main.rs",
                        "files": [
                            "/Users/developer/code/backend/server.go",
                            "D:\\Shared\\docs\\spec.pdf"
                        ]
                    }
                }
            }
        });
        sanitize_workspace_paths_in_json(&mut sample);
        let serialized = sample.to_string();
        assert!(!serialized.contains("admin"));
        assert!(!serialized.contains("developer"));
        assert!(!serialized.contains("C:\\\\Users"));
        assert!(!serialized.contains("/Users/"));
        assert!(!serialized.contains("D:\\\\Shared"));
        assert_eq!(
            sample["event"]["raw"]["params"]["filePath"]
                .as_str()
                .unwrap(),
            workspace_id_for_path("C:\\Users\\admin\\repo\\src\\main.rs")
        );
        assert_eq!(
            sample["event"]["raw"]["params"]["files"][0]
                .as_str()
                .unwrap(),
            workspace_id_for_path("/Users/developer/code/backend/server.go")
        );
        assert_eq!(
            sample["event"]["raw"]["params"]["files"][1]
                .as_str()
                .unwrap(),
            workspace_id_for_path("D:\\Shared\\docs\\spec.pdf")
        );
    }
}
