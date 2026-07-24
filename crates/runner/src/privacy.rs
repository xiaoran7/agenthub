use std::path::Path;

use hmac::{Hmac, Mac};
use sha2::Sha256;
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

fn normalize_path_for_fingerprint(raw_cwd: &str) -> String {
    let normalized = raw_cwd.trim().replace('\\', "/");
    #[cfg(windows)]
    {
        normalized.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

fn project_display_name(raw_cwd: &str) -> String {
    let trimmed = raw_cwd.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return "unknown-project".into();
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
}
