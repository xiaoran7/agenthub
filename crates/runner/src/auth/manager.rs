use super::*;

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
        }
    }

    pub fn quota(&mut self, provider: AuthProvider) -> Result<AuthSnapshot, AuthError> {
        match provider {
            AuthProvider::ClaudeCode => self.claude_status(false),
            AuthProvider::Codex => self.codex_status(false),
            AuthProvider::Antigravity => self.antigravity_status(),
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
        let user_home = crate::collectors::home_dir();
        let ag_home = crate::collectors::discover_antigravity_home()
            .ok()
            .flatten();
        let local_cfg = user_home
            .as_deref()
            .map(|h| {
                crate::collectors::antigravity::load_antigravity_local_config_from(
                    h,
                    ag_home.as_deref(),
                )
            })
            .unwrap_or_default();
        let env_tokens = if self.allow_sensitive {
            tokens_from_environment(AuthProvider::Antigravity)
        } else {
            None
        };
        let has_env_token = env_tokens
            .as_ref()
            .and_then(|t| t.access_token.as_ref())
            .is_some();
        let cc = crate::claude_gateway::discover_cc_switch_provider_for_app("antigravity")
            .or_else(|| crate::claude_gateway::discover_cc_switch_provider_for_app("gemini"))
            .filter(|c| !crate::claude_gateway::is_cc_switch_default_placeholder(c));

        let has_daemon = local_cfg.active_daemon.is_some();
        let has_config = local_cfg.installation_uuid.is_some()
            || local_cfg.state_model.is_some()
            || local_cfg.enabled_plugins_count > 0;
        let is_authenticated = has_env_token || has_daemon || has_config || cc.is_some();

        let (state, source, auth_mode) = if has_env_token {
            (
                AuthState::Authenticated,
                CredentialSource::Environment,
                Some("environment".into()),
            )
        } else if let Some(ref daemon) = local_cfg.active_daemon
            && daemon.has_csrf_token
        {
            (
                AuthState::Authenticated,
                CredentialSource::File,
                Some("daemon_session".into()),
            )
        } else if cc.is_some() {
            (
                AuthState::Authenticated,
                CredentialSource::File,
                Some("gateway".into()),
            )
        } else if has_config {
            (
                AuthState::Authenticated,
                CredentialSource::File,
                Some("local_configuration".into()),
            )
        } else {
            (
                AuthState::Unauthenticated,
                CredentialSource::File,
                Some("local_configuration".into()),
            )
        };

        let plan_type = if is_authenticated {
            local_cfg
                .resolved_model
                .clone()
                .or_else(|| cc.as_ref().map(|c| c.name.clone()))
                .or_else(|| {
                    local_cfg
                        .active_daemon
                        .as_ref()
                        .and_then(|d| d.ls_version.as_ref().map(|v| format!("IDE {v}")))
                })
                .or_else(|| Some("Antigravity IDE".into()))
        } else {
            None
        };

        let account_hint = local_cfg
            .remote_control_hostname
            .clone()
            .unwrap_or_else(|| "Google Antigravity".into());

        Ok(AuthSnapshot {
            provider: AuthProvider::Antigravity,
            state,
            source,
            checked_at,
            account_hint: Some(account_hint),
            plan_type,
            auth_mode,
            access_token_present: has_env_token
                || local_cfg
                    .active_daemon
                    .as_ref()
                    .is_some_and(|d| d.has_csrf_token)
                || local_cfg.installation_uuid.is_some(),
            refresh_token_present: false,
            access_token_expires_at: None,
            quota: None,
            error_code: None,
        })
    }

    fn refresh_claude_with_refresh_token(&mut self) -> Result<(), AuthError> {
        let Ok((mut tokens, _)) = self.load_or_discover_tokens(AuthProvider::ClaudeCode) else {
            return Ok(());
        };
        let Some(refresh_token) = tokens.refresh_token.take() else {
            return Ok(());
        };
        let mut envs = vec![("CLAUDE_CODE_OAUTH_REFRESH_TOKEN", refresh_token)];
        if let Some(scope) = tokens.scope.take() {
            envs.push(("CLAUDE_CODE_OAUTH_SCOPES", scope));
        }
        let result = self.run_interactive(&self.claude_binary, &["auth", "login"], &envs);
        for (_, value) in &mut envs {
            zeroize::Zeroize::zeroize(value);
        }
        result
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
