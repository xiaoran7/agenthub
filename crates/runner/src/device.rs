use std::{fs, io::Write, path::Path};

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

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

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct PersistedDeviceIdentity {
    version: u16,
    device_id: String,
    public_key: String,
    private_key: String,
    #[zeroize(skip)]
    created_at: DateTime<Utc>,
}

#[derive(Clone)]
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
        let contents = Zeroizing::new(fs::read_to_string(path)?);
        let persisted: PersistedDeviceIdentity = serde_json::from_str(&contents)?;
        if persisted.version != IDENTITY_VERSION {
            return Err(DeviceIdentityError::UnsupportedVersion(persisted.version));
        }

        let mut private_key = decode_array::<32>(&persisted.private_key)?;
        let expected_public_key = decode_array::<32>(&persisted.public_key)?;
        let signing_key = SigningKey::from_bytes(&private_key);
        private_key.zeroize();
        let actual_public_key = signing_key.verifying_key().to_bytes();
        if actual_public_key != expected_public_key {
            return Err(DeviceIdentityError::PublicKeyMismatch);
        }
        if device_id_for_public_key(&actual_public_key) != persisted.device_id {
            return Err(DeviceIdentityError::DeviceIdMismatch);
        }

        Ok(Self {
            device_id: persisted.device_id.clone(),
            signing_key,
            created_at: persisted.created_at,
        })
    }

    fn serialize_persisted(&self) -> Result<Zeroizing<Vec<u8>>, DeviceIdentityError> {
        let mut raw_key = self.signing_key.to_bytes();
        let private_key = hex::encode(raw_key);
        raw_key.zeroize();
        let persisted = PersistedDeviceIdentity {
            version: IDENTITY_VERSION,
            device_id: self.device_id.clone(),
            public_key: self.public_key_hex(),
            private_key,
            created_at: self.created_at,
        };
        let mut serialized = serde_json::to_vec_pretty(&persisted)?;
        serialized.push(b'\n');
        Ok(Zeroizing::new(serialized))
    }

    pub fn save_new(&self, path: impl AsRef<Path>) -> Result<(), DeviceIdentityError> {
        let serialized = self.serialize_persisted()?;
        write_private_file(path.as_ref(), &serialized, ".identity-", false)?;
        Ok(())
    }

    pub fn overwrite(&self, path: impl AsRef<Path>) -> Result<(), DeviceIdentityError> {
        let path = path.as_ref();
        if path.exists() {
            let ts = Utc::now().timestamp();
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("identity.json");
            let backup_name = format!("{file_name}.revoked-{ts}.bak");
            let backup_path = path.with_file_name(backup_name);
            if let Ok(old_bytes) = fs::read(path).map(Zeroizing::new) {
                let _ = write_private_file(&backup_path, &old_bytes, ".identity-bak-", true);
            }
        }
        let serialized = self.serialize_persisted()?;
        write_private_file(path, &serialized, ".identity-", true)?;
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

    pub fn sign(&self, message: &[u8]) -> Signature {
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

/// Write sensitive material into a freshly created private temp file (owner-only
/// DACL on Windows via SDDL protected inheritance, `0600` on Unix) and promote
/// it atomically so secrets are never briefly world-readable.
pub(crate) fn write_private_file(
    path: &Path,
    bytes: &[u8],
    prefix: &str,
    clobber: bool,
) -> std::io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    #[cfg(unix)]
    if !directory.exists() {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(directory)?;

    let mut suffix = [0_u8; 8];
    OsRng.fill_bytes(&mut suffix);
    let temp_path = directory.join(format!("{prefix}{}.tmp", hex::encode(suffix)));

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::{FromRawHandle, RawHandle};
        use windows_sys::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
        use windows_sys::Win32::Storage::FileSystem::{
            CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, MOVEFILE_REPLACE_EXISTING,
            MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let descriptor = crate::daemon_ipc::platform::current_user_descriptor()
            .map_err(std::io::Error::other)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let temp_wide: Vec<u16> = temp_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                temp_wide.as_ptr(),
                GENERIC_WRITE,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let write_result = (|| -> std::io::Result<()> {
            let mut file = unsafe { fs::File::from_raw_handle(handle as RawHandle) };
            file.write_all(bytes)?;
            file.sync_all()?;
            Ok(())
        })();
        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(err);
        }
        let target_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let flags = if clobber {
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
        } else {
            0
        };
        if unsafe { MoveFileExW(temp_wide.as_ptr(), target_wide.as_ptr(), flags) } == 0 {
            let err = std::io::Error::last_os_error();
            let _ = fs::remove_file(&temp_path);
            return Err(err);
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp_path)?;
        let write_result = (|| -> std::io::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            Ok(())
        })();
        drop(file);
        if let Err(err) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(err);
        }
        if clobber {
            if let Err(err) = fs::rename(&temp_path, path) {
                let _ = fs::remove_file(&temp_path);
                return Err(err);
            }
        } else {
            let link_res = fs::hard_link(&temp_path, path);
            let _ = fs::remove_file(&temp_path);
            link_res?;
        }
    }

    sync_directory(directory);
    Ok(())
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

    #[test]
    fn identity_file_can_be_explicitly_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("identity.json");
        let first = DeviceIdentity::generate();
        first.save_new(&path).unwrap();
        assert_eq!(DeviceIdentity::load(&path).unwrap().info(), first.info());

        let second = DeviceIdentity::generate();
        second.overwrite(&path).unwrap();
        assert_eq!(DeviceIdentity::load(&path).unwrap().info(), second.info());
        assert_ne!(first.info().device_id, second.info().device_id);

        let backups: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".revoked-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            DeviceIdentity::load(backups[0].path()).unwrap().info(),
            first.info()
        );
    }

    #[test]
    fn private_file_writer_creates_nested_dirs_and_respects_clobber() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested").join("secret.bin");
        write_private_file(&nested, b"v1", ".test-", false).unwrap();
        assert_eq!(fs::read(&nested).unwrap(), b"v1");
        let err = write_private_file(&nested, b"v2", ".test-", false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        write_private_file(&nested, b"v2", ".test-", true).unwrap();
        assert_eq!(fs::read(&nested).unwrap(), b"v2");
    }
}
