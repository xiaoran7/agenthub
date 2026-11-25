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

#[test]
fn usable_env_token_value_rejects_placeholders_and_mismatched_ccs_tokens() {
    assert_eq!(usable_env_token_value(None, None), None);
    assert_eq!(usable_env_token_value(Some("   "), None), None);
    assert_eq!(
        usable_env_token_value(Some("__sealed__:v1:blob"), None),
        None
    );
    assert_eq!(usable_env_token_value(Some("PROXY_MANAGED"), None), None);
    assert_eq!(usable_env_token_value(Some("proxy_managed"), None), None);

    assert_eq!(
        usable_env_token_value(Some("ccs-123"), Some("http://127.0.0.1:8316")),
        None
    );
    assert_eq!(
        usable_env_token_value(Some("ccs-123"), Some("https://api.anthropic.com")),
        None
    );
    assert_eq!(
        usable_env_token_value(
            Some("  ccs-123  "),
            Some("http://127.0.0.1:15721/claude-desktop")
        ),
        Some("ccs-123".to_string())
    );
    assert_eq!(
        usable_env_token_value(Some("  sk-live-key  "), Some("http://127.0.0.1:8316")),
        Some("sk-live-key".to_string())
    );
}

#[test]
fn antigravity_status_ignores_env_token_when_allow_sensitive_is_false() {
    struct EnvGuard(&'static str, Option<std::ffi::OsString>);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match self.1.take() {
                    Some(v) => std::env::set_var(self.0, v),
                    None => std::env::remove_var(self.0),
                }
            }
        }
    }

    let prev = std::env::var_os("GEMINI_API_KEY");
    unsafe {
        std::env::set_var("GEMINI_API_KEY", "test-sensitive-gemini-key");
    }
    let _guard = EnvGuard("GEMINI_API_KEY", prev);

    let mut manager =
        AuthManager::from_identity(None, None, false, "claude", "codex", Duration::from_secs(1))
            .unwrap();
    let snapshot = manager.status(AuthProvider::Antigravity).unwrap();
    assert_ne!(
        snapshot.source,
        CredentialSource::Environment,
        "allow_sensitive=false must not read GEMINI_API_KEY from environment"
    );
    assert_ne!(snapshot.auth_mode.as_deref(), Some("environment"));
}
