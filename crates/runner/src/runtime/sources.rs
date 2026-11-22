use super::*;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error(transparent)]
    Collector(#[from] CollectorError),
    #[error(transparent)]
    ClaudeAdapter(#[from] ClaudeAdapterError),
    #[error(transparent)]
    CodexAdapter(#[from] CodexAdapterError),
    #[error(transparent)]
    AntigravityAdapter(#[from] AntigravityAdapterError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl SourceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Collector(error) => error.code(),
            Self::ClaudeAdapter(_) => "claude_payload_invalid",
            Self::CodexAdapter(_) => "codex_payload_invalid",
            Self::AntigravityAdapter(_) => "antigravity_payload_invalid",
            Self::Json(_) => "internal_json_failed",
        }
    }
}

pub trait SessionSource: Send {
    fn agent_type(&self) -> AgentType;

    fn collect(
        &mut self,
        meta: &ObservationMeta,
        redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError>;
}

pub struct ClaudeSessionSource {
    include_completed: bool,
}

impl ClaudeSessionSource {
    pub fn new(include_completed: bool) -> Self {
        Self { include_completed }
    }
}

impl SessionSource for ClaudeSessionSource {
    fn agent_type(&self) -> AgentType {
        AgentType::ClaudeCode
    }

    fn collect(
        &mut self,
        meta: &ObservationMeta,
        redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError> {
        let payload = collect_claude_transcripts(self.include_completed)?;
        Ok(claude::normalize_agents_json(&payload, meta, redactor)?)
    }
}

pub struct CodexSessionSource {
    binary: PathBuf,
    limit: u32,
    server: Option<CodexAppServer>,
    context: CodexThreadContext,
    timeout: Duration,
    local_only: bool,
}

impl CodexSessionSource {
    pub fn new(binary: impl AsRef<Path>, limit: u32) -> Self {
        Self::new_with_timeout(binary, limit, DEFAULT_COLLECTOR_TIMEOUT)
    }

    pub fn new_with_timeout(binary: impl AsRef<Path>, limit: u32, timeout: Duration) -> Self {
        Self {
            binary: binary.as_ref().to_owned(),
            limit,
            server: None,
            context: CodexThreadContext::default(),
            timeout,
            local_only: false,
        }
    }

    fn collect_result(
        &mut self,
    ) -> Result<(serde_json::Value, Vec<serde_json::Value>, bool), CollectorError> {
        if self.local_only {
            return collect_codex_local_threads(self.limit)
                .map(|result| (result, Vec::new(), true));
        }
        if self.server.is_none() {
            match CodexAppServer::start_with_timeout(&self.binary, self.timeout) {
                Ok(server) => self.server = Some(server),
                Err(CollectorError::Start { .. }) => {
                    self.local_only = true;
                    return collect_codex_local_threads(self.limit)
                        .map(|result| (result, Vec::new(), true));
                }
                Err(error) => return Err(error),
            }
        }
        let server = self
            .server
            .as_mut()
            .expect("Codex app-server was initialized");
        let result = server.list_threads(self.limit).and_then(|result| {
            server
                .drain_messages()
                .map(|messages| (result, messages, false))
        });
        if result.is_err() {
            self.server = None;
        }
        result
    }
}

impl SessionSource for CodexSessionSource {
    fn agent_type(&self) -> AgentType {
        AgentType::Codex
    }

    fn collect(
        &mut self,
        meta: &ObservationMeta,
        redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError> {
        let (result, messages, local) = self.collect_result()?;
        normalize_codex_collection(&result, messages, meta, redactor, &mut self.context, local)
    }
}

pub struct AntigravitySessionSource {
    include_completed: bool,
}

impl AntigravitySessionSource {
    pub fn new(include_completed: bool) -> Self {
        Self { include_completed }
    }
}

impl SessionSource for AntigravitySessionSource {
    fn agent_type(&self) -> AgentType {
        AgentType::Antigravity
    }

    fn collect(
        &mut self,
        meta: &ObservationMeta,
        redactor: &ProjectRedactor,
    ) -> Result<Vec<SessionObservation>, SourceError> {
        let payload = collect_antigravity_sessions(self.include_completed)?;
        let sessions: Vec<serde_json::Value> = serde_json::from_str(&payload)?;
        let mut observations = Vec::new();
        for session in sessions {
            let session_id = session
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let cwd = session
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            let started_at = session
                .get("startedAt")
                .and_then(|v| v.as_i64())
                .and_then(DateTime::from_timestamp_millis);
            let last_activity_at = session
                .get("lastActivityAt")
                .and_then(|v| v.as_i64())
                .and_then(DateTime::from_timestamp_millis);
            let steps_val = session
                .get("steps")
                .cloned()
                .unwrap_or(serde_json::json!([]));
            let steps: Vec<crate::adapters::antigravity::AntigravityStep> =
                serde_json::from_value(steps_val).unwrap_or_default();
            let session_meta = crate::adapters::antigravity::AntigravitySessionMetadata {
                session_id,
                cwd,
                is_process_alive: true,
                started_at,
                last_activity_at,
            };
            if let Some(obs) = crate::adapters::antigravity::normalize_transcript_steps(
                &steps,
                &session_meta,
                meta,
                redactor,
            ) {
                observations.push(obs);
            }
        }
        Ok(observations)
    }
}

pub(crate) fn normalize_codex_collection(
    result: &serde_json::Value,
    messages: Vec<serde_json::Value>,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
    context: &mut CodexThreadContext,
    local: bool,
) -> Result<Vec<SessionObservation>, SourceError> {
    let payload = serde_json::to_string(result)?;
    let mut observations = if local {
        codex::normalize_local_thread_list_result(&payload, meta, redactor, context)?
    } else {
        codex::normalize_thread_list_result(&payload, meta, redactor, context)?
    };
    for message in messages {
        let payload = serde_json::to_string(&message)?;
        if let Some(observation) =
            codex::normalize_app_server_message(&payload, meta, redactor, context)?
        {
            observations.push(observation);
        }
    }
    Ok(observations)
}
