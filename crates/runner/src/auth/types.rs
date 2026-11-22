use super::*;

pub(crate) const CACHE_VERSION: u16 = 1;
pub(crate) const MAX_STATUS_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const DEFAULT_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthProvider {
    ClaudeCode,
    Codex,
    Antigravity,
}

impl AuthProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
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

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Sensitive material.  Do not add `Debug` or expose this type from a web/API
/// boundary.  It is serialized only inside the encrypted local cache.
#[derive(Clone, Default, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct TokenSet {
    pub(crate) access_token: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) id_token: Option<String>,
    #[zeroize(skip)]
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) scope: Option<String>,
    pub(crate) account_id: Option<String>,
    pub(crate) auth_mode: Option<String>,
}

impl TokenSet {
    pub(crate) fn is_empty(&self) -> bool {
        self.access_token.is_none()
            && self.refresh_token.is_none()
            && self.id_token.is_none()
            && self.account_id.is_none()
    }

    pub(crate) fn merge(mut self, mut other: Self) -> Self {
        if self.access_token.is_none() {
            self.access_token = other.access_token.take();
        }
        if self.refresh_token.is_none() {
            self.refresh_token = other.refresh_token.take();
        }
        if self.id_token.is_none() {
            self.id_token = other.id_token.take();
        }
        if self.expires_at.is_none() {
            self.expires_at = other.expires_at.take();
        }
        if self.scope.is_none() {
            self.scope = other.scope.take();
        }
        if self.account_id.is_none() {
            self.account_id = other.account_id.take();
        }
        if self.auth_mode.is_none() {
            self.auth_mode = other.auth_mode.take();
        }
        self
    }

    pub(crate) fn reveal_json(&self, provider: AuthProvider, source: CredentialSource) -> Value {
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
