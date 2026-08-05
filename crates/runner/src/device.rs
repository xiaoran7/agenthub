use std::{fs, io::Write, path::Path};

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const IDENTITY_VERSION: u16 = 1;

#[derive(Debug, Error)]
pub enum DeviceIdentityError {
    #[error("failed to access device identity: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid device identity JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid device identity key encoding")]
    InvalidKeyEncoding,
    #[error("unsupported device identity version {0}")]
    UnsupportedVersion(u16),
    #[error("device identity public key does not match its private key")]
    PublicKeyMismatch,
    #[error("device identity ID does not match its public key")]
    DeviceIdMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceIdentityInfo {
    pub version: u16,
    pub device_id: String,
    pub public_key: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct PersistedDeviceIdentity {
    version: u16,
    device_id: String,
    public_key: String,
    private_key: String,
    created_at: DateTime<Utc>,
}

pub struct DeviceIdentity {
    device_id: String,
    signing_key: SigningKey,
    created_at: DateTime<Utc>,
}

impl DeviceIdentity {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let device_id = device_id_for_public_key(&signing_key.verifying_key().to_bytes());
        Self {
            device_id,
            signing_key,
            created_at: Utc::now(),
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, DeviceIdentityError> {
        let contents = fs::read_to_string(path)?;
        let persisted: PersistedDeviceIdentity = serde_json::from_str(&contents)?;
        if persisted.version != IDENTITY_VERSION {
            return Err(DeviceIdentityError::UnsupportedVersion(persisted.version));
        }

        let private_key = decode_array::<32>(&persisted.private_key)?;
        let expected_public_key = decode_array::<32>(&persisted.public_key)?;
        let signing_key = SigningKey::from_bytes(&private_key);
        let actual_public_key = signing_key.verifying_key().to_bytes();
        if actual_public_key != expected_public_key {
            return Err(DeviceIdentityError::PublicKeyMismatch);
        }
        if device_id_for_public_key(&actual_public_key) != persisted.device_id {
            return Err(DeviceIdentityError::DeviceIdMismatch);
        }

        Ok(Self {
            device_id: persisted.device_id,
            signing_key,
            created_at: persisted.created_at,
        })
    }

    pub fn save_new(&self, path: impl AsRef<Path>) -> Result<(), DeviceIdentityError> {
        let path = path.as_ref();
        let persisted = PersistedDeviceIdentity {
            version: IDENTITY_VERSION,
            device_id: self.device_id.clone(),
            public_key: self.public_key_hex(),
            private_key: hex::encode(self.signing_key.to_bytes()),
            created_at: self.created_at,
        };
        let mut serialized = serde_json::to_vec_pretty(&persisted)?;
        serialized.push(b'\n');

        // Write to a same-directory temp file and link it into place without
        // replacing an existing identity: a crash mid-write must never leave a
        // truncated identity file at the final path. tempfile creates the temp
        // file with 0600 on Unix.
        let directory = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let mut temp_file = tempfile::Builder::new()
            .prefix(".identity-")
            .suffix(".tmp")
            .tempfile_in(directory)?;
        temp_file.write_all(&serialized)?;
        temp_file.as_file().sync_all()?;
        temp_file
            .persist_noclobber(path)
            .map_err(|error| DeviceIdentityError::Io(error.error))?;
        sync_directory(directory);
        Ok(())
    }

    pub fn info(&self) -> DeviceIdentityInfo {
        DeviceIdentityInfo {
            version: IDENTITY_VERSION,
            device_id: self.device_id.clone(),
            public_key: self.public_key_hex(),
            created_at: self.created_at,
        }
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.signing_key.verifying_key().to_bytes())
    }

    pub fn fingerprint_key(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        // Keep the fingerprint namespace stable so existing devices retain their identity
        // after the AgentPulse product rename.
        digest.update(b"agent-platform/project-fingerprint/v1");
        digest.update(self.signing_key.to_bytes());
        digest.finalize().into()
    }

    /// Derive a separate local-only key for the encrypted credential cache.
    /// It is intentionally namespaced away from the project fingerprint key so
    /// a cache compromise cannot be used as a project identity oracle.
    pub fn credential_cache_key(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"agent-pulse/local-credential-cache/v1");
        digest.update(self.signing_key.to_bytes());
        digest.finalize().into()
    }

    pub(crate) fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }
}

pub(crate) fn device_id_for_public_key(public_key: &[u8; 32]) -> String {
    let digest = Sha256::digest(public_key);
    format!("dev_{}", hex::encode(&digest[..16]))
}

pub(crate) fn verifying_key_from_hex(value: &str) -> Result<VerifyingKey, DeviceIdentityError> {
    VerifyingKey::from_bytes(&decode_array::<32>(value)?)
        .map_err(|_| DeviceIdentityError::InvalidKeyEncoding)
}

pub(crate) fn signature_from_hex(value: &str) -> Result<Signature, DeviceIdentityError> {
    Ok(Signature::from_bytes(&decode_array::<64>(value)?))
}

fn decode_array<const N: usize>(value: &str) -> Result<[u8; N], DeviceIdentityError> {
    hex::decode(value)
        .map_err(|_| DeviceIdentityError::InvalidKeyEncoding)?
        .try_into()
        .map_err(|_| DeviceIdentityError::InvalidKeyEncoding)
}

/// Flush the directory entry so a power loss immediately after `save_new`
/// cannot lose the freshly linked identity file. Windows cannot fsync
/// directories, so this is best-effort and Unix-only.
fn sync_directory(directory: &Path) {
    #[cfg(unix)]
    {
        if let Ok(handle) = fs::File::open(directory) {
            let _ = handle.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trips_without_exposing_private_key_in_info() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        let identity = DeviceIdentity::generate();
        identity.save_new(&path).unwrap();

        let loaded = DeviceIdentity::load(&path).unwrap();
        assert_eq!(loaded.info(), identity.info());

        let public_info = serde_json::to_string(&loaded.info()).unwrap();
        let persisted = fs::read_to_string(path).unwrap();
        let private_key = serde_json::from_str::<serde_json::Value>(&persisted)
            .unwrap()
            .get("private_key")
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!public_info.contains(&private_key));
    }

    #[test]
    fn identity_file_cannot_be_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        let first = DeviceIdentity::generate();
        first.save_new(&path).unwrap();

        let second = DeviceIdentity::generate();
        assert!(matches!(
            second.save_new(&path),
            Err(DeviceIdentityError::Io(error))
                if error.kind() == std::io::ErrorKind::AlreadyExists
        ));
        assert_eq!(DeviceIdentity::load(path).unwrap().info(), first.info());
    }

    #[test]
    fn tampered_device_id_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        DeviceIdentity::generate().save_new(&path).unwrap();
        let mut persisted: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        persisted["device_id"] = serde_json::Value::String("dev_tampered".into());
        fs::write(&path, serde_json::to_vec(&persisted).unwrap()).unwrap();

        assert!(matches!(
            DeviceIdentity::load(path),
            Err(DeviceIdentityError::DeviceIdMismatch)
        ));
    }
}
