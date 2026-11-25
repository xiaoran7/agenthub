use super::*;

/// One configured model inside a third-party gateway profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeGatewayModel {
    pub name: String,
    pub label: Option<String>,
    pub supports_1m: bool,
}

/// The model name contract understood by a discovered gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayModelNamespace {
    /// Requests carry `claude-*` route ids; the gateway translates them upstream.
    RouteAlias,
    /// Requests carry the upstream's own model id; the runner must not remap them.
    Upstream,
}

/// A discovered gateway runtime environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaudeGatewayProfile {
    pub base_url: String,
    pub auth_token: String,
    pub source: String,
    pub models: Vec<ClaudeGatewayModel>,
    pub model_namespace: GatewayModelNamespace,
}

pub(crate) fn strip_model_ui_suffix(raw: &str) -> &str {
    let clean = raw
        .trim_end_matches(" [1m]")
        .trim_end_matches("[1m]")
        .trim_end_matches(" [1M]")
        .trim_end_matches("[1M]")
        .trim();
    let clean = if clean.is_empty() { raw } else { clean };
    clean
        .rsplit_once(" (")
        .filter(|(_, suffix)| suffix.ends_with(')'))
        .map_or(clean, |(prefix, _)| prefix.trim_end())
}

impl ClaudeGatewayProfile {
    pub fn new(
        base_url: impl Into<String>,
        auth_token: impl Into<String>,
        source: impl Into<String>,
        models: Vec<ClaudeGatewayModel>,
    ) -> Self {
        let base_url = base_url.into();
        let auth_token = auth_token.into();
        let model_namespace = Self::namespace_for_endpoint(&base_url, &auth_token);
        Self {
            base_url,
            auth_token,
            source: source.into(),
            models,
            model_namespace,
        }
    }

    pub(crate) fn namespace_for_endpoint(
        base_url: &str,
        auth_token: &str,
    ) -> GatewayModelNamespace {
        let is_ccs_route = is_cc_switch_endpoint(base_url) && auth_token.trim().starts_with("ccs-");
        if is_ccs_route {
            GatewayModelNamespace::RouteAlias
        } else {
            GatewayModelNamespace::Upstream
        }
    }

    pub fn with_model_namespace(mut self, namespace: GatewayModelNamespace) -> Self {
        self.model_namespace = namespace;
        self
    }

    /// Removes UI-only context markers without translating the upstream model id.
    pub fn passthrough_model(requested: Option<&str>) -> Option<String> {
        let raw = requested.map(str::trim).filter(|s| !s.is_empty())?;
        Some(strip_model_ui_suffix(raw).to_string())
    }

    /// Maps a Claude Desktop route id onto the current upstream model id when
    /// this profile talks directly to the upstream instead of the CCS router.
    ///
    /// Returns `None` when the requested id is a real upstream id, so a caller
    /// must not silently rewrite valid explicit model choices.
    pub fn upstream_model_for_route_alias(
        &self,
        requested: Option<&str>,
        route_models: &[ClaudeGatewayModel],
        upstream_models: &[ClaudeGatewayModel],
    ) -> Option<String> {
        let raw = requested.map(str::trim).filter(|s| !s.is_empty())?;
        let key = strip_model_ui_suffix(raw);
        let lower = key.to_ascii_lowercase();
        if !lower.starts_with("claude-") {
            return None;
        }
        let real_upstream_models: Vec<&ClaudeGatewayModel> = upstream_models
            .iter()
            .filter(|m| !m.name.to_ascii_lowercase().starts_with("claude-"))
            .collect();
        if real_upstream_models.is_empty() {
            return None;
        }
        let route = route_models
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(key))?;
        let target = route
            .label
            .as_deref()
            .map(strip_model_ui_suffix)
            .filter(|s| !s.is_empty())
            .unwrap_or(route.name.as_str());
        let target_lower = target.to_ascii_lowercase();
        let exact = real_upstream_models
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(target))
            .or_else(|| {
                real_upstream_models.iter().find(|m| {
                    m.label
                        .as_deref()
                        .is_some_and(|label| label.eq_ignore_ascii_case(target))
                })
            });
        if let Some(model) = exact {
            return Some(model.name.clone());
        }
        let family = ["opus", "sonnet", "fable", "haiku"]
            .into_iter()
            .find(|word| lower.contains(word));
        real_upstream_models
            .iter()
            .find(|m| {
                family.is_some_and(|word| m.name.to_ascii_lowercase().contains(word))
                    || m.label.as_deref().is_some_and(|label| {
                        label.to_ascii_lowercase().contains(&target_lower)
                            || target_lower.contains(&label.to_ascii_lowercase())
                    })
            })
            .map(|m| m.name.clone())
    }

    /// Resolves an input model name to a model name understood by the gateway router.
    ///
    /// Router backends often report underlying model identifiers (such as `gemini-3.8-flash`
    /// or `glm-4`) into transcripts, but the router itself requires Claude-compatible aliases
    /// (`claude-fable-5`, `claude-opus-5`, etc.). This function translates:
    /// 1. Exact match with router model name (`name`).
    /// 2. Exact or prefix match with backend label override (`label`).
    /// 3. Family keyword heuristics (`gemini`, `fable`, `opus`, `sonnet`, `haiku`).
    /// 4. Fallback to the gateway's primary configured model.
    pub fn resolve_model(&self, requested: Option<&str>) -> Option<String> {
        let requested = requested.map(str::trim).filter(|s| !s.is_empty());

        if self.models.is_empty() {
            return requested.map(str::to_owned);
        }

        let Some(raw_req) = requested else {
            // No model requested: default to the gateway's flagship configured model (opus > sonnet > first).
            return self
                .models
                .iter()
                .find(|m| m.name.to_ascii_lowercase().contains("opus"))
                .or_else(|| {
                    self.models
                        .iter()
                        .find(|m| m.name.to_ascii_lowercase().contains("sonnet"))
                })
                .or_else(|| self.models.first())
                .map(|m| m.name.clone());
        };

        let req = raw_req
            .trim_end_matches(" [1m]")
            .trim_end_matches("[1m]")
            .trim_end_matches(" [1M]")
            .trim_end_matches("[1M]")
            .trim();
        let req = if req.is_empty() { raw_req } else { req };

        // 1. Exact name match (case-insensitive)
        if let Some(m) = self
            .models
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(raw_req) || m.name.eq_ignore_ascii_case(req))
        {
            return Some(m.name.clone());
        }

        // 2. Exact label match (case-insensitive)
        if let Some(m) = self.models.iter().find(|m| {
            m.label
                .as_deref()
                .is_some_and(|l| l.eq_ignore_ascii_case(req))
        }) {
            return Some(m.name.clone());
        }

        // 3. Prefix / Substring match on label (e.g. transcript says "gemini-3.8-flash"
        // and labelOverride is "gemini-3.8-flash-high" or vice-versa)
        let req_lower = req.to_ascii_lowercase();
        for m in &self.models {
            if let Some(label) = &m.label {
                let label_lower = label.to_ascii_lowercase();
                if label_lower.starts_with(&req_lower) || req_lower.starts_with(&label_lower) {
                    return Some(m.name.clone());
                }
            }
        }

        // 4. Family keyword heuristics
        for keyword in ["gemini", "fable", "opus", "sonnet", "haiku"] {
            if req_lower.contains(keyword)
                && let Some(m) = self.models.iter().find(|m| {
                    m.name.to_ascii_lowercase().contains(keyword)
                        || m.label
                            .as_deref()
                            .is_some_and(|l| l.to_ascii_lowercase().contains(keyword))
                })
            {
                return Some(m.name.clone());
            }
        }

        // 5. Fallback: if the requested model doesn't exist in the gateway's whitelist,
        // use the gateway's flagship model (opus > sonnet > first) to avoid router 400/502/429 errors.
        self.models
            .iter()
            .find(|m| m.name.to_ascii_lowercase().contains("opus"))
            .or_else(|| {
                self.models
                    .iter()
                    .find(|m| m.name.to_ascii_lowercase().contains("sonnet"))
            })
            .or_else(|| self.models.first())
            .map(|m| m.name.clone())
    }

    /// Injects the discovered gateway environment variables and resolved model into a child command.
    pub fn inject_into_command(&self, command: &mut Command, model_override: &mut Option<String>) {
        self.inject_into_command_with_session(command, model_override, None);
    }

    /// Injects the discovered gateway environment variables, session affinity headers, and resolved model into a child command.
    pub fn inject_into_command_with_session(
        &self,
        command: &mut Command,
        model_override: &mut Option<String>,
        session_id: Option<&str>,
    ) {
        // A discovered profile must reach the CLI as a pair. Keeping only the
        // parent process's base URL while injecting a different token produces
        // a hybrid endpoint/credential that no gateway can authenticate.
        command.env("ANTHROPIC_BASE_URL", &self.base_url);
        command.env("ANTHROPIC_AUTH_TOKEN", &self.auth_token);
        command.env("CLAUDE_CODE_HOST_AUTH_ENV_VAR", "ANTHROPIC_AUTH_TOKEN");
        // Bound retries on third-party gateways to prevent 10-retry 5-minute freeze on 429
        if env::var_os("CLAUDE_CODE_MAX_RETRIES").is_none() {
            command.env("CLAUDE_CODE_MAX_RETRIES", "2");
        }

        // Session Affinity & Prompt Cache Protection:
        // Inject session identifier into headers / environment so upstream gateways (e.g. Magpie, CC Switch)
        // can stick to the same upstream account, maximizing prompt cache hits and reducing latency.
        if let Some(sid) = session_id.map(str::trim).filter(|s| !s.is_empty()) {
            let clean_sid = sid
                .strip_prefix("claude_")
                .or_else(|| sid.strip_prefix("antigravity_"))
                .or_else(|| sid.strip_prefix("grok_"))
                .unwrap_or(sid)
                .trim();
            if crate::collectors::process::validate_safe_session_id(clean_sid) {
                command.env("X_MAGPIE_SESSION", clean_sid);
                command.env("X_CONVERSATION_ID", clean_sid);
                command.env("CLAUDE_CODE_CONVERSATION_ID", clean_sid);

                if env::var_os("ANTHROPIC_CUSTOM_HEADERS").is_none() {
                    command.env(
                        "ANTHROPIC_CUSTOM_HEADERS",
                        format!(
                            "x-session-affinity={clean_sid},x-magpie-session={clean_sid},x-conversation-id={clean_sid}"
                        ),
                    );
                }
            }
        }

        match self.model_namespace {
            GatewayModelNamespace::RouteAlias => {
                if let Some(resolved) = self.resolve_model(model_override.as_deref()) {
                    *model_override = Some(resolved);
                }
            }
            GatewayModelNamespace::Upstream => {
                let requested = model_override.take().map(|requested| {
                    Self::passthrough_model(Some(&requested)).unwrap_or(requested)
                });
                let alias_requested = requested
                    .as_deref()
                    .is_some_and(|model| model.to_ascii_lowercase().starts_with("claude-"));
                // Only this branch bypasses the local CCS translator.
                let desktop_bypass = self.source.starts_with("cc-switch:upstream:");
                // A gateway whose own catalog advertises `claude-*` ids speaks
                // the alias contract itself; rewriting those names would break it.
                let gateway_speaks_aliases = self
                    .models
                    .iter()
                    .any(|m| m.name.to_ascii_lowercase().starts_with("claude-"));
                let rewrite_aliases = alias_requested && !gateway_speaks_aliases;
                *model_override = requested;
                if rewrite_aliases || (desktop_bypass && model_override.is_none()) {
                    let real_models: Vec<ClaudeGatewayModel> = if self
                        .models
                        .iter()
                        .any(|m| !m.name.to_ascii_lowercase().starts_with("claude-"))
                    {
                        self.models.clone()
                    } else if desktop_bypass {
                        discover_cc_switch_provider_for_app("claude-desktop")
                            .map(|cc| cc.upstream_models)
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    if alias_requested
                        && let Some(mapped) = self.upstream_model_for_route_alias(
                            model_override.as_deref(),
                            &discover_config_library_models(),
                            &real_models,
                        )
                    {
                        *model_override = Some(mapped);
                    } else if model_override
                        .as_deref()
                        .is_none_or(|model| model.to_ascii_lowercase().starts_with("claude-"))
                    {
                        let configured_default = default_upstream_model_for_claude_code();
                        *model_override = preferred_upstream_default(
                            &real_models,
                            model_override.as_deref(),
                            configured_default.as_deref(),
                        );
                    }
                }
            }
        }
    }
}

pub(crate) fn preferred_upstream_default(
    upstream_models: &[ClaudeGatewayModel],
    requested: Option<&str>,
    configured_default: Option<&str>,
) -> Option<String> {
    let real: Vec<&ClaudeGatewayModel> = upstream_models
        .iter()
        .filter(|m| !m.name.to_ascii_lowercase().starts_with("claude-"))
        .collect();
    if let Some(default) = configured_default
        && real.iter().any(|m| m.name.eq_ignore_ascii_case(default))
    {
        return Some(default.to_string());
    }
    let family = requested.map(str::to_ascii_lowercase).and_then(|lower| {
        ["opus", "sonnet", "fable", "haiku"]
            .into_iter()
            .find(|word| lower.contains(word))
            .map(str::to_string)
    });
    real.iter()
        .find(|m| {
            family
                .as_deref()
                .is_some_and(|word| m.name.to_ascii_lowercase().contains(word))
        })
        .or_else(|| real.first())
        .map(|m| m.name.clone())
        .or_else(|| configured_default.map(str::to_string))
}

pub(crate) type UpstreamModelsCacheKey = (String, u64);
pub(crate) type UpstreamModelsCacheEntry = (Instant, Duration, Vec<ClaudeGatewayModel>);

pub(crate) static UPSTREAM_MODELS_CACHE: RwLock<
    Option<HashMap<UpstreamModelsCacheKey, UpstreamModelsCacheEntry>>,
> = RwLock::new(None);

/// Rejects empty values, Electron DPAPI-sealed tokens (`__sealed__:*`), and CC Switch
/// proxy placeholders (`PROXY_MANAGED`) that cannot authenticate directly against an upstream.
pub(crate) fn is_usable_gateway_token(token: &str) -> bool {
    let trimmed = token.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("__sealed__:")
        && !trimmed.eq_ignore_ascii_case("PROXY_MANAGED")
}

pub(crate) fn is_token_compatible_with_endpoint(base_url: &str, token: &str) -> bool {
    let trimmed = token.trim();
    is_usable_gateway_token(trimmed)
        && (!trimmed.starts_with("ccs-") || is_cc_switch_endpoint(base_url))
}

pub(crate) fn token_fingerprint(token: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    token.trim().hash(&mut hasher);
    hasher.finish()
}

#[cfg(not(test))]
pub(crate) static ROUTE_SUMMARIES_CACHE: RwLock<Option<(Instant, Vec<ClientRouteSummary>)>> =
    RwLock::new(None);

#[cfg(not(test))]
pub(crate) static CLAUDE_CODE_GATEWAY_CACHE: RwLock<
    Option<(Instant, Option<ClaudeGatewayProfile>)>,
> = RwLock::new(None);

#[cfg(not(test))]
pub(crate) static CLAUDE_DESKTOP_GATEWAY_CACHE: RwLock<
    Option<(Instant, Option<ClaudeGatewayProfile>)>,
> = RwLock::new(None);

/// Invalidate in-memory cached upstream models, forcing a fresh fetch on the next call.
pub fn invalidate_claude_gateway_cache() {
    if let Ok(mut guard) = UPSTREAM_MODELS_CACHE.write() {
        *guard = None;
    }
    #[cfg(not(test))]
    if let Ok(mut guard) = ROUTE_SUMMARIES_CACHE.write() {
        *guard = None;
    }
    #[cfg(not(test))]
    if let Ok(mut guard) = CLAUDE_CODE_GATEWAY_CACHE.write() {
        *guard = None;
    }
    #[cfg(not(test))]
    if let Ok(mut guard) = CLAUDE_DESKTOP_GATEWAY_CACHE.write() {
        *guard = None;
    }
}

pub fn default_upstream_model_for_claude_code() -> Option<String> {
    let home = crate::device_name::home_dir()?;
    let settings = fs::read_to_string(home.join(".claude").join("settings.json")).ok()?;
    let value: Value = serde_json::from_str(&settings).ok()?;
    let env = value.get("env")?;
    for key in ["ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL"] {
        if let Some(model) = env.get(key).and_then(Value::as_str).map(str::trim)
            && !model.is_empty()
        {
            return Some(
                ClaudeGatewayProfile::passthrough_model(Some(model))
                    .unwrap_or_else(|| model.to_string()),
            );
        }
    }
    None
}
