use super::*;

#[derive(Debug)]
pub(crate) struct CommandProbe {
    pub(crate) stdout: Vec<u8>,
    pub(crate) success: bool,
}

pub(crate) fn run_json_command(
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
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| AuthError::ProcessStart("missing_stdout_pipe".into()))?;
    let stdout_reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::with_capacity(4096);
        let res = stdout_pipe
            .take((MAX_STATUS_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut buf);
        res.map(|_| buf)
    });
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AuthError::ProcessStart(error.to_string()))?
        {
            let stdout = stdout_reader
                .join()
                .map_err(|_| AuthError::ProcessStart("stdout_reader_panicked".into()))?
                .map_err(|error| AuthError::ProcessStart(error.to_string()))?;
            if stdout.len() > MAX_STATUS_OUTPUT_BYTES {
                return Err(AuthError::ProcessOutputTooLarge);
            }
            return Ok(CommandProbe {
                stdout,
                success: status.success(),
            });
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            return Err(AuthError::ProcessStart("auth_status_timeout".into()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn default_cache_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-pulse")
        .join("auth-cache.bin")
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
}

pub(crate) fn credential_paths(provider: AuthProvider) -> Vec<PathBuf> {
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
    }
}

pub(crate) fn usable_env_token_value(raw: Option<&str>, base_url: Option<&str>) -> Option<String> {
    let token = raw.map(str::trim)?;
    if !crate::claude_gateway::is_usable_gateway_token(token) {
        return None;
    }
    if let Some(base) = base_url.map(str::trim).filter(|b| !b.is_empty())
        && !crate::claude_gateway::is_token_compatible_with_endpoint(base, token)
    {
        return None;
    }
    Some(token.to_string())
}

pub(crate) fn usable_env_var(name: &str, base_url: Option<&str>) -> Option<String> {
    let raw = env::var(name).ok()?;
    usable_env_token_value(Some(&raw), base_url)
}

pub(crate) fn tokens_from_environment(provider: AuthProvider) -> Option<TokenSet> {
    let mut tokens = TokenSet::default();
    match provider {
        AuthProvider::Codex => {
            let openai_base = env::var("OPENAI_BASE_URL").ok();
            tokens.access_token = usable_env_var("CODEX_ACCESS_TOKEN", None)
                .or_else(|| usable_env_var("OPENAI_API_KEY", openai_base.as_deref()));
            tokens.auth_mode = if tokens.access_token.is_some() {
                Some("environment".into())
            } else {
                None
            };
        }
        AuthProvider::ClaudeCode => {
            let anthropic_base = env::var("ANTHROPIC_BASE_URL").ok();
            tokens.access_token = usable_env_var("CLAUDE_CODE_OAUTH_TOKEN", None)
                .or_else(|| usable_env_var("ANTHROPIC_AUTH_TOKEN", anthropic_base.as_deref()))
                .or_else(|| usable_env_var("ANTHROPIC_API_KEY", anthropic_base.as_deref()));
            tokens.refresh_token = env::var("CLAUDE_CODE_OAUTH_REFRESH_TOKEN")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|value| !value.is_empty());
            tokens.scope = env::var("CLAUDE_CODE_OAUTH_SCOPES")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|value| !value.is_empty());
            if tokens.access_token.is_none()
                && let Some(gateway) = crate::claude_gateway::discover_claude_code_gateway()
            {
                tokens.access_token = Some(gateway.auth_token);
                tokens.auth_mode = Some("gateway".into());
            }
            if tokens.auth_mode.is_none()
                && (tokens.refresh_token.is_some() || tokens.access_token.is_some())
            {
                tokens.auth_mode = Some("environment".into());
            }
        }
        AuthProvider::Antigravity => {
            let gemini_base = env::var("GOOGLE_GEMINI_BASE_URL")
                .or_else(|_| env::var("GEMINI_BASE_URL"))
                .or_else(|_| env::var("GOOGLE_BASE_URL"))
                .ok();
            tokens.access_token = usable_env_var("GEMINI_API_KEY", gemini_base.as_deref());
            if tokens.access_token.is_some() {
                tokens.auth_mode = Some("environment".into());
            }
        }
    }
    (!tokens.is_empty()).then_some(tokens)
}

pub(crate) fn read_keyring_tokens(provider: AuthProvider) -> Result<Option<TokenSet>, AuthError> {
    let service_var = match provider {
        AuthProvider::Codex => "AGENT_PULSE_CODEX_KEYRING_SERVICE",
        AuthProvider::ClaudeCode => "AGENT_PULSE_CLAUDE_KEYRING_SERVICE",
        AuthProvider::Antigravity => "AGENT_PULSE_ANTIGRAVITY_KEYRING_SERVICE",
    };
    let user_var = match provider {
        AuthProvider::Codex => "AGENT_PULSE_CODEX_KEYRING_USER",
        AuthProvider::ClaudeCode => "AGENT_PULSE_CLAUDE_KEYRING_USER",
        AuthProvider::Antigravity => "AGENT_PULSE_ANTIGRAVITY_KEYRING_USER",
    };
    let service = env::var(service_var).unwrap_or_else(|_| match provider {
        AuthProvider::Codex => "Codex".into(),
        AuthProvider::ClaudeCode => "Claude Code".into(),
        AuthProvider::Antigravity => "Antigravity".into(),
    });
    let user = env::var(user_var).unwrap_or_else(|_| match provider {
        AuthProvider::Codex => "codex".into(),
        AuthProvider::ClaudeCode => "Claude Code".into(),
        AuthProvider::Antigravity => "antigravity".into(),
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

pub(crate) fn parse_token_payload(input: &str) -> Option<TokenSet> {
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

pub(crate) fn parse_codex_account(
    value: &Value,
) -> (AuthState, Option<&serde_json::Map<String, Value>>) {
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

pub(crate) fn parse_claude_state(value: Option<&Value>, process_success: bool) -> AuthState {
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

pub(crate) fn parse_codex_quota(
    value: &Value,
    refreshed_at: DateTime<Utc>,
) -> Option<QuotaSnapshot> {
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

pub(crate) fn parse_claude_quota(
    value: &Value,
    refreshed_at: DateTime<Utc>,
) -> Option<QuotaSnapshot> {
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

pub(crate) fn find_string(value: &Value, names: &[&str]) -> Option<String> {
    find_value(value, names).and_then(|value| value.as_str().map(str::to_owned))
}

pub(crate) fn find_bool(value: &Value, names: &[&str]) -> Option<bool> {
    find_value(value, names).and_then(Value::as_bool)
}

pub(crate) fn find_value<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
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

pub(crate) fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
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

pub(crate) fn mask_account_hint(value: &str) -> String {
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
