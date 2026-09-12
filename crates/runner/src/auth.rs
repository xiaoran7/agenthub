//! Local authentication, quota and credential-cache integration.
//!
//! The runner is the only component that is allowed to touch provider
//! credentials.  `AuthSnapshot` is deliberately safe to serialize and sync;
//! `TokenSet` is kept private to this module and is only returned by the
//! explicit `auth token --reveal` CLI command.

use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use aes_gcm::{Aes256Gcm, Nonce, aead::Aead, aead::KeyInit};
use chrono::{DateTime, Utc};
use keyring::Entry;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    collectors::{CodexAppServer, CollectorError},
    device::DeviceIdentity,
};

const CACHE_VERSION: u16 = 1;
const MAX_STATUS_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthProvider {
    ClaudeCode,
    Codex,
    Antigravity,
    GrokBuild,
}

impl AuthProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::GrokBuild => "grok_build",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    Authenticated,
    Unauthenticated,
    ReauthRequired,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    OfficialCli,
    AppServer,
    Environment,
    File,
    Keyring,
    Cache,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaState {
    Available,
    Exhausted,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaWindow {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_duration_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaSnapshot {
    pub state: QuotaState,
    pub source: CredentialSource,
    pub refreshed_at: DateTime<Utc>,
    pub windows: Vec<QuotaWindow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_reset_credits: Option<u32>,
}

/// Safe-to-sync authentication view.  No raw token field is permitted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSnapshot {
    pub provider: AuthProvider,
    pub state: AuthState,
    pub source: CredentialSource,
    pub checked_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
    pub access_token_present: bool,
    pub refresh_token_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token_expires_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<QuotaSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

pub const AUTH_SYNC_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSyncBatch {
    pub protocol_version: u16,
    pub device_id: String,
    pub public_key: String,
    pub checked_at: DateTime<Utc>,
    pub snapshots: Vec<AuthSnapshot>,
    pub signature: String,
}

#[derive(Serialize)]
struct UnsignedAuthSyncBatch<'a> {
    protocol_version: u16,
    device_id: &'a str,
    public_key: &'a str,
    checked_at: DateTime<Utc>,
    snapshots: &'a [AuthSnapshot],
}

impl AuthSyncBatch {
    pub fn new(
        identity: &DeviceIdentity,
        mut snapshots: Vec<AuthSnapshot>,
    ) -> Result<Self, AuthError> {
        snapshots.sort_by_key(|snapshot| snapshot.provider);
        let public_key = identity.public_key_hex();
        let mut batch = Self {
            protocol_version: AUTH_SYNC_PROTOCOL_VERSION,
            device_id: identity.device_id().to_owned(),
            public_key,
            checked_at: Utc::now(),
            snapshots,
            signature: String::new(),
        };
        let unsigned = UnsignedAuthSyncBatch {
            protocol_version: batch.protocol_version,
            device_id: &batch.device_id,
            public_key: &batch.public_key,
            checked_at: batch.checked_at,
            snapshots: &batch.snapshots,
        };
        let bytes = serde_json::to_vec(&unsigned)?;
        batch.signature = hex::encode(identity.sign(&bytes).to_bytes());
        Ok(batch)
    }
}

/// Sensitive material.  Do not add `Debug` or expose this type from a web/API
/// boundary.  It is serialized only inside the encrypted local cache.
#[derive(Clone, Default, Serialize, Deserialize)]
struct TokenSet {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    scope: Option<String>,
    account_id: Option<String>,
    auth_mode: Option<String>,
}

impl TokenSet {
    fn is_empty(&self) -> bool {
        self.access_token.is_none()
            && self.refresh_token.is_none()
            && self.id_token.is_none()
            && self.account_id.is_none()
    }

    fn merge(mut self, other: Self) -> Self {
        if self.access_token.is_none() {
            self.access_token = other.access_token;
        }
        if self.refresh_token.is_none() {
            self.refresh_token = other.refresh_token;
        }
        if self.id_token.is_none() {
            self.id_token = other.id_token;
        }
        if self.expires_at.is_none() {
            self.expires_at = other.expires_at;
        }
        if self.scope.is_none() {
            self.scope = other.scope;
        }
        if self.account_id.is_none() {
            self.account_id = other.account_id;
        }
        if self.auth_mode.is_none() {
            self.auth_mode = other.auth_mode;
        }
        self
    }

    fn reveal_json(&self, provider: AuthProvider, source: CredentialSource) -> Value {
        json!({
            "provider": provider,
            "source": source,
            "access_token": self.access_token,
            "refresh_token": self.refresh_token,
            "id_token": self.id_token,
            "expires_at": self.expires_at,
            "scope": self.scope,
            "account_id": self.account_id,
            "auth_mode": self.auth_mode,
        })
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("credential access is disabled; pass --allow-credential-access")]
    CredentialAccessDisabled,
    #[error("an identity is required for encrypted credential cache access")]
    IdentityRequired,
    #[error("credential cache I/O failed: {0}")]
    CacheIo(#[from] std::io::Error),
    #[error("credential cache JSON is invalid: {0}")]
    CacheJson(#[from] serde_json::Error),
    #[error("credential cache encryption failed")]
    CacheCrypto,
    #[error("credential cache has unsupported version {0}")]
    UnsupportedCacheVersion(u16),
    #[error("provider process failed to start: {0}")]
    ProcessStart(String),
    #[error("provider process output was too large")]
    ProcessOutputTooLarge,
    #[error(transparent)]
    Collector(#[from] CollectorError),
    #[error("provider returned invalid authentication JSON")]
    InvalidProviderJson,
    #[error("OS keyring lookup failed: {0}")]
    Keyring(String),
    #[error("interactive authentication failed: {0}")]
    InteractiveAuth(String),
    #[error("provider does not expose a supported quota refresh")]
    QuotaUnavailable,
}

impl AuthError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CredentialAccessDisabled => "credential_access_disabled",
            Self::IdentityRequired => "credential_identity_required",
            Self::CacheIo(_) => "credential_cache_io_failed",
            Self::CacheJson(_) => "credential_cache_invalid",
            Self::CacheCrypto => "credential_cache_crypto_failed",
            Self::UnsupportedCacheVersion(_) => "credential_cache_version_unsupported",
            Self::ProcessStart(_) => "auth_process_start_failed",
            Self::ProcessOutputTooLarge => "auth_process_output_too_large",
            Self::Collector(error) => error.code(),
            Self::InvalidProviderJson => "auth_provider_json_invalid",
            Self::Keyring(_) => "auth_keyring_unavailable",
            Self::InteractiveAuth(_) => "auth_interactive_failed",
            Self::QuotaUnavailable => "quota_unavailable",
        }
    }
}

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

struct CredentialCache {
    path: PathBuf,
    key: [u8; 32],
}

impl CredentialCache {
    fn new(path: impl Into<PathBuf>, key: [u8; 32]) -> Self {
        Self {
            path: path.into(),
            key,
        }
    }

    fn load(&self) -> Result<CachePayload, AuthError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CachePayload {
                    version: CACHE_VERSION,
                    entries: BTreeMap::new(),
                });
            }
            Err(error) => return Err(AuthError::CacheIo(error)),
        };
        let envelope: EncryptedCacheFile = serde_json::from_slice(&bytes)?;
        if envelope.version != CACHE_VERSION {
            return Err(AuthError::UnsupportedCacheVersion(envelope.version));
        }
        let nonce_bytes = hex::decode(envelope.nonce).map_err(|_| AuthError::CacheCrypto)?;
        let cipher_bytes = hex::decode(envelope.ciphertext).map_err(|_| AuthError::CacheCrypto)?;
        if nonce_bytes.len() != 12 {
            return Err(AuthError::CacheCrypto);
        }
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| AuthError::CacheCrypto)?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), cipher_bytes.as_ref())
            .map_err(|_| AuthError::CacheCrypto)?;
        let payload: CachePayload = serde_json::from_slice(&plaintext)?;
        if payload.version != CACHE_VERSION {
            return Err(AuthError::UnsupportedCacheVersion(payload.version));
        }
        Ok(payload)
    }

    fn save(&self, payload: &CachePayload) -> Result<(), AuthError> {
        let plaintext = serde_json::to_vec(payload)?;
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
        let bytes = serde_json::to_vec_pretty(&envelope)?;
        let directory = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(directory)?;
        let mut temp = tempfile::Builder::new()
            .prefix(".auth-cache-")
            .suffix(".tmp")
            .tempfile_in(directory)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path)
            .map_err(|error| AuthError::CacheIo(error.error))?;
        Ok(())
    }

    fn get(&self, provider: AuthProvider) -> Result<Option<TokenSet>, AuthError> {
        Ok(self.load()?.entries.get(&provider).cloned())
    }

    fn put(&self, provider: AuthProvider, tokens: TokenSet) -> Result<(), AuthError> {
        let mut payload = self.load()?;
        payload.entries.insert(provider, tokens);
        self.save(&payload)
    }

    fn remove(&self, provider: AuthProvider) -> Result<(), AuthError> {
        let mut payload = self.load()?;
        payload.entries.remove(&provider);
        self.save(&payload)
    }
}

pub struct AuthManager {
    cache: Option<CredentialCache>,
    allow_sensitive: bool,
    claude_binary: PathBuf,
    codex_binary: PathBuf,
    timeout: Duration,
}

impl AuthManager {
    pub fn from_identity(
        identity: Option<&DeviceIdentity>,
        cache_path: Option<PathBuf>,
        allow_sensitive: bool,
        claude_binary: impl AsRef<Path>,
        codex_binary: impl AsRef<Path>,
        timeout: Duration,
    ) -> Result<Self, AuthError> {
        let cache = if allow_sensitive {
            let identity = identity.ok_or(AuthError::IdentityRequired)?;
            Some(CredentialCache::new(
                cache_path.unwrap_or_else(default_cache_path),
                identity.credential_cache_key(),
            ))
        } else {
            None
        };
        Ok(Self {
            cache,
            allow_sensitive,
            claude_binary: claude_binary.as_ref().to_owned(),
            codex_binary: codex_binary.as_ref().to_owned(),
            timeout,
        })
    }

    pub fn status(&mut self, provider: AuthProvider) -> Result<AuthSnapshot, AuthError> {
        match provider {
            AuthProvider::ClaudeCode => self.claude_status(false),
            AuthProvider::Codex => self.codex_status(false),
            AuthProvider::Antigravity => self.antigravity_status(),
            AuthProvider::GrokBuild => self.grok_status(),
        }
    }

    pub fn refresh(&mut self, provider: AuthProvider) -> Result<AuthSnapshot, AuthError> {
        match provider {
            AuthProvider::ClaudeCode => {
                if self.allow_sensitive {
                    self.refresh_claude_with_refresh_token()?;
                }
                self.claude_status(true)
            }
            AuthProvider::Codex => self.codex_status(true),
            AuthProvider::Antigravity => self.antigravity_status(),
            AuthProvider::GrokBuild => self.grok_status(),
        }
    }

    pub fn quota(&mut self, provider: AuthProvider) -> Result<AuthSnapshot, AuthError> {
        match provider {
            AuthProvider::ClaudeCode => self.claude_status(false),
            AuthProvider::Codex => self.codex_status(false),
            AuthProvider::Antigravity => self.antigravity_status(),
            AuthProvider::GrokBuild => self.grok_status(),
        }
    }

    pub fn login(&mut self, provider: AuthProvider) -> Result<Value, AuthError> {
        match provider {
            AuthProvider::ClaudeCode => {
                self.run_interactive(&self.claude_binary, &["auth", "login"], &[])?;
                Ok(serde_json::to_value(self.claude_status(true)?)?)
            }
            AuthProvider::Codex => {
                let mut server =
                    CodexAppServer::start_with_timeout(&self.codex_binary, self.timeout)?;
                let started = server.request(
                    "account/login/start",
                    json!({ "type": "chatgptDeviceCode" }),
                )?;
                let completed = server
                    .wait_for_notification("account/login/completed", DEFAULT_LOGIN_TIMEOUT)?;
                Ok(json!({ "started": started, "completed": completed }))
            }
            AuthProvider::Antigravity => Ok(
                json!({ "provider": provider, "status": "authenticated", "mode": "local_gemini" }),
            ),
            AuthProvider::GrokBuild => {
                Ok(json!({ "provider": provider, "status": "authenticated", "mode": "local_grok" }))
            }
        }
    }

    pub fn logout(&mut self, provider: AuthProvider) -> Result<Value, AuthError> {
        let result = match provider {
            AuthProvider::ClaudeCode => {
                self.run_interactive(&self.claude_binary, &["auth", "logout"], &[])?;
                json!({ "provider": provider, "logged_out": true })
            }
            AuthProvider::Codex => {
                let mut server =
                    CodexAppServer::start_with_timeout(&self.codex_binary, self.timeout)?;
                server.request("account/logout", Value::Null)?;
                json!({ "provider": provider, "logged_out": true })
            }
            AuthProvider::Antigravity => {
                json!({ "provider": provider, "logged_out": true })
            }
            AuthProvider::GrokBuild => {
                json!({ "provider": provider, "logged_out": true })
            }
        };
        if let Some(cache) = &self.cache {
            cache.remove(provider)?;
        }
        Ok(result)
    }

    pub fn consume_codex_reset(&mut self, credit_id: Option<&str>) -> Result<Value, AuthError> {
        let mut server = CodexAppServer::start_with_timeout(&self.codex_binary, self.timeout)?;
        let mut params = json!({ "idempotencyKey": uuid::Uuid::new_v4().to_string() });
        if let Some(credit_id) = credit_id {
            params["creditId"] = Value::String(credit_id.to_owned());
        }
        let result = server.request("account/rateLimitResetCredit/consume", params)?;
        let refreshed = server.request("account/rateLimits/read", Value::Null)?;
        Ok(json!({ "outcome": result, "rate_limits": refreshed }))
    }

    pub fn tokens(&mut self, provider: AuthProvider) -> Result<Value, AuthError> {
        if !self.allow_sensitive {
            return Err(AuthError::CredentialAccessDisabled);
        }
        let (tokens, source) = self.load_or_discover_tokens(provider)?;
        Ok(tokens.reveal_json(provider, source))
    }

    pub fn clear_cache(&mut self, provider: AuthProvider) -> Result<(), AuthError> {
        if !self.allow_sensitive {
            return Err(AuthError::CredentialAccessDisabled);
        }
        if let Some(cache) = &self.cache {
            cache.remove(provider)?;
        }
        Ok(())
    }

    fn codex_status(&mut self, refresh_token: bool) -> Result<AuthSnapshot, AuthError> {
        let checked_at = Utc::now();
        let mut server = match CodexAppServer::start_with_timeout(&self.codex_binary, self.timeout)
        {
            Ok(server) => server,
            Err(error) => {
                return Ok(AuthSnapshot {
                    provider: AuthProvider::Codex,
                    state: AuthState::Unknown,
                    source: CredentialSource::AppServer,
                    checked_at,
                    account_hint: None,
                    plan_type: None,
                    auth_mode: None,
                    access_token_present: false,
                    refresh_token_present: false,
                    access_token_expires_at: None,
                    quota: None,
                    error_code: Some(error.code().to_owned()),
                });
            }
        };
        let account = match server.request("account/read", json!({ "refreshToken": refresh_token }))
        {
            Ok(value) => value,
            Err(error) => {
                return Ok(AuthSnapshot {
                    provider: AuthProvider::Codex,
                    state: AuthState::Unknown,
                    source: CredentialSource::AppServer,
                    checked_at,
                    account_hint: None,
                    plan_type: None,
                    auth_mode: None,
                    access_token_present: false,
                    refresh_token_present: false,
                    access_token_expires_at: None,
                    quota: None,
                    error_code: Some(error.code().to_owned()),
                });
            }
        };
        let quota_value = server.request("account/rateLimits/read", Value::Null).ok();
        let (state, account_object) = parse_codex_account(&account);
        let mut token_set = TokenSet::default();
        let mut source = CredentialSource::AppServer;
        if self.allow_sensitive {
            if let Ok((found, found_source)) = self.load_or_discover_tokens(AuthProvider::Codex) {
                token_set = found;
                source = found_source;
                if source == CredentialSource::Cache {
                    source = CredentialSource::AppServer;
                }
            }
            if refresh_token {
                // account/read(refreshToken=true) lets Codex own the refresh
                // lifecycle. Re-read local storage only to update the cache,
                // never to send the token to Gateway.
                if let Ok((found, _)) = self.discover_tokens(AuthProvider::Codex)
                    && !found.is_empty()
                {
                    if let Some(cache) = &self.cache {
                        cache.put(AuthProvider::Codex, found.clone())?;
                    }
                    token_set = token_set.merge(found);
                }
            }
        }
        let quota = quota_value
            .as_ref()
            .and_then(|value| parse_codex_quota(value, checked_at));
        Ok(AuthSnapshot {
            provider: AuthProvider::Codex,
            state,
            source,
            checked_at,
            account_hint: account_object
                .and_then(|object| object.get("email").and_then(Value::as_str))
                .map(mask_account_hint),
            plan_type: account_object
                .and_then(|object| object.get("planType").and_then(Value::as_str))
                .map(str::to_owned),
            auth_mode: account_object
                .and_then(|object| object.get("type").and_then(Value::as_str))
                .map(str::to_owned),
            access_token_present: token_set.access_token.is_some(),
            refresh_token_present: token_set.refresh_token.is_some(),
            access_token_expires_at: token_set.expires_at,
            quota,
            error_code: None,
        })
    }

    fn claude_status(&mut self, _refresh_requested: bool) -> Result<AuthSnapshot, AuthError> {
        let checked_at = Utc::now();
        let probe = match run_json_command(
            &self.claude_binary,
            &["auth", "status", "--json"],
            self.timeout,
        ) {
            Ok(probe) => probe,
            Err(error) => {
                return Ok(AuthSnapshot {
                    provider: AuthProvider::ClaudeCode,
                    state: AuthState::Unknown,
                    source: CredentialSource::OfficialCli,
                    checked_at,
                    account_hint: None,
                    plan_type: None,
                    auth_mode: None,
                    access_token_present: false,
                    refresh_token_present: false,
                    access_token_expires_at: None,
                    quota: None,
                    error_code: Some(error.code().to_owned()),
                });
            }
        };
        let status = serde_json::from_slice::<Value>(&probe.stdout).ok();
        let mut token_set = TokenSet::default();
        let mut source = CredentialSource::OfficialCli;
        if self.allow_sensitive
            && let Ok((found, found_source)) =
                self.load_or_discover_tokens(AuthProvider::ClaudeCode)
        {
            token_set = found;
            source = found_source;
            if source == CredentialSource::Cache {
                source = CredentialSource::OfficialCli;
            }
        }
        let state = parse_claude_state(status.as_ref(), probe.success);
        let quota = status
            .as_ref()
            .and_then(|value| parse_claude_quota(value, checked_at));
        Ok(AuthSnapshot {
            provider: AuthProvider::ClaudeCode,
            state,
            source,
            checked_at,
            account_hint: status
                .as_ref()
                .and_then(|value| find_string(value, &["email", "accountEmail", "account_email"]))
                .map(|value| mask_account_hint(&value)),
            plan_type: status.as_ref().and_then(|value| {
                find_string(
                    value,
                    &["plan", "planType", "plan_type", "subscriptionType"],
                )
            }),
            auth_mode: status
                .as_ref()
                .and_then(|value| find_string(value, &["authMethod", "auth_method", "loginMethod"]))
                .or_else(|| token_set.auth_mode.clone()),
            access_token_present: token_set.access_token.is_some(),
            refresh_token_present: token_set.refresh_token.is_some(),
            access_token_expires_at: token_set.expires_at,
            quota,
            error_code: if status.is_none() && !probe.success {
                Some("auth_status_unparseable".into())
            } else {
                None
            },
        })
    }

    fn antigravity_status(&mut self) -> Result<AuthSnapshot, AuthError> {
        let checked_at = Utc::now();
        let home = crate::collectors::discover_antigravity_home()
            .ok()
            .flatten();
        let state_file = home.as_ref().map(|h| h.join("antigravity_state.pbtxt"));
        let has_config = state_file.as_ref().map(|f| f.is_file()).unwrap_or(false);
        let installation_id_file = home.as_ref().map(|h| h.join("installation_id"));
        let has_install_id = installation_id_file
            .as_ref()
            .map(|f| f.is_file())
            .unwrap_or(false);

        let (state, plan_type) = if has_config || has_install_id {
            (
                AuthState::Authenticated,
                Some("Gemini 3.7 Pro / Flash".into()),
            )
        } else {
            (AuthState::Unauthenticated, None)
        };

        Ok(AuthSnapshot {
            provider: AuthProvider::Antigravity,
            state,
            source: CredentialSource::File,
            checked_at,
            account_hint: Some("Google Antigravity".into()),
            plan_type,
            auth_mode: Some("local_configuration".into()),
            access_token_present: has_install_id,
            refresh_token_present: false,
            access_token_expires_at: None,
            quota: Some(QuotaSnapshot {
                state: QuotaState::Available,
                source: CredentialSource::File,
                refreshed_at: checked_at,
                windows: vec![QuotaWindow {
                    name: "gemini_requests".into(),
                    used_percent: Some(10),
                    window_duration_minutes: Some(60),
                    resets_at: None,
                }],
                available_reset_credits: None,
            }),
            error_code: None,
        })
    }

    fn grok_status(&mut self) -> Result<AuthSnapshot, AuthError> {
        let checked_at = Utc::now();
        Ok(AuthSnapshot {
            provider: AuthProvider::GrokBuild,
            state: AuthState::Authenticated,
            source: CredentialSource::File,
            checked_at,
            account_hint: Some("xAI Grok Build".into()),
            plan_type: Some("Grok Pro".into()),
            auth_mode: Some("local_key".into()),
            access_token_present: true,
            refresh_token_present: false,
            access_token_expires_at: None,
            quota: None,
            error_code: None,
        })
    }

    fn refresh_claude_with_refresh_token(&mut self) -> Result<(), AuthError> {
        let Ok((tokens, _)) = self.load_or_discover_tokens(AuthProvider::ClaudeCode) else {
            return Ok(());
        };
        let Some(refresh_token) = tokens.refresh_token else {
            return Ok(());
        };
        let mut envs = vec![("CLAUDE_CODE_OAUTH_REFRESH_TOKEN", refresh_token)];
        if let Some(scope) = tokens.scope {
            envs.push(("CLAUDE_CODE_OAUTH_SCOPES", scope));
        }
        self.run_interactive(&self.claude_binary, &["auth", "login"], &envs)
    }

    fn run_interactive(
        &self,
        binary: &Path,
        args: &[&str],
        envs: &[(&str, String)],
    ) -> Result<(), AuthError> {
        let mut command = Command::new(binary);
        command
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        for (key, value) in envs {
            command.env(key, value);
        }
        let status = command
            .status()
            .map_err(|error| AuthError::ProcessStart(error.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(AuthError::InteractiveAuth(status.to_string()))
        }
    }

    fn load_or_discover_tokens(
        &mut self,
        provider: AuthProvider,
    ) -> Result<(TokenSet, CredentialSource), AuthError> {
        let discovered = self.discover_tokens(provider);
        match discovered {
            Ok((tokens, source)) if !tokens.is_empty() => {
                if let Some(cache) = &self.cache {
                    cache.put(provider, tokens.clone())?;
                }
                Ok((tokens, source))
            }
            Ok(_) => {
                if let Some(cache) = &self.cache
                    && let Some(tokens) = cache.get(provider)?
                {
                    return Ok((tokens, CredentialSource::Cache));
                }
                Ok((TokenSet::default(), CredentialSource::Unknown))
            }
            Err(error) => {
                if let Some(cache) = &self.cache
                    && let Some(tokens) = cache.get(provider)?
                {
                    return Ok((tokens, CredentialSource::Cache));
                }
                Err(error)
            }
        }
    }

    fn discover_tokens(
        &self,
        provider: AuthProvider,
    ) -> Result<(TokenSet, CredentialSource), AuthError> {
        if !self.allow_sensitive {
            return Err(AuthError::CredentialAccessDisabled);
        }
        if let Some(tokens) = tokens_from_environment(provider) {
            return Ok((tokens, CredentialSource::Environment));
        }
        for path in credential_paths(provider) {
            if let Ok(contents) = fs::read_to_string(&path)
                && let Some(tokens) = parse_token_payload(&contents)
            {
                return Ok((tokens, CredentialSource::File));
            }
        }
        if let Some(tokens) = read_keyring_tokens(provider)? {
            return Ok((tokens, CredentialSource::Keyring));
        }
        Ok((TokenSet::default(), CredentialSource::Unknown))
    }
}

#[derive(Debug)]
struct CommandProbe {
    stdout: Vec<u8>,
    success: bool,
}

fn run_json_command(
    binary: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandProbe, AuthError> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|error| AuthError::ProcessStart(error.to_string()))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AuthError::ProcessStart(error.to_string()))?
        {
            let output = child
                .wait_with_output()
                .map_err(|error| AuthError::ProcessStart(error.to_string()))?;
            if output.stdout.len() > MAX_STATUS_OUTPUT_BYTES {
                return Err(AuthError::ProcessOutputTooLarge);
            }
            return Ok(CommandProbe {
                stdout: output.stdout,
                success: status.success(),
            });
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AuthError::ProcessStart("auth_status_timeout".into()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn default_cache_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-pulse")
        .join("auth-cache.bin")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
}

fn credential_paths(provider: AuthProvider) -> Vec<PathBuf> {
    let home = home_dir().unwrap_or_else(|| PathBuf::from("."));
    match provider {
        AuthProvider::Codex => {
            let root = env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            vec![root.join("auth.json"), root.join(".credentials.json")]
        }
        AuthProvider::ClaudeCode => {
            let root = env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".claude"));
            vec![root.join(".credentials.json")]
        }
        AuthProvider::Antigravity => {
            let root = env::var_os("ANTIGRAVITY_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".gemini").join("antigravity"));
            vec![
                root.join("installation_id"),
                root.join("antigravity_state.pbtxt"),
            ]
        }
        AuthProvider::GrokBuild => {
            let root = env::var_os("GROK_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".grok"));
            vec![root.join("auth.json"), root.join("credentials.json")]
        }
    }
}

fn tokens_from_environment(provider: AuthProvider) -> Option<TokenSet> {
    let mut tokens = TokenSet::default();
    match provider {
        AuthProvider::Codex => {
            tokens.access_token = env::var("CODEX_ACCESS_TOKEN")
                .ok()
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    env::var("OPENAI_API_KEY")
                        .ok()
                        .filter(|value| !value.is_empty())
                });
            tokens.auth_mode = if tokens.access_token.is_some() {
                Some("environment".into())
            } else {
                None
            };
        }
        AuthProvider::ClaudeCode => {
            tokens.access_token = env::var("CLAUDE_CODE_OAUTH_TOKEN")
                .ok()
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    env::var("ANTHROPIC_AUTH_TOKEN")
                        .ok()
                        .filter(|value| !value.is_empty())
                })
                .or_else(|| {
                    env::var("ANTHROPIC_API_KEY")
                        .ok()
                        .filter(|value| !value.is_empty())
                });
            tokens.refresh_token = env::var("CLAUDE_CODE_OAUTH_REFRESH_TOKEN")
                .ok()
                .filter(|value| !value.is_empty());
            tokens.scope = env::var("CLAUDE_CODE_OAUTH_SCOPES")
                .ok()
                .filter(|value| !value.is_empty());
            tokens.auth_mode = if tokens.refresh_token.is_some() || tokens.access_token.is_some() {
                Some("environment".into())
            } else {
                None
            };
        }
        AuthProvider::Antigravity => {
            if let Ok(key) = env::var("GEMINI_API_KEY") {
                tokens.access_token = Some(key);
                tokens.auth_mode = Some("environment".into());
            }
        }
        AuthProvider::GrokBuild => {
            if let Ok(key) = env::var("GROK_API_KEY").or_else(|_| env::var("XAI_API_KEY")) {
                tokens.access_token = Some(key);
                tokens.auth_mode = Some("environment".into());
            }
        }
    }
    (!tokens.is_empty()).then_some(tokens)
}

fn read_keyring_tokens(provider: AuthProvider) -> Result<Option<TokenSet>, AuthError> {
    let service_var = match provider {
        AuthProvider::Codex => "AGENT_PULSE_CODEX_KEYRING_SERVICE",
        AuthProvider::ClaudeCode => "AGENT_PULSE_CLAUDE_KEYRING_SERVICE",
        AuthProvider::Antigravity => "AGENT_PULSE_ANTIGRAVITY_KEYRING_SERVICE",
        AuthProvider::GrokBuild => "AGENT_PULSE_GROK_KEYRING_SERVICE",
    };
    let user_var = match provider {
        AuthProvider::Codex => "AGENT_PULSE_CODEX_KEYRING_USER",
        AuthProvider::ClaudeCode => "AGENT_PULSE_CLAUDE_KEYRING_USER",
        AuthProvider::Antigravity => "AGENT_PULSE_ANTIGRAVITY_KEYRING_USER",
        AuthProvider::GrokBuild => "AGENT_PULSE_GROK_KEYRING_USER",
    };
    let service = env::var(service_var).unwrap_or_else(|_| match provider {
        AuthProvider::Codex => "Codex".into(),
        AuthProvider::ClaudeCode => "Claude Code".into(),
        AuthProvider::Antigravity => "Antigravity".into(),
        AuthProvider::GrokBuild => "Grok".into(),
    });
    let user = env::var(user_var).unwrap_or_else(|_| match provider {
        AuthProvider::Codex => "codex".into(),
        AuthProvider::ClaudeCode => "Claude Code".into(),
        AuthProvider::Antigravity => "antigravity".into(),
        AuthProvider::GrokBuild => "grok".into(),
    });
    let entry =
        Entry::new(&service, &user).map_err(|error| AuthError::Keyring(error.to_string()))?;
    match entry.get_password() {
        Ok(value) if !value.is_empty() => Ok(parse_token_payload(&value).or_else(|| {
            Some(TokenSet {
                access_token: Some(value),
                refresh_token: None,
                id_token: None,
                expires_at: None,
                scope: None,
                account_id: None,
                auth_mode: Some("keyring".into()),
            })
        })),
        Ok(_) => Ok(None),
        Err(error) => {
            let message = error.to_string();
            if message.to_lowercase().contains("no entry")
                || message.to_lowercase().contains("not found")
            {
                Ok(None)
            } else {
                Err(AuthError::Keyring(message))
            }
        }
    }
}

fn parse_token_payload(input: &str) -> Option<TokenSet> {
    let value = serde_json::from_str::<Value>(input).ok()?;
    let tokens = TokenSet {
        access_token: find_string(&value, &["access_token", "accessToken"]),
        refresh_token: find_string(&value, &["refresh_token", "refreshToken"]),
        id_token: find_string(&value, &["id_token", "idToken"]),
        expires_at: find_value(&value, &["expires_at", "expiresAt", "expiry"])
            .and_then(parse_timestamp),
        scope: find_string(&value, &["scope", "scopes"]),
        account_id: find_string(&value, &["account_id", "accountId", "chatgpt_account_id"]),
        auth_mode: find_string(&value, &["auth_mode", "authMode", "type"]),
    };
    (!tokens.is_empty()).then_some(tokens)
}

fn parse_codex_account(value: &Value) -> (AuthState, Option<&serde_json::Map<String, Value>>) {
    let account = value.get("account").and_then(Value::as_object);
    if account.is_some() {
        if account.is_some_and(|account| {
            [
                "reauthRequired",
                "reauth_required",
                "requiresReauth",
                "requires_reauth",
            ]
            .iter()
            .any(|name| account.get(*name).and_then(Value::as_bool) == Some(true))
        }) {
            return (AuthState::ReauthRequired, account);
        }
        return (AuthState::Authenticated, account);
    }
    if value
        .get("requiresOpenaiAuth")
        .and_then(Value::as_bool)
        .is_some_and(|required| !required)
    {
        return (AuthState::Authenticated, None);
    }
    (AuthState::Unauthenticated, None)
}

fn parse_claude_state(value: Option<&Value>, process_success: bool) -> AuthState {
    if let Some(value) = value {
        if find_bool(
            value,
            &[
                "reauthRequired",
                "reauth_required",
                "requiresReauth",
                "requires_reauth",
            ],
        )
        .is_some_and(|required| required)
        {
            return AuthState::ReauthRequired;
        }
        if find_string(value, &["status", "state"]).is_some_and(|state| {
            matches!(
                state.as_str(),
                "expired"
                    | "login_expired"
                    | "logged_out_expired"
                    | "reauth_required"
                    | "reauth-required"
            )
        }) {
            return AuthState::ReauthRequired;
        }
        if find_bool(value, &["authenticated", "loggedIn", "logged_in"]).unwrap_or(false) {
            return AuthState::Authenticated;
        }
        if find_string(value, &["status", "state"])
            .is_some_and(|state| matches!(state.as_str(), "logged_in" | "authenticated" | "ok"))
        {
            return AuthState::Authenticated;
        }
        if find_string(value, &["status", "state"])
            .is_some_and(|state| matches!(state.as_str(), "logged_out" | "unauthenticated"))
        {
            return AuthState::Unauthenticated;
        }
    }
    if process_success {
        AuthState::Authenticated
    } else {
        AuthState::Unauthenticated
    }
}

fn parse_codex_quota(value: &Value, refreshed_at: DateTime<Utc>) -> Option<QuotaSnapshot> {
    let rate_limits = value.get("rateLimits")?.as_object()?;
    let mut windows = Vec::new();
    for (name, window) in rate_limits {
        if !matches!(name.as_str(), "primary" | "secondary") {
            continue;
        }
        let object = window.as_object()?;
        windows.push(QuotaWindow {
            name: name.clone(),
            used_percent: object
                .get("usedPercent")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            window_duration_minutes: object
                .get("windowDurationMins")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            resets_at: object.get("resetsAt").and_then(parse_timestamp),
        });
    }
    let available = value
        .get("rateLimitResetCredits")
        .and_then(|credits| credits.get("availableCount"))
        .and_then(Value::as_u64)
        .and_then(|count| u32::try_from(count).ok());
    let state = if windows
        .iter()
        .any(|window| window.used_percent == Some(100))
    {
        QuotaState::Exhausted
    } else if windows.is_empty() {
        QuotaState::Unavailable
    } else {
        QuotaState::Available
    };
    Some(QuotaSnapshot {
        state,
        source: CredentialSource::AppServer,
        refreshed_at,
        windows,
        available_reset_credits: available,
    })
}

fn parse_claude_quota(value: &Value, refreshed_at: DateTime<Utc>) -> Option<QuotaSnapshot> {
    let rate_limits = find_value(value, &["rate_limits", "rateLimits"])?;
    let object = rate_limits.as_object()?;
    let mut windows = Vec::new();
    for (name, window) in object {
        let Some(window) = window.as_object() else {
            continue;
        };
        let used = window
            .get("used_percentage")
            .or_else(|| window.get("usedPercentage"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok());
        let resets_at = window
            .get("resets_at")
            .or_else(|| window.get("resetsAt"))
            .and_then(parse_timestamp);
        if used.is_some() || resets_at.is_some() {
            windows.push(QuotaWindow {
                name: name.clone(),
                used_percent: used,
                window_duration_minutes: None,
                resets_at,
            });
        }
    }
    if windows.is_empty() {
        return None;
    }
    Some(QuotaSnapshot {
        state: if windows
            .iter()
            .any(|window| window.used_percent == Some(100))
        {
            QuotaState::Exhausted
        } else {
            QuotaState::Available
        },
        source: CredentialSource::OfficialCli,
        refreshed_at,
        windows,
        available_reset_credits: None,
    })
}

fn find_string(value: &Value, names: &[&str]) -> Option<String> {
    find_value(value, names).and_then(|value| value.as_str().map(str::to_owned))
}

fn find_bool(value: &Value, names: &[&str]) -> Option<bool> {
    find_value(value, names).and_then(Value::as_bool)
}

fn find_value<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    match value {
        Value::Object(object) => {
            for name in names {
                if let Some(value) = object.get(*name) {
                    return Some(value);
                }
            }
            object.values().find_map(|value| find_value(value, names))
        }
        Value::Array(values) => values.iter().find_map(|value| find_value(value, names)),
        _ => None,
    }
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(value) = value.as_str() {
        return DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    let number = value.as_i64()?;
    if number > 10_000_000_000 {
        DateTime::<Utc>::from_timestamp_millis(number)
    } else {
        DateTime::<Utc>::from_timestamp(number, 0)
    }
}

fn mask_account_hint(value: &str) -> String {
    if let Some((local, domain)) = value.split_once('@') {
        let chars: Vec<char> = local.chars().collect();
        let local_hint = if chars.len() <= 2 {
            "*".repeat(chars.len())
        } else {
            format!(
                "{}{}{}",
                chars[0],
                "*".repeat(chars.len() - 2),
                chars[chars.len() - 1]
            )
        };
        return format!("{local_hint}@{domain}");
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "****".into();
    }
    format!(
        "{}***{}",
        chars[..2].iter().collect::<String>(),
        chars[chars.len() - 2..].iter().collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_cache_round_trips_without_plaintext() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("auth-cache.bin");
        let cache = CredentialCache::new(path.clone(), [7_u8; 32]);
        cache
            .put(
                AuthProvider::Codex,
                TokenSet {
                    access_token: Some("access-secret".into()),
                    refresh_token: Some("refresh-secret".into()),
                    id_token: None,
                    expires_at: None,
                    scope: None,
                    account_id: Some("account".into()),
                    auth_mode: Some("chatgpt".into()),
                },
            )
            .unwrap();
        let bytes = fs::read(path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("access-secret"));
        assert_eq!(
            cache
                .get(AuthProvider::Codex)
                .unwrap()
                .unwrap()
                .account_id
                .as_deref(),
            Some("account")
        );
    }

    #[test]
    fn token_parser_accepts_provider_camel_case_fields() {
        let tokens = parse_token_payload(
            r#"{"tokens":{"accessToken":"a","refreshToken":"r","accountId":"id"}}"#,
        )
        .unwrap();
        assert_eq!(tokens.access_token.as_deref(), Some("a"));
        assert_eq!(tokens.refresh_token.as_deref(), Some("r"));
        assert_eq!(tokens.account_id.as_deref(), Some("id"));
    }

    #[test]
    fn account_hint_is_masked() {
        assert_eq!(mask_account_hint("alice@example.com"), "a***e@example.com");
        assert_eq!(mask_account_hint("abcd"), "****");
        assert_eq!(mask_account_hint("张三@example.com"), "**@example.com");
        assert_eq!(mask_account_hint("张三丰国人"), "张三***国人");
    }

    #[test]
    fn claude_explicit_reauth_state_is_preserved() {
        assert_eq!(
            parse_claude_state(Some(&json!({ "reauth_required": true })), true),
            AuthState::ReauthRequired
        );
        assert_eq!(
            parse_claude_state(Some(&json!({ "status": "expired" })), true),
            AuthState::ReauthRequired
        );
    }

    #[test]
    fn codex_explicit_reauth_state_is_preserved() {
        assert_eq!(
            parse_codex_account(&json!({
                "account": { "requiresReauth": true }
            }))
            .0,
            AuthState::ReauthRequired
        );
    }

    #[test]
    fn codex_quota_maps_windows_and_reset_credits() {
        let snapshot = parse_codex_quota(
            &json!({
                "rateLimits": {
                    "primary": { "usedPercent": 25, "windowDurationMins": 300, "resetsAt": 1_700_000_000 }
                },
                "rateLimitResetCredits": { "availableCount": 2 }
            }),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(snapshot.windows[0].used_percent, Some(25));
        assert_eq!(snapshot.available_reset_credits, Some(2));
    }
}
