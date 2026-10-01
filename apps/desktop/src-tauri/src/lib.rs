use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

mod daemon_client;
pub use daemon_client::safe_daemon_code;
use daemon_client::{
    DaemonClient, remote_host_status, save_host_workspaces, start_remote_host, stop_remote_host,
};

use agent_pulse_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
        grok, zcode,
    },
    collectors::{
        LocalSessionContext, collect_antigravity_session_contexts, collect_claude_session_contexts,
        collect_claude_transcripts, collect_codex_local_threads, collect_codex_session_contexts,
        collect_grok_session_contexts, collect_grok_sessions, collect_zcode_session_contexts,
        collect_zcode_sessions,
    },
    device::DeviceIdentity,
    domain::{AgentType, ObservationMeta, SessionObservation},
    gateway::GatewayClient,
    outbox::Outbox,
    privacy::ProjectRedactor,
    runtime::{
        AntigravitySessionSource, ClaudeSessionSource, CodexSessionSource, GrokSessionSource,
        PollPolicy, RunnerRuntime, SessionSource, ZCodeSessionSource,
    },
    sync::MAX_SYNC_BATCH_EVENTS,
    sync_worker::{SyncPolicy, SyncWorker},
};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State, ipc::Channel};
use thiserror::Error;
use url::{Host, Url};

const DEFAULT_SERVER_URL: &str = "https://platform.tannerlab.xyz";
const KEYRING_SERVICE: &str = "AgentHub Desktop";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const LOCAL_SESSION_LIMIT: u32 = 1_000;
const DESKTOP_COLLECTOR_TIMEOUT: Duration = Duration::from_secs(5);
const AGENT_TURN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_PROMPT_CHARS: usize = 16_000;
const MAX_SESSION_ID_CHARS: usize = 160;
const MAX_SSE_LINE_BYTES: usize = 64 * 1024;
const STREAM_MAX_BACKOFF: Duration = Duration::from_secs(30);
const SESSION_CONTEXT_CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Error)]
enum DesktopError {
    #[error("服务器地址无效")]
    InvalidServer,
    #[error("无法连接 AgentHub 服务器")]
    Unreachable,
    #[error("服务器返回了无效响应")]
    InvalidResponse,
    #[error("本机服务通信失败，请重试（{0}）")]
    LocalIpc(&'static str),
    #[error("本机服务启动失败，请检查本机连接设置（{0}）")]
    LocalStartup(&'static str),
    #[error("本机服务协议不兼容或响应无效，请更新后重试（{0}）")]
    LocalProtocol(&'static str),
    #[error("本机会话读取失败，请重试（{0}）")]
    LocalConversation(&'static str),
    #[error("登录信息不正确")]
    InvalidCredentials,
    #[error("登录尝试过多，请稍后重试")]
    LoginRateLimited,
    #[error("请先验证邮箱，然后重试自动连接")]
    EmailVerificationRequired,
    #[error("这台设备已绑定到另一个账号")]
    DeviceConflict,
    #[error("本机安全存储不可用")]
    SecureStorage,
    #[error("无法初始化本机设备身份")]
    Identity,
    #[error("无法启动本地同步")]
    Sync,
    #[error("当前没有登录")]
    NotAuthenticated,
    #[error("会话参数无效")]
    InvalidAction,
    #[error("本机会话已不可用，请刷新后重试")]
    SessionUnavailable,
    #[error("{0}")]
    Detailed(String),
    #[error("Agent 执行失败；请检查 CLI 登录状态和控制路径")]
    AgentFailed,
    #[error("这个会话已有一个回合正在运行")]
    AgentBusy,
    #[error("本机会话已停止")]
    AgentCancelled,
    #[error("{0}")]
    Attachment(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct AuthInput {
    mode: AuthMode,
    email: String,
    password: String,
    display_name: Option<String>,
    server_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthMode {
    Login,
    Register,
}

#[derive(Debug, Serialize, Deserialize)]
struct DesktopConfig {
    server_url: String,
    #[serde(default = "default_refresh_interval_seconds")]
    refresh_interval_seconds: u64,
    #[serde(default = "default_claude_binary")]
    claude_binary: String,
    #[serde(default = "default_codex_binary")]
    codex_binary: String,
    #[serde(default = "default_grok_binary")]
    grok_binary: String,
    #[serde(default)]
    archived_sessions: Vec<ArchivedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchivedSession {
    agent_type: AgentType,
    agent_session_id: String,
    project_display_name: String,
    project_fingerprint: String,
    observed_at: String,
}

#[derive(Debug, Deserialize)]
struct SessionMessageInput {
    agent_type: AgentType,
    agent_session_id: String,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    attachments: Vec<SessionAttachmentInput>,
}

#[derive(Debug, Deserialize)]
struct SessionAttachmentInput {
    name: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct LaunchSessionInput {
    agent_type: AgentType,
    cwd: Option<String>,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionTurnRef {
    agent_type: AgentType,
    agent_session_id: String,
}

#[derive(Debug, Deserialize)]
struct SessionApprovalInput {
    agent_type: AgentType,
    agent_session_id: String,
    approval_id: String,
    approved: bool,
}

#[derive(Debug, Deserialize)]
struct SessionTimelineInput {
    device_id: String,
    agent_type: AgentType,
    agent_session_id: String,
}

#[derive(Debug, Deserialize)]
struct SessionConversationInput {
    agent_type: AgentType,
    agent_session_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionTurnEvent {
    Started,
    AssistantDelta {
        text: String,
    },
    AssistantMessage {
        text: String,
    },
    Activity {
        label: &'static str,
    },
    ApprovalRequested {
        approval: agent_pulse_runner::remote_host::PendingApprovalSnapshot,
    },
}

#[derive(Default)]
struct CodexAssistantAccumulator {
    text: String,
}

impl CodexAssistantAccumulator {
    fn apply(
        &mut self,
        event: &agent_pulse_runner::remote_host::HostEvent,
        expected_turn_id: &str,
    ) -> Option<SessionTurnEvent> {
        let event_turn_id = event.raw["params"]["turnId"]
            .as_str()
            .or_else(|| event.raw["params"]["turn"]["id"].as_str());
        // If event carries a turnId, verify it matches expected_turn_id; if omitted (e.g. from non-Codex adapters), accept as long as hub_session_id/threadId match
        if let Some(id) = event_turn_id
            && id != expected_turn_id
        {
            return None;
        }
        match event.kind.as_str() {
            "item/agentMessage/delta" => {
                let delta = event.raw["params"]["delta"].as_str()?;
                self.text.push_str(delta);
                Some(SessionTurnEvent::AssistantDelta {
                    text: delta.to_owned(),
                })
            }
            "item/completed"
                if event.raw["params"]["item"]["type"].as_str() == Some("agentMessage") =>
            {
                let text = event.raw["params"]["item"]["text"].as_str()?;
                self.text = text.to_owned();
                Some(SessionTurnEvent::AssistantMessage {
                    text: self.text.clone(),
                })
            }
            _ => None,
        }
    }

    fn finish_with_fallback(&mut self, fallback: Option<&str>) -> String {
        if self.text.trim().is_empty()
            && let Some(fallback) = fallback
        {
            self.text = fallback.to_owned();
        }
        self.text.trim().to_owned()
    }
}

#[derive(Debug, Clone, Serialize)]
struct DashboardRealtimeEvent {
    connected: bool,
    changed: bool,
}

#[derive(Debug, Deserialize)]
struct ArchiveSessionInput {
    agent_type: AgentType,
    agent_session_id: String,
    project_display_name: String,
    project_fingerprint: String,
    observed_at: String,
    archived: bool,
}

#[derive(Debug, Deserialize)]
struct DesktopPreferencesInput {
    refresh_interval_seconds: u64,
    claude_binary: String,
    grok_binary: String,
}

fn default_refresh_interval_seconds() -> u64 {
    4
}

fn default_claude_binary() -> String {
    "claude".to_owned()
}

fn default_codex_binary() -> String {
    "codex".to_owned()
}

fn default_grok_binary() -> String {
    "grok".to_owned()
}

struct EngineHandle {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl EngineHandle {
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

struct RealtimeHandle {
    stop: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
}

#[derive(Default)]
struct SessionContextCache {
    refreshed_at: Option<Instant>,
    /// Shared so the read path — `list_agent_models` runs on every model-picker
    /// open — hands out a refcount bump instead of copying up to
    /// `LOCAL_SESSION_LIMIT` contexts with a dozen owned strings each.
    entries: Arc<HashMap<String, LocalSessionContext>>,
}

impl RealtimeHandle {
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.connected.store(false, Ordering::Relaxed);
    }
}

struct DesktopState {
    data_dir: PathBuf,
    engine: Mutex<Option<EngineHandle>>,
    realtime: Mutex<Option<RealtimeHandle>>,
    active_turns: Mutex<HashMap<String, Arc<AtomicBool>>>,
    session_contexts: Mutex<SessionContextCache>,
}

#[tauri::command]
async fn authenticate(
    input: AuthInput,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let snapshot = authenticate_inner(input, &state).map_err(|error| error.to_string())?;
    start_dashboard_realtime(&state, &app).map_err(|error| error.to_string())?;
    daemon_client::try_connect_remote(&state.data_dir);
    Ok(snapshot)
}

#[tauri::command]
async fn restore_session(
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Option<Value>, String> {
    let identity = load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
    let config = match read_config(&state.data_dir) {
        Ok(config) => config,
        Err(_) => {
            // No configuration or server saved: user must login first.
            return Ok(None);
        }
    };
    let cookie = match load_cookie(&config.server_url) {
        Ok(cookie) => cookie,
        Err(_) => {
            // Not authenticated: strictly disallow bypass, require user login.
            return Ok(None);
        }
    };

    let online = ensure_device_bound(&config.server_url, &cookie, &identity).is_ok();
    if online {
        let _ = start_engine(&state, &config.server_url, &identity);
        let _ = start_dashboard_realtime(&state, &app);
        daemon_client::try_connect_remote(&state.data_dir);
    }

    let mut snapshot = match dashboard_snapshot(&config.server_url, &cookie, &identity, &state) {
        Ok(s) => s,
        Err(_) => offline_dashboard_snapshot(&identity, &state).map_err(|e| e.to_string())?,
    };
    snapshot["sync_running"] = Value::Bool(online);
    snapshot["realtime_connected"] = Value::Bool(online);
    Ok(Some(snapshot))
}

#[tauri::command]
async fn refresh_dashboard(state: State<'_, DesktopState>) -> Result<Value, String> {
    let identity = load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
    let config = match read_config(&state.data_dir) {
        Ok(c) => c,
        Err(_) => {
            return Err(DesktopError::NotAuthenticated.to_string());
        }
    };
    let cookie = match load_cookie(&config.server_url) {
        Ok(c) => c,
        Err(_) => {
            return Err(DesktopError::NotAuthenticated.to_string());
        }
    };
    match dashboard_snapshot(&config.server_url, &cookie, &identity, &state) {
        Ok(s) => Ok(s),
        Err(_) => offline_dashboard_snapshot(&identity, &state).map_err(|e| e.to_string()),
    }
}

#[tauri::command]
async fn load_session_timeline(
    input: SessionTimelineInput,
    state: State<'_, DesktopState>,
) -> Result<Vec<Value>, String> {
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || load_session_timeline_inner(input, &data_dir))
        .await
        .map_err(|_| DesktopError::Unreachable.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn load_session_conversation(
    input: SessionConversationInput,
    state: State<'_, DesktopState>,
) -> Result<Vec<Value>, String> {
    let data_dir = state.data_dir.clone();
    // Lock acquisition, startup waits and IPC all run off the async executor.
    tauri::async_runtime::spawn_blocking(move || {
        validate_session_id(&input.agent_session_id)?;
        let client = daemon_client::ensure_readonly_daemon_client(&data_dir)?;
        load_session_conversation_inner(input, client)
    })
    .await
    .map_err(|_| DesktopError::LocalIpc("local_worker_failed").to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_agent_models(state: State<'_, DesktopState>) -> Result<Vec<Value>, String> {
    list_agent_models_inner(&state).map_err(|error| error.to_string())
}

fn builtin_model_catalog(
    agent: AgentType,
) -> &'static [(&'static str, &'static str, &'static str)] {
    match agent {
        AgentType::ClaudeCode => &[
            ("claude-3-7-sonnet", "Claude 3.7 Sonnet", "最强混合推理与自主开发"),
            ("claude-sonnet-4-5", "Claude Sonnet 4.5", "日常开发主力"),
            ("claude-3-5-sonnet", "Claude 3.5 Sonnet", "经典编程与工具调用"),
            ("claude-opus-4-1", "Claude Opus 4.1", "深度复杂工程与规划"),
            ("claude-haiku-4-5", "Claude Haiku 4.5", "极速轻量低延迟响应"),
        ],
        AgentType::Codex => &[
            ("gpt-5.1-codex-max", "GPT-5.1 Codex Max", "最强代码推理"),
            ("gpt-5.1-codex", "GPT-5.1 Codex", "全栈自主开发"),
            ("gpt-5-codex", "GPT-5 Codex", "稳定代码代理"),
        ],
        AgentType::Antigravity => &[
            ("gemini-3-pro", "Gemini 3 Pro", "DeepMind 最新推理"),
            ("gemini-2.5-pro", "Gemini 2.5 Pro", "超长上下文架构"),
        ],
        AgentType::GrokBuild => &[
            ("grok-4", "Grok 4", "xAI 旗舰全栈开发"),
            ("grok-code-fast-1", "Grok Code Fast", "低延迟快速任务"),
        ],
        AgentType::ZCode => &[
            (
                "zcode-fast",
                "ZCode Fast",
                "极速代码生成与工具调用",
            ),
            ("zcode-pro", "ZCode Pro", "超长上下文与深度工程推理"),
            ("claude-sonnet-4-5", "Claude Sonnet 4.5", "多语言工程能力"),
        ],
    }
}

fn read_codex_configured_model() -> Option<String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    let codex_dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));
    let config_path = codex_dir.join("config.toml");
    let content = fs::read_to_string(config_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("model") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let val = rest.trim().trim_matches(|c| c == '"' || c == '\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn model_family(value: &str) -> String {
    let lower = value.trim().to_ascii_lowercase();
    lower
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches('-')
        .to_owned()
}

fn model_label(value: &str) -> String {
    for (id, label, _) in [
        builtin_model_catalog(AgentType::ClaudeCode),
        builtin_model_catalog(AgentType::Codex),
        builtin_model_catalog(AgentType::Antigravity),
        builtin_model_catalog(AgentType::GrokBuild),
        builtin_model_catalog(AgentType::ZCode),
    ]
    .into_iter()
    .flatten()
    {
        if *id == value {
            return (*label).to_owned();
        }
    }
    let is_gpt = value.trim().to_ascii_lowercase().starts_with("gpt-");
    let mut parts: Vec<String> = value
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_owned())
        .collect();
    for part in &mut parts {
        if part.len() == 1 || part.eq_ignore_ascii_case("gpt") {
            part.make_ascii_uppercase();
        } else if part
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
        {
            let mut chars = part.chars();
            let first = chars.next().unwrap_or_default().to_ascii_uppercase();
            *part = format!("{first}{}", chars.as_str());
        }
    }
    if is_gpt {
        parts.join("-")
    } else {
        parts.join(" ")
    }
}

fn list_agent_models_inner(state: &DesktopState) -> Result<Vec<Value>, DesktopError> {
    let contexts = local_session_contexts(state);
    let mut result = Vec::new();
    for agent in [
        AgentType::ClaudeCode,
        AgentType::Codex,
        AgentType::Antigravity,
        AgentType::GrokBuild,
        AgentType::ZCode,
    ] {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for context in contexts.values() {
            if context.agent_type != agent {
                continue;
            }
            if let Some(model) = context
                .model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                *counts.entry(model.to_owned()).or_default() += 1;
            }
        }
        let mut dynamic: Vec<(String, usize)> = counts.into_iter().collect();
        dynamic.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        dynamic.truncate(6);

        let mut models: Vec<Value> = Vec::new();
        let mut seen_families: HashSet<String> = HashSet::new();

        if agent == AgentType::Codex {
            // 1. If user explicitly configured a model in local ~/.codex/config.toml,
            // dynamically prioritize it as the top choice.
            if let Some(configured) = read_codex_configured_model() {
                seen_families.insert(model_family(&configured));
                let count = dynamic
                    .iter()
                    .find(|(k, _)| k == &configured)
                    .map(|(_, c)| *c)
                    .unwrap_or(0);
                let desc = if count > 0 {
                    format!("Codex 当前配置 · 实测 {count} 个会话")
                } else {
                    "Codex 当前配置模型".to_string()
                };
                models.push(json!({
                    "value": configured,
                    "label": model_label(&configured),
                    "desc": desc,
                    "source": "config"
                }));
            }

            // 2. Query official real-time models from native host
            if let Some(client) = daemon_client::get_daemon_client(state)
                && let Ok(val) = client.query_models()
                && let Some(arr) = val.get("data").and_then(Value::as_array)
                && !arr.is_empty()
            {
                for item in arr {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                    if !id.is_empty() && !seen_families.contains(&model_family(id)) {
                        seen_families.insert(model_family(id));
                        let label = item
                            .get("displayName")
                            .or_else(|| item.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or(id);
                        models.push(json!({
                            "value": id,
                            "label": label,
                            "desc": "Codex 官方实时模型",
                            "source": "native_host"
                        }));
                    }
                }
            }

            // 3. Dynamically append models measured in local session transcripts
            for (value, count) in &dynamic {
                if seen_families.contains(&model_family(value)) {
                    continue;
                }
                seen_families.insert(model_family(value));
                models.push(json!({
                    "value": value,
                    "label": model_label(value),
                    "desc": format!("本机 Agent 实测（{count} 个会话）"),
                    "source": "agent"
                }));
            }

            // 4. Safety net fallback only if absolutely no models found
            if models.is_empty() {
                for (value, label, desc) in builtin_model_catalog(agent) {
                    if seen_families.contains(&model_family(value)) {
                        continue;
                    }
                    models.push(json!({
                        "value": value,
                        "label": label,
                        "desc": desc,
                        "source": "builtin"
                    }));
                }
            }
        } else if agent == AgentType::ClaudeCode {
            let mut seen_ids: HashSet<String> = HashSet::new();

            // 1. Gateway configured models (if active)
            if let Some(gateway) = agent_pulse_runner::claude_gateway::discover_claude_gateway()
                && !gateway.models.is_empty()
            {
                for m in &gateway.models {
                    if !m.name.is_empty() && seen_ids.insert(m.name.clone()) {
                        let label = model_label(&m.name);
                        let desc = if let Some(target) = m
                            .label
                            .as_deref()
                            .filter(|l| !l.trim().is_empty())
                        {
                            format!("网关动态路由（至 {}）", target)
                        } else {
                            format!("网关动态路由（{}）", gateway.source)
                        };
                        models.push(json!({
                            "value": m.name,
                            "label": label,
                            "desc": desc,
                            "source": "gateway"
                        }));
                    }
                }
            }

            // 2. Models measured in local transcripts
            for (value, count) in &dynamic {
                if seen_ids.insert(value.clone()) {
                    models.push(json!({
                        "value": value,
                        "label": model_label(value),
                        "desc": format!("本机 Agent 实测（{count} 个会话）"),
                        "source": "agent"
                    }));
                }
            }

            // 3. Always include official Claude catalog models
            for (value, label, desc) in builtin_model_catalog(agent) {
                if seen_ids.insert((*value).to_string()) {
                    models.push(json!({
                        "value": value,
                        "label": label,
                        "desc": desc,
                        "source": "builtin"
                    }));
                }
            }
        } else {
            // For other agents, populate from local transcripts then builtins
            for (value, count) in &dynamic {
                seen_families.insert(model_family(value));
                models.push(json!({
                    "value": value,
                    "label": model_label(value),
                    "desc": format!("本机 Agent 实测（{count} 个会话）"),
                    "source": "agent"
                }));
            }
            if models.is_empty() {
                for (value, label, desc) in builtin_model_catalog(agent) {
                    if seen_families.contains(&model_family(value)) {
                        continue;
                    }
                    models.push(json!({
                        "value": value,
                        "label": label,
                        "desc": desc,
                        "source": "builtin"
                    }));
                }
            }
        }
        result.push(json!({ "agent_type": agent_type_name(agent), "models": models }));
    }
    Ok(result)
}

#[tauri::command]
async fn pick_folder(start_dir: Option<String>) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = start_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
        {
            dialog = dialog.set_directory(dir);
        }
        Ok(dialog
            .pick_folder()
            .map(|path| dunce::simplified(&path).to_string_lossy().into_owned()))
    })
    .await
    .map_err(|_| DesktopError::AgentFailed.to_string())?
}

#[tauri::command]
async fn pick_files(start_dir: Option<String>) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = start_dir
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
        {
            dialog = dialog.set_directory(dir);
        }
        Ok(dialog
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|path| dunce::simplified(&path).to_string_lossy().into_owned())
            .collect())
    })
    .await
    .map_err(|_| DesktopError::AgentFailed.to_string())?
}

#[tauri::command]
async fn send_session_message(
    input: SessionMessageInput,
    on_event: Channel<SessionTurnEvent>,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    validate_session_id(&input.agent_session_id).map_err(|error| error.to_string())?;
    if input.prompt.trim().is_empty() || input.prompt.trim().chars().count() > MAX_PROMPT_CHARS {
        return Err(DesktopError::InvalidAction.to_string());
    }
    if input.agent_type != AgentType::Codex
        && input.agent_type != AgentType::ZCode
        && input.agent_type != AgentType::ClaudeCode
        && input.agent_type != AgentType::Antigravity
        && input.agent_type != AgentType::GrokBuild
    {
        return Err(
            "当前版本仅支持受控 Codex / ZCode / Claude Code / Antigravity / Grok 会话继续".into(),
        );
    }
    let key = session_turn_key(input.agent_type, &input.agent_session_id);
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active_turns = state
            .active_turns
            .lock()
            .map_err(|_| DesktopError::AgentFailed.to_string())?;
        if active_turns.contains_key(&key) {
            return Err(DesktopError::AgentBusy.to_string());
        }
        active_turns.insert(key.clone(), cancelled.clone());
    }
    let _ = on_event.send(SessionTurnEvent::Started);

    let client = {
        let required_workspace = find_session_cwd(input.agent_type, &input.agent_session_id);
        let ensure_res = daemon_client::ensure_daemon_client(&state, required_workspace.as_deref());
        match ensure_res {
            Ok(client) => client,
            Err(error) => {
                if let Ok(mut active_turns) = state.active_turns.lock() {
                    active_turns.remove(&key);
                }
                return Err(error.to_string());
            }
        }
    };

    let data_dir = state.data_dir.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        send_codex_message_with_client(input, client, &data_dir, &cancelled, on_event)
    })
    .await;
    if let Ok(mut active_turns) = state.active_turns.lock() {
        active_turns.remove(&key);
    }
    result
        .map_err(|_| DesktopError::AgentFailed.to_string())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn launch_new_session(
    input: LaunchSessionInput,
    on_event: Channel<SessionTurnEvent>,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let prompt = input.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(DesktopError::InvalidAction.to_string());
    }
    if input.agent_type != AgentType::Codex
        && input.agent_type != AgentType::ZCode
        && input.agent_type != AgentType::ClaudeCode
        && input.agent_type != AgentType::Antigravity
        && input.agent_type != AgentType::GrokBuild
    {
        return Err("该 Provider 尚未接入远程控制；可写 Session 将在官方协议验证后开放".into());
    }
    let cwd_path = input
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .ok_or_else(|| DesktopError::InvalidAction.to_string())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let key = format!("launch:{}", uuid::Uuid::new_v4());
    {
        let mut active_turns = state
            .active_turns
            .lock()
            .map_err(|_| DesktopError::AgentFailed.to_string())?;
        active_turns.insert(key.clone(), cancelled.clone());
    }

    let client = match daemon_client::ensure_daemon_client(&state, Some(&cwd_path)) {
        Ok(client) => client,
        Err(error) => {
            if let Ok(mut active_turns) = state.active_turns.lock() {
                active_turns.remove(&key);
            }
            return Err(error.to_string());
        }
    };

    let result = tauri::async_runtime::spawn_blocking(move || {
        launch_codex_session_with_client(input, client, &cancelled, on_event)
    })
    .await
    .map_err(|_| DesktopError::AgentFailed.to_string())
    .and_then(|result| result.map_err(|error| error.to_string()));
    if let Ok(mut active_turns) = state.active_turns.lock() {
        active_turns.remove(&key);
    }
    result
}

fn launch_codex_session_with_client(
    input: LaunchSessionInput,
    client: DaemonClient,
    cancelled: &AtomicBool,
    on_event: Channel<SessionTurnEvent>,
) -> Result<Value, DesktopError> {
    let prompt = input.prompt.trim();
    let cwd = input
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .ok_or(DesktopError::InvalidAction)?;
    let cwd = dunce::canonicalize(&cwd).map_err(|_| DesktopError::InvalidAction)?;

    let (sub, rx, _initial_snapshot) = client
        .subscribe(256)
        .map_err(|_| DesktopError::AgentFailed)?;

    let host_generation = client
        .query_generation()
        .map_err(|_| DesktopError::AgentFailed)?;
    let hub_session_id = format!("hub-{}", uuid::Uuid::new_v4());
    let connect_cmd = agent_pulse_runner::remote_host::RemoteCommand {
        client_cmd_id: format!("cmd-{}", uuid::Uuid::new_v4()),
        hub_session_id: hub_session_id.clone(),
        host_generation: Some(host_generation.clone()),
        command: agent_pulse_runner::remote_host::RemoteAction::Connect {
            mode: agent_pulse_runner::remote_host::ConnectMode::Create,
            workspace_path: PathBuf::from(agent_pulse_runner::privacy::workspace_id_for_path(
                &cwd.to_string_lossy(),
            )),
            provider_session_id: None,
            model: input.model.filter(|m| !m.trim().is_empty()),
            sandbox: match input.permission_mode.as_deref() {
                Some("danger-full-access" | "danger" | "bypass") => {
                    agent_pulse_runner::remote_host::CodexSandbox::DangerFullAccess
                }
                Some("read-only") => agent_pulse_runner::remote_host::CodexSandbox::ReadOnly,
                _ => agent_pulse_runner::remote_host::CodexSandbox::WorkspaceWrite,
            },
            approval_policy: match input.permission_mode.as_deref() {
                Some("never") => agent_pulse_runner::remote_host::CodexApprovalPolicy::Never,
                Some("untrusted") => {
                    agent_pulse_runner::remote_host::CodexApprovalPolicy::Untrusted
                }
                _ => agent_pulse_runner::remote_host::CodexApprovalPolicy::OnRequest,
            },
            agent_type: Some(input.agent_type.as_str().to_string()),
        },
    };

    let receipt = client
        .execute(connect_cmd)
        .map_err(|e| DesktopError::Detailed(format!("启动会话失败: {}", e)))?;
    if receipt.status != "accepted" {
        let detail = receipt
            .code
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "session_launch_rejected".into());
        return Err(DesktopError::Detailed(format!(
            "创建会话被拒绝: {}",
            detail
        )));
    }
    let binding_val = receipt.result.ok_or(DesktopError::AgentFailed)?;
    let mut provider_session_id = binding_val["binding"]["provider_session_id"]
        .as_str()
        .ok_or(DesktopError::AgentFailed)?
        .to_owned();

    let _ = on_event.send(SessionTurnEvent::Started);

    let prompt_cmd = agent_pulse_runner::remote_host::RemoteCommand {
        client_cmd_id: format!("cmd-{}", uuid::Uuid::new_v4()),
        hub_session_id: hub_session_id.clone(),
        host_generation: Some(host_generation.clone()),
        command: agent_pulse_runner::remote_host::RemoteAction::SendPrompt {
            text: prompt.to_owned(),
            effort: input.reasoning_effort,
        },
    };

    let send_receipt = client
        .execute(prompt_cmd)
        .map_err(|e| DesktopError::Detailed(format!("发送提示词失败: {}", e)))?;
    if send_receipt.status != "accepted" {
        let detail = send_receipt
            .code
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "agent_turn_rejected".into());
        return Err(DesktopError::Detailed(format!(
            "Agent 拒绝执行: {}",
            detail
        )));
    }
    let expected_turn_id = send_receipt
        .result
        .as_ref()
        .and_then(|r| r["turn"]["id"].as_str())
        .filter(|id| !id.trim().is_empty())
        .ok_or(DesktopError::AgentFailed)?
        .to_owned();

    let mut final_response = String::new();
    let mut assistant = CodexAssistantAccumulator::default();
    let mut turn_completed = false;
    let deadline = Instant::now() + AGENT_TURN_TIMEOUT;
    while Instant::now() < deadline {
        if cancelled.load(Ordering::Relaxed) {
            return Err(DesktopError::AgentCancelled);
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                if event.hub_session_id.as_deref() == Some(&hub_session_id) {
                    if let Some(message) = assistant.apply(&event, &expected_turn_id) {
                        let _ = on_event.send(message);
                    }
                    match event.kind.as_str() {
                        "turn/started" => {
                            let _ = on_event.send(SessionTurnEvent::Activity {
                                label: "正在思考与执行...",
                            });
                        }
                        "approval_requested" => {
                            if let Some(approval) = event.approval {
                                let _ =
                                    on_event.send(SessionTurnEvent::ApprovalRequested { approval });
                            } else {
                                let _ = on_event.send(SessionTurnEvent::Activity {
                                    label: "等待审批确认...",
                                });
                            }
                        }
                        "turn/completed" => {
                            let event_turn_id = event.raw["params"]["turn"]["id"]
                                .as_str()
                                .or_else(|| event.raw["params"]["turnId"].as_str());
                            if event_turn_id != Some(expected_turn_id.as_str()) {
                                continue;
                            }
                            let status = event.raw["params"]["turn"]["status"]
                                .as_str()
                                .or_else(|| event.raw["params"]["turn"]["status"]["type"].as_str())
                                .unwrap_or("unknown");
                            if status == "interrupted" {
                                return Err(DesktopError::AgentCancelled);
                            }
                            if status != "completed" {
                                return Err(DesktopError::AgentFailed);
                            }
                            let had_streamed_response = !assistant.text.trim().is_empty();
                            final_response = assistant.finish_with_fallback(
                                event.raw["params"]["turn"]["output"].as_str(),
                            );
                            if !had_streamed_response && !final_response.is_empty() {
                                let _ = on_event.send(SessionTurnEvent::AssistantMessage {
                                    text: final_response.clone(),
                                });
                            }
                            if let Some(real_id) = event.raw["params"]["realSessionId"].as_str() {
                                provider_session_id = real_id.to_string();
                            }
                            turn_completed = true;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) if sub.is_healthy() => continue,
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(sub);

    if !turn_completed || final_response.is_empty() {
        return Err(DesktopError::AgentFailed);
    }

    Ok(json!({
        "agent_session_id": provider_session_id,
        "agent_type": input.agent_type.as_str(),
        "cwd": cwd.to_string_lossy(),
        "response": final_response,
        "completed_at": chrono::Utc::now(),
    }))
}

fn send_codex_message_with_client(
    input: SessionMessageInput,
    client: DaemonClient,
    _data_dir: &Path,
    cancelled: &AtomicBool,
    on_event: Channel<SessionTurnEvent>,
) -> Result<Value, DesktopError> {
    validate_session_id(&input.agent_session_id)?;
    let prompt = compose_turn_prompt(&input.prompt, &input.attachments)?;
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS * 4 {
        return Err(DesktopError::InvalidAction);
    }

    let mut provider_session_id = match input.agent_type {
        AgentType::ClaudeCode if !input.agent_session_id.starts_with("claude_") => {
            format!("claude_{}", input.agent_session_id)
        }
        _ => input.agent_session_id.clone(),
    };

    let sessions = client
        .query_sessions()
        .map_err(|_| DesktopError::SessionUnavailable)?;
    let existing_binding = sessions
        .iter()
        .find(|s| {
            s.provider_session_id == provider_session_id
                || s.provider_session_id == input.agent_session_id
        })
        .cloned();

    if let Some(binding) = &existing_binding
        && binding.control_connected
        && binding.current_turn_id.is_some()
        && matches!(
            binding.execution_state,
            agent_pulse_runner::remote_host::ExecutionState::Running
                | agent_pulse_runner::remote_host::ExecutionState::AwaitingPermission
                | agent_pulse_runner::remote_host::ExecutionState::AwaitingInput
        )
    {
        return Err(DesktopError::AgentBusy);
    }

    let (hub_session_id, cwd, needs_resume) = match existing_binding {
        Some(binding) => (
            binding.hub_session_id,
            binding.workspace_path,
            !binding.control_connected,
        ),
        None => (
            format!("hub-{}", uuid::Uuid::new_v4()),
            find_session_cwd(input.agent_type, &provider_session_id)
                .or_else(|| std::env::current_dir().ok())
                .ok_or(DesktopError::SessionUnavailable)?,
            true,
        ),
    };

    let host_generation = client
        .query_generation()
        .map_err(|_| DesktopError::AgentFailed)?;
    if needs_resume {
        let resume_cmd = agent_pulse_runner::remote_host::RemoteCommand {
            client_cmd_id: format!("cmd-{}", uuid::Uuid::new_v4()),
            hub_session_id: hub_session_id.clone(),
            host_generation: Some(host_generation.clone()),
            command: agent_pulse_runner::remote_host::RemoteAction::Connect {
                mode: agent_pulse_runner::remote_host::ConnectMode::Resume,
                workspace_path: PathBuf::from(agent_pulse_runner::privacy::workspace_id_for_path(
                    &cwd.to_string_lossy(),
                )),
                provider_session_id: Some(provider_session_id.clone()),
                model: input.model.filter(|m| !m.trim().is_empty()),
                sandbox: match input.permission_mode.as_deref() {
                    Some("danger-full-access" | "danger" | "bypass") => {
                        agent_pulse_runner::remote_host::CodexSandbox::DangerFullAccess
                    }
                    Some("read-only") => agent_pulse_runner::remote_host::CodexSandbox::ReadOnly,
                    _ => agent_pulse_runner::remote_host::CodexSandbox::WorkspaceWrite,
                },
                approval_policy: match input.permission_mode.as_deref() {
                    Some("never") => agent_pulse_runner::remote_host::CodexApprovalPolicy::Never,
                    Some("untrusted") => {
                        agent_pulse_runner::remote_host::CodexApprovalPolicy::Untrusted
                    }
                    _ => agent_pulse_runner::remote_host::CodexApprovalPolicy::OnRequest,
                },
                agent_type: Some(input.agent_type.as_str().to_string()),
            },
        };
        let receipt = client
            .execute(resume_cmd)
            .map_err(|e| DesktopError::Detailed(format!("连接会话失败: {}", e)))?;
        if receipt.status != "accepted" {
            let detail = receipt
                .code
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| "external_session_not_resumable".into());
            let human_msg = match detail.as_str() {
                "external_session_not_resumable" => "本机会话不可恢复或模型不可用，请刷新后重试",
                "workspace_not_authorized" => "工作区目录未授权，请检查工作区设置",
                "invalid_model" => "指定的模型无效或当前运行时不可用",
                "resume_workspace_mismatch" => "会话历史工作区与当前工作区不匹配",
                other => other,
            };
            return Err(DesktopError::Detailed(human_msg.to_string()));
        }
    }

    let (sub, rx, _initial_snapshot) = client
        .subscribe(256)
        .map_err(|_| DesktopError::AgentFailed)?;

    let prompt_cmd = agent_pulse_runner::remote_host::RemoteCommand {
        client_cmd_id: format!("cmd-{}", uuid::Uuid::new_v4()),
        hub_session_id: hub_session_id.clone(),
        host_generation: Some(host_generation.clone()),
        command: agent_pulse_runner::remote_host::RemoteAction::SendPrompt {
            text: prompt,
            effort: input.reasoning_effort,
        },
    };

    let send_receipt = client
        .execute(prompt_cmd)
        .map_err(|e| DesktopError::Detailed(format!("发送提示词失败: {}", e)))?;
    if send_receipt.status != "accepted" {
        let detail = send_receipt
            .code
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "agent_turn_rejected".into());
        return Err(DesktopError::Detailed(format!(
            "Agent 拒绝执行: {}",
            detail
        )));
    }
    let expected_turn_id = send_receipt
        .result
        .as_ref()
        .and_then(|r| r["turn"]["id"].as_str())
        .filter(|id| !id.trim().is_empty())
        .ok_or(DesktopError::AgentFailed)?
        .to_owned();

    let mut final_response = String::new();
    let mut assistant = CodexAssistantAccumulator::default();
    let mut turn_completed = false;
    let deadline = Instant::now() + AGENT_TURN_TIMEOUT;
    while Instant::now() < deadline {
        if cancelled.load(Ordering::Relaxed) {
            return Err(DesktopError::AgentCancelled);
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => {
                if event.hub_session_id.as_deref() == Some(&hub_session_id) {
                    if let Some(message) = assistant.apply(&event, &expected_turn_id) {
                        let _ = on_event.send(message);
                    }
                    match event.kind.as_str() {
                        "turn/started" => {
                            let _ = on_event.send(SessionTurnEvent::Activity {
                                label: "正在思考与执行...",
                            });
                        }
                        "approval_requested" => {
                            if let Some(approval) = event.approval {
                                let _ =
                                    on_event.send(SessionTurnEvent::ApprovalRequested { approval });
                            } else {
                                let _ = on_event.send(SessionTurnEvent::Activity {
                                    label: "等待审批确认...",
                                });
                            }
                        }
                        "turn/completed" => {
                            let event_turn_id = event.raw["params"]["turn"]["id"]
                                .as_str()
                                .or_else(|| event.raw["params"]["turnId"].as_str());
                            if event_turn_id != Some(expected_turn_id.as_str()) {
                                continue;
                            }
                            let status = event.raw["params"]["turn"]["status"]
                                .as_str()
                                .or_else(|| event.raw["params"]["turn"]["status"]["type"].as_str())
                                .unwrap_or("unknown");
                            if status == "interrupted" {
                                return Err(DesktopError::AgentCancelled);
                            }
                            if status != "completed" {
                                return Err(DesktopError::AgentFailed);
                            }
                            let had_streamed_response = !assistant.text.trim().is_empty();
                            final_response = assistant.finish_with_fallback(
                                event.raw["params"]["turn"]["output"].as_str(),
                            );
                            if !had_streamed_response && !final_response.is_empty() {
                                let _ = on_event.send(SessionTurnEvent::AssistantMessage {
                                    text: final_response.clone(),
                                });
                            }
                            if let Some(real_id) = event.raw["params"]["realSessionId"].as_str() {
                                provider_session_id = real_id.to_string();
                            }
                            turn_completed = true;
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) if sub.is_healthy() => continue,
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(sub);

    if !turn_completed || final_response.is_empty() {
        return Err(DesktopError::AgentFailed);
    }

    Ok(json!({
        "agent_session_id": provider_session_id,
        "agent_type": input.agent_type.as_str(),
        "cwd": cwd.to_string_lossy(),
        "response": final_response,
        "completed_at": chrono::Utc::now(),
    }))
}

fn find_codex_session_cwd(session_id: &str) -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    let codex_dir = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".codex"));

    if let Ok(entries) = fs::read_dir(&codex_dir) {
        let mut sqlite_files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.starts_with("state_")
                && name.ends_with(".sqlite")
            {
                sqlite_files.push(path);
            }
        }
        sqlite_files.sort();
        sqlite_files.reverse();

        for db_path in sqlite_files {
            if let Ok(conn) = rusqlite::Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            ) {
                let res: rusqlite::Result<String> = conn.query_row(
                    "SELECT cwd FROM threads WHERE id = ?1",
                    [session_id],
                    |row| row.get(0),
                );
                if let Ok(cwd_str) = res {
                    let p = PathBuf::from(cwd_str);
                    if p.is_dir() {
                        return dunce::canonicalize(&p).ok().or(Some(p));
                    }
                }
            }
        }
    }
    None
}

fn find_zcode_session_cwd(session_id: &str) -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    let db_path = home.join(".zcode").join("cli").join("db").join("db.sqlite");
    if !db_path.is_file() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let res: rusqlite::Result<String> = conn.query_row(
        "SELECT directory FROM session WHERE id = ?1",
        [session_id],
        |row| row.get(0),
    );
    if let Ok(dir_str) = res {
        let p = PathBuf::from(dir_str);
        if p.is_dir() {
            return dunce::canonicalize(&p).ok().or(Some(p));
        }
    }
    None
}

fn find_session_cwd(agent_type: AgentType, session_id: &str) -> Option<PathBuf> {
    match agent_type {
        AgentType::ZCode => find_zcode_session_cwd(session_id),
        AgentType::Codex => find_codex_session_cwd(session_id),
        AgentType::ClaudeCode => {
            let raw_id = session_id.trim_start_matches("claude_");
            agent_pulse_runner::collectors::find_claude_session_info(raw_id)
                .ok()
                .flatten()
                .map(|(c, _)| c)
        }
        _ => {
            if session_id.starts_with("sess_") {
                find_zcode_session_cwd(session_id)
            } else if session_id.starts_with("claude_") {
                let raw_id = session_id.trim_start_matches("claude_");
                agent_pulse_runner::collectors::find_claude_session_info(raw_id)
                    .ok()
                    .flatten()
                    .map(|(c, _)| c)
            } else {
                find_codex_session_cwd(session_id)
            }
        }
    }
}

#[tauri::command]
async fn cancel_session_turn(
    input: SessionTurnRef,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    validate_session_id(&input.agent_session_id).map_err(|error| error.to_string())?;
    if input.agent_type != AgentType::Codex
        && input.agent_type != AgentType::ZCode
        && input.agent_type != AgentType::ClaudeCode
    {
        return Err("该 Provider 尚未接入远程控制".to_owned());
    }
    // Resolve the client before moving off the runtime: `get_daemon_client`
    // probes liveness, and every daemon call below blocks for up to the 25 s
    // local query timeout. Running the rest on a blocking thread keeps a wedged
    // daemon from stalling the async runtime the rest of the window shares.
    let client = daemon_client::get_daemon_client(&state)
        .ok_or_else(|| "受控 Agent NativeSessionHost 未在运行".to_owned())?;
    tauri::async_runtime::spawn_blocking(move || cancel_session_turn_inner(input, client))
        .await
        .map_err(|_| DesktopError::AgentFailed.to_string())?
}

fn cancel_session_turn_inner(
    input: SessionTurnRef,
    client: daemon_client::DaemonClient,
) -> Result<Value, String> {
    let sessions = client
        .query_sessions()
        .map_err(|e| format!("无法查询会话注册表: {e}"))?;
    let provider_session_id = match input.agent_type {
        AgentType::ClaudeCode if !input.agent_session_id.starts_with("claude_") => {
            format!("claude_{}", input.agent_session_id)
        }
        _ => input.agent_session_id.clone(),
    };
    let binding = sessions
        .iter()
        .find(|s| {
            s.provider_session_id == provider_session_id
                || s.provider_session_id == input.agent_session_id
        })
        .ok_or_else(|| format!("未找到绑定的会话 {}", input.agent_session_id))?;
    let generation = client
        .query_generation()
        .map_err(|e| format!("无法获取 Host 代际: {e}"))?;
    let interrupt_cmd = agent_pulse_runner::remote_host::RemoteCommand {
        client_cmd_id: format!("cmd-{}", uuid::Uuid::new_v4()),
        hub_session_id: binding.hub_session_id.clone(),
        host_generation: Some(generation),
        command: agent_pulse_runner::remote_host::RemoteAction::Interrupt,
    };
    let receipt = client
        .execute(interrupt_cmd)
        .map_err(|e| format!("执行中断指令失败: {e}"))?;
    if receipt.status != "accepted" {
        return Err(format!("中断请求被拒绝: {}", receipt.status));
    }
    Ok(json!({ "accepted": true, "receipt": receipt }))
}

#[tauri::command]
async fn resolve_session_approval(
    input: SessionApprovalInput,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    if (input.agent_type != AgentType::Codex
        && input.agent_type != AgentType::ZCode
        && input.agent_type != AgentType::ClaudeCode)
        || input.approval_id.trim().is_empty()
        || input.approval_id.len() > 256
    {
        return Err(DesktopError::InvalidAction.to_string());
    }
    validate_session_id(&input.agent_session_id).map_err(|error| error.to_string())?;
    // Approvals are the one interaction where blocking the async runtime hurts
    // most: the operator is waiting on the agent, and the daemon round trip can
    // take the full 25 s local query timeout.
    let client =
        daemon_client::get_daemon_client(&state).ok_or_else(|| "agenthubd 未在运行".to_owned())?;
    tauri::async_runtime::spawn_blocking(move || resolve_session_approval_inner(input, client))
        .await
        .map_err(|_| DesktopError::AgentFailed.to_string())?
}

fn resolve_session_approval_inner(
    input: SessionApprovalInput,
    client: daemon_client::DaemonClient,
) -> Result<Value, String> {
    let claude_id = format!("claude_{}", input.agent_session_id);
    let binding = client
        .query_sessions()
        .map_err(|error| format!("无法查询会话注册表: {error}"))?
        .into_iter()
        .find(|binding| {
            binding.provider_session_id == input.agent_session_id
                || binding.provider_session_id == claude_id
        })
        .ok_or_else(|| "未找到可控的会话".to_owned())?;
    let receipt = client
        .execute(agent_pulse_runner::remote_host::RemoteCommand {
            client_cmd_id: format!("cmd-{}", uuid::Uuid::new_v4()),
            hub_session_id: binding.hub_session_id,
            host_generation: Some(binding.host_generation),
            command: agent_pulse_runner::remote_host::RemoteAction::ResolveApproval {
                approval_id: input.approval_id,
                approved: input.approved,
            },
        })
        .map_err(|error| format!("审批指令发送失败: {error}"))?;
    if receipt.status != "accepted" {
        return Err(format!(
            "审批请求被拒绝: {}",
            receipt.code.as_deref().unwrap_or(&receipt.status)
        ));
    }
    Ok(json!({"accepted": true, "receipt": receipt}))
}

#[tauri::command]
async fn set_session_archived(
    input: ArchiveSessionInput,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let client = if input.agent_type == AgentType::Codex
        || input.agent_type == AgentType::ZCode
        || input.agent_type == AgentType::ClaudeCode
    {
        Some(
            daemon_client::get_daemon_client(&state)
                .ok_or_else(|| "agenthubd 未在运行".to_owned())?,
        )
    } else {
        None
    };
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        set_session_archived_inner(input, &data_dir, client)
    })
    .await
    .map_err(|_| DesktopError::AgentFailed.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_desktop_preferences(
    input: DesktopPreferencesInput,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let mut config = read_config(&state.data_dir).map_err(|error| error.to_string())?;
    if !(2..=60).contains(&input.refresh_interval_seconds)
        || !valid_binary(&input.claude_binary)
        || !valid_binary(&input.grok_binary)
    {
        return Err(DesktopError::InvalidAction.to_string());
    }
    let identity = load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
    config.refresh_interval_seconds = input.refresh_interval_seconds;
    config.claude_binary = input.claude_binary.trim().to_owned();
    config.grok_binary = input.grok_binary.trim().to_owned();
    write_config(&state.data_dir, &config).map_err(|error| error.to_string())?;
    Ok(preferences_json(
        &state.data_dir,
        &config,
        identity.device_id(),
    ))
}

#[tauri::command]
async fn set_sync_paused(paused: bool, state: State<'_, DesktopState>) -> Result<Value, String> {
    let mut engine = state
        .engine
        .lock()
        .map_err(|_| DesktopError::Sync.to_string())?;
    if paused {
        if let Some(handle) = engine.take() {
            handle.stop();
        }
    } else if engine.is_none() {
        let config = read_config(&state.data_dir).map_err(|error| error.to_string())?;
        let identity =
            load_or_create_identity(&state.data_dir).map_err(|error| error.to_string())?;
        *engine = Some(
            build_engine(&state.data_dir, &config.server_url, &identity)
                .map_err(|error| error.to_string())?,
        );
    }
    Ok(json!({ "paused": paused }))
}

#[tauri::command]
async fn logout(state: State<'_, DesktopState>) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    let authorization = tauri::async_runtime::spawn_blocking(move || {
        daemon_client::take_logout_authorization(&data_dir)
    })
    .await
    .map_err(|_| "无法清除本机登录状态".to_owned())??;
    if let Ok(mut engine) = state.engine.lock()
        && let Some(handle) = engine.take()
    {
        handle.stop();
    }
    stop_dashboard_realtime(&state);
    if let Ok(active_turns) = state.active_turns.lock() {
        for cancelled in active_turns.values() {
            cancelled.store(true, Ordering::Relaxed);
        }
    }
    if let Some((server_url, cookie)) = authorization {
        let _ = request_json(
            "POST",
            &server_url,
            "/v1/auth/logout",
            Some(&cookie),
            Some(&json!({})),
        );
    }
    Ok(())
}

fn finish_login(
    server_url: &str,
    cookie: &str,
    account: Value,
    state: &DesktopState,
) -> Result<Value, DesktopError> {
    store_cookie(server_url, cookie)?;
    write_config(
        &state.data_dir,
        &DesktopConfig {
            server_url: server_url.to_owned(),
            refresh_interval_seconds: default_refresh_interval_seconds(),
            claude_binary: default_claude_binary(),
            codex_binary: default_codex_binary(),
            grok_binary: default_grok_binary(),
            archived_sessions: Vec::new(),
        },
    )?;

    let identity = load_or_create_identity(&state.data_dir)?;
    ensure_device_bound(server_url, cookie, &identity)?;
    start_engine(state, server_url, &identity)?;

    // Automatically enable Remote host and seed workspace on login
    let (_, profile_path) =
        daemon_client::host_database_path(&state.data_dir, identity.device_id());
    let mut ws = Vec::new();
    if let Ok(cwd) = std::env::current_dir().and_then(|p| dunce::canonicalize(&p))
        && cwd.is_dir()
    {
        ws.push(cwd);
    }
    let mut profile = fs::read(&profile_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<daemon_client::DaemonProfile>(&bytes).ok())
        .unwrap_or(daemon_client::DaemonProfile {
            workspace_paths: ws.clone(),
            codex_binary: default_codex_binary(),
            remote_enabled: true,
        });
    profile.remote_enabled = true;
    if profile.workspace_paths.is_empty() && !ws.is_empty() {
        profile.workspace_paths = ws;
    }
    profile.workspace_paths = daemon_client::normalize_workspaces(profile.workspace_paths);
    if let Ok(bytes) = serde_json::to_vec(&profile) {
        let _ = fs::write(&profile_path, bytes);
    }
    daemon_client::ensure_daemon_started(state.data_dir.clone());

    let mut snapshot = dashboard_snapshot(server_url, cookie, &identity, state)?;
    snapshot["account"] = account;
    Ok(snapshot)
}

fn authenticate_inner(input: AuthInput, state: &DesktopState) -> Result<Value, DesktopError> {
    let configured_server = input
        .server_url
        .or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))?;
    let path = match input.mode {
        AuthMode::Login => "/v1/auth/login",
        AuthMode::Register => "/v1/auth/register",
    };
    let body = match input.mode {
        AuthMode::Login => json!({ "email": input.email, "password": input.password }),
        AuthMode::Register => json!({
            "email": input.email,
            "password": input.password,
            "display_name": input.display_name.unwrap_or_default()
        }),
    };
    let (account, cookie) = authenticate_request(&server_url, path, &body)?;
    finish_login(&server_url, &cookie, account, state)
}

#[tauri::command]
async fn open_external_url(url: String) -> Result<(), String> {
    let target = url.trim().to_owned();
    if !target.starts_with("https://") && !target.starts_with("http://") {
        return Err("invalid_url_scheme".to_owned());
    }
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "shell32")]
        unsafe extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                lpOperation: *const u16,
                lpFile: *const u16,
                lpParameters: *const u16,
                lpDirectory: *const u16,
                nShowCmd: i32,
            ) -> isize;
        }

        let operation: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        let file: Vec<u16> = OsStr::new(&target).encode_wide().chain(Some(0)).collect();

        let res = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // SW_SHOWNORMAL
            )
        };

        if res <= 32 {
            return Err(format!("ShellExecuteW failed with code {res}"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&target).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&target).spawn();
    }
    Ok(())
}

#[tauri::command]
async fn start_tg_browser_oauth(
    server_url: Option<String>,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let configured_server = server_url.or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))
            .map_err(|e| e.to_string())?;

    // Bind ephemeral loopback port for RFC 8252 PKCE loopback redirect
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind local loopback port: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local port: {e}"))?
        .port();

    let redirect_uri = format!("http://127.0.0.1:{local_port}/callback");

    // Request authorize URL and PKCE verifier from Gateway API
    let init_res = request_json(
        "POST",
        &server_url,
        "/v1/auth/oauth/tg/authorize-url",
        None,
        Some(&json!({
            "redirect_uri": redirect_uri,
            "scope": "openid profile email offline_access"
        })),
    )
    .map_err(|e| format!("Failed to initialize OAuth: {e}"))?;

    let authorize_url = init_res["authorize_url"]
        .as_str()
        .ok_or_else(|| "Missing authorize_url in response".to_owned())?
        .to_owned();
    let code_verifier = init_res["code_verifier"]
        .as_str()
        .ok_or_else(|| "Missing code_verifier in response".to_owned())?
        .to_owned();
    let expected_state = init_res["state"]
        .as_str()
        .ok_or_else(|| "Missing state in response".to_owned())?
        .to_owned();

    // Emit event so frontend has the URL immediately for retry/reopen button
    let _ = app.emit("oauth_url_ready", json!({ "url": &authorize_url }));

    // Open user's system default browser
    let _ = open_external_url(authorize_url).await;

    // Accept redirect connection in a blocking thread with timeout
    let (code, returned_state) = tauri::async_runtime::spawn_blocking(move || {
        use std::io::{Read, Write};

        listener
            .incoming()
            .next()
            .ok_or_else(|| "No connection received".to_owned())
            .and_then(|stream_res| {
                let mut stream = stream_res.map_err(|e| format!("Connection failed: {e}"))?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(120)))
                    .map_err(|e| format!("Timeout config error: {e}"))?;
                let mut buffer = [0u8; 4096];
                let n = stream
                    .read(&mut buffer)
                    .map_err(|e| format!("Failed to read callback request: {e}"))?;
                let req_text = String::from_utf8_lossy(&buffer[..n]);

                let first_line = req_text.lines().next().unwrap_or_default();
                let path_and_query = first_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default();

                let callback_url = format!("http://127.0.0.1{path_and_query}");
                let parsed_url =
                    Url::parse(&callback_url).map_err(|e| format!("Malformed callback: {e}"))?;

                let code = parsed_url
                    .query_pairs()
                    .find(|(k, _)| k == "code")
                    .map(|(_, v)| v.to_string());
                let state = parsed_url
                    .query_pairs()
                    .find(|(k, _)| k == "state")
                    .map(|(_, v)| v.to_string());
                let error = parsed_url
                    .query_pairs()
                    .find(|(k, _)| k == "error")
                    .map(|(_, v)| v.to_string());

                let html_body = if code.is_some() {
                    r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <title>AgentHub 授权成功</title>
  <style>
    body { background: #0b0d0f; color: #fff; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
    .card { text-align: center; padding: 48px; background: #14171c; border-radius: 16px; border: 1px solid rgba(255,255,255,0.1); box-shadow: 0 16px 48px rgba(0,0,0,0.6); max-width: 420px; }
    .icon { font-size: 56px; margin-bottom: 16px; }
    h2 { color: #7ed5a0; margin: 0 0 12px 0; font-size: 24px; font-weight: 700; }
    p { color: #8a99a8; margin: 0; font-size: 14px; line-height: 1.6; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✅</div>
    <h2>授权成功</h2>
    <p>已成功连接到 AgentHub 桌面客户端。<br>您可以关闭此标签页并返回桌面端继续操作。</p>
  </div>
  <script>setTimeout(function(){ window.close(); }, 2000);</script>
</body>
</html>"#
                } else {
                    r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <title>AgentHub 授权未完成</title>
  <style>
    body { background: #0b0d0f; color: #fff; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
    .card { text-align: center; padding: 48px; background: #14171c; border-radius: 16px; border: 1px solid rgba(255,255,255,0.1); box-shadow: 0 16px 48px rgba(0,0,0,0.6); max-width: 420px; }
    .icon { font-size: 56px; margin-bottom: 16px; }
    h2 { color: #ff6b6b; margin: 0 0 12px 0; font-size: 24px; font-weight: 700; }
    p { color: #8a99a8; margin: 0; font-size: 14px; line-height: 1.6; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">⚠️</div>
    <h2>授权未完成</h2>
    <p>未能获取有效授权代码，请返回客户端重新发起。</p>
  </div>
</body>
</html>"#
                };

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html_body.len(),
                    html_body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();

                if let Some(err) = error {
                    return Err(format!("Authorization denied: {err}"));
                }
                let code =
                    code.ok_or_else(|| "No authorization code found in callback".to_owned())?;
                Ok((code, state))
            })
    })
    .await
    .map_err(|e| format!("Async task failed: {e}"))??;

    if let Some(st) = returned_state
        && st != expected_state
    {
        return Err("OAuth state verification mismatch".to_owned());
    }

    // Exchange code via Gateway API
    let callback_url = format!("{server_url}/v1/auth/oauth/tg/callback");
    let (account, cookie) = execute_http_with_fallback(|agent| {
        let mut response = agent.post(&callback_url).send_json(json!({
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri
        }))?;
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(extract_session_cookie)
            .ok_or_else(|| {
                ureq::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "missing cookie",
                ))
            })?;
        let account = response.body_mut().read_json::<Value>().map_err(|e| {
            ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        Ok((account, cookie))
    })
    .map_err(|e| e.to_string())?;

    let snapshot =
        finish_login(&server_url, &cookie, account, &state).map_err(|e| e.to_string())?;
    start_dashboard_realtime(&state, &app).map_err(|error| error.to_string())?;
    Ok(snapshot)
}

#[tauri::command]
async fn start_tg_device_auth(server_url: Option<String>) -> Result<Value, String> {
    let configured_server = server_url.or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))
            .map_err(|e| e.to_string())?;
    request_json(
        "POST",
        &server_url,
        "/v1/auth/oauth/tg/device/code",
        None,
        Some(&json!({})),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn poll_tg_device_auth(
    server_url: Option<String>,
    device_code: String,
    app: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<Value, String> {
    let configured_server = server_url.or_else(|| std::env::var("AGENT_PULSE_SERVER_URL").ok());
    let server_url =
        normalize_server_url(configured_server.as_deref().unwrap_or(DEFAULT_SERVER_URL))
            .map_err(|e| e.to_string())?;
    let url = format!("{server_url}/v1/auth/oauth/tg/device/poll");
    let (body, cookie) = execute_http_with_fallback(|agent| {
        let mut response = agent
            .post(&url)
            .send_json(json!({ "device_code": device_code }))?;
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(extract_session_cookie);
        let body = response.body_mut().read_json::<Value>().map_err(|e| {
            ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        Ok((body, cookie))
    })
    .map_err(|e| e.to_string())?;

    if body["status"].as_str() == Some("pending") {
        return Ok(body);
    }

    if body["status"].as_str() == Some("success") {
        let cookie = cookie.ok_or_else(|| DesktopError::InvalidResponse.to_string())?;
        let account = body["account"].clone();
        let snapshot =
            finish_login(&server_url, &cookie, account, &state).map_err(|e| e.to_string())?;
        start_dashboard_realtime(&state, &app).map_err(|error| error.to_string())?;
        daemon_client::try_connect_remote(&state.data_dir);
        return Ok(json!({ "status": "success", "snapshot": snapshot }));
    }

    Err(DesktopError::InvalidResponse.to_string())
}

fn authenticate_request(
    server_url: &str,
    path: &str,
    body: &Value,
) -> Result<(Value, String), DesktopError> {
    let url = format!("{server_url}{path}");
    execute_http_with_fallback(|agent| {
        let mut response = agent.post(&url).send_json(body)?;
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(extract_session_cookie)
            .ok_or_else(|| {
                ureq::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "missing cookie",
                ))
            })?;
        let account = response.body_mut().read_json::<Value>().map_err(|e| {
            ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        Ok((account, cookie))
    })
}

fn ensure_device_bound(
    server_url: &str,
    cookie: &str,
    identity: &DeviceIdentity,
) -> Result<(), DesktopError> {
    let devices = request_json("GET", server_url, "/v1/devices", Some(cookie), None)?;
    let already_bound = devices["devices"].as_array().is_some_and(|devices| {
        devices.iter().any(|device| {
            device["device_id"].as_str() == Some(identity.device_id())
                && device["revoked"].as_bool() != Some(true)
        })
    });
    if already_bound {
        return Ok(());
    }
    let binding = request_json(
        "POST",
        server_url,
        "/v1/device-bindings",
        Some(cookie),
        Some(&json!({})),
    )?;
    let code = binding["binding_code"]
        .as_str()
        .ok_or(DesktopError::InvalidResponse)?;
    GatewayClient::new(server_url, REQUEST_TIMEOUT)
        .and_then(|client| client.bind(identity, code))
        .map_err(|error| match error {
            agent_pulse_runner::gateway::GatewayError::Rejected { status: 409 } => {
                DesktopError::DeviceConflict
            }
            _ => DesktopError::Sync,
        })?;
    Ok(())
}

fn dashboard_snapshot(
    server_url: &str,
    cookie: &str,
    identity: &DeviceIdentity,
    state: &DesktopState,
) -> Result<Value, DesktopError> {
    let desktop_config = read_config(&state.data_dir)?;
    let (account, devices, remote_sessions) = thread::scope(|scope| {
        let account =
            scope.spawn(|| request_json("GET", server_url, "/v1/auth/me", Some(cookie), None));
        let devices =
            scope.spawn(|| request_json("GET", server_url, "/v1/devices", Some(cookie), None));
        let sessions =
            scope.spawn(|| request_json("GET", server_url, "/v1/sessions", Some(cookie), None));
        Ok::<_, DesktopError>((
            account
                .join()
                .map_err(|_| DesktopError::InvalidResponse)??,
            devices
                .join()
                .map_err(|_| DesktopError::InvalidResponse)??,
            sessions
                .join()
                .map_err(|_| DesktopError::InvalidResponse)??,
        ))
    })?;
    let outbox =
        Outbox::open(state.data_dir.join("outbox.sqlite")).map_err(|_| DesktopError::Sync)?;
    let stats = outbox.stats().map_err(|_| DesktopError::Sync)?;
    let local_sessions = outbox.snapshots().map_err(|_| DesktopError::Sync)?;
    let claude_health = outbox
        .adapter_health(AgentType::ClaudeCode)
        .map_err(|_| DesktopError::Sync)?;
    let codex_health = outbox
        .adapter_health(AgentType::Codex)
        .map_err(|_| DesktopError::Sync)?;
    let antigravity_health = outbox
        .adapter_health(AgentType::Antigravity)
        .map_err(|_| DesktopError::Sync)?;
    let grok_health = outbox
        .adapter_health(AgentType::GrokBuild)
        .map_err(|_| DesktopError::Sync)?;
    let zcode_health = outbox
        .adapter_health(AgentType::ZCode)
        .map_err(|_| DesktopError::Sync)?;
    let local_contexts = local_session_contexts(state);
    let (host_bindings, pending_approvals) = daemon_client::get_daemon_client(state)
        .and_then(|client| client.snapshot().ok())
        .map(|snapshot| (snapshot.sessions, snapshot.pending_approvals))
        .unwrap_or_default();
    let sessions = merge_dashboard_sessions(
        &remote_sessions["sessions"],
        &local_sessions,
        &desktop_config.archived_sessions,
        identity.device_id(),
        &local_contexts,
        &host_bindings,
        &pending_approvals,
    );
    let running = state
        .engine
        .lock()
        .map(|engine| engine.is_some())
        .unwrap_or(false);
    let realtime_connected = state
        .realtime
        .lock()
        .ok()
        .and_then(|realtime| {
            realtime
                .as_ref()
                .map(|handle| handle.connected.load(Ordering::Relaxed))
        })
        .unwrap_or(false);
    Ok(json!({
        "account": account,
        "devices": devices["devices"],
        "sessions": sessions,
        "device": {
            "device_id": identity.device_id(),
            "name": machine_name()
        },
        "local": {
            "pending_events": stats.pending_events,
            "session_count": local_sessions.len(),
            "sources": [
                local_source_summary(AgentType::ClaudeCode, &local_sessions, claude_health),
                local_source_summary(AgentType::Codex, &local_sessions, codex_health),
                local_source_summary(AgentType::Antigravity, &local_sessions, antigravity_health),
                local_source_summary(AgentType::GrokBuild, &local_sessions, grok_health),
                local_source_summary(AgentType::ZCode, &local_sessions, zcode_health)
            ],
            "refreshed_at": chrono::Utc::now()
        },
        "sync_running": running,
        "realtime_connected": realtime_connected,
        "server_url": server_url,
        "preferences": preferences_json(&state.data_dir, &desktop_config, identity.device_id())
    }))
}

fn offline_dashboard_snapshot(
    identity: &DeviceIdentity,
    state: &DesktopState,
) -> Result<Value, DesktopError> {
    let desktop_config = read_config(&state.data_dir).unwrap_or(DesktopConfig {
        server_url: DEFAULT_SERVER_URL.to_owned(),
        refresh_interval_seconds: default_refresh_interval_seconds(),
        claude_binary: default_claude_binary(),
        codex_binary: default_codex_binary(),
        grok_binary: default_grok_binary(),
        archived_sessions: Vec::new(),
    });
    let outbox_path = state.data_dir.join("outbox.sqlite");
    let (
        stats,
        local_sessions,
        claude_health,
        codex_health,
        antigravity_health,
        grok_health,
        zcode_health,
    ) = if let Ok(outbox) = Outbox::open(&outbox_path) {
        (
            outbox
                .stats()
                .unwrap_or(agent_pulse_runner::outbox::OutboxStats {
                    pending_events: 0,
                    session_snapshots: 0,
                }),
            outbox.snapshots().unwrap_or_default(),
            outbox.adapter_health(AgentType::ClaudeCode).ok().flatten(),
            outbox.adapter_health(AgentType::Codex).ok().flatten(),
            outbox.adapter_health(AgentType::Antigravity).ok().flatten(),
            outbox.adapter_health(AgentType::GrokBuild).ok().flatten(),
            outbox.adapter_health(AgentType::ZCode).ok().flatten(),
        )
    } else {
        (
            agent_pulse_runner::outbox::OutboxStats {
                pending_events: 0,
                session_snapshots: 0,
            },
            Vec::new(),
            None,
            None,
            None,
            None,
            None,
        )
    };

    let local_contexts = local_session_contexts(state);
    let (host_bindings, pending_approvals) = daemon_client::get_daemon_client(state)
        .and_then(|client| client.snapshot().ok())
        .map(|snapshot| (snapshot.sessions, snapshot.pending_approvals))
        .unwrap_or_default();
    let empty_remote = json!([]);
    let sessions = merge_dashboard_sessions(
        &empty_remote,
        &local_sessions,
        &desktop_config.archived_sessions,
        identity.device_id(),
        &local_contexts,
        &host_bindings,
        &pending_approvals,
    );

    let now_str = chrono::Utc::now().to_rfc3339();
    Ok(json!({
        "account": {
            "user": {
                "user_id": "offline",
                "email": "离线本地模式",
                "display_name": "本机离线用户"
            },
            "subscription": {
                "plan": "offline",
                "status": "local_only"
            },
            "entitlements": {
                "max_devices": 1,
                "history_days": 365
            },
            "offline": true
        },
        "devices": [
            {
                "device_id": identity.device_id(),
                "name": machine_name(),
                "online": true,
                "revoked": false,
                "last_seen_at": now_str
            }
        ],
        "sessions": sessions,
        "device": {
            "device_id": identity.device_id(),
            "name": machine_name()
        },
        "local": {
            "pending_events": stats.pending_events,
            "session_count": local_sessions.len(),
            "sources": [
                local_source_summary(AgentType::ClaudeCode, &local_sessions, claude_health),
                local_source_summary(AgentType::Codex, &local_sessions, codex_health),
                local_source_summary(AgentType::Antigravity, &local_sessions, antigravity_health),
                local_source_summary(AgentType::GrokBuild, &local_sessions, grok_health),
                local_source_summary(AgentType::ZCode, &local_sessions, zcode_health)
            ],
            "refreshed_at": chrono::Utc::now()
        },
        "sync_running": false,
        "realtime_connected": false,
        "server_url": desktop_config.server_url,
        "preferences": preferences_json(&state.data_dir, &desktop_config, identity.device_id())
    }))
}

const MAX_ATTACHMENTS: usize = 8;
const MAX_ATTACHMENT_CHARS: usize = 128_000;
const MAX_ATTACHMENT_TOTAL_CHARS: usize = 400_000;

fn compose_turn_prompt(
    prompt: &str,
    attachments: &[SessionAttachmentInput],
) -> Result<String, DesktopError> {
    if attachments.is_empty() {
        return Ok(prompt.trim().to_owned());
    }
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(DesktopError::Attachment("一次最多附带 8 个文件".to_owned()));
    }
    let mut sections = vec![prompt.trim().to_owned()];
    let mut total = 0_usize;
    for attachment in attachments {
        let (name, content) = read_attachment_text(&attachment.path, &attachment.name)?;
        total += content.chars().count();
        if total > MAX_ATTACHMENT_TOTAL_CHARS {
            return Err(DesktopError::Attachment(
                "附件总内容超过 400K 字符上限".to_owned(),
            ));
        }
        sections.push(format!("--- 附件：{name} ---\n```text\n{content}\n```"));
    }
    sections.push("以上附件由用户在 AgentHub 中选择，请优先读取并使用它们。".to_owned());
    Ok(sections.join("\n\n"))
}

fn read_attachment_text(path: &str, name: &str) -> Result<(String, String), DesktopError> {
    let trimmed = name.trim();
    let display_name = if trimmed.is_empty() {
        Path::new(path)
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".to_owned())
    } else {
        trimmed.to_owned()
    };
    let file_path = PathBuf::from(path.trim());
    let metadata = fs::metadata(&file_path)
        .map_err(|_| DesktopError::Attachment(format!("{display_name}：文件不存在或不可读")))?;
    if !metadata.is_file() {
        return Err(DesktopError::Attachment(format!(
            "{display_name}：不是文件"
        )));
    }
    if metadata.len() > 8 * 1024 * 1024 {
        return Err(DesktopError::Attachment(format!(
            "{display_name}：文件超过 8MB 上限"
        )));
    }
    let bytes = fs::read(&file_path)
        .map_err(|_| DesktopError::Attachment(format!("{display_name}：读取失败")))?;
    if bytes.iter().take(8 * 1024).any(|byte| *byte == 0) {
        return Err(DesktopError::Attachment(format!(
            "{display_name}：暂不支持二进制文件，请粘贴文本或 Markdown"
        )));
    }
    let mut text = agent_pulse_runner::conversation::decode_local_text(&bytes);
    if text.chars().count() > MAX_ATTACHMENT_CHARS {
        text = format!(
            "{}\n\n[已截断：原文超过 {} 字符]",
            text.chars().take(MAX_ATTACHMENT_CHARS).collect::<String>(),
            MAX_ATTACHMENT_CHARS
        );
    }
    Ok((display_name, text))
}

fn set_session_archived_inner(
    input: ArchiveSessionInput,
    data_dir: &Path,
    daemon: Option<DaemonClient>,
) -> Result<Value, DesktopError> {
    validate_session_id(&input.agent_session_id)?;
    if input.project_display_name.chars().count() > 160
        || input.project_fingerprint.chars().count() > 160
        || chrono::DateTime::parse_from_rfc3339(&input.observed_at).is_err()
    {
        return Err(DesktopError::InvalidAction);
    }
    let mut config = read_config(data_dir)?;
    if input.agent_type == AgentType::Codex {
        let client = daemon.ok_or(DesktopError::AgentFailed)?;
        let binding = client
            .query_sessions()
            .map_err(|_| DesktopError::AgentFailed)?
            .into_iter()
            .find(|binding| binding.provider_session_id == input.agent_session_id)
            .ok_or(DesktopError::SessionUnavailable)?;
        let receipt = client
            .execute(agent_pulse_runner::remote_host::RemoteCommand {
                client_cmd_id: format!("cmd-{}", uuid::Uuid::new_v4()),
                hub_session_id: binding.hub_session_id,
                host_generation: Some(binding.host_generation),
                command: agent_pulse_runner::remote_host::RemoteAction::SetArchived {
                    archived: input.archived,
                },
            })
            .map_err(|_| DesktopError::AgentFailed)?;
        if receipt.status != "accepted" {
            return Err(DesktopError::AgentFailed);
        }
    }
    config.archived_sessions.retain(|session| {
        session.agent_type != input.agent_type || session.agent_session_id != input.agent_session_id
    });
    if input.archived {
        config.archived_sessions.push(ArchivedSession {
            agent_type: input.agent_type,
            agent_session_id: input.agent_session_id,
            project_display_name: input.project_display_name,
            project_fingerprint: input.project_fingerprint,
            observed_at: input.observed_at,
        });
    }
    write_config(data_dir, &config)?;
    Ok(json!({ "archived": input.archived }))
}

fn load_session_timeline_inner(
    input: SessionTimelineInput,
    data_dir: &Path,
) -> Result<Vec<Value>, DesktopError> {
    validate_identifier(&input.device_id)?;
    validate_session_id(&input.agent_session_id)?;
    let config = read_config(data_dir)?;
    let cookie = load_cookie(&config.server_url)?;
    let path = format!(
        "/v1/sessions/events?device_id={}&agent_type={}&agent_session_id={}&limit=100",
        input.device_id,
        agent_type_name(input.agent_type),
        input.agent_session_id
    );
    let response = request_json("GET", &config.server_url, &path, Some(&cookie), None)?;
    let events = response["events"]
        .as_array()
        .ok_or(DesktopError::InvalidResponse)?;
    Ok(events.iter().map(sanitize_timeline_event).collect())
}

fn load_session_conversation_inner(
    input: SessionConversationInput,
    client: DaemonClient,
) -> Result<Vec<Value>, DesktopError> {
    validate_session_id(&input.agent_session_id)?;
    client
        .session_conversation(input.agent_type, &input.agent_session_id)
        .map_err(daemon_client::classify_daemon_error)
}

fn sanitize_timeline_event(event: &Value) -> Value {
    json!({
        "event_id": event["event_id"],
        "device_id": event["device_id"],
        "agent_type": event["agent_type"],
        "agent_session_id": event["agent_session_id"],
        "sequence": event["sequence"],
        "observed_at": event["observed_at"],
        "received_at": event["received_at"],
        "payload": {
            "project": event["payload"]["project"],
            "state": {
                "presence": event["payload"]["state"]["presence"],
                "activity_status": event["payload"]["state"]["activity_status"]
            },
            "last_activity_at": event["payload"]["last_activity_at"]
        }
    })
}

fn preferences_json(data_dir: &Path, config: &DesktopConfig, device_id: &str) -> Value {
    let (_, profile_path) = daemon_client::host_database_path(data_dir, device_id);
    let codex_binary = fs::read(profile_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<daemon_client::DaemonProfile>(&bytes).ok())
        .map(|profile| profile.codex_binary)
        .unwrap_or_else(|| config.codex_binary.clone());
    json!({
        "refresh_interval_seconds": config.refresh_interval_seconds,
        "claude_binary": config.claude_binary,
        "codex_binary": codex_binary,
        "grok_binary": config.grok_binary,
    })
}

fn valid_binary(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.chars().count() <= 260 && !value.chars().any(char::is_control)
}

fn validate_session_id(value: &str) -> Result<(), DesktopError> {
    validate_identifier(value)
}

fn validate_identifier(value: &str) -> Result<(), DesktopError> {
    if value.is_empty()
        || value.chars().count() > MAX_SESSION_ID_CHARS
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(DesktopError::InvalidAction);
    }
    Ok(())
}

fn session_turn_key(agent_type: AgentType, session_id: &str) -> String {
    format!("{}:{session_id}", agent_type_name(agent_type))
}

fn start_engine(
    state: &DesktopState,
    server_url: &str,
    identity: &DeviceIdentity,
) -> Result<(), DesktopError> {
    let mut engine = state.engine.lock().map_err(|_| DesktopError::Sync)?;
    if engine.is_none() {
        *engine = Some(build_engine(&state.data_dir, server_url, identity)?);
    }
    Ok(())
}

fn build_engine(
    data_dir: &Path,
    server_url: &str,
    identity: &DeviceIdentity,
) -> Result<EngineHandle, DesktopError> {
    let database_path = data_dir.join("outbox.sqlite");
    let identity_path = data_dir.join("device-identity.json");
    let sync_identity = DeviceIdentity::load(&identity_path).map_err(|_| DesktopError::Identity)?;
    let stop = Arc::new(AtomicBool::new(false));
    let client = GatewayClient::new(server_url, REQUEST_TIMEOUT).map_err(|_| DesktopError::Sync)?;
    let sources: Vec<Box<dyn SessionSource>> = vec![
        Box::new(ClaudeSessionSource::new(true)),
        Box::new(CodexSessionSource::new_with_timeout(
            "codex",
            LOCAL_SESSION_LIMIT,
            DESKTOP_COLLECTOR_TIMEOUT,
        )),
        Box::new(AntigravitySessionSource::new(true)),
        Box::new(GrokSessionSource::new(true)),
        Box::new(ZCodeSessionSource::new(true)),
    ];
    let redactor =
        ProjectRedactor::new(identity.fingerprint_key()).map_err(|_| DesktopError::Sync)?;
    let mut outbox = Outbox::open(&database_path).map_err(|_| DesktopError::Sync)?;
    bootstrap_local_sessions(identity, &redactor, &mut outbox)?;
    let sync_worker = SyncWorker::start(
        database_path.clone(),
        client,
        sync_identity,
        SyncPolicy::default(),
        MAX_SYNC_BATCH_EVENTS.min(100),
        20,
        Arc::clone(&stop),
    )
    .map_err(|_| DesktopError::Sync)?;
    let mut runtime = RunnerRuntime::new(
        identity.device_id(),
        redactor,
        outbox,
        PollPolicy::default(),
        sources,
    )
    .with_sync_trigger(sync_worker.trigger_sender());
    let thread_stop = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("agent-pulse-desktop-engine".into())
        .spawn(move || {
            let _ = runtime.run(Arc::clone(&thread_stop), None);
            thread_stop.store(true, Ordering::Relaxed);
            let _ = sync_worker.join();
        })
        .map_err(|_| DesktopError::Sync)?;
    Ok(EngineHandle { stop, thread })
}

fn bootstrap_local_sessions(
    identity: &DeviceIdentity,
    redactor: &ProjectRedactor,
    outbox: &mut Outbox,
) -> Result<(), DesktopError> {
    let meta = ObservationMeta::now(identity.device_id());
    let next_attempt = meta.observed_at + chrono::Duration::seconds(10);

    match collect_claude_transcripts(true)
        .map_err(|error| error.code().to_owned())
        .and_then(|payload| {
            claude::normalize_agents_json(&payload, &meta, redactor)
                .map_err(|_| "claude_payload_invalid".to_owned())
        }) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::ClaudeCode,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(code) => {
            outbox
                .record_adapter_health(
                    AgentType::ClaudeCode,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(&code),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }

    match collect_codex_local_threads(LOCAL_SESSION_LIMIT)
        .map_err(|error| error.code().to_owned())
        .and_then(|value| {
            let payload =
                serde_json::to_string(&value).map_err(|_| "codex_payload_invalid".to_owned())?;
            codex::normalize_local_thread_list_result(
                &payload,
                &meta,
                redactor,
                &mut CodexThreadContext::default(),
            )
            .map_err(|_| "codex_payload_invalid".to_owned())
        }) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::Codex,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(code) => {
            outbox
                .record_adapter_health(
                    AgentType::Codex,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(&code),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }

    let mut antigravity_source = AntigravitySessionSource::new(true);
    match antigravity_source.collect(&meta, redactor) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::Antigravity,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(error) => {
            outbox
                .record_adapter_health(
                    AgentType::Antigravity,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(error.code()),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }

    match collect_grok_sessions(true)
        .map_err(|error| error.code().to_owned())
        .and_then(|payload| {
            grok::normalize_sessions_json(&payload, &meta, redactor)
                .map_err(|_| "grok_payload_invalid".to_owned())
        }) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::GrokBuild,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(code) => {
            outbox
                .record_adapter_health(
                    AgentType::GrokBuild,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(&code),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }

    match collect_zcode_sessions(true)
        .map_err(|error| error.code().to_owned())
        .and_then(|payload| {
            zcode::normalize_sessions_json(&payload, &meta, redactor)
                .map_err(|_| "zcode_payload_invalid".to_owned())
        }) {
        Ok(observations) => {
            for observation in observations {
                outbox
                    .enqueue_if_changed(&observation)
                    .map_err(|_| DesktopError::Sync)?;
            }
            outbox
                .record_adapter_health(
                    AgentType::ZCode,
                    meta.observed_at,
                    true,
                    0,
                    next_attempt,
                    None,
                )
                .map_err(|_| DesktopError::Sync)?;
        }
        Err(code) => {
            outbox
                .record_adapter_health(
                    AgentType::ZCode,
                    meta.observed_at,
                    false,
                    1,
                    next_attempt,
                    Some(&code),
                )
                .map_err(|_| DesktopError::Sync)?;
        }
    }
    Ok(())
}

fn merge_dashboard_sessions(
    remote: &Value,
    local: &[SessionObservation],
    archived: &[ArchivedSession],
    device_id: &str,
    contexts: &HashMap<String, LocalSessionContext>,
    host_bindings: &[agent_pulse_runner::remote_host::SessionBinding],
    pending_approvals: &[agent_pulse_runner::remote_host::PendingApprovalSnapshot],
) -> Vec<Value> {
    let mut sessions = std::collections::BTreeMap::new();
    if let Some(remote) = remote.as_array() {
        for session in remote {
            if let Some(key) = dashboard_session_key(session) {
                let mut session = session.clone();
                session["archived"] = Value::Bool(archived.iter().any(|item| {
                    session["device_id"].as_str() == Some(device_id)
                        && agent_type_name(item.agent_type)
                            == session["agent_type"].as_str().unwrap_or_default()
                        && item.agent_session_id
                            == session["agent_session_id"].as_str().unwrap_or_default()
                }));
                sessions.insert(key, session);
            }
        }
    }
    for observation in local {
        let key = observation.session_key();
        let context = contexts.get(&session_context_key(
            observation.agent_type,
            &observation.agent_session_id,
        ));
        let sync_state = if sessions.contains_key(&key) {
            "synced"
        } else {
            "local"
        };
        sessions.insert(
            key,
            json!({
                "event_id": observation.event_id,
                "device_id": observation.device_id,
                "agent_type": observation.agent_type,
                "agent_session_id": observation.agent_session_id,
                "observed_at": observation.observed_at,
                "payload": {
                    "project": observation.project,
                    "state": observation.state,
                    "source": observation.source,
                    "confidence": observation.confidence,
                    "started_at": observation.started_at,
                    "last_activity_at": observation.last_activity_at,
                    "runtime": context
                },
                "sync_state": sync_state
                ,"archived": archived.iter().any(|item| {
                    item.agent_type == observation.agent_type
                        && item.agent_session_id == observation.agent_session_id
                })
            }),
        );
    }
    // Overwrite or populate authoritative managed sessions (Codex / ZCode / Claude Code) from running NativeSessionHost registry
    for binding in host_bindings {
        let is_zcode = binding.provider_session_id.starts_with("sess_");
        let is_claude = binding.provider_session_id.starts_with("claude_");
        let agent_type_str = if is_zcode {
            "zcode"
        } else if is_claude {
            "claude_code"
        } else {
            "codex"
        };
        let agent_type = if is_zcode {
            AgentType::ZCode
        } else if is_claude {
            AgentType::ClaudeCode
        } else {
            AgentType::Codex
        };
        let key = format!(
            "{}:{}:{}",
            agent_type_str, device_id, binding.provider_session_id
        );
        let status = match binding.execution_state {
            agent_pulse_runner::remote_host::ExecutionState::Running => "WORKING",
            agent_pulse_runner::remote_host::ExecutionState::AwaitingPermission => {
                "WAITING_PERMISSION"
            }
            agent_pulse_runner::remote_host::ExecutionState::AwaitingInput => "WAITING_INPUT",
            agent_pulse_runner::remote_host::ExecutionState::Idle => "IDLE",
            agent_pulse_runner::remote_host::ExecutionState::Error => "FAILED",
            agent_pulse_runner::remote_host::ExecutionState::Completed => "COMPLETED",
            agent_pulse_runner::remote_host::ExecutionState::Terminated => "STOPPED",
            _ => "UNKNOWN",
        };
        let presence = if binding.control_connected {
            "LIVE"
        } else {
            "PERSISTED"
        };
        let pending_approval = pending_approvals.iter().find(|approval| {
            approval.hub_session_id == binding.hub_session_id
                && approval.host_generation == binding.host_generation
        });
        if let Some(existing) = sessions.get_mut(&key) {
            existing["payload"]["state"]["activity_status"] = json!(status);
            existing["payload"]["state"]["presence"] = json!(presence);
            existing["payload"]["pending_approval"] = json!(pending_approval);
        } else {
            let context = contexts.get(&session_context_key(
                agent_type,
                &binding.provider_session_id,
            ));
            let project_name = binding
                .workspace_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("workspace");
            let fp = format!(
                "{:x}",
                Sha256::digest(binding.workspace_path.to_string_lossy().as_bytes())
            );
            sessions.insert(
                key,
                json!({
                    "event_id": format!("host:{}", binding.hub_session_id),
                    "device_id": device_id,
                    "agent_type": agent_type_str,
                    "agent_session_id": binding.provider_session_id,
                    "observed_at": chrono::Utc::now().to_rfc3339(),
                    "payload": {
                        "project": {
                            "display_name": project_name,
                            "fingerprint": &fp[..16],
                        },
                        "state": {
                            "activity_status": status,
                            "presence": presence,
                        },
                        "source": "native_host",
                        "confidence": 1.0,
                        "started_at": chrono::Utc::now().to_rfc3339(),
                        "last_activity_at": chrono::Utc::now().to_rfc3339(),
                        "runtime": context,
                        "pending_approval": pending_approval,
                    },
                    "sync_state": "local",
                    "archived": false,
                }),
            );
        }
    }
    for item in archived {
        let key = format!(
            "{}:{}:{}",
            agent_type_name(item.agent_type),
            device_id,
            item.agent_session_id
        );
        sessions.entry(key).or_insert_with(|| {
            let context = contexts.get(&session_context_key(
                item.agent_type,
                &item.agent_session_id,
            ));
            json!({
                "event_id": format!("archived:{}:{}", agent_type_name(item.agent_type), item.agent_session_id),
                "device_id": device_id,
                "agent_type": item.agent_type,
                "agent_session_id": item.agent_session_id,
                "observed_at": item.observed_at,
                "payload": {
                    "project": {
                        "display_name": item.project_display_name,
                        "fingerprint": item.project_fingerprint,
                    },
                    "state": { "activity_status": "UNKNOWN", "presence": "PERSISTED" },
                    "last_activity_at": item.observed_at,
                    "runtime": context,
                },
                "sync_state": "synced",
                "archived": true,
            })
        });
    }
    sessions.into_values().collect()
}

/// Returns the cached session contexts, refreshing them if the TTL has passed.
///
/// The `Arc` is what keeps this cheap: `list_agent_models` runs on every
/// model-picker open and only reads `model` out of the map, so cloning up to
/// `LOCAL_SESSION_LIMIT` contexts with a dozen owned strings each per call was
/// pure overhead. Callers read through the handle; the refresh path is the only
/// place that builds a new map.
fn local_session_contexts(state: &DesktopState) -> Arc<HashMap<String, LocalSessionContext>> {
    let Ok(mut cache) = state.session_contexts.lock() else {
        return Arc::new(HashMap::new());
    };
    if cache
        .refreshed_at
        .is_some_and(|updated| updated.elapsed() < SESSION_CONTEXT_CACHE_TTL)
    {
        return Arc::clone(&cache.entries);
    }

    let claude = collect_claude_session_contexts(LOCAL_SESSION_LIMIT as usize);
    let codex = collect_codex_session_contexts(LOCAL_SESSION_LIMIT);
    let antigravity = collect_antigravity_session_contexts(LOCAL_SESSION_LIMIT as usize);
    let grok = collect_grok_session_contexts(LOCAL_SESSION_LIMIT as usize);
    let zcode = collect_zcode_session_contexts(LOCAL_SESSION_LIMIT);
    let mut next = HashMap::new();
    match claude {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::ClaudeCode)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    match codex {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::Codex)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    if let Some(client) = daemon_client::get_daemon_client(state)
        && let Ok(discovery) = client.discover()
        && let Some(candidates) = discovery.get("candidates").and_then(Value::as_array)
    {
        for item in candidates {
            if let Some(id) = item.get("provider_session_id").and_then(Value::as_str) {
                let is_zcode = id.starts_with("sess_");
                let is_claude = id.starts_with("claude_");
                let agent_type = if is_zcode {
                    AgentType::ZCode
                } else if is_claude {
                    AgentType::ClaudeCode
                } else {
                    AgentType::Codex
                };
                let key = session_context_key(agent_type, id);
                let title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                let entry = next
                    .entry(key)
                    .or_insert_with(|| LocalSessionContext::new(agent_type, id.to_owned()));
                if title.is_some() {
                    entry.title = title;
                }
            }
        }
    }
    match antigravity {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::Antigravity)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    match grok {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::GrokBuild)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    match zcode {
        Ok(contexts) => insert_session_contexts(&mut next, contexts),
        Err(_) => next.extend(
            cache
                .entries
                .iter()
                .filter(|(_, context)| context.agent_type == AgentType::ZCode)
                .map(|(key, context)| (key.clone(), context.clone())),
        ),
    }
    cache.entries = Arc::new(next);
    cache.refreshed_at = Some(Instant::now());
    Arc::clone(&cache.entries)
}

fn insert_session_contexts(
    target: &mut HashMap<String, LocalSessionContext>,
    contexts: Vec<LocalSessionContext>,
) {
    for context in contexts {
        target.insert(
            session_context_key(context.agent_type, &context.agent_session_id),
            context,
        );
    }
}

fn session_context_key(agent_type: AgentType, session_id: &str) -> String {
    format!("{}:{session_id}", agent_type_name(agent_type))
}

fn agent_type_name(agent_type: AgentType) -> &'static str {
    agent_type.as_str()
}

fn dashboard_session_key(session: &Value) -> Option<String> {
    Some(format!(
        "{}:{}:{}",
        session.get("agent_type")?.as_str()?,
        session.get("device_id")?.as_str()?,
        session.get("agent_session_id")?.as_str()?
    ))
}

fn local_source_summary(
    agent_type: AgentType,
    sessions: &[SessionObservation],
    health: Option<agent_pulse_runner::outbox::AdapterHealth>,
) -> Value {
    let session_count = sessions
        .iter()
        .filter(|session| session.agent_type == agent_type)
        .count();
    json!({
        "agent_type": agent_type,
        "session_count": session_count,
        "detected": session_count > 0 || health.as_ref().is_some_and(|value| value.status == "healthy"),
        "status": health.as_ref().map(|value| value.status.as_str()).unwrap_or("scanning"),
        "last_success_at": health.as_ref().and_then(|value| value.last_success_at),
        "error_code": health.and_then(|value| value.last_error_code)
    })
}

fn start_dashboard_realtime(state: &DesktopState, app: &AppHandle) -> Result<(), DesktopError> {
    let config = read_config(&state.data_dir)?;
    let cookie = load_cookie(&config.server_url)?;
    let mut realtime = state.realtime.lock().map_err(|_| DesktopError::Sync)?;
    if realtime.is_some() {
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let connected = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread_connected = connected.clone();
    let app = app.clone();
    thread::Builder::new()
        .name("agent-pulse-desktop-realtime".to_owned())
        .spawn(move || {
            dashboard_realtime_loop(
                &app,
                &config.server_url,
                &cookie,
                &thread_stop,
                &thread_connected,
            );
        })
        .map_err(|_| DesktopError::Sync)?;
    *realtime = Some(RealtimeHandle { stop, connected });
    Ok(())
}

fn stop_dashboard_realtime(state: &DesktopState) {
    if let Ok(mut realtime) = state.realtime.lock()
        && let Some(handle) = realtime.take()
    {
        handle.stop();
    }
}

fn dashboard_realtime_loop(
    app: &AppHandle,
    server_url: &str,
    cookie: &str,
    stop: &AtomicBool,
    connected: &AtomicBool,
) {
    let mut attempt = 0_u32;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        let result = read_dashboard_stream_with_fallback(app, server_url, cookie, stop, connected);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        publish_realtime_state(app, connected, false, false);

        let elapsed = started.elapsed();
        if elapsed >= Duration::from_secs(15) {
            // Once a connection maintained stability for >= 15s, reset backoff attempts
            attempt = 0;
        }

        let delay = match &result {
            _ if stop.load(Ordering::Relaxed) => break,
            Err(DesktopError::LoginRateLimited) => Duration::from_secs(60),
            _ if elapsed >= Duration::from_secs(15) => Duration::from_secs(1),
            Ok(_) => Duration::from_secs(1),
            Err(_) => {
                attempt = attempt.saturating_add(1);
                Duration::from_secs(2_u64.saturating_pow(attempt.min(4))).min(STREAM_MAX_BACKOFF)
            }
        };
        sleep_until_cancelled(stop, jittered(delay));
    }
    connected.store(false, Ordering::Relaxed);
}

fn jittered(duration: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.subsec_nanos())
        .unwrap_or(0);
    duration + Duration::from_millis((nanos % 400) as u64)
}

fn read_dashboard_stream_with_fallback(
    app: &AppHandle,
    server_url: &str,
    cookie: &str,
    stop: &AtomicBool,
    connected: &AtomicBool,
) -> Result<(), DesktopError> {
    let proxy_configs = [
        None,
        Some("http://127.0.0.1:7897"),
        Some("socks5://127.0.0.1:7897"),
    ];

    let mut last_err = DesktopError::Unreachable;
    for proxy in proxy_configs {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let agent = dashboard_stream_agent(proxy);
        match read_dashboard_stream_with_agent(&agent, app, server_url, cookie, stop, connected) {
            Ok(()) => return Ok(()),
            Err(err) => {
                let is_conn_err = matches!(
                    err,
                    DesktopError::Unreachable | DesktopError::InvalidResponse
                );
                last_err = err;
                if !is_conn_err {
                    // Auth/RateLimit error should not cycle proxies
                    return Err(last_err);
                }
            }
        }
    }
    Err(last_err)
}

fn read_dashboard_stream_with_agent(
    agent: &ureq::Agent,
    app: &AppHandle,
    server_url: &str,
    cookie: &str,
    stop: &AtomicBool,
    connected: &AtomicBool,
) -> Result<(), DesktopError> {
    let url = format!("{server_url}/v1/stream");
    let response = agent
        .get(&url)
        .header("cookie", cookie)
        .header("accept", "text/event-stream")
        .call()
        .map_err(map_http_error)?;
    if !response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
    {
        return Err(DesktopError::InvalidResponse);
    }

    // Immediately mark realtime connected once HTTP 200 SSE handshake succeeds
    publish_realtime_state(app, connected, true, false);

    let mut reader = BufReader::new(response.into_parts().1.into_reader());
    let mut event = String::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let line = read_sse_line(&mut reader);
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match line {
            SseLine::Line(line) if line.is_empty() => {
                match event.as_str() {
                    "ready" => publish_realtime_state(app, connected, true, false),
                    "sessions" => publish_realtime_state(app, connected, true, true),
                    "keepalive" | "ping" | "heartbeat" => {
                        publish_realtime_state(app, connected, true, false);
                    }
                    _ => {
                        publish_realtime_state(app, connected, true, false);
                    }
                }
                event.clear();
            }
            SseLine::Line(line) => {
                if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim_start().to_owned();
                } else if line.starts_with(':') {
                    // Keepalive comment (e.g. ": keepalive")
                    publish_realtime_state(app, connected, true, false);
                }
            }
            SseLine::Eof => return Ok(()),
            SseLine::TooLarge | SseLine::InvalidUtf8 | SseLine::Io => {
                return Err(DesktopError::InvalidResponse);
            }
        }
    }
}

fn publish_realtime_state(
    app: &AppHandle,
    connected: &AtomicBool,
    next_connected: bool,
    changed: bool,
) {
    let state_changed = connected.swap(next_connected, Ordering::Relaxed) != next_connected;
    if state_changed || changed {
        let _ = app.emit(
            "dashboard-realtime",
            DashboardRealtimeEvent {
                connected: next_connected,
                changed,
            },
        );
    }
}

fn sleep_until_cancelled(stop: &AtomicBool, duration: Duration) {
    let deadline = std::time::Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
}

enum SseLine {
    Line(String),
    TooLarge,
    InvalidUtf8,
    Io,
    Eof,
}

fn read_sse_line(reader: &mut impl BufRead) -> SseLine {
    let mut line = Vec::new();
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(_) => return SseLine::Io,
        };
        if buffer.is_empty() {
            if line.is_empty() {
                return SseLine::Eof;
            }
            break;
        }
        let (take, complete) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (buffer.len(), false),
        };
        if line.len() + take > MAX_SSE_LINE_BYTES {
            return SseLine::TooLarge;
        }
        line.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if complete {
            break;
        }
    }
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    String::from_utf8(line)
        .map(SseLine::Line)
        .unwrap_or(SseLine::InvalidUtf8)
}

fn execute_http_with_fallback<T, F>(operation: F) -> Result<T, DesktopError>
where
    F: Fn(&ureq::Agent) -> Result<T, ureq::Error>,
{
    let agents = [
        http_agent(None),
        http_agent(Some("http://127.0.0.1:7897")),
        http_agent(Some("socks5://127.0.0.1:7897")),
    ];

    let mut last_error = None;
    for agent in &agents {
        for attempt in 0..2 {
            match operation(agent) {
                Ok(val) => return Ok(val),
                Err(err) => {
                    let is_unreachable = matches!(
                        &err,
                        ureq::Error::Timeout(_)
                            | ureq::Error::HostNotFound
                            | ureq::Error::ConnectionFailed
                            | ureq::Error::Io(_)
                            | ureq::Error::Tls(_)
                            | ureq::Error::Rustls(_)
                    );
                    let mapped = map_http_error(err);
                    if !is_unreachable {
                        return Err(mapped);
                    }
                    last_error = Some(mapped);
                    if attempt == 0 {
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        }
    }
    Err(last_error.unwrap_or(DesktopError::Unreachable))
}

fn request_json(
    method: &str,
    server_url: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<&Value>,
) -> Result<Value, DesktopError> {
    let url = format!("{server_url}{path}");
    execute_http_with_fallback(|agent| {
        let mut response = match method {
            "GET" => {
                let mut request = agent.get(&url);
                if let Some(cookie) = cookie {
                    request = request.header("cookie", cookie);
                }
                request.call()
            }
            "POST" => {
                let mut request = agent.post(&url);
                if let Some(cookie) = cookie {
                    request = request.header("cookie", cookie);
                }
                request.send_json(body.unwrap_or(&Value::Null))
            }
            _ => return Err(ureq::Error::StatusCode(400)),
        }?;
        if response.status().as_u16() == 204 {
            return Ok(json!({}));
        }
        response
            .body_mut()
            .read_json::<Value>()
            .map_err(|e| ureq::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))
    })
}

fn http_agent(proxy_url: Option<&str>) -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(6)))
        .user_agent(concat!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AgentHub/",
            env!("CARGO_PKG_VERSION")
        ))
        .max_redirects(0);

    if let Some(proxy_str) = proxy_url
        && let Ok(proxy) = ureq::Proxy::new(proxy_str)
    {
        builder = builder.proxy(Some(proxy));
    }

    builder.build().new_agent()
}

fn dashboard_stream_agent(proxy_url: Option<&str>) -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder()
        .timeout_connect(Some(REQUEST_TIMEOUT))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(None) // Long-lived SSE stream body must NEVER have a timeout
        .user_agent(concat!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AgentHub/",
            env!("CARGO_PKG_VERSION")
        ))
        .max_redirects(0);

    if let Some(proxy_str) = proxy_url
        && let Ok(proxy) = ureq::Proxy::new(proxy_str)
    {
        builder = builder.proxy(Some(proxy));
    }

    builder.build().new_agent()
}

fn map_http_error(error: ureq::Error) -> DesktopError {
    match error {
        ureq::Error::StatusCode(401) => DesktopError::InvalidCredentials,
        ureq::Error::StatusCode(429) => DesktopError::LoginRateLimited,
        ureq::Error::StatusCode(403) => DesktopError::EmailVerificationRequired,
        ureq::Error::StatusCode(409) => DesktopError::DeviceConflict,
        ureq::Error::Timeout(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed
        | ureq::Error::Io(_)
        | ureq::Error::Tls(_)
        | ureq::Error::Rustls(_) => DesktopError::Unreachable,
        _ => DesktopError::InvalidResponse,
    }
}

fn normalize_server_url(value: &str) -> Result<String, DesktopError> {
    let url = Url::parse(value).map_err(|_| DesktopError::InvalidServer)?;
    let loopback = match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
    {
        return Err(DesktopError::InvalidServer);
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn extract_session_cookie(header: &str) -> Option<String> {
    let first = header.split(';').next()?.trim();
    let (name, token) = first.split_once('=')?;
    if !matches!(name, "agent_pulse_session" | "__Host-agent_pulse_session")
        || token.len() != 43
        || !token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return None;
    }
    Some(format!("{name}={token}"))
}

fn credential_entry(server_url: &str) -> Result<Entry, DesktopError> {
    let digest = Sha256::digest(server_url.as_bytes());
    Entry::new(KEYRING_SERVICE, &format!("server-{}", hex_prefix(&digest)))
        .map_err(|_| DesktopError::SecureStorage)
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn store_cookie(server_url: &str, cookie: &str) -> Result<(), DesktopError> {
    credential_entry(server_url)?
        .set_password(cookie)
        .map_err(|_| DesktopError::SecureStorage)
}

fn load_cookie(server_url: &str) -> Result<String, DesktopError> {
    credential_entry(server_url)?
        .get_password()
        .map_err(|_| DesktopError::NotAuthenticated)
}

fn delete_cookie(server_url: &str) -> Result<(), DesktopError> {
    credential_entry(server_url)?
        .delete_credential()
        .map_err(|_| DesktopError::SecureStorage)
}

fn load_or_create_identity(data_dir: &Path) -> Result<DeviceIdentity, DesktopError> {
    fs::create_dir_all(data_dir).map_err(|_| DesktopError::Identity)?;
    let path = data_dir.join("device-identity.json");
    if path.exists() {
        return DeviceIdentity::load(path).map_err(|_| DesktopError::Identity);
    }
    let identity = DeviceIdentity::generate();
    identity
        .save_new(path)
        .map_err(|_| DesktopError::Identity)?;
    Ok(identity)
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("desktop-config.json")
}

fn write_config(data_dir: &Path, config: &DesktopConfig) -> Result<(), DesktopError> {
    fs::create_dir_all(data_dir).map_err(|_| DesktopError::Identity)?;
    let bytes = serde_json::to_vec_pretty(config).map_err(|_| DesktopError::InvalidResponse)?;
    fs::write(config_path(data_dir), bytes).map_err(|_| DesktopError::Identity)
}

fn read_config(data_dir: &Path) -> Result<DesktopConfig, DesktopError> {
    let bytes = fs::read(config_path(data_dir)).map_err(|_| DesktopError::NotAuthenticated)?;
    serde_json::from_slice(&bytes).map_err(|_| DesktopError::NotAuthenticated)
}

fn machine_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "这台电脑".to_owned())
}

pub fn run_daemon_process(data_dir: PathBuf) -> Result<(), String> {
    if data_dir.as_os_str().is_empty() || !data_dir.is_absolute() {
        return Err("invalid_daemon_data_dir".into());
    }
    fs::create_dir_all(&data_dir).map_err(|_| "daemon_data_dir_unavailable".to_owned())?;
    let identity = load_or_create_identity(&data_dir).map_err(|error| error.to_string())?;
    let (database, profile_path) =
        daemon_client::host_database_path(&data_dir, identity.device_id());
    let config = read_config(&data_dir).ok();
    let mut profile = fs::read(profile_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<daemon_client::DaemonProfile>(&bytes).ok())
        .ok_or_else(|| "daemon_profile_unavailable".to_owned())?;
    profile.workspace_paths = daemon_client::normalize_workspaces(profile.workspace_paths);
    if profile.workspace_paths.is_empty()
        || profile.workspace_paths.len() > 32
        || !valid_binary(&profile.codex_binary)
        || profile.workspace_paths.iter().any(|path| !path.is_dir())
    {
        return Err("daemon_profile_invalid".into());
    }
    let server = if profile.remote_enabled {
        config
            .and_then(|config| {
                load_cookie(&config.server_url)
                    .ok()
                    .map(|_| config.server_url)
            })
            .and_then(|server_url| {
                let mut server = Url::parse(&server_url).ok()?;
                server
                    .set_scheme(if server.scheme() == "https" {
                        "wss"
                    } else {
                        "ws"
                    })
                    .ok()?;
                server.set_path("/v3/remote/ws");
                Some(server.to_string())
            })
    } else {
        None
    };
    agent_pulse_runner::daemon::run(agent_pulse_runner::daemon::DaemonConfig {
        codex_binary: PathBuf::from(profile.codex_binary),
        workspaces: profile.workspace_paths,
        database,
        identity,
        server,
        ipc_endpoint: None,
    })
    .map_err(|error| error.to_string())
}

pub mod commands;
pub use commands::*;

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            app.manage(DesktopState {
                data_dir,
                engine: Mutex::new(None),
                realtime: Mutex::new(None),
                active_turns: Mutex::new(HashMap::new()),
                session_contexts: Mutex::new(SessionContextCache::default()),
            });
            daemon_client::ensure_daemon_started(app.state::<DesktopState>().data_dir.clone());

            #[cfg(target_os = "windows")]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main")
                    && let Ok(hwnd) = window.hwnd()
                {
                    unsafe {
                        let use_dark_mode: i32 = 1;
                        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                            hwnd.0 as _,
                            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
                            (&raw const use_dark_mode).cast::<std::ffi::c_void>(),
                            std::mem::size_of::<i32>() as u32,
                        );
                        let caption_color: u32 = 0x0013110E; // #0E1113 in 0x00BBGGRR
                        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                            hwnd.0 as _,
                            35, // DWMWA_CAPTION_COLOR
                            (&raw const caption_color).cast::<std::ffi::c_void>(),
                            std::mem::size_of::<u32>() as u32,
                        );
                        let text_color: u32 = 0x00EAEAEA; // Light text
                        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                            hwnd.0 as _,
                            36, // DWMWA_TEXT_COLOR
                            (&raw const text_color).cast::<std::ffi::c_void>(),
                            std::mem::size_of::<u32>() as u32,
                        );
                    }
                }
            }

            {
                use tauri::Manager;
                let show_i = tauri::menu::MenuItem::with_id(
                    app,
                    "show",
                    "打开 AgentHub",
                    true,
                    None::<&str>,
                )?;
                let quit_i = tauri::menu::MenuItem::with_id(
                    app,
                    "quit",
                    "退出 AgentHub",
                    true,
                    None::<&str>,
                )?;
                let menu = tauri::menu::Menu::with_items(app, &[&show_i, &quit_i])?;

                let icon = app.default_window_icon().cloned().unwrap();
                let _tray = tauri::tray::TrayIconBuilder::new()
                    .icon(icon)
                    .tooltip("AgentHub - 原生 Coding Agent 远程控制台")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "quit" => {
                            daemon_client::terminate_daemon();
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            authenticate,
            open_external_url,
            start_tg_browser_oauth,
            start_tg_device_auth,
            poll_tg_device_auth,
            restore_session,
            refresh_dashboard,
            remote_host_status,
            start_remote_host,
            stop_remote_host,
            save_host_workspaces,
            send_session_message,
            launch_new_session,
            cancel_session_turn,
            resolve_session_approval,
            set_session_archived,
            save_desktop_preferences,
            set_sync_paused,
            load_session_timeline,
            load_session_conversation,
            list_agent_models,
            pick_folder,
            pick_files,
            desktop_window_minimize,
            desktop_window_toggle_maximize,
            desktop_window_close,
            logout
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build AgentHub desktop app")
        .run(|app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
            ) {
                daemon_client::terminate_daemon();
                if let Some(state) = app_handle.try_state::<DesktopState>() {
                    stop_dashboard_realtime(&state);
                    if let Ok(active_turns) = state.active_turns.lock() {
                        for cancelled in active_turns.values() {
                            cancelled.store(true, Ordering::Relaxed);
                        }
                    }
                    if let Ok(mut engine) = state.engine.lock()
                        && let Some(handle) = engine.take()
                    {
                        handle.stop();
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_label_formats_gpt_and_other_models_dynamically() {
        assert_eq!(model_label("gpt-6-astra"), "GPT-6-Astra");
        assert_eq!(model_label("gpt-5.6-terra"), "GPT-5.6-Terra");
        assert_eq!(model_label("custom-fast-agent"), "Custom Fast Agent");
        assert_eq!(model_label("claude-sonnet-4-5"), "Claude Sonnet 4.5");
    }

    #[test]
    fn read_codex_configured_model_extracts_from_toml_dynamically() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, "model = \"gpt-6-astra\"\nother = 123\n").unwrap();

        let old_codex_home = std::env::var_os("CODEX_HOME");
        unsafe {
            std::env::set_var("CODEX_HOME", temp.path());
        }
        let parsed = read_codex_configured_model();
        match old_codex_home {
            Some(v) => unsafe { std::env::set_var("CODEX_HOME", v) },
            None => unsafe { std::env::remove_var("CODEX_HOME") },
        }
        assert_eq!(parsed.as_deref(), Some("gpt-6-astra"));
    }

    fn codex_host_event(kind: &str, raw: Value) -> agent_pulse_runner::remote_host::HostEvent {
        agent_pulse_runner::remote_host::HostEvent {
            event_id: format!("event-{kind}"),
            host_generation: "generation-1".to_owned(),
            hub_session_id: Some("hub-1".to_owned()),
            provider_session_id: Some("thread-1".to_owned()),
            kind: kind.to_owned(),
            raw,
            binding: None,
            approval_id: None,
            approval: None,
            question_id: None,
            question: None,
        }
    }

    #[test]
    fn live_approval_event_preserves_daemon_capabilities() {
        let serialized = serde_json::to_value(SessionTurnEvent::ApprovalRequested {
            approval: agent_pulse_runner::remote_host::PendingApprovalSnapshot {
                approval_id: "approval-1".to_owned(),
                hub_session_id: "hub-1".to_owned(),
                host_generation: "generation-1".to_owned(),
                turn_id: "turn-1".to_owned(),
                can_approve: false,
                can_decline: true,
                tool_name: Some("Bash".into()),
                command_preview: Some("cargo test".into()),
                path: None,
                description: None,
                risk_level: Some("high".into()),
            },
        })
        .unwrap();

        assert_eq!(serialized["type"], "approval_requested");
        assert_eq!(serialized["approval"]["can_approve"], false);
        assert_eq!(serialized["approval"]["can_decline"], true);
    }

    #[test]
    fn codex_final_agent_message_replaces_partial_deltas() {
        let mut assistant = CodexAssistantAccumulator::default();
        let delta = codex_host_event(
            "item/agentMessage/delta",
            json!({"params": {"turnId": "turn-1", "delta": "partial"}}),
        );
        assert!(matches!(
            assistant.apply(&delta, "turn-1"),
            Some(SessionTurnEvent::AssistantDelta { text }) if text == "partial"
        ));

        let completed = codex_host_event(
            "item/completed",
            json!({"params": {
                "turnId": "turn-1",
                "item": {"type": "agentMessage", "text": "authoritative final"}
            }}),
        );
        assert!(matches!(
            assistant.apply(&completed, "turn-1"),
            Some(SessionTurnEvent::AssistantMessage { text }) if text == "authoritative final"
        ));
        assert_eq!(
            assistant.finish_with_fallback(Some("turn fallback")),
            "authoritative final"
        );
    }

    #[test]
    fn codex_final_agent_message_supplies_response_without_deltas() {
        let mut assistant = CodexAssistantAccumulator::default();
        let completed = codex_host_event(
            "item/completed",
            json!({"params": {
                "turn": {"id": "turn-1"},
                "item": {"type": "agentMessage", "text": "complete response"}
            }}),
        );

        assert!(assistant.apply(&completed, "turn-1").is_some());
        assert_eq!(assistant.finish_with_fallback(None), "complete response");
    }

    #[test]
    fn codex_assistant_accumulator_ignores_stale_turn_final_items() {
        let mut assistant = CodexAssistantAccumulator::default();
        let completed = codex_host_event(
            "item/completed",
            json!({"params": {
                "turnId": "turn-old",
                "item": {"type": "agentMessage", "text": "stale response"}
            }}),
        );

        assert!(assistant.apply(&completed, "turn-current").is_none());
        assert_eq!(assistant.finish_with_fallback(None), "");
    }

    #[test]
    fn codex_assistant_accumulator_ignores_non_message_completed_items() {
        let mut assistant = CodexAssistantAccumulator::default();
        let delta = codex_host_event(
            "item/agentMessage/delta",
            json!({"params": {"turnId": "turn-1", "delta": "answer"}}),
        );
        let command = codex_host_event(
            "item/completed",
            json!({"params": {
                "turnId": "turn-1",
                "item": {"type": "commandExecution", "text": "secret output"}
            }}),
        );

        assert!(assistant.apply(&delta, "turn-1").is_some());
        assert!(assistant.apply(&command, "turn-1").is_none());
        assert_eq!(assistant.finish_with_fallback(None), "answer");
    }

    #[test]
    fn extracts_only_the_session_cookie_pair() {
        let token = "A".repeat(43);
        let header = format!("__Host-agent_pulse_session={token}; Path=/; Secure; HttpOnly");
        assert_eq!(
            extract_session_cookie(&header),
            Some(format!("__Host-agent_pulse_session={token}"))
        );
        assert!(extract_session_cookie("other=secret; Path=/").is_none());
    }
    #[test]
    fn rejects_insecure_remote_servers() {
        assert!(normalize_server_url("https://platform.tannerlab.xyz/").is_ok());
        assert!(normalize_server_url("http://localhost:3000").is_ok());
        assert!(normalize_server_url("http://example.com").is_err());
    }

    #[test]
    fn sse_line_reader_handles_crlf_and_blank_frames() {
        let mut input = std::io::Cursor::new(b"event: ready\r\n\r\n".to_vec());
        assert!(matches!(
            read_sse_line(&mut input),
            SseLine::Line(line) if line == "event: ready"
        ));
        assert!(matches!(
            read_sse_line(&mut input),
            SseLine::Line(line) if line.is_empty()
        ));
    }

    #[test]
    fn sse_line_reader_rejects_unbounded_lines() {
        let mut input = std::io::Cursor::new(vec![b'x'; MAX_SSE_LINE_BYTES + 1]);
        assert!(matches!(read_sse_line(&mut input), SseLine::TooLarge));
    }

    #[test]
    fn session_ids_reject_paths_and_control_characters() {
        assert!(validate_session_id("019abc-session_1").is_ok());
        assert!(validate_session_id("../auth.json").is_err());
        assert!(validate_session_id("bad\nvalue").is_err());
        assert!(validate_identifier("device-remote_1").is_ok());
        assert!(validate_identifier("device/remote").is_err());
    }

    #[test]
    fn timeline_sanitizer_keeps_status_metadata_only() {
        let event = json!({
            "event_id": "event-1",
            "device_id": "device-1",
            "agent_type": "codex",
            "agent_session_id": "session-1",
            "sequence": 7,
            "observed_at": "2026-08-15T05:00:00Z",
            "received_at": "2026-08-15T05:00:01Z",
            "payload": {
                "project": {"display_name": "demo", "fingerprint": "fp"},
                "state": {
                    "presence": "LIVE",
                    "activity_status": "WAITING_INPUT",
                    "raw_status": "secret raw status",
                    "raw_status_detail": {"prompt": "do not expose"}
                },
                "last_activity_at": "2026-08-15T05:00:00Z",
                "prompt": "never return this"
            }
        });
        let sanitized = sanitize_timeline_event(&event);
        assert_eq!(
            sanitized["payload"]["state"]["activity_status"],
            "WAITING_INPUT"
        );
        assert!(sanitized["payload"]["state"].get("raw_status").is_none());
        assert!(sanitized["payload"].get("prompt").is_none());
    }

    #[test]
    fn dashboard_attaches_runtime_context_only_to_local_sessions() {
        let redactor = ProjectRedactor::new(b"desktop-test-key").unwrap();
        let observations = claude::normalize_agents_json(
            r#"[{"sessionId":"session-1","cwd":"/work/project","state":"done"}]"#,
            &ObservationMeta::now("device-local"),
            &redactor,
        )
        .unwrap();
        let context = LocalSessionContext {
            agent_type: AgentType::ClaudeCode,
            agent_session_id: "session-1".to_owned(),
            title: Some("Explicit local title".to_owned()),
            model: Some("claude-opus-4-7".to_owned()),
            reasoning_effort: None,
            permission_mode: Some("acceptEdits".to_owned()),
            approval_policy: None,
            sandbox_mode: None,
            context_window_tokens: Some(200_000),
            context_used_tokens: Some(40_000),
            session_total_tokens: None,
            input_tokens: Some(40_000),
            output_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
        };
        let contexts = HashMap::from([("claude_code:session-1".to_owned(), context)]);
        let remote = json!([{
            "agent_type": "codex",
            "device_id": "device-remote",
            "agent_session_id": "session-1",
            "payload": {"project": {"display_name": "remote", "fingerprint": "fp"}}
        }]);

        let merged = merge_dashboard_sessions(
            &remote,
            &observations,
            &[],
            "device-local",
            &contexts,
            &[],
            &[],
        );
        let local = merged
            .iter()
            .find(|session| session["device_id"] == "device-local")
            .unwrap();
        let remote = merged
            .iter()
            .find(|session| session["device_id"] == "device-remote")
            .unwrap();

        assert_eq!(local["payload"]["runtime"]["title"], "Explicit local title");
        assert!(remote["payload"].get("runtime").is_none());
    }

    #[test]
    fn daemon_profile_is_the_authoritative_codex_binary_for_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let config = DesktopConfig {
            server_url: DEFAULT_SERVER_URL.to_owned(),
            refresh_interval_seconds: 4,
            claude_binary: "claude-custom".to_owned(),
            codex_binary: "legacy-codex".to_owned(),
            grok_binary: "grok-custom".to_owned(),
            archived_sessions: Vec::new(),
        };
        let (_, profile_path) = daemon_client::host_database_path(dir.path(), "device-1");
        fs::write(
            profile_path,
            serde_json::to_vec(&daemon_client::DaemonProfile {
                workspace_paths: vec![dir.path().to_path_buf()],
                codex_binary: "profile-codex".to_owned(),
                remote_enabled: false,
            })
            .unwrap(),
        )
        .unwrap();

        let preferences = preferences_json(dir.path(), &config, "device-1");

        assert_eq!(preferences["codex_binary"], "profile-codex");
        assert_eq!(preferences["claude_binary"], "claude-custom");
        assert_eq!(preferences["grok_binary"], "grok-custom");
    }

    #[test]
    fn host_session_registry_overrides_dashboard_state_and_populates_missing_session() {
        let contexts = HashMap::new();
        let binding = agent_pulse_runner::remote_host::SessionBinding {
            hub_session_id: "hub-1".to_owned(),
            provider_session_id: "session-active".to_owned(),
            workspace_path: PathBuf::from("D:/test-ws"),
            host_generation: "gen-1".to_owned(),
            control_connected: true,
            execution_state: agent_pulse_runner::remote_host::ExecutionState::Completed,
            current_turn_id: None,
        };
        let approval = agent_pulse_runner::remote_host::PendingApprovalSnapshot {
            approval_id: "approval-1".to_owned(),
            hub_session_id: "hub-1".to_owned(),
            host_generation: "gen-1".to_owned(),
            turn_id: "turn-1".to_owned(),
            can_approve: true,
            can_decline: false,
            tool_name: Some("Bash".into()),
            command_preview: Some("cargo test".into()),
            path: None,
            description: None,
            risk_level: Some("high".into()),
        };
        let merged = merge_dashboard_sessions(
            &json!([]),
            &[],
            &[],
            "device-local",
            &contexts,
            &[binding],
            &[approval],
        );
        let found = merged
            .iter()
            .find(|s| s["agent_session_id"] == "session-active")
            .unwrap();
        assert_eq!(found["payload"]["state"]["activity_status"], "COMPLETED");
        assert_eq!(found["payload"]["state"]["presence"], "LIVE");
        assert_eq!(found["payload"]["source"], "native_host");
        assert_eq!(
            found["payload"]["pending_approval"]["approval_id"],
            "approval-1"
        );
        assert_eq!(found["payload"]["pending_approval"]["can_approve"], true);
        assert_eq!(found["payload"]["pending_approval"]["can_decline"], false);
    }

    #[test]
    fn desktop_session_control_uses_only_the_daemon_transport() {
        let source = include_str!("lib.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        let forbidden = [
            ["run_with_local", "_control"].concat(),
            ["create_local_host", "_channel"].concat(),
            ["send_session", "_turn"].concat(),
        ];
        for symbol in forbidden {
            assert!(!source.contains(&symbol), "obsolete transport: {symbol}");
        }
        for command in [
            "send_session_message,",
            "launch_new_session,",
            "cancel_session_turn,",
            "resolve_session_approval,",
        ] {
            assert!(source.contains(command), "missing Tauri command: {command}");
        }
    }

    #[test]
    fn offline_cold_start_snapshot_generates_valid_local_dashboard() {
        let dir = tempfile::tempdir().unwrap();
        let identity = DeviceIdentity::generate();
        let state = DesktopState {
            data_dir: dir.path().to_path_buf(),
            engine: Mutex::new(None),
            realtime: Mutex::new(None),
            active_turns: Mutex::new(HashMap::new()),
            session_contexts: Mutex::new(SessionContextCache::default()),
        };

        let snapshot = offline_dashboard_snapshot(&identity, &state).unwrap();
        assert_eq!(snapshot["account"]["user"]["user_id"], "offline");
        assert_eq!(snapshot["account"]["user"]["email"], "离线本地模式");
        assert_eq!(snapshot["account"]["subscription"]["plan"], "offline");
        assert_eq!(snapshot["account"]["offline"], true);
        assert_eq!(snapshot["sync_running"], false);
        assert_eq!(snapshot["realtime_connected"], false);
        assert_eq!(snapshot["device"]["device_id"], identity.device_id());
        assert!(!snapshot["devices"].as_array().unwrap().is_empty());
        assert!(snapshot["devices"][0].get("last_seen_at").is_some());
        assert!(snapshot["sessions"].is_array());
    }
}
