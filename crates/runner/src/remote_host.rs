//! Native session control core. This is local IPC, not the legacy v2 wire format.
//! A Host owns one long-lived app-server and never resumes arbitrary live processes.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::collectors::{ClaudeProcessSession, CodexAppServer, ZCodeAppServer};
use crate::domain::AgentType;

/// In-memory replay window for command receipts.
///
/// The daemon's durable ledger (`remote_journal::RemoteJournal`) is the
/// authority for "has this command already run", so this bound only needs to
/// cover repeats seen while the Host is alive. Oldest entries are evicted
/// first; a command whose id is gone from here is answered from the journal
/// rather than re-executed.
pub const MAX_COMMAND_LEDGER_ENTRIES: usize = 10_000;

/// In-memory replay window for answered approval/question request ids.
///
/// Bounds re-delivery suppression so the set cannot grow for the life of a
/// long-lived daemon. Comfortably above the number of requests a single
/// provider re-delivers after a response.
pub const MAX_ANSWERED_REQUESTS: usize = 10_000;

/// Protocol errors are safe codes; provider payloads belong to the content stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    pub code: String,
    pub outcome_unknown: bool,
}

impl NativeError {
    fn rejected(code: impl AsRef<str>) -> Self {
        Self {
            code: code.as_ref().to_owned(),
            outcome_unknown: false,
        }
    }

    fn uncertain(code: impl AsRef<str>) -> Self {
        Self {
            code: code.as_ref().to_owned(),
            outcome_unknown: true,
        }
    }
}

pub trait NativeTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError>;
    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError>;
    fn drain(&mut self) -> Result<Vec<Value>, NativeError>;
}

impl NativeTransport for CodexAppServer {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
        self.request(method, params).map_err(|error| match error {
            crate::collectors::CollectorError::Protocol { .. } => {
                NativeError::rejected("provider_rejected")
            }
            _ => NativeError::uncertain("provider_transport_lost"),
        })
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        self.respond(id, result)
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }

    fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
        self.drain_messages()
            .map_err(|_| NativeError::uncertain("provider_transport_lost"))
    }
}

pub struct MultiProviderTransport {
    codex: Option<CodexAppServer>,
    zcode: Option<ZCodeAppServer>,
    zcode_active_turns: HashMap<String, String>,
    zcode_buffered_events: VecDeque<Value>,
    zcode_cached_cwds: HashMap<String, PathBuf>,
    claude_binary: PathBuf,
    claude_active_turns: HashMap<String, String>,
    claude_cancels: HashMap<String, Arc<AtomicBool>>,
    claude_buffered_events: Arc<Mutex<VecDeque<Value>>>,
    claude_cached_cwds: HashMap<String, PathBuf>,
    claude_cached_models: HashMap<String, String>,
    claude_sessions: HashMap<String, ClaudeProcessSession>,
    antigravity_binary: PathBuf,
    antigravity_active_turns: HashMap<String, String>,
    antigravity_cancels: HashMap<String, Arc<AtomicBool>>,
    antigravity_buffered_events: Arc<Mutex<VecDeque<Value>>>,
    antigravity_cached_cwds: HashMap<String, PathBuf>,
    grok_binary: PathBuf,
    grok_active_turns: HashMap<String, String>,
    grok_cancels: HashMap<String, Arc<AtomicBool>>,
    grok_buffered_events: Arc<Mutex<VecDeque<Value>>>,
    grok_cached_cwds: HashMap<String, PathBuf>,
}

impl MultiProviderTransport {
    pub fn new(codex: Option<CodexAppServer>, zcode: Option<ZCodeAppServer>) -> Self {
        Self {
            codex,
            zcode,
            zcode_active_turns: HashMap::new(),
            zcode_buffered_events: VecDeque::new(),
            zcode_cached_cwds: HashMap::new(),
            claude_binary: crate::collectors::discover_claude_binary(),
            claude_active_turns: HashMap::new(),
            claude_cancels: HashMap::new(),
            claude_buffered_events: Arc::new(Mutex::new(VecDeque::new())),
            claude_cached_cwds: HashMap::new(),
            claude_cached_models: HashMap::new(),
            claude_sessions: HashMap::new(),
            antigravity_binary: crate::collectors::discover_antigravity_binary(),
            antigravity_active_turns: HashMap::new(),
            antigravity_cancels: HashMap::new(),
            antigravity_buffered_events: Arc::new(Mutex::new(VecDeque::new())),
            antigravity_cached_cwds: HashMap::new(),
            grok_binary: crate::collectors::discover_grok_binary(),
            grok_active_turns: HashMap::new(),
            grok_cancels: HashMap::new(),
            grok_buffered_events: Arc::new(Mutex::new(VecDeque::new())),
            grok_cached_cwds: HashMap::new(),
        }
    }

    fn ensure_zcode(&mut self) -> Result<&mut ZCodeAppServer, NativeError> {
        if self.zcode.is_none()
            && let Ok(server) = ZCodeAppServer::start_if_available(Duration::from_secs(20))
        {
            self.zcode = Some(server);
        }
        self.zcode
            .as_mut()
            .ok_or_else(|| NativeError::rejected("zcode_not_available"))
    }

    fn get_or_spawn_claude_session(
        &mut self,
        session_id: &str,
        cwd: &Path,
        model: Option<&str>,
        effort: Option<&str>,
        permission_mode: Option<&str>,
    ) -> Result<&mut ClaudeProcessSession, NativeError> {
        if !self.claude_sessions.contains_key(session_id) {
            let raw_id = session_id.trim_start_matches("claude_");
            let resume_id = if raw_id.is_empty() {
                None
            } else if crate::collectors::find_claude_session_info(raw_id)
                .ok()
                .flatten()
                .is_some()
            {
                Some(raw_id)
            } else {
                None
            };

            let options = crate::collectors::TurnOptions {
                model,
                reasoning_effort: effort,
                permission_mode,
                ..Default::default()
            };

            let session = ClaudeProcessSession::start(&self.claude_binary, cwd, resume_id, options)
                .map_err(|e| NativeError::rejected(format!("claude_start_failed: {}", e)))?;
            self.claude_sessions.insert(session_id.to_string(), session);
        }

        self.claude_sessions
            .get_mut(session_id)
            .ok_or_else(|| NativeError::rejected("claude_session_not_found"))
    }
}

impl NativeTransport for MultiProviderTransport {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
        let is_zcode = params
            .get("threadId")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("sess_"));

        let is_claude = params
            .get("threadId")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("claude_"));

        let is_antigravity = params
            .get("threadId")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("antigravity_"));

        let is_grok = params
            .get("threadId")
            .or_else(|| params.get("sessionId"))
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("grok_"));

        let agent_type = params
            .get("agentType")
            .and_then(Value::as_str)
            .unwrap_or("");

        match method {
            "thread/start" if agent_type == "zcode" => {
                let cwd = params["cwd"].as_str().unwrap_or(".").to_string();
                let zcode = self.ensure_zcode()?;
                let res = zcode.session_create(&cwd).map_err(|error| match error {
                    crate::collectors::CollectorError::Protocol { ref message, .. } => {
                        NativeError::rejected(message.as_str())
                    }
                    _ => NativeError::uncertain("provider_transport_lost"),
                })?;
                let session_id = res
                    .pointer("/session/sessionId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| NativeError::uncertain("provider_missing_native_id"))?
                    .to_string();
                self.zcode_cached_cwds
                    .insert(session_id.clone(), PathBuf::from(&cwd));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/start" if agent_type == "claude_code" => {
                let cwd = params["cwd"].as_str().unwrap_or(".").to_string();
                let session_id = format!("claude_{}", uuid::Uuid::new_v4());
                self.claude_cached_cwds
                    .insert(session_id.clone(), PathBuf::from(&cwd));
                if let Some(m) = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.claude_cached_models
                        .insert(session_id.clone(), m.to_string());
                }
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/start" if agent_type == "antigravity" => {
                let cwd = params["cwd"].as_str().unwrap_or(".").to_string();
                let session_id = format!("antigravity_{}", uuid::Uuid::new_v4());
                self.antigravity_cached_cwds
                    .insert(session_id.clone(), PathBuf::from(&cwd));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/start" if agent_type == "grok_build" => {
                let cwd = params["cwd"].as_str().unwrap_or(".").to_string();
                let session_id = format!("grok_{}", uuid::Uuid::new_v4());
                self.grok_cached_cwds
                    .insert(session_id.clone(), PathBuf::from(&cwd));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd,
                        "status": { "type": "idle" }
                    }
                }))
            }
            "thread/read" if is_claude => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_id = session_id.trim_start_matches("claude_");
                let (cwd, title) =
                    if let Ok(Some((c, t))) = crate::collectors::find_claude_session_info(raw_id) {
                        (c, t)
                    } else {
                        (PathBuf::from("."), None)
                    };
                self.claude_cached_cwds
                    .insert(session_id.to_string(), cwd.clone());
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" },
                        "name": title.unwrap_or_else(|| raw_id.to_string())
                    }
                }))
            }
            "thread/resume" if is_claude => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                if let Some(m) = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    self.claude_cached_models
                        .insert(session_id.to_string(), m.to_string());
                }
                let cwd = self
                    .claude_cached_cwds
                    .get(session_id)
                    .cloned()
                    .or_else(|| {
                        let raw_id = session_id.trim_start_matches("claude_");
                        crate::collectors::find_claude_session_info(raw_id)
                            .ok()
                            .flatten()
                            .map(|(c, _)| c)
                    })
                    .unwrap_or_else(|| PathBuf::from("."));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" }
                    }
                }))
            }
            "turn/start" if is_claude => {
                let session_id = params["threadId"].as_str().unwrap_or("").to_string();
                let raw_id = session_id.trim_start_matches("claude_").to_string();
                let session_exists = params.get("sessionExists").and_then(Value::as_bool);
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("")
                    .to_string();
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());
                self.claude_active_turns
                    .insert(session_id.clone(), turn_id.clone());
                let cancel = Arc::new(AtomicBool::new(false));
                self.claude_cancels
                    .insert(session_id.clone(), cancel.clone());

                let events_tx = self.claude_buffered_events.clone();
                if let Ok(mut q) = events_tx.lock() {
                    q.push_back(json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": session_id,
                            "turn": {
                                "id": turn_id.clone(),
                                "status": { "type": "inProgress" }
                            }
                        }
                    }));
                }

                let cwd = self.claude_cached_cwds.get(&session_id).cloned();
                let model = params["model"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .or_else(|| self.claude_cached_models.get(&session_id).cloned());
                if let Some(m) = &model {
                    self.claude_cached_models
                        .insert(session_id.clone(), m.clone());
                }
                let effort = params["effort"]
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);

                // Attempt persistent bidirectional session driving
                if let Some(ref cwd_path) = cwd {
                    let permission_mode = params["permissionMode"].as_str();
                    if let Ok(session) = self.get_or_spawn_claude_session(
                        &session_id,
                        cwd_path,
                        model.as_deref(),
                        effort.as_deref(),
                        permission_mode,
                    ) && session.send_prompt(&turn_id, &text).is_ok()
                    {
                        return Ok(json!({
                            "turn": {
                                "id": turn_id,
                                "status": { "type": "inProgress" }
                            }
                        }));
                    }
                }

                let binary = self.claude_binary.clone();
                let session_id_clone = session_id.clone();
                let turn_id_clone = turn_id.clone();

                std::thread::spawn(move || {
                    let events_queue = events_tx.clone();
                    let s_id = session_id_clone.clone();
                    let t_id = turn_id_clone.clone();
                    let on_update = {
                        let events_queue = events_queue.clone();
                        let s_id = s_id.clone();
                        let t_id = t_id.clone();
                        move |update: crate::collectors::AgentTurnUpdate| match update {
                            crate::collectors::AgentTurnUpdate::AssistantDelta(delta) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "claude_msg",
                                            "delta": delta
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::AssistantMessage(msg) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "claude_msg",
                                            "delta": msg
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::Activity(_) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/started",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "item": {
                                                "id": "claude_tool",
                                                "type": "commandExecution"
                                            }
                                        }
                                    }));
                                }
                            }
                        }
                    };

                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::collectors::run_agent_session_turn_streaming(
                            crate::domain::AgentType::ClaudeCode,
                            binary,
                            cwd.as_deref(),
                            &raw_id,
                            &text,
                            crate::collectors::TurnOptions {
                                model: model.as_deref(),
                                reasoning_effort: effort.as_deref(),
                                session_exists,
                                ..Default::default()
                            },
                            Duration::from_secs(600),
                            &cancel,
                            on_update,
                        )
                    }));

                    if let Ok(mut q) = events_queue.lock() {
                        let status_type = if cancel.load(Ordering::Relaxed) {
                            "interrupted"
                        } else if matches!(outcome, Ok(Ok(_))) {
                            "completed"
                        } else {
                            "failed"
                        };
                        let mut completed_params = json!({
                            "threadId": session_id_clone,
                            "turn": {
                                "id": turn_id_clone,
                                "status": status_type
                            }
                        });
                        if let Ok(Ok(ref res)) = outcome {
                            completed_params["turn"]["output"] = json!(&res.response);
                            if let Some(ref real_id) = res.session_id {
                                completed_params["realSessionId"] =
                                    json!(format!("claude_{}", real_id));
                            }
                        } else if let Ok(Err(ref err)) = outcome {
                            completed_params["turn"]["error"] = json!(err.to_string());
                        } else if outcome.is_err() {
                            completed_params["turn"]["error"] = json!("进程执行异常中断");
                        }
                        q.push_back(json!({
                            "method": "turn/completed",
                            "params": completed_params
                        }));
                    }
                });

                Ok(json!({
                    "turn": {
                        "id": turn_id,
                        "status": { "type": "inProgress" }
                    }
                }))
            }
            "turn/interrupt" if is_claude => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                if let Some(session) = self.claude_sessions.get_mut(session_id) {
                    let _ = session.interrupt();
                }
                if let Some(cancel) = self.claude_cancels.get(session_id) {
                    cancel.store(true, Ordering::SeqCst);
                }
                self.claude_active_turns.remove(session_id);
                Ok(json!({"ok": true}))
            }
            "thread/archive" | "thread/unarchive" if is_claude => Ok(json!({"ok": true})),
            "thread/read" if is_antigravity => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_id = session_id.trim_start_matches("antigravity_");
                let (cwd, title) = if let Ok(Some((c, t))) =
                    crate::collectors::find_antigravity_session_info(raw_id)
                {
                    (c, t)
                } else {
                    (PathBuf::from("."), None)
                };
                self.antigravity_cached_cwds
                    .insert(session_id.to_string(), cwd.clone());
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" },
                        "name": title.unwrap_or_else(|| raw_id.to_string())
                    }
                }))
            }
            "thread/resume" if is_antigravity => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let cwd = self
                    .antigravity_cached_cwds
                    .get(session_id)
                    .cloned()
                    .or_else(|| {
                        let raw_id = session_id.trim_start_matches("antigravity_");
                        crate::collectors::find_antigravity_session_info(raw_id)
                            .ok()
                            .flatten()
                            .map(|(c, _)| c)
                    })
                    .unwrap_or_else(|| PathBuf::from("."));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" }
                    }
                }))
            }
            "turn/start" if is_antigravity => {
                let session_id = params["threadId"].as_str().unwrap_or("").to_string();
                let raw_id = session_id.trim_start_matches("antigravity_").to_string();
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("")
                    .to_string();
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());
                self.antigravity_active_turns
                    .insert(session_id.clone(), turn_id.clone());

                let cancel = Arc::new(AtomicBool::new(false));
                self.antigravity_cancels
                    .insert(session_id.clone(), cancel.clone());

                let events_tx = self.antigravity_buffered_events.clone();
                if let Ok(mut q) = events_tx.lock() {
                    q.push_back(json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": session_id,
                            "turn": {
                                "id": turn_id.clone(),
                                "status": { "type": "inProgress" }
                            }
                        }
                    }));
                }

                let cwd = self.antigravity_cached_cwds.get(&session_id).cloned();
                let binary = self.antigravity_binary.clone();
                let session_id_clone = session_id.clone();
                let turn_id_clone = turn_id.clone();

                std::thread::spawn(move || {
                    let events_queue = events_tx.clone();
                    let s_id = session_id_clone.clone();
                    let t_id = turn_id_clone.clone();
                    let on_update = {
                        let events_queue = events_queue.clone();
                        let s_id = s_id.clone();
                        let t_id = t_id.clone();
                        move |update: crate::collectors::AgentTurnUpdate| match update {
                            crate::collectors::AgentTurnUpdate::AssistantDelta(delta) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "antigravity_msg",
                                            "delta": delta
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::AssistantMessage(msg) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "antigravity_msg",
                                            "delta": msg
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::Activity(_) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/started",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "item": {
                                                "id": "antigravity_tool",
                                                "type": "commandExecution"
                                            }
                                        }
                                    }));
                                }
                            }
                        }
                    };

                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::collectors::run_agent_session_turn_streaming(
                            crate::domain::AgentType::Antigravity,
                            binary,
                            cwd.as_deref(),
                            &raw_id,
                            &text,
                            crate::collectors::TurnOptions::default(),
                            Duration::from_secs(600),
                            &cancel,
                            on_update,
                        )
                    }));

                    if let Ok(mut q) = events_queue.lock() {
                        let status_type = if cancel.load(Ordering::Relaxed) {
                            "interrupted"
                        } else if matches!(outcome, Ok(Ok(_))) {
                            "completed"
                        } else {
                            "failed"
                        };
                        let mut completed_params = json!({
                            "threadId": session_id_clone,
                            "turn": {
                                "id": turn_id_clone,
                                "status": status_type
                            }
                        });
                        if let Ok(Ok(ref res)) = outcome {
                            completed_params["turn"]["output"] = json!(&res.response);
                        } else if let Ok(Err(ref err)) = outcome {
                            completed_params["turn"]["error"] = json!(err.to_string());
                        } else if outcome.is_err() {
                            completed_params["turn"]["error"] = json!("进程执行异常中断");
                        }
                        q.push_back(json!({
                            "method": "turn/completed",
                            "params": completed_params
                        }));
                    }
                });

                Ok(json!({
                    "turn": {
                        "id": turn_id,
                        "status": { "type": "inProgress" }
                    }
                }))
            }
            "turn/interrupt" if is_antigravity => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                if let Some(cancel) = self.antigravity_cancels.get(session_id) {
                    cancel.store(true, Ordering::SeqCst);
                }
                self.antigravity_active_turns.remove(session_id);
                Ok(json!({"ok": true}))
            }
            "thread/archive" | "thread/unarchive" if is_antigravity => Ok(json!({"ok": true})),
            "thread/read" if is_grok => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let raw_id = session_id.trim_start_matches("grok_");
                let (cwd, title) =
                    if let Ok(Some((c, t))) = crate::collectors::find_grok_session_info(raw_id) {
                        (c, t)
                    } else {
                        (PathBuf::from("."), None)
                    };
                self.grok_cached_cwds
                    .insert(session_id.to_string(), cwd.clone());
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" },
                        "name": title.unwrap_or_else(|| raw_id.to_string())
                    }
                }))
            }
            "thread/resume" if is_grok => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let cwd = self
                    .grok_cached_cwds
                    .get(session_id)
                    .cloned()
                    .or_else(|| {
                        let raw_id = session_id.trim_start_matches("grok_");
                        crate::collectors::find_grok_session_info(raw_id)
                            .ok()
                            .flatten()
                            .map(|(c, _)| c)
                    })
                    .unwrap_or_else(|| PathBuf::from("."));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" }
                    }
                }))
            }
            "turn/start" if is_grok => {
                let session_id = params["threadId"].as_str().unwrap_or("").to_string();
                let raw_id = session_id.trim_start_matches("grok_").to_string();
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("")
                    .to_string();
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());
                self.grok_active_turns
                    .insert(session_id.clone(), turn_id.clone());

                let cancel = Arc::new(AtomicBool::new(false));
                self.grok_cancels.insert(session_id.clone(), cancel.clone());

                let events_tx = self.grok_buffered_events.clone();
                if let Ok(mut q) = events_tx.lock() {
                    q.push_back(json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": session_id,
                            "turn": {
                                "id": turn_id.clone(),
                                "status": { "type": "inProgress" }
                            }
                        }
                    }));
                }

                let cwd = self.grok_cached_cwds.get(&session_id).cloned();
                let binary = self.grok_binary.clone();
                let session_id_clone = session_id.clone();
                let turn_id_clone = turn_id.clone();

                std::thread::spawn(move || {
                    let events_queue = events_tx.clone();
                    let s_id = session_id_clone.clone();
                    let t_id = turn_id_clone.clone();
                    let on_update = {
                        let events_queue = events_queue.clone();
                        let s_id = s_id.clone();
                        let t_id = t_id.clone();
                        move |update: crate::collectors::AgentTurnUpdate| match update {
                            crate::collectors::AgentTurnUpdate::AssistantDelta(delta) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "grok_msg",
                                            "delta": delta
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::AssistantMessage(msg) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "itemId": "grok_msg",
                                            "delta": msg
                                        }
                                    }));
                                }
                            }
                            crate::collectors::AgentTurnUpdate::Activity(_) => {
                                if let Ok(mut q) = events_queue.lock() {
                                    q.push_back(json!({
                                        "method": "item/started",
                                        "params": {
                                            "threadId": s_id,
                                            "turnId": t_id,
                                            "item": {
                                                "id": "grok_tool",
                                                "type": "commandExecution"
                                            }
                                        }
                                    }));
                                }
                            }
                        }
                    };

                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::collectors::run_agent_session_turn_streaming(
                            crate::domain::AgentType::GrokBuild,
                            binary,
                            cwd.as_deref(),
                            &raw_id,
                            &text,
                            crate::collectors::TurnOptions::default(),
                            Duration::from_secs(600),
                            &cancel,
                            on_update,
                        )
                    }));

                    if let Ok(mut q) = events_queue.lock() {
                        let status_type = if cancel.load(Ordering::Relaxed) {
                            "interrupted"
                        } else if matches!(outcome, Ok(Ok(_))) {
                            "completed"
                        } else {
                            "failed"
                        };
                        let mut completed_params = json!({
                            "threadId": session_id_clone,
                            "turn": {
                                "id": turn_id_clone,
                                "status": status_type
                            }
                        });
                        if let Ok(Ok(ref res)) = outcome {
                            completed_params["turn"]["output"] = json!(&res.response);
                        } else if let Ok(Err(ref err)) = outcome {
                            completed_params["turn"]["error"] = json!(err.to_string());
                        } else if outcome.is_err() {
                            completed_params["turn"]["error"] = json!("进程执行异常中断");
                        }
                        q.push_back(json!({
                            "method": "turn/completed",
                            "params": completed_params
                        }));
                    }
                });

                Ok(json!({
                    "turn": {
                        "id": turn_id,
                        "status": { "type": "inProgress" }
                    }
                }))
            }
            "turn/interrupt" if is_grok => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                if let Some(cancel) = self.grok_cancels.get(session_id) {
                    cancel.store(true, Ordering::SeqCst);
                }
                self.grok_active_turns.remove(session_id);
                Ok(json!({"ok": true}))
            }
            "thread/archive" | "thread/unarchive" if is_grok => Ok(json!({"ok": true})),
            "thread/read" if is_zcode => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let zcode = self.ensure_zcode()?;
                let resume_res = zcode
                    .session_resume(session_id)
                    .map_err(|error| match error {
                        crate::collectors::CollectorError::Protocol { .. } => {
                            NativeError::rejected("external_session_not_resumable")
                        }
                        _ => NativeError::uncertain("provider_transport_lost"),
                    })?;
                let raw_cwd = resume_res
                    .pointer("/session/workspace/workspacePath")
                    .and_then(Value::as_str)
                    .map(PathBuf::from)
                    .or_else(|| {
                        crate::collectors::find_zcode_session_cwd(session_id)
                            .ok()
                            .flatten()
                    })
                    .unwrap_or_else(|| PathBuf::from("."));
                let cwd = dunce::canonicalize(&raw_cwd).unwrap_or(raw_cwd);
                self.zcode_cached_cwds
                    .insert(session_id.to_string(), cwd.clone());
                let title = resume_res
                    .pointer("/session/title")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" },
                        "name": title
                    }
                }))
            }
            "thread/resume" if is_zcode => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let cwd = self
                    .zcode_cached_cwds
                    .get(session_id)
                    .cloned()
                    .or_else(|| {
                        crate::collectors::find_zcode_session_cwd(session_id)
                            .ok()
                            .flatten()
                    })
                    .unwrap_or_else(|| PathBuf::from("."));
                Ok(json!({
                    "thread": {
                        "id": session_id,
                        "cwd": cwd.to_string_lossy(),
                        "status": { "type": "idle" }
                    }
                }))
            }
            "turn/start" if is_zcode => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                let cached_cwd = self.zcode_cached_cwds.get(session_id).cloned();
                let zcode = self.ensure_zcode()?;
                // Ensure the session is loaded and subscribed on this zcode child process
                let _ = zcode.session_resume(session_id);
                if let Some(cwd) = cached_cwd {
                    let cwd_str = cwd.to_string_lossy();
                    let clean_cwd = cwd_str.trim_start_matches(r"\\?\");
                    let _ = zcode.sync_provider_registry_with_key(clean_cwd, clean_cwd);
                }
                let text = params["input"]
                    .as_array()
                    .and_then(|arr| arr.iter().find_map(|item| item["text"].as_str()))
                    .unwrap_or("");
                zcode
                    .session_send(session_id, text)
                    .map_err(|error| match error {
                        crate::collectors::CollectorError::Protocol { ref message, .. } => {
                            NativeError::rejected(message.as_str())
                        }
                        crate::collectors::CollectorError::Timeout {
                            ref program,
                            timeout_ms,
                        } => NativeError::uncertain(format!(
                            "zcode_timeout: {} after {}ms",
                            program, timeout_ms
                        )),
                        ref err => NativeError::uncertain(format!("zcode_send_failed: {}", err)),
                    })?;
                let turn_id = format!("turn_{}", uuid::Uuid::new_v4());
                self.zcode_active_turns
                    .insert(session_id.to_string(), turn_id.clone());
                self.zcode_buffered_events.push_back(json!({
                    "method": "turn/started",
                    "params": {
                        "threadId": session_id,
                        "turn": {
                            "id": turn_id.clone(),
                            "status": { "type": "inProgress" }
                        }
                    }
                }));
                Ok(json!({
                    "turn": {
                        "id": turn_id,
                        "status": { "type": "inProgress" }
                    }
                }))
            }
            "turn/interrupt" if is_zcode => {
                let session_id = params["threadId"].as_str().unwrap_or("");
                if let Some(ref mut zcode) = self.zcode {
                    let _ = zcode.session_stop(session_id);
                }
                Ok(json!({"ok": true}))
            }
            "thread/archive" | "thread/unarchive" if is_zcode => Ok(json!({"ok": true})),
            "thread/list" => {
                let is_first_page = params
                    .get("cursor")
                    .map_or(true, |c| c.is_null() || c.as_str().map_or(true, |s| s.is_empty()));
                let mut response = if let Some(ref mut codex) = self.codex {
                    codex
                        .request("thread/list", params)
                        .map_err(|error| match error {
                            crate::collectors::CollectorError::Protocol { .. } => {
                                NativeError::rejected("provider_rejected")
                            }
                            _ => NativeError::uncertain("provider_transport_lost"),
                        })?
                } else {
                    // A missing app-server must not hide locally discoverable Codex
                    // sessions. Metadata-only candidates cannot be resumed here.
                    crate::collectors::collect_codex_local_threads(5000)
                        .unwrap_or_else(|_| json!({"data": [], "nextCursor": null}))
                };
                if let Some(items) = response["data"].as_array_mut() {
                    for item in items {
                        item["provider"] = json!("codex");
                        item["agent_name"] = json!("codex");
                        item["resumable"] = json!(self.codex.is_some());
                    }
                }
                if is_first_page {
                    if let Ok(Some(db_path)) = crate::collectors::discover_zcode_database()
                        && let Ok(payload) =
                            crate::collectors::collect_zcode_sessions_from(&db_path, true)
                        && let Ok(entries) = serde_json::from_str::<
                            Vec<crate::adapters::zcode::ZCodeSessionEntry>,
                        >(&payload)
                        && let Some(items) = response["data"].as_array_mut()
                    {
                        for entry in entries.into_iter().take(5000) {
                            let pt = entry.input_tokens.unwrap_or(0);
                            let ct = entry.output_tokens.unwrap_or(0);
                            let tt = entry.total_tokens.unwrap_or(pt + ct);
                            items.push(json!({
                                "id": entry.session_id,
                                "session_id": entry.session_id,
                                "cwd": entry.cwd,
                                "workspace_path": entry.cwd,
                                "name": entry.title.clone().unwrap_or_else(|| entry.session_id.clone()),
                                "title": entry.title,
                                "provider": "zcode",
                                "agent_name": "zcode",
                                "model": entry.model,
                                "total_tokens": tt,
                                "prompt_tokens": pt,
                                "completion_tokens": ct,
                                "cache_read_tokens": entry.cache_read_tokens.unwrap_or(0),
                                "last_activity_at_ms": entry.time_updated,
                                "time_updated": entry.time_updated,
                                "time_created": entry.time_created,
                                "resumable": true,
                                "status": { "type": "idle" }
                            }));
                        }
                    }
                    if let Ok(claude_entries) = crate::collectors::collect_claude_sessions_for_remote()
                        && let Some(items) = response["data"].as_array_mut()
                    {
                        for entry in claude_entries {
                            items.push(json!({
                                "id": format!("claude_{}", entry.session_id),
                                "session_id": format!("claude_{}", entry.session_id),
                                "cwd": entry.cwd,
                                "workspace_path": entry.cwd,
                                "name": entry.title.clone().unwrap_or_else(|| entry.session_id.clone()),
                                "title": entry.title,
                                "provider": "claude_code",
                                "agent_name": "claude_code",
                                "model": entry.model,
                                "prompt_tokens": entry.prompt_tokens,
                                "completion_tokens": entry.completion_tokens,
                                "total_tokens": entry.prompt_tokens + entry.completion_tokens,
                                "cache_read_tokens": entry.cache_read_tokens,
                                "last_activity_at_ms": entry.last_activity_at_ms,
                                "status": { "type": "idle" }
                            }));
                        }
                    }
                    if let Ok(antigravity_entries) =
                        crate::collectors::collect_antigravity_sessions_for_remote()
                        && let Some(items) = response["data"].as_array_mut()
                    {
                        for (session_id, cwd, title, modified_at_ms) in antigravity_entries {
                            items.push(json!({
                                "id": format!("antigravity_{}", session_id),
                                "session_id": format!("antigravity_{}", session_id),
                                "cwd": cwd,
                                "workspace_path": cwd,
                                "name": title.clone().unwrap_or_else(|| session_id.clone()),
                                "title": title,
                                "provider": "antigravity",
                                "agent_name": "antigravity",
                                "last_activity_at_ms": modified_at_ms,
                                "status": { "type": "idle" }
                            }));
                        }
                    }
                    if let Ok(grok_entries) = crate::collectors::collect_grok_sessions_for_remote()
                        && let Some(items) = response["data"].as_array_mut()
                    {
                        for (session_id, cwd, title, model, time_ms) in grok_entries {
                            items.push(json!({
                                "id": format!("grok_{}", session_id),
                                "session_id": format!("grok_{}", session_id),
                                "cwd": cwd,
                                "workspace_path": cwd,
                                "name": title.clone().unwrap_or_else(|| session_id.clone()),
                                "title": title,
                                "provider": "grok_build",
                                "agent_name": "grok_build",
                                "model": model,
                                "last_activity_at_ms": time_ms,
                                "status": { "type": "idle" }
                            }));
                        }
                    }
                }
                if let Some(items) = response["data"].as_array_mut() {
                    items.sort_by_key(|item| {
                        let ms = item.get("last_activity_at_ms")
                            .and_then(Value::as_i64)
                            .or_else(|| item.get("updatedAt").and_then(Value::as_i64))
                            .or_else(|| item.get("time_updated").and_then(Value::as_i64))
                            .or_else(|| item.get("recencyAt").and_then(Value::as_i64))
                            .or_else(|| item.get("createdAt").and_then(Value::as_i64))
                            .unwrap_or(0);
                        std::cmp::Reverse(ms)
                    });
                }
                Ok(response)
            }
            _ => {
                if let Some(ref mut codex) = self.codex {
                    codex.request(method, params).map_err(|error| match error {
                        crate::collectors::CollectorError::Protocol { .. } => {
                            NativeError::rejected("provider_rejected")
                        }
                        _ => NativeError::uncertain("provider_transport_lost"),
                    })
                } else {
                    Err(NativeError::rejected("provider_not_available"))
                }
            }
        }
    }

    fn respond(&mut self, id: Value, result: Value) -> Result<(), NativeError> {
        // If request ID is string and starts with "claude_req_", route to Claude session
        if let Some(id_str) = id.as_str()
            && id_str.starts_with("claude_req_")
        {
            let actual_id = id_str.trim_start_matches("claude_req_");
            let response_payload =
                if let Some(decision) = result.get("decision").and_then(Value::as_str) {
                    let behavior = if decision == "accept" {
                        "allow"
                    } else {
                        "deny"
                    };
                    json!({
                        "decision": decision,
                        "behavior": behavior
                    })
                } else {
                    result
                };
            for session in self.claude_sessions.values_mut() {
                let _ =
                    session.send_control_response(actual_id, "success", response_payload.clone());
            }
            return Ok(());
        }
        // If request ID is string and starts with "zcode_", route to ZCode
        if let Some(id_str) = id.as_str()
            && id_str.starts_with("zcode_")
            && let Some(ref mut zcode) = self.zcode
        {
            let actual_id = id_str.trim_start_matches("zcode_");
            let parsed_id = actual_id
                .parse::<u64>()
                .map(Value::from)
                .unwrap_or_else(|_| Value::String(actual_id.to_string()));
            return zcode
                .respond(parsed_id, result)
                .map_err(|_| NativeError::uncertain("provider_transport_lost"));
        }
        if let Some(ref mut codex) = self.codex {
            codex
                .respond(id, result)
                .map_err(|_| NativeError::uncertain("provider_transport_lost"))
        } else {
            Err(NativeError::uncertain("provider_transport_lost"))
        }
    }

    fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
        let mut messages: Vec<Value> = Vec::new();
        let mut codex_lost = false;

        if let Some(ref mut codex) = self.codex {
            match codex.drain_messages() {
                Ok(codex_msgs) => messages.extend(codex_msgs),
                Err(e) => {
                    eprintln!("[MultiProviderTransport] codex drain error: {:?}", e);
                    codex_lost = true;
                }
            }
        }

        while let Some(evt) = self.zcode_buffered_events.pop_front() {
            messages.push(evt);
        }

        if let Ok(mut claude_events) = self.claude_buffered_events.lock() {
            while let Some(evt) = claude_events.pop_front() {
                if evt.get("method").and_then(Value::as_str) == Some("turn/completed")
                    && let Some(sid) = evt["params"]["threadId"].as_str()
                {
                    self.claude_active_turns.remove(sid);
                    self.claude_cancels.remove(sid);
                }
                messages.push(evt);
            }
        }

        // Drain active persistent Claude sessions
        let mut claude_completed = Vec::new();
        for (session_id, session) in self.claude_sessions.iter_mut() {
            if let Ok(msgs) = session.drain_messages() {
                for msg in msgs {
                    if let Some(msg_type) = msg.get("type").and_then(Value::as_str) {
                        match msg_type {
                            "stream_event" => {
                                if let Some(delta) = msg
                                    .pointer("/event/delta/text")
                                    .or_else(|| msg.pointer("/delta/text"))
                                    .and_then(Value::as_str)
                                {
                                    messages.push(json!({
                                        "method": "item/agentMessage/delta",
                                        "params": {
                                            "threadId": session_id,
                                            "turnId": self.claude_active_turns.get(session_id).cloned().unwrap_or_default(),
                                            "itemId": "claude_msg",
                                            "delta": delta
                                        }
                                    }));
                                }
                            }
                            "assistant" => {
                                if let Some(content) =
                                    msg.pointer("/message/content").and_then(Value::as_array)
                                {
                                    for block in content {
                                        if block.get("type").and_then(Value::as_str)
                                            == Some("tool_use")
                                        {
                                            let tool_name = block
                                                .get("name")
                                                .and_then(Value::as_str)
                                                .unwrap_or("unknown");
                                            messages.push(json!({
                                                "method": "item/started",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": self.claude_active_turns.get(session_id).cloned().unwrap_or_default(),
                                                    "item": {
                                                        "id": format!("claude_tool_{}", tool_name),
                                                        "type": "commandExecution",
                                                        "toolName": tool_name
                                                    }
                                                }
                                            }));
                                        } else if block.get("type").and_then(Value::as_str)
                                            == Some("text")
                                            && let Some(text) =
                                                block.get("text").and_then(Value::as_str)
                                        {
                                            messages.push(json!({
                                                "method": "item/agentMessage/delta",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": self.claude_active_turns.get(session_id).cloned().unwrap_or_default(),
                                                    "itemId": "claude_msg",
                                                    "delta": text
                                                }
                                            }));
                                        }
                                    }
                                }
                            }
                            "control_request" => {
                                if let Some(req_id) = msg.get("request_id").and_then(Value::as_str)
                                {
                                    let req = &msg["request"];
                                    let subtype = req
                                        .get("subtype")
                                        .and_then(Value::as_str)
                                        .unwrap_or("unknown");
                                    let active_turn_id = self
                                        .claude_active_turns
                                        .get(session_id)
                                        .cloned()
                                        .unwrap_or_default();
                                    let tool_name = req.get("tool_name").and_then(Value::as_str);
                                    let is_question = subtype == "ask_user_question"
                                        || tool_name == Some("AskUserQuestion");

                                    if is_question {
                                        let input = &req["input"];
                                        let questions =
                                            input.get("questions").and_then(Value::as_array);
                                        let first_q = questions.and_then(|q| q.first());
                                        let prompt = first_q
                                            .and_then(|q| q.get("question").and_then(Value::as_str))
                                            .or_else(|| {
                                                input.get("question").and_then(Value::as_str)
                                            })
                                            .or_else(|| input.get("prompt").and_then(Value::as_str))
                                            .unwrap_or("Agent 提出了一个问题");
                                        let header = first_q
                                            .and_then(|q| q.get("header").and_then(Value::as_str));
                                        let multi_select = first_q.and_then(|q| {
                                            q.get("multiSelect").and_then(Value::as_bool)
                                        });
                                        let options = first_q
                                            .and_then(|q| q.get("options").and_then(Value::as_array))
                                            .map(|opts| {
                                                opts.iter()
                                                    .filter_map(|opt| {
                                                        opt.get("label")
                                                            .and_then(Value::as_str)
                                                            .map(|lbl| {
                                                                json!({
                                                                    "label": lbl,
                                                                    "description": opt.get("description").and_then(Value::as_str),
                                                                    "preview": opt.get("preview").and_then(Value::as_str)
                                                                })
                                                            })
                                                    })
                                                    .collect::<Vec<_>>()
                                            });
                                        messages.push(json!({
                                            "id": format!("claude_req_{}", req_id),
                                            "method": "client/askQuestion",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": active_turn_id,
                                                "requestId": req_id,
                                                "prompt": prompt,
                                                "header": header,
                                                "options": options,
                                                "multiSelect": multi_select
                                            }
                                        }));
                                    } else {
                                        let tool_name_str = tool_name.unwrap_or("Bash");
                                        let is_file = matches!(
                                            tool_name_str,
                                            "Edit" | "Write" | "NotebookEdit" | "fileChange"
                                        );
                                        let input = &req["input"];
                                        let file_path = input
                                            .get("file_path")
                                            .or_else(|| input.get("path"))
                                            .or_else(|| input.get("notebook_path"))
                                            .and_then(Value::as_str);
                                        let command = input
                                            .get("command")
                                            .or_else(|| input.get("cmd"))
                                            .and_then(Value::as_str);
                                        let reason = req
                                            .get("description")
                                            .or_else(|| req.get("reason"))
                                            .and_then(Value::as_str);

                                        if is_file {
                                            messages.push(json!({
                                                "id": format!("claude_req_{}", req_id),
                                                "method": "item/fileChange/requestApproval",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": active_turn_id,
                                                    "requestId": req_id,
                                                    "toolName": tool_name_str,
                                                    "filePath": file_path,
                                                    "reason": reason,
                                                    "availableDecisions": ["accept", "decline"]
                                                }
                                            }));
                                        } else {
                                            let cmd_preview = command.unwrap_or(tool_name_str);
                                            messages.push(json!({
                                                "id": format!("claude_req_{}", req_id),
                                                "method": "item/commandExecution/requestApproval",
                                                "params": {
                                                    "threadId": session_id,
                                                    "turnId": active_turn_id,
                                                    "requestId": req_id,
                                                    "toolName": tool_name_str,
                                                    "command": cmd_preview,
                                                    "reason": reason,
                                                    "availableDecisions": ["accept", "decline"]
                                                }
                                            }));
                                        }
                                    }
                                }
                            }
                            "result" => {
                                let result_text =
                                    msg.get("result").and_then(Value::as_str).unwrap_or("");
                                let turn_id = self
                                    .claude_active_turns
                                    .get(session_id)
                                    .cloned()
                                    .unwrap_or_default();
                                messages.push(json!({
                                    "method": "turn/completed",
                                    "params": {
                                        "threadId": session_id,
                                        "turn": {
                                            "id": turn_id,
                                            "status": "completed",
                                            "output": result_text
                                        }
                                    }
                                }));
                                claude_completed.push(session_id.clone());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        for sid in claude_completed {
            self.claude_active_turns.remove(&sid);
            self.claude_cancels.remove(&sid);
        }

        if let Ok(mut antigravity_events) = self.antigravity_buffered_events.lock() {
            while let Some(evt) = antigravity_events.pop_front() {
                if evt.get("method").and_then(Value::as_str) == Some("turn/completed")
                    && let Some(sid) = evt["params"]["threadId"].as_str()
                {
                    self.antigravity_active_turns.remove(sid);
                    self.antigravity_cancels.remove(sid);
                }
                messages.push(evt);
            }
        }

        if let Ok(mut grok_events) = self.grok_buffered_events.lock() {
            while let Some(evt) = grok_events.pop_front() {
                if evt.get("method").and_then(Value::as_str) == Some("turn/completed")
                    && let Some(sid) = evt["params"]["threadId"].as_str()
                {
                    self.grok_active_turns.remove(sid);
                    self.grok_cancels.remove(sid);
                }
                messages.push(evt);
            }
        }

        let mut zcode_lost = false;
        if let Some(ref mut zcode) = self.zcode {
            match zcode.drain_messages() {
                Ok(raw_events) => {
                    for raw in raw_events {
                        let method = raw.get("method").and_then(Value::as_str).unwrap_or("");
                        if method == "session/event" {
                            let params = &raw["params"];
                            let session_id = params["sessionId"].as_str().unwrap_or("");
                            let event_type = params["type"].as_str().unwrap_or("");
                            let payload = &params["payload"];
                            let active_turn_id = self.zcode_active_turns.get(session_id).cloned();

                            match event_type {
                                "model.streaming" => {
                                    let delta = payload["delta"].as_str().unwrap_or("");
                                    let part_id = payload["partId"]
                                        .as_str()
                                        .or_else(|| payload["assistantMessageId"].as_str())
                                        .unwrap_or("agent_msg");
                                    let active_tid = active_turn_id.as_deref().unwrap_or("turn_1");
                                    if !delta.is_empty() {
                                        messages.push(json!({
                                            "method": "item/agentMessage/delta",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": active_tid,
                                                "itemId": part_id,
                                                "delta": delta
                                            }
                                        }));
                                    }
                                }
                                "tool.updated" => {
                                    let kind = payload["kind"].as_str().unwrap_or("");
                                    let tool_id =
                                        payload["toolCallId"].as_str().unwrap_or("tool_call");
                                    let active_tid = active_turn_id.as_deref().unwrap_or("turn_1");
                                    if kind == "started" {
                                        messages.push(json!({
                                            "method": "item/started",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": active_tid,
                                                "item": {
                                                    "id": tool_id,
                                                    "type": "commandExecution"
                                                }
                                            }
                                        }));
                                    } else if matches!(kind, "result" | "batch" | "completed") {
                                        messages.push(json!({
                                            "method": "item/completed",
                                            "params": {
                                                "threadId": session_id,
                                                "turnId": active_tid,
                                                "item": {
                                                    "id": tool_id,
                                                    "type": "commandExecution"
                                                }
                                            }
                                        }));
                                    }
                                }
                                "turn.completed" => {
                                    let turn_id = active_turn_id.unwrap_or_else(|| "turn_1".into());
                                    self.zcode_active_turns.remove(session_id);
                                    messages.push(json!({
                                        "method": "turn/completed",
                                        "params": {
                                            "threadId": session_id,
                                            "turn": {
                                                "id": turn_id,
                                                "status": "completed"
                                            }
                                        }
                                    }));
                                }
                                "turn.failed" => {
                                    let turn_id = active_turn_id.unwrap_or_else(|| "turn_1".into());
                                    self.zcode_active_turns.remove(session_id);
                                    messages.push(json!({
                                        "method": "turn/completed",
                                        "params": {
                                            "threadId": session_id,
                                            "turn": {
                                                "id": turn_id,
                                                "status": "failed"
                                            }
                                        }
                                    }));
                                }
                                _ => {}
                            }
                        } else if method == "state.updated" {
                            let session_id = raw["params"]["sessionId"].as_str().unwrap_or("");
                            let patch_status = raw["params"]["patch"]["status"].as_str();
                            if patch_status == Some("idle") {
                                if let Some(turn_id) = self.zcode_active_turns.remove(session_id) {
                                    messages.push(json!({
                                        "method": "turn/completed",
                                        "params": {
                                            "threadId": session_id,
                                            "turn": {
                                                "id": turn_id,
                                                "status": "completed"
                                            }
                                        }
                                    }));
                                }
                                messages.push(json!({
                                    "method": "thread/status/changed",
                                    "params": {
                                        "threadId": session_id,
                                        "status": { "type": "idle" }
                                    }
                                }));
                            } else if patch_status == Some("running") {
                                messages.push(json!({
                                    "method": "thread/status/changed",
                                    "params": {
                                        "threadId": session_id,
                                        "status": { "type": "active", "activeFlags": [] }
                                    }
                                }));
                            }
                        } else if method == "interaction/requestPermission" {
                            let params = &raw["params"];
                            let session_id = params["sessionId"].as_str().unwrap_or("");
                            let req_id = raw.get("id").cloned().unwrap_or(Value::Null);
                            let req_id_str = match &req_id {
                                Value::Number(n) => format!("zcode_{n}"),
                                Value::String(s) => format!("zcode_{s}"),
                                _ => "zcode_req".to_string(),
                            };
                            let tool_name = params["toolName"].as_str().unwrap_or("tool");
                            let reason = params["reason"].as_str().unwrap_or("");
                            let input = &params["input"];
                            let command_preview = input
                                .get("command")
                                .and_then(Value::as_str)
                                .or_else(|| input.get("cmd").and_then(Value::as_str));
                            let path = input
                                .get("file_path")
                                .and_then(Value::as_str)
                                .or_else(|| input.get("path").and_then(Value::as_str));

                            messages.push(json!({
                                "id": req_id_str,
                                "method": "item/commandExecution/requestApproval",
                                "params": {
                                    "threadId": session_id,
                                    "command": command_preview.unwrap_or(tool_name),
                                    "filePath": path,
                                    "reason": reason,
                                    "availableDecisions": ["accept", "decline"]
                                }
                            }));
                        } else if method == "interaction/requestUserInput" {
                            let params = &raw["params"];
                            let session_id = params["sessionId"].as_str().unwrap_or("");
                            let req_id = raw.get("id").cloned().unwrap_or(Value::Null);
                            let req_id_str = match &req_id {
                                Value::Number(n) => format!("zcode_{n}"),
                                Value::String(s) => format!("zcode_{s}"),
                                _ => "zcode_req".to_string(),
                            };
                            let prompt = params["prompt"].as_str().unwrap_or("Agent 需要您的输入");
                            let questions = params.get("questions").and_then(Value::as_array);
                            let mut options = Vec::new();
                            let mut header = None;
                            if let Some(qs) = questions
                                && let Some(first_q) = qs.first()
                            {
                                header = first_q.get("header").and_then(Value::as_str);
                                if let Some(opts) = first_q.get("options").and_then(Value::as_array)
                                {
                                    for opt in opts {
                                        if let Some(label) =
                                            opt.get("label").and_then(Value::as_str)
                                        {
                                            options.push(json!({
                                                "label": label,
                                                "description": opt.get("description").and_then(Value::as_str),
                                                "preview": opt.get("preview").and_then(Value::as_str)
                                            }));
                                        }
                                    }
                                }
                            }

                            messages.push(json!({
                                "id": req_id_str,
                                "method": "client/askQuestion",
                                "params": {
                                    "threadId": session_id,
                                    "prompt": prompt,
                                    "header": header,
                                    "options": options
                                }
                            }));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[MultiProviderTransport] zcode drain error: {:?}", e);
                    zcode_lost = true;
                }
            }
        }
        if zcode_lost {
            self.zcode = None;
        }

        // Only mark transport lost if codex itself dropped AND there are no other active provider streams
        if codex_lost
            && self.zcode.is_none()
            && self.claude_active_turns.is_empty()
            && self.antigravity_active_turns.is_empty()
            && self.grok_active_turns.is_empty()
        {
            return Err(NativeError::uncertain("provider_transport_lost"));
        }

        Ok(messages)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectMode {
    Create,
    Resume,
    Attach,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexSandbox {
    #[default]
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexApprovalPolicy {
    #[default]
    Untrusted,
    OnRequest,
    Never,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteAction {
    Connect {
        mode: ConnectMode,
        workspace_path: PathBuf,
        #[serde(default)]
        provider_session_id: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        sandbox: CodexSandbox,
        #[serde(default)]
        approval_policy: CodexApprovalPolicy,
        #[serde(default)]
        agent_type: Option<String>,
    },
    SendPrompt {
        text: String,
        /// Per-turn reasoning effort. Providers that do not accept one ignore it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        /// Per-turn model override.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    ResolveApproval {
        approval_id: String,
        approved: bool,
    },
    AnswerQuestion {
        question_id: String,
        answer: Value,
    },
    SetArchived {
        archived: bool,
    },
    Interrupt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteCommand {
    pub client_cmd_id: String,
    pub hub_session_id: String,
    /// Required for all controls of an existing binding. Fences stale clients.
    pub host_generation: Option<String>,
    pub command: RemoteAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    Unknown,
    Idle,
    Running,
    AwaitingPermission,
    AwaitingInput,
    Error,
    Completed,
    Terminated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionBinding {
    pub hub_session_id: String,
    pub provider_session_id: String,
    pub workspace_path: PathBuf,
    pub host_generation: String,
    pub control_connected: bool,
    pub execution_state: ExecutionState,
    pub current_turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandReceipt {
    pub client_cmd_id: String,
    pub hub_session_id: String,
    /// accepted means protocol acceptance, never execution completion.
    pub status: String,
    pub code: Option<String>,
    pub result: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostEvent {
    pub event_id: String,
    pub host_generation: String,
    pub hub_session_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub kind: String,
    /// Unmodified provider message; this is content, never a diagnostic log.
    pub raw: Value,
    pub binding: Option<SessionBinding>,
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<PendingApprovalSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<PendingQuestionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApprovalSnapshot {
    pub approval_id: String,
    pub hub_session_id: String,
    pub host_generation: String,
    pub turn_id: String,
    pub can_approve: bool,
    pub can_decline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionOptionSnapshot {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingQuestionSnapshot {
    pub question_id: String,
    pub hub_session_id: String,
    pub host_generation: String,
    pub turn_id: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOptionSnapshot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
}

struct PendingApproval {
    hub_session_id: String,
    turn_id: String,
    request_id: Value,
    supported: bool,
    available_decisions: Option<Vec<Value>>,
    tool_name: Option<String>,
    command_preview: Option<String>,
    path: Option<String>,
    description: Option<String>,
    risk_level: Option<String>,
}

struct PendingQuestion {
    hub_session_id: String,
    turn_id: String,
    request_id: Value,
    prompt: String,
    header: Option<String>,
    options: Option<Vec<QuestionOptionSnapshot>>,
    multi_select: Option<bool>,
}

pub struct NativeSessionHost<T: NativeTransport> {
    transport: T,
    generation: String,
    workspaces: Vec<PathBuf>,
    discovered_workspaces: HashMap<String, PathBuf>,
    sessions: HashMap<String, SessionBinding>,
    native_to_hub: HashMap<String, String>,
    newly_created: HashSet<String>,
    approvals: HashMap<String, PendingApproval>,
    questions: HashMap<String, PendingQuestion>,
    /// Requests this Host already answered, keyed by their derived id. Some
    /// providers re-deliver a request the client already responded to; replaying
    /// it would raise a second approval the operator has to dismiss.
    answered_requests: HashSet<String>,
    /// Insertion order of `answered_requests`, for the same bounded-lifetime
    /// reason as `receipt_order`. Without it a long-lived daemon would keep every
    /// id it ever answered for as long as it ran.
    answered_order: VecDeque<String>,
    receipts: HashMap<String, (RemoteCommand, CommandReceipt)>,
    /// Insertion order of `receipts` keys. Lets the ledger drop its oldest
    /// entries instead of growing until `command_ledger_full` permanently
    /// rejects every new command on a long-lived daemon.
    receipt_order: VecDeque<String>,
    pending_historical_events: VecDeque<HostEvent>,
    alive: bool,
}

impl<T: NativeTransport> NativeSessionHost<T> {
    pub fn new(transport: T, workspaces: Vec<PathBuf>) -> Result<Self, NativeError> {
        let mut workspaces = workspaces
            .into_iter()
            .map(|path| {
                dunce::canonicalize(&path)
                    .ok()
                    .filter(|path| path.is_dir())
                    .ok_or_else(|| NativeError::rejected("workspace_unavailable"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        workspaces.sort();
        workspaces.dedup();
        if workspaces.is_empty() {
            return Err(NativeError::rejected("workspace_required"));
        }
        Ok(Self {
            transport,
            generation: uuid::Uuid::new_v4().to_string(),
            workspaces,
            discovered_workspaces: HashMap::new(),
            sessions: HashMap::new(),
            native_to_hub: HashMap::new(),
            newly_created: HashSet::new(),
            approvals: HashMap::new(),
            questions: HashMap::new(),
            answered_requests: HashSet::new(),
            answered_order: VecDeque::new(),
            receipts: HashMap::new(),
            receipt_order: VecDeque::new(),
            pending_historical_events: VecDeque::new(),
            alive: true,
        })
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn is_alive(&self) -> bool {
        self.alive
    }
    pub fn sessions(&self) -> Vec<SessionBinding> {
        self.sessions.values().cloned().collect()
    }
    pub fn pending_approvals(&self) -> Vec<PendingApprovalSnapshot> {
        let mut approvals = self
            .approvals
            .iter()
            .filter(|(_, approval)| approval.supported)
            .map(|(approval_id, approval)| {
                let decisions = approval.available_decisions.as_ref();
                PendingApprovalSnapshot {
                    approval_id: approval_id.clone(),
                    hub_session_id: approval.hub_session_id.clone(),
                    host_generation: self.generation.clone(),
                    turn_id: approval.turn_id.clone(),
                    can_approve: decisions.is_none_or(|values| values.contains(&json!("accept"))),
                    can_decline: decisions.is_none_or(|values| values.contains(&json!("decline"))),
                    tool_name: approval.tool_name.clone(),
                    command_preview: approval.command_preview.clone(),
                    path: approval.path.clone(),
                    description: approval.description.clone(),
                    risk_level: approval.risk_level.clone(),
                }
            })
            .collect::<Vec<_>>();
        approvals.sort_by(|left, right| left.approval_id.cmp(&right.approval_id));
        approvals
    }
    pub fn pending_questions(&self) -> Vec<PendingQuestionSnapshot> {
        let mut questions = self
            .questions
            .iter()
            .map(|(question_id, q)| PendingQuestionSnapshot {
                question_id: question_id.clone(),
                hub_session_id: q.hub_session_id.clone(),
                host_generation: self.generation.clone(),
                turn_id: q.turn_id.clone(),
                prompt: q.prompt.clone(),
                header: q.header.clone(),
                options: q.options.clone(),
                multi_select: q.multi_select,
            })
            .collect::<Vec<_>>();
        questions.sort_by(|left, right| left.question_id.cmp(&right.question_id));
        questions
    }
    pub fn workspaces(&self) -> &[PathBuf] {
        &self.workspaces
    }
    pub fn discovered_workspaces(&self) -> &HashMap<String, PathBuf> {
        &self.discovered_workspaces
    }
    pub fn authorize_workspace(&mut self, path: PathBuf) -> Result<PathBuf, NativeError> {
        let canonical = dunce::canonicalize(&path)
            .ok()
            .filter(|p| p.is_dir())
            .ok_or_else(|| NativeError::rejected("workspace_unavailable"))?;
        if !self.workspaces.contains(&canonical) {
            if self.workspaces.len() >= 32 {
                return Err(NativeError::rejected("workspace_limit_exceeded"));
            }
            self.workspaces.push(canonical.clone());
        }
        Ok(canonical)
    }

    pub(crate) fn restore_detached(&mut self, bindings: Vec<SessionBinding>) {
        for mut binding in bindings {
            binding.control_connected = false;
            binding.execution_state = ExecutionState::Unknown;
            binding.host_generation = self.generation.clone();
            self.native_to_hub.insert(
                binding.provider_session_id.clone(),
                binding.hub_session_id.clone(),
            );
            self.sessions
                .insert(binding.hub_session_id.clone(), binding);
        }
    }

    /// Actual provider model/list response, with no invented availability or fallback catalog.
    pub fn models(&mut self) -> Result<Value, NativeError> {
        if let Ok(resp) = self.transport.request("model/list", json!({"limit": 100})) {
            if resp.get("data").and_then(Value::as_array).map(|a| !a.is_empty()).unwrap_or(false) {
                return Ok(resp);
            }
        }
        let models = crate::model_catalog::fetch_catalog_models(false);
        Ok(crate::model_catalog::catalog_models_to_json(&models))
    }

    pub fn discover(&mut self) -> Result<Value, NativeError> {
        let mut candidates = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut params = json!({"limit": 500});
            if let Some(c) = &cursor {
                params["cursor"] = json!(c);
            }
            let response = self.transport.request("thread/list", params)?;
            if let Some(items) = response["data"].as_array() {
                for thread in items {
                    let Some(id) = thread["id"].as_str() else {
                        continue;
                    };
                    if !seen_ids.insert(id.to_string()) {
                        continue;
                    }
                    let raw_cwd = thread["cwd"].as_str().unwrap_or(".");
                    let cwd = dunce::canonicalize(PathBuf::from(raw_cwd))
                        .unwrap_or_else(|_| PathBuf::from(raw_cwd));
                    let ws_id = crate::privacy::workspace_id_for_path(&cwd.to_string_lossy());
                    self.discovered_workspaces.insert(ws_id, cwd.clone());
                    self.discovered_workspaces
                        .insert(cwd.to_string_lossy().to_string(), cwd.clone());
                    let clean = cwd
                        .to_string_lossy()
                        .trim_start_matches(r"\\?\")
                        .to_string();
                    self.discovered_workspaces.insert(clean, cwd.clone());
                    let title = thread["name"]
                        .as_str()
                        .filter(|name| !name.trim().is_empty())
                        .or_else(|| {
                            thread["preview"]
                                .as_str()
                                .and_then(|p| p.lines().find(|l| !l.trim().is_empty()))
                        })
                        .map(|name| name.chars().take(160).collect::<String>());
                    let pt = thread.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
                    let ct = thread.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0);
                    let tt = thread.get("total_tokens").and_then(Value::as_u64).unwrap_or(pt + ct);
                    let cr = thread.get("cache_read_tokens").and_then(Value::as_u64).unwrap_or(0);
                    let last_ms = thread.get("last_activity_at_ms").and_then(Value::as_i64).unwrap_or(0);
                    let model = thread.get("model").cloned().unwrap_or(Value::Null);

                    candidates.push(json!({
                        "provider_session_id": id,
                        "provider": thread["provider"],
                        "resumable": thread["resumable"],
                        "workspace_path": cwd,
                        "title": title,
                        "execution_state": execution_state(&thread["status"]),
                        "control_connected": false,
                        "model": model,
                        "prompt_tokens": pt,
                        "completion_tokens": ct,
                        "total_tokens": tt,
                        "cache_read_tokens": cr,
                        "last_activity_at_ms": last_ms
                    }));
                }
            }
            if let Some(next) = response["nextCursor"].as_str() {
                cursor = Some(next.to_string());
            } else {
                break;
            }
        }
        Ok(json!({"candidates": candidates, "has_more": false}))
    }

    pub fn handle_command(&mut self, command: RemoteCommand) -> CommandReceipt {
        let rejected = |code: &str| CommandReceipt {
            client_cmd_id: command.client_cmd_id.clone(),
            hub_session_id: command.hub_session_id.clone(),
            status: "rejected".into(),
            code: Some(code.into()),
            result: None,
        };
        if command.client_cmd_id.is_empty()
            || command.client_cmd_id.len() > 128
            || command.hub_session_id.is_empty()
            || command.hub_session_id.len() > 128
        {
            return rejected("invalid_command_identity");
        }
        if let Some((previous, receipt)) = self.receipts.get(&command.client_cmd_id) {
            return if previous == &command {
                receipt.clone()
            } else {
                rejected("command_id_conflict")
            };
        }
        // Do not evict IDs and accidentally execute them again. R2 adds a durable ledger.
        if self.receipts.len() >= MAX_COMMAND_LEDGER_ENTRIES {
            // The daemon answers every command id the Gateway still holds in
            // `remote_inbound_commands`, so it stays replay-safe after a
            // restart. This in-memory copy only deduplicates repeats inside one
            // Host lifetime; without eviction it would reach the cap and then
            // reject every later command for the life of the process.
            self.evict_oldest_receipts(self.receipts.len() + 1 - MAX_COMMAND_LEDGER_ENTRIES);
        }
        let result = if self.alive {
            self.execute(&command)
        } else {
            Err(NativeError::rejected("host_lost"))
        };
        let receipt = match result {
            Ok(result) => CommandReceipt {
                client_cmd_id: command.client_cmd_id.clone(),
                hub_session_id: command.hub_session_id.clone(),
                status: "accepted".into(),
                code: None,
                result: Some(result),
            },
            Err(error) => {
                if error.outcome_unknown {
                    self.mark_lost();
                }
                CommandReceipt {
                    client_cmd_id: command.client_cmd_id.clone(),
                    hub_session_id: command.hub_session_id.clone(),
                    status: if error.outcome_unknown {
                        "indeterminate"
                    } else {
                        "rejected"
                    }
                    .into(),
                    code: Some(error.code),
                    result: None,
                }
            }
        };
        self.receipts
            .insert(command.client_cmd_id.clone(), (command, receipt.clone()));
        self.receipt_order.push_back(receipt.client_cmd_id.clone());
        receipt
    }

    /// Records a request id as answered and evicts the oldest once the
    /// suppression window is full. The window only has to outlive a provider's
    /// re-delivery of a request the client has already seen, so bounding it by
    /// count rather than by age is enough to stop a long-lived daemon from
    /// retaining every id it ever answered.
    fn mark_answered(&mut self, id: &str) {
        if self.answered_requests.insert(id.to_string()) {
            self.answered_order.push_back(id.to_string());
        }
        while self.answered_requests.len() > MAX_ANSWERED_REQUESTS {
            let Some(oldest) = self.answered_order.pop_front() else {
                break;
            };
            self.answered_requests.remove(&oldest);
        }
    }

    /// Drops the `count` oldest ledger entries. Callers only reach this once the
    /// ledger is full, so it is expected to run on a long-lived daemon.
    fn evict_oldest_receipts(&mut self, count: usize) {
        for _ in 0..count {
            let Some(oldest) = self.receipt_order.pop_front() else {
                break;
            };
            self.receipts.remove(&oldest);
        }
    }

    fn execute(&mut self, command: &RemoteCommand) -> Result<Value, NativeError> {
        let hub_id = &command.hub_session_id;
        if command.host_generation.as_deref() != Some(&self.generation) {
            return Err(NativeError::rejected("host_generation_mismatch"));
        }
        if let RemoteAction::Connect {
            mode,
            workspace_path,
            provider_session_id,
            model,
            sandbox,
            approval_policy,
            agent_type,
        } = &command.command
        {
            if *mode == ConnectMode::Attach {
                return Err(NativeError::rejected("attach_not_supported"));
            }
            if let Some(existing) = self.sessions.get(hub_id)
                && (existing.control_connected
                    || *mode != ConnectMode::Resume
                    || provider_session_id.as_ref() != Some(&existing.provider_session_id))
            {
                return Err(NativeError::rejected("session_already_bound"));
            }
            let ws_str = workspace_path.to_string_lossy();
            let cwd = self
                .workspaces
                .iter()
                .find(|w| crate::privacy::workspace_id_for_path(&w.to_string_lossy()) == ws_str)
                .cloned()
                .or_else(|| {
                    if *mode == ConnectMode::Resume {
                        self.discovered_workspaces
                            .get(ws_str.as_ref())
                            .or_else(|| {
                                let clean = ws_str.trim_start_matches(r"\\?\");
                                self.discovered_workspaces.get(clean)
                            })
                            .or_else(|| {
                                let computed_id = crate::privacy::workspace_id_for_path(&ws_str);
                                self.discovered_workspaces.get(&computed_id)
                            })
                            .or_else(|| {
                                self.discovered_workspaces
                                    .values()
                                    .find(|p| p.as_path() == workspace_path.as_path())
                            })
                            .cloned()
                            .or_else(|| {
                                dunce::canonicalize(workspace_path)
                                    .ok()
                                    .filter(|p| p.is_dir())
                            })
                    } else {
                        None
                    }
                })
                .ok_or_else(|| NativeError::rejected("workspace_not_authorized"))?;
            if model.as_ref().is_some_and(|model| model.trim().is_empty()) {
                return Err(NativeError::rejected("invalid_model"));
            }
            let mut params = json!({"cwd": cwd, "sandbox": sandbox, "approvalPolicy": approval_policy,
                "approvalsReviewer": "user"});
            if let Some(model) = model {
                params["model"] = json!(model);
            }
            if let Some(at) = agent_type {
                params["agentType"] = json!(at);
            }
            let method = match mode {
                ConnectMode::Create => {
                    if provider_session_id.is_some() {
                        return Err(NativeError::rejected("create_cannot_set_native_id"));
                    }
                    params["experimentalRawEvents"] = json!(false);
                    "thread/start"
                }
                ConnectMode::Resume => {
                    let id = provider_session_id
                        .as_ref()
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| NativeError::rejected("native_id_required"))?;
                    if self
                        .native_to_hub
                        .get(id)
                        .is_some_and(|bound| bound != hub_id)
                    {
                        return Err(NativeError::rejected("native_session_already_bound"));
                    }
                    // Read does not take control. Never resume a known active thread.
                    let history = self
                        .transport
                        .request("thread/read", json!({"threadId": id, "includeTurns": true}))?;
                    let thread = &history["thread"];
                    if thread["id"].as_str() != Some(id) {
                        return Err(NativeError::rejected("native_id_mismatch"));
                    }
                    if !matches!(
                        thread["status"]["type"].as_str(),
                        Some("notLoaded" | "idle")
                    ) {
                        return Err(NativeError::rejected("external_session_not_resumable"));
                    }
                    if let Some(orig_str) = thread["cwd"].as_str() {
                        let orig_clean = orig_str.trim_start_matches(r"\\?\");
                        let cwd_str = cwd.to_string_lossy();
                        let cwd_clean = cwd_str.trim_start_matches(r"\\?\");
                        if !orig_clean.eq_ignore_ascii_case(cwd_clean) {
                            let orig_canon = dunce::canonicalize(PathBuf::from(orig_str)).ok();
                            if orig_canon.as_ref() != Some(&cwd) {
                                return Err(NativeError::rejected("resume_workspace_mismatch"));
                            }
                        }
                    }
                    params["threadId"] = json!(id);
                    "thread/resume"
                }
                ConnectMode::Attach => unreachable!(),
            };
            let response = self.transport.request(method, params)?;
            let id = response["thread"]["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| NativeError::uncertain("provider_missing_native_id"))?
                .to_string();
            if provider_session_id
                .as_ref()
                .is_some_and(|expected| expected != &id)
                || self
                    .native_to_hub
                    .get(&id)
                    .is_some_and(|bound| bound != hub_id)
            {
                return Err(NativeError::uncertain("provider_identity_conflict"));
            }
            let binding = SessionBinding {
                hub_session_id: hub_id.clone(),
                provider_session_id: id.clone(),
                workspace_path: cwd.clone(),
                host_generation: self.generation.clone(),
                control_connected: true,
                execution_state: execution_state(&response["thread"]["status"]),
                current_turn_id: None,
            };
            self.native_to_hub.insert(id.clone(), hub_id.clone());
            self.sessions.insert(hub_id.clone(), binding.clone());
            if *mode == ConnectMode::Create {
                self.newly_created.insert(hub_id.clone());
            }
            if !self.workspaces.contains(&cwd) {
                self.workspaces.push(cwd);
            }

            if let Some(turns) = response["thread"]["turns"].as_array() {
                for turn in turns {
                    let turn_id = turn["id"].as_str().unwrap_or_default();
                    if let Some(items) = turn["items"].as_array() {
                        for item in items {
                            let item_type = item["type"].as_str().unwrap_or_default();
                            if matches!(
                                item_type,
                                "userMessage" | "agentMessage" | "commandExecution"
                            ) {
                                let synthetic_raw = json!({
                                    "method": "item/completed",
                                    "params": {
                                        "threadId": id,
                                        "turnId": turn_id,
                                        "item": item
                                    }
                                });
                                self.pending_historical_events.push_back(HostEvent {
                                    event_id: uuid::Uuid::new_v4().to_string(),
                                    host_generation: self.generation.clone(),
                                    binding: Some(binding.clone()),
                                    hub_session_id: Some(hub_id.clone()),
                                    provider_session_id: Some(id.clone()),
                                    kind: "item/completed".to_string(),
                                    raw: synthetic_raw,
                                    approval_id: None,
                                    approval: None,
                                    question_id: None,
                                    question: None,
                                });
                            }
                        }
                    }
                }
            }

            // Providers without native thread/read replay use the daemon's
            // single bounded transcript reader after explicit resume. This
            // normalized snapshot is sent only for a managed session.
            if *mode == ConnectMode::Resume
                && response["thread"]["turns"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
                && let Some(home) = crate::collectors::home_dir()
            {
                let provider = if id.starts_with("sess_") {
                    AgentType::ZCode
                } else if id.starts_with("claude_") {
                    AgentType::ClaudeCode
                } else if id.starts_with("antigravity_") {
                    AgentType::Antigravity
                } else if id.starts_with("grok_") {
                    AgentType::GrokBuild
                } else {
                    AgentType::Codex
                };
                let items = crate::conversation::load_session_conversation(&home, provider, &id);
                if !items.is_empty() {
                    self.pending_historical_events.push_back(HostEvent {
                        event_id: uuid::Uuid::new_v4().to_string(),
                        host_generation: self.generation.clone(),
                        binding: Some(binding.clone()),
                        hub_session_id: Some(hub_id.clone()),
                        provider_session_id: Some(id.clone()),
                        kind: "history/snapshot".to_string(),
                        raw: json!({"method": "history/snapshot", "params": {"items": items}}),
                        approval_id: None,
                        approval: None,
                        question_id: None,
                        question: None,
                    });
                }
            }

            // Bound provider_response in receipt: IPC frame limit is MAX_DAEMON_FRAME_BYTES (1MB).
            // Retain thread summary (id, cwd, status) and discard large historical turns array.
            let mut sanitized_response = response.clone();
            if let Some(thread_obj) = sanitized_response
                .get_mut("thread")
                .and_then(Value::as_object_mut)
            {
                thread_obj.remove("turns");
            }
            return Ok(json!({"binding": binding, "provider_response": sanitized_response}));
        }

        let session = self
            .sessions
            .get(hub_id)
            .cloned()
            .ok_or_else(|| NativeError::rejected("session_not_bound"))?;
        if !session.control_connected {
            return Err(NativeError::rejected("session_control_lost"));
        }
        match &command.command {
            RemoteAction::SendPrompt {
                text,
                effort,
                model,
            } => {
                if text.trim().is_empty() || text.len() > 512 * 1024 {
                    return Err(NativeError::rejected("invalid_prompt"));
                }
                if text.trim_start().starts_with('/') {
                    return Err(NativeError::rejected("slash_not_supported"));
                }
                if session.current_turn_id.is_some()
                    && matches!(
                        session.execution_state,
                        ExecutionState::Running
                            | ExecutionState::AwaitingPermission
                            | ExecutionState::AwaitingInput
                    )
                {
                    return Err(NativeError::rejected("session_busy_or_unknown"));
                }
                if let Some(binding) = self.sessions.get_mut(hub_id) {
                    binding.current_turn_id = None;
                }
                let is_newly_created = self.newly_created.remove(hub_id);
                let mut turn_params = json!({"threadId": session.provider_session_id,
                    "input": [{"type": "text", "text": text, "text_elements": []}]});
                if is_newly_created {
                    turn_params["sessionExists"] = json!(false);
                }
                // Reasoning effort is a per-turn override on Codex; omitting it
                // keeps whatever the thread was started with.
                if let Some(effort) = effort
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    turn_params["effort"] = json!(effort);
                }
                if let Some(model) = model
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    turn_params["model"] = json!(model);
                }
                let result = self.transport.request("turn/start", turn_params)?;
                let turn_id = result["turn"]["id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| NativeError::uncertain("provider_missing_turn_id"))?;
                let binding = self.sessions.get_mut(hub_id).expect("binding checked");
                binding.current_turn_id = Some(turn_id.to_string());
                binding.execution_state = turn_state(&result["turn"]["status"]);
                Ok(result)
            }
            RemoteAction::ResolveApproval {
                approval_id,
                approved,
            } => {
                let approval = self
                    .approvals
                    .get(approval_id)
                    .ok_or_else(|| NativeError::rejected("approval_not_pending"))?;
                if approval.hub_session_id != *hub_id
                    || session.current_turn_id.as_ref() != Some(&approval.turn_id)
                {
                    return Err(NativeError::rejected("approval_session_mismatch"));
                }
                if !approval.supported {
                    return Err(NativeError::rejected("approval_type_not_supported"));
                }
                let decision = if *approved { "accept" } else { "decline" };
                if approval
                    .available_decisions
                    .as_ref()
                    .is_some_and(|allowed| !allowed.contains(&json!(decision)))
                {
                    return Err(NativeError::rejected("approval_decision_not_available"));
                }
                self.transport
                    .respond(approval.request_id.clone(), json!({"decision": decision}))?;
                self.approvals.remove(approval_id);
                self.mark_answered(&approval_id.clone());
                // Native notifications, not the act of sending a response, update execution state.
                Ok(json!({"approval_id": approval_id, "response_sent": true}))
            }
            RemoteAction::AnswerQuestion {
                question_id,
                answer,
            } => {
                let question = self
                    .questions
                    .get(question_id)
                    .ok_or_else(|| NativeError::rejected("question_not_pending"))?;
                if question.hub_session_id != *hub_id
                    || session.current_turn_id.as_ref() != Some(&question.turn_id)
                {
                    return Err(NativeError::rejected("question_session_mismatch"));
                }
                // Send RPC response back to provider
                self.transport.respond(
                    question.request_id.clone(),
                    json!({
                        "answers": {
                            &question.prompt: answer
                        },
                        "response": answer
                    }),
                )?;
                self.questions.remove(question_id);
                self.mark_answered(&question_id.clone());
                Ok(json!({"question_id": question_id, "response_sent": true}))
            }
            RemoteAction::SetArchived { archived } => {
                if session.current_turn_id.is_some()
                    && matches!(
                        session.execution_state,
                        ExecutionState::Running
                            | ExecutionState::AwaitingPermission
                            | ExecutionState::AwaitingInput
                    )
                {
                    return Err(NativeError::rejected("session_busy_or_unknown"));
                }
                self.transport.request(
                    if *archived {
                        "thread/archive"
                    } else {
                        "thread/unarchive"
                    },
                    json!({"threadId": session.provider_session_id}),
                )
            }
            RemoteAction::Interrupt => {
                if let Some(ref turn_id) = session.current_turn_id {
                    self.transport.request(
                        "turn/interrupt",
                        json!({"threadId": session.provider_session_id, "turnId": turn_id}),
                    )
                } else {
                    let _ = self.transport.request(
                        "turn/interrupt",
                        json!({"threadId": session.provider_session_id}),
                    );
                    if let Some(binding) = self.sessions.get_mut(hub_id) {
                        binding.current_turn_id = None;
                        binding.execution_state = ExecutionState::Idle;
                    }
                    self.approvals
                        .retain(|_, approval| approval.hub_session_id != *hub_id);
                    self.questions.retain(|_, q| q.hub_session_id != *hub_id);
                    Ok(json!({"ok": true, "interrupted": true}))
                }
            }
            RemoteAction::Connect { .. } => unreachable!(),
        }
    }

    pub fn poll(&mut self) -> Result<Vec<HostEvent>, NativeError> {
        if !self.alive {
            return Ok(Vec::new());
        }
        let mut events: Vec<HostEvent> = Vec::new();
        while let Some(evt) = self.pending_historical_events.pop_front() {
            events.push(evt);
            if events.len() >= 64 {
                break;
            }
        }
        let messages = match self.transport.drain() {
            Ok(messages) => messages,
            Err(error) => {
                self.mark_lost();
                return Err(error);
            }
        };
        events.extend(messages.into_iter().map(|raw| self.consume(raw)));
        Ok(events)
    }

    fn mark_lost(&mut self) {
        self.alive = false;
        self.approvals.clear();
        for binding in self.sessions.values_mut() {
            binding.control_connected = false;
            binding.execution_state = ExecutionState::Unknown;
        }
    }

    fn consume(&mut self, raw: Value) -> HostEvent {
        let method = raw["method"].as_str().unwrap_or("provider_response");
        let params = &raw["params"];
        let native_id = params["threadId"]
            .as_str()
            .or_else(|| params["thread"]["id"].as_str())
            .map(str::to_owned);
        let hub_id = native_id
            .as_ref()
            .and_then(|id| self.native_to_hub.get(id))
            .cloned();
        let mut approval_id: Option<String> = None;
        let mut approval: Option<PendingApprovalSnapshot> = None;
        let mut question: Option<PendingQuestionSnapshot> = None;
        let mut kind = method.to_string();
        if let Some(binding) = hub_id.as_ref().and_then(|id| self.sessions.get_mut(id)) {
            match method {
                "thread/status/changed" => {
                    binding.execution_state = execution_state(&params["status"])
                }
                "turn/started" => {
                    binding.current_turn_id = params["turn"]["id"].as_str().map(str::to_owned);
                    binding.execution_state = turn_state(&params["turn"]["status"]);
                }
                // Delayed events for an older turn must not end a newer turn.
                "turn/completed" => {
                    if params["turn"]["id"].as_str() == binding.current_turn_id.as_deref()
                        || binding.current_turn_id.is_none()
                    {
                        binding.execution_state = turn_state(&params["turn"]["status"]);
                        binding.current_turn_id = None;
                        self.approvals.retain(|_, approval| {
                            approval.hub_session_id != binding.hub_session_id
                        });
                        self.questions
                            .retain(|_, q| q.hub_session_id != binding.hub_session_id);
                    }
                    if let Some(real_id) = params.get("realSessionId").and_then(Value::as_str) {
                        let real_id = real_id.trim();
                        if !real_id.is_empty() && binding.provider_session_id != real_id {
                            self.native_to_hub.remove(&binding.provider_session_id);
                            binding.provider_session_id = real_id.to_string();
                            self.native_to_hub
                                .insert(real_id.to_string(), binding.hub_session_id.clone());
                        }
                    }
                }
                "serverRequest/resolved" => {
                    if let Some(id) = self.approvals.iter().find_map(|(id, approval)| {
                        (approval.hub_session_id == binding.hub_session_id
                            && approval.request_id == params["requestId"])
                            .then(|| id.clone())
                    }) {
                        self.approvals.remove(&id);
                        approval_id = Some(id);
                        kind = "approval_resolved".into();
                    }
                    if let Some(id) = self.questions.iter().find_map(|(id, q)| {
                        (q.hub_session_id == binding.hub_session_id
                            && q.request_id == params["requestId"])
                            .then(|| id.clone())
                    }) {
                        self.questions.remove(&id);
                        kind = "question_resolved".into();
                    }
                }
                _ if raw.get("id").is_some() && raw.get("method").is_some() => {
                    let request_id = raw["id"].clone();
                    let turn_id = params["turnId"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .or(binding.current_turn_id.as_deref())
                        .unwrap_or("");
                    if binding.current_turn_id.as_deref() == Some(turn_id)
                        || binding.current_turn_id.is_none()
                    {
                        let mut digest = Sha256::new();
                        digest.update(b"agenthub.approval.v1\0");
                        digest.update(self.generation.as_bytes());
                        digest.update([0]);
                        digest.update(binding.provider_session_id.as_bytes());
                        digest.update([0]);
                        digest.update(
                            serde_json::to_vec(&request_id).unwrap_or_else(|_| b"invalid".to_vec()),
                        );
                        let id = format!("approval-{:x}", digest.finalize());
                        let is_command_approval = method == "item/commandExecution/requestApproval";
                        let is_file_approval = method == "item/fileChange/requestApproval";
                        let is_question_request = method == "item/askUserQuestion"
                            || method == "item/question"
                            || method == "client/askQuestion";

                        if is_question_request {
                            let prompt = params["prompt"]
                                .as_str()
                                .or_else(|| params["question"].as_str())
                                .unwrap_or("Agent 提出了一个问题")
                                .to_string();
                            let header = params["header"].as_str().map(str::to_owned);
                            let multi_select = params["multiSelect"].as_bool();
                            let options = params["options"].as_array().map(|arr| {
                                arr.iter()
                                    .filter_map(|opt| {
                                        opt["label"].as_str().map(|label| QuestionOptionSnapshot {
                                            label: label.to_string(),
                                            description: opt["description"]
                                                .as_str()
                                                .map(str::to_owned),
                                            preview: opt["preview"].as_str().map(str::to_owned),
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            });
                            let q_snapshot = PendingQuestionSnapshot {
                                question_id: id.clone(),
                                hub_session_id: binding.hub_session_id.clone(),
                                host_generation: self.generation.clone(),
                                turn_id: turn_id.into(),
                                prompt: prompt.clone(),
                                header: header.clone(),
                                options: options.clone(),
                                multi_select,
                            };
                            if !self.answered_requests.contains(&id) {
                                binding.execution_state = ExecutionState::AwaitingInput;
                                self.questions.insert(
                                    id.clone(),
                                    PendingQuestion {
                                        hub_session_id: binding.hub_session_id.clone(),
                                        turn_id: turn_id.into(),
                                        request_id,
                                        prompt,
                                        header,
                                        options,
                                        multi_select,
                                    },
                                );
                                question = Some(q_snapshot);
                                kind = "question_requested".into();
                            }
                        } else {
                            let supported = is_command_approval || is_file_approval;
                            if !self.answered_requests.contains(&id) {
                                binding.execution_state = if supported {
                                    ExecutionState::AwaitingPermission
                                } else {
                                    ExecutionState::AwaitingInput
                                };
                                let available_decisions =
                                    params["availableDecisions"].as_array().cloned();
                                let tool_name =
                                    params["toolName"].as_str().map(str::to_owned).or_else(|| {
                                        if is_command_approval {
                                            Some("Bash".to_string())
                                        } else if is_file_approval {
                                            Some("FileChange".to_string())
                                        } else {
                                            None
                                        }
                                    });
                                let command_preview = params["command"].as_str().map(str::to_owned);
                                let path = params["filePath"].as_str().map(str::to_owned);
                                let description = params["reason"].as_str().map(str::to_owned);
                                let risk_level = if is_command_approval {
                                    Some("high".to_string())
                                } else if is_file_approval {
                                    Some("medium".to_string())
                                } else {
                                    None
                                };

                                let descriptor = PendingApprovalSnapshot {
                                    approval_id: id.clone(),
                                    hub_session_id: binding.hub_session_id.clone(),
                                    host_generation: self.generation.clone(),
                                    turn_id: turn_id.into(),
                                    can_approve: available_decisions
                                        .as_ref()
                                        .is_none_or(|values| values.contains(&json!("accept"))),
                                    can_decline: available_decisions
                                        .as_ref()
                                        .is_none_or(|values| values.contains(&json!("decline"))),
                                    tool_name: tool_name.clone(),
                                    command_preview: command_preview.clone(),
                                    path: path.clone(),
                                    description: description.clone(),
                                    risk_level: risk_level.clone(),
                                };
                                self.approvals.entry(id.clone()).or_insert_with(|| {
                                    PendingApproval {
                                        hub_session_id: binding.hub_session_id.clone(),
                                        turn_id: turn_id.into(),
                                        request_id,
                                        supported,
                                        available_decisions,
                                        tool_name,
                                        command_preview,
                                        path,
                                        description,
                                        risk_level,
                                    }
                                });
                                approval_id = Some(id);
                                if supported {
                                    approval = Some(descriptor);
                                }
                                kind = if supported {
                                    "approval_requested"
                                } else {
                                    "unsupported_provider_request"
                                }
                                .into();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let question_id = question.as_ref().map(|q| q.question_id.clone());
        HostEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            host_generation: self.generation.clone(),
            binding: hub_id
                .as_ref()
                .and_then(|id| self.sessions.get(id))
                .cloned(),
            hub_session_id: hub_id,
            provider_session_id: native_id,
            kind,
            raw,
            approval_id,
            approval,
            question_id,
            question,
        }
    }
}

fn execution_state(status: &Value) -> ExecutionState {
    let type_str = status["type"].as_str().or_else(|| status.as_str());
    match type_str {
        Some("idle") => ExecutionState::Idle,
        Some("systemError" | "error" | "failed") => ExecutionState::Error,
        Some("completed") => ExecutionState::Completed,
        Some("active" | "running" | "inProgress") => {
            let flags = status["activeFlags"]
                .as_array()
                .or_else(|| status["flags"].as_array());
            if flags.is_some_and(|flags| flags.contains(&json!("waitingOnApproval"))) {
                ExecutionState::AwaitingPermission
            } else if flags.is_some_and(|flags| flags.contains(&json!("waitingOnUserInput"))) {
                ExecutionState::AwaitingInput
            } else {
                ExecutionState::Running
            }
        }
        _ => ExecutionState::Unknown,
    }
}

fn turn_state(status: &Value) -> ExecutionState {
    let status_str = status.as_str().or_else(|| status["type"].as_str());
    match status_str {
        Some("inProgress" | "running") => ExecutionState::Running,
        Some("completed") => ExecutionState::Completed,
        Some("interrupted") => ExecutionState::Idle,
        Some("failed") => ExecutionState::Error,
        _ => ExecutionState::Unknown,
    }
}
