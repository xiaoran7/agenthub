use super::*;
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedCacheFile {
    version: u16,
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize, Deserialize, Default)]
struct CachePayload {
    version: u16,
    entries: BTreeMap<AuthProvider, TokenSet>,
}

pub(crate) struct CredentialCache {
    path: PathBuf,
    key: [u8; 32],
}

impl Drop for CredentialCache {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl CredentialCache {
    pub(crate) fn new(path: impl Into<PathBuf>, key: [u8; 32]) -> Self {
        Self {
            path: path.into(),
            key,
        }
    }

    fn load(&self) -> Result<CachePayload, AuthError> {
        let mut bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CachePayload {
                    version: CACHE_VERSION,
                    entries: BTreeMap::new(),
                });
            }
            Err(error) => return Err(AuthError::CacheIo(error)),
        };
        let envelope_res: Result<EncryptedCacheFile, _> = serde_json::from_slice(&bytes);
        bytes.zeroize();
        let envelope = envelope_res?;
        if envelope.version != CACHE_VERSION {
            return Err(AuthError::UnsupportedCacheVersion(envelope.version));
        }
        let nonce_bytes = hex::decode(envelope.nonce).map_err(|_| AuthError::CacheCrypto)?;
        let cipher_bytes = hex::decode(envelope.ciphertext).map_err(|_| AuthError::CacheCrypto)?;
        if nonce_bytes.len() != 12 {
            return Err(AuthError::CacheCrypto);
        }
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| AuthError::CacheCrypto)?;
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(Nonce::from_slice(&nonce_bytes), cipher_bytes.as_ref())
                .map_err(|_| AuthError::CacheCrypto)?,
        );
        let payload: CachePayload = serde_json::from_slice(&plaintext)?;
        if payload.version != CACHE_VERSION {
            return Err(AuthError::UnsupportedCacheVersion(payload.version));
        }
        Ok(payload)
    }

    fn save(&self, payload: &CachePayload) -> Result<(), AuthError> {
        let plaintext = Zeroizing::new(serde_json::to_vec(payload)?);
        let mut nonce_bytes = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| AuthError::CacheCrypto)?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
            .map_err(|_| AuthError::CacheCrypto)?;
        let envelope = EncryptedCacheFile {
            version: CACHE_VERSION,
            nonce: hex::encode(nonce_bytes),
            ciphertext: hex::encode(ciphertext),
        };
        let bytes = Zeroizing::new(serde_json::to_vec_pretty(&envelope)?);
        crate::device::write_private_file(&self.path, &bytes, ".auth-cache-", true)
            .map_err(AuthError::CacheIo)?;
        Ok(())
    }

    pub(crate) fn get(&self, provider: AuthProvider) -> Result<Option<TokenSet>, AuthError> {
        Ok(self.load()?.entries.get(&provider).cloned())
    }

    pub(crate) fn put(&self, provider: AuthProvider, tokens: TokenSet) -> Result<(), AuthError> {
        let mut payload = self.load()?;
        payload.entries.insert(provider, tokens);
        self.save(&payload)
    }

    pub(crate) fn remove(&self, provider: AuthProvider) -> Result<(), AuthError> {
        let mut payload = self.load()?;
        payload.entries.remove(&provider);
        self.save(&payload)
    }
}
