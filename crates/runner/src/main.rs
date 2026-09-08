use std::{
    env, fs,
    io::{self, Read},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use agent_pulse_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
        grok,
    },
    auth::{AuthManager, AuthProvider},
    auth_worker::AuthWorker,
    collectors::{
        CodexAppServer, collect_antigravity_sessions, collect_claude_transcripts,
        collect_codex_local_threads, collect_grok_sessions,
    },
    device::DeviceIdentity,
    domain::{AgentType, ObservationMeta, SessionObservation},
    gateway::{GatewayClient, push_outbox},
    outbox::Outbox,
    privacy::ProjectRedactor,
    runtime::{
        AntigravitySessionSource, ClaudeSessionSource, CodexSessionSource, GrokSessionSource,
        PollPolicy, RunnerRuntime, SessionSource,
    },
    sync::{MAX_SYNC_BATCH_EVENTS, SyncAck, SyncBatch, apply_sync_ack, prepare_sync_batch},
    sync_worker::{SyncPolicy, SyncWorker},
};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "agenthub-runner")]
#[command(about = "AgentHub native session host and metadata collector")]
struct Cli {
    #[arg(long, env = "AGENT_PLATFORM_DEVICE_ID", default_value = "local-dev")]
    device_id: String,

    #[arg(
        long,
        env = "AGENT_PLATFORM_IDENTITY",
        help = "Use a persisted device identity for the device ID and project fingerprint key"
    )]
    identity: Option<PathBuf>,

    #[arg(
        long,
        default_value = "AGENT_PLATFORM_FINGERPRINT_KEY",
        help = "Environment variable containing the project fingerprint key"
    )]
    fingerprint_key_env: String,

    #[arg(
        long,
        env = "AGENT_PLATFORM_ALLOW_CREDENTIAL_ACCESS",
        help = "Allow the Runner to read provider auth files, keyrings and credential environment variables"
    )]
    allow_credential_access: bool,

    #[arg(
        long,
        env = "AGENT_PLATFORM_AUTH_CACHE",
        help = "Encrypted local credential cache path"
    )]
    auth_cache: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Connect a durable native Host to the authenticated v3 Gateway.
    RemoteRun {
        #[arg(long)]
        codex_binary: PathBuf,
        #[arg(long, required = true)]
        workspace: Vec<PathBuf>,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        server: String,
    },
    /// Local JSONL control channel for a persistent Codex app-server Host.
    /// This is not yet the authenticated Gateway transport.
    RemoteHost {
        #[arg(long)]
        codex_binary: PathBuf,
        #[arg(
            long,
            required = true,
            help = "Authorize an exact working directory; repeat for multiple projects"
        )]
        workspace: Vec<PathBuf>,
    },
    Normalize {
        #[arg(value_enum)]
        format: InputFormat,
        #[arg(long)]
        input: PathBuf,
    },
    IngestHook {
        #[arg(value_enum)]
        agent: Agent,
        #[arg(long)]
        db: PathBuf,
    },
    CollectOnce {
        #[arg(value_enum)]
        agent: Agent,
        #[arg(long)]
        db: PathBuf,
        #[arg(long, help = "Override the Codex or Grok executable path")]
        binary: Option<PathBuf>,
        #[arg(long, help = "Scan up to 1000 Claude history sessions instead of 100")]
        include_completed: bool,
        #[arg(long, default_value_t = 100, help = "Maximum Codex threads to request")]
        limit: u32,
        #[arg(long, default_value_t = 15)]
        timeout_seconds: u64,
    },
    Run {
        #[arg(value_enum, default_value_t = AgentSelection::All)]
        agent: AgentSelection,
        #[arg(long)]
        db: PathBuf,
        #[arg(long, help = "Override the Claude executable path")]
        claude_binary: Option<PathBuf>,
        #[arg(long, help = "Override the Codex executable path")]
        codex_binary: Option<PathBuf>,
        #[arg(long, help = "Override the Grok Build executable path")]
        grok_binary: Option<PathBuf>,
        #[arg(long, help = "Scan up to 1000 Claude history sessions instead of 100")]
        include_completed: bool,
        #[arg(long, default_value_t = 100)]
        codex_limit: u32,
        #[arg(long, default_value_t = 2)]
        active_interval_seconds: u64,
        #[arg(long, default_value_t = 10)]
        idle_interval_seconds: u64,
        #[arg(long, default_value_t = 2)]
        failure_initial_seconds: u64,
        #[arg(long, default_value_t = 60)]
        failure_max_seconds: u64,
        #[arg(long, default_value_t = 15)]
        collector_timeout_seconds: u64,
        #[arg(
            long,
            env = "AGENT_PLATFORM_GATEWAY_URL",
            help = "Continuously drain the Outbox to this Gateway; remote URLs require HTTPS"
        )]
        sync_server: Option<String>,
        #[arg(
            long,
            env = "AGENT_PLATFORM_GATEWAY_CA_CERT",
            value_name = "PEM",
            help = "Trust this private CA certificate for Gateway HTTPS"
        )]
        gateway_ca_cert: Option<PathBuf>,
        #[arg(long, default_value_t = 5)]
        sync_interval_seconds: u64,
        #[arg(long, default_value_t = 15)]
        heartbeat_interval_seconds: u64,
        #[arg(long, default_value_t = 60)]
        auth_interval_seconds: u64,
        #[arg(long, default_value_t = 2)]
        sync_failure_initial_seconds: u64,
        #[arg(long, default_value_t = 60)]
        sync_failure_max_seconds: u64,
        #[arg(long, default_value_t = 100)]
        sync_limit: usize,
        #[arg(long, default_value_t = 10)]
        sync_max_batches: usize,
        #[arg(long, default_value_t = 15)]
        sync_timeout_seconds: u64,
        #[arg(
            long,
            help = "Stop after this many adapter attempts; intended for diagnostics and tests"
        )]
        max_attempts: Option<u64>,
    },
    Outbox {
        #[command(subcommand)]
        command: OutboxCommand,
    },
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
    },
    Auth {
        #[arg(
            long,
            default_value = "claude",
            help = "Override the Claude executable path"
        )]
        claude_binary: PathBuf,
        #[arg(
            long,
            default_value = "codex",
            help = "Override the Codex executable path"
        )]
        codex_binary: PathBuf,
        #[command(subcommand)]
        command: AuthCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AuthCommand {
    Status {
        #[arg(value_enum, default_value_t = AgentSelection::All)]
        agent: AgentSelection,
    },
    Refresh {
        #[arg(value_enum, default_value_t = AgentSelection::All)]
        agent: AgentSelection,
    },
    Login {
        #[arg(value_enum)]
        agent: Agent,
    },
    Logout {
        #[arg(value_enum)]
        agent: Agent,
    },
    Quota {
        #[arg(value_enum, default_value_t = AgentSelection::All)]
        agent: AgentSelection,
    },
    ResetCodex {
        #[arg(long)]
        credit_id: Option<String>,
    },
    Token {
        #[arg(value_enum)]
        agent: Agent,
        #[arg(
            long,
            help = "Required acknowledgement before printing raw token material"
        )]
        reveal: bool,
    },
    ClearCache {
        #[arg(value_enum, default_value_t = AgentSelection::All)]
        agent: AgentSelection,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InputFormat {
    ClaudeAgents,
    ClaudeHook,
    CodexMessage,
    CodexThreadList,
    CodexHook,
    AntigravityBrain,
    AntigravityHook,
    GrokSessions,
    GrokHook,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Agent {
    Claude,
    Codex,
    Antigravity,
    Grok,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AgentSelection {
    All,
    Claude,
    Codex,
    Antigravity,
    Grok,
}

impl std::fmt::Display for AgentSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::All => "all",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Grok => "grok",
        })
    }
}

#[derive(Debug, Subcommand)]
enum OutboxCommand {
    Stats {
        #[arg(long)]
        db: PathBuf,
    },
    Pending {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        device_id: String,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Ack {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        device_id: String,
        #[arg(long)]
        through: i64,
    },
    Health {
        #[arg(long)]
        db: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum DeviceCommand {
    Init {
        #[arg(long)]
        identity: PathBuf,
    },
    Show {
        #[arg(long)]
        identity: PathBuf,
    },
    Bind {
        #[arg(long)]
        identity: PathBuf,
        #[arg(long, env = "AGENT_PLATFORM_GATEWAY_URL")]
        server: String,
        #[arg(
            long,
            env = "AGENT_PLATFORM_BINDING_CODE",
            help = "One-time binding code; prefer AGENT_PLATFORM_BINDING_CODE to avoid shell history"
        )]
        binding_code: String,
        #[arg(long, default_value_t = 15)]
        timeout_seconds: u64,
        #[arg(
            long,
            env = "AGENT_PLATFORM_GATEWAY_CA_CERT",
            value_name = "PEM",
            help = "Trust this private CA certificate for Gateway HTTPS"
        )]
        gateway_ca_cert: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum SyncCommand {
    Prepare {
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        db: PathBuf,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Verify {
        #[arg(long)]
        batch: PathBuf,
    },
    Ack {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        batch: PathBuf,
        #[arg(long)]
        acknowledgement: PathBuf,
    },
    Push {
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        db: PathBuf,
        #[arg(long, env = "AGENT_PLATFORM_GATEWAY_URL")]
        server: String,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long, default_value_t = 1)]
        max_batches: usize,
        #[arg(long, default_value_t = 15)]
        timeout_seconds: u64,
        #[arg(
            long,
            env = "AGENT_PLATFORM_GATEWAY_CA_CERT",
            value_name = "PEM",
            help = "Trust this private CA certificate for Gateway HTTPS"
        )]
        gateway_ca_cert: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let identity_path = cli.identity.clone();
    let runtime_identity = cli
        .identity
        .as_ref()
        .map(|path| {
            DeviceIdentity::load(path).with_context(|| format!("failed to load {}", path.display()))
        })
        .transpose()?;
    let device_id = runtime_identity
        .as_ref()
        .map(|identity| identity.device_id().to_owned())
        .unwrap_or(cli.device_id);
    let fingerprint_key_env = cli.fingerprint_key_env;
    let allow_credential_access = cli.allow_credential_access;
    let auth_cache = cli.auth_cache;

    match cli.command {
        Command::RemoteRun {
            codex_binary,
            workspace,
            db,
            server,
        } => {
            let identity = runtime_identity
                .ok_or_else(|| anyhow::anyhow!("remote_requires_bound_device_identity"))?;
            agent_pulse_runner::remote_network::run(codex_binary, workspace, db, identity, server)?;
        }
        Command::RemoteHost {
            codex_binary,
            workspace,
        } => {
            agent_pulse_runner::remote_ipc::run(codex_binary, workspace)?;
        }
        Command::Normalize { format, input } => {
            let redactor = load_redactor(&fingerprint_key_env, runtime_identity.as_ref())?;
            let meta = ObservationMeta::now(&device_id);
            let input = fs::read_to_string(&input)
                .with_context(|| format!("failed to read {}", input.display()))?;
            let observations = normalize(format, &input, &meta, &redactor)?;
            println!("{}", serde_json::to_string_pretty(&observations)?);
        }
        Command::IngestHook { agent, db } => {
            let redactor = load_redactor(&fingerprint_key_env, runtime_identity.as_ref())?;
            let meta = ObservationMeta::now(&device_id);
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            let observation = match agent {
                Agent::Claude => claude::normalize_hook(&input, &meta, &redactor)?,
                Agent::Codex => codex::normalize_hook(&input, &meta, &redactor)?,
                Agent::Antigravity => agent_pulse_runner::adapters::antigravity::normalize_hook(
                    &input, &meta, &redactor,
                )?,
                Agent::Grok => grok::normalize_hook(&input, &meta, &redactor)?,
            };
            let mut outbox = Outbox::open(db)?;
            let sequence = outbox.enqueue(&observation)?;
            println!(
                "{}",
                serde_json::json!({
                    "accepted": true,
                    "sequence": sequence,
                    "event_id": observation.event_id
                })
            );
        }
        Command::CollectOnce {
            agent,
            db,
            binary,
            include_completed,
            limit,
            timeout_seconds,
        } => {
            let redactor = load_redactor(&fingerprint_key_env, runtime_identity.as_ref())?;
            let meta = ObservationMeta::now(&device_id);
            let timeout = nonzero_duration("collector timeout", timeout_seconds)?;
            let observations = match agent {
                Agent::Claude => {
                    let _ = binary;
                    let payload = collect_claude_transcripts(include_completed)?;
                    claude::normalize_agents_json(&payload, &meta, &redactor)?
                }
                Agent::Codex => {
                    let binary = binary.unwrap_or_else(|| PathBuf::from("codex"));
                    let (result, local) = match CodexAppServer::start_with_timeout(binary, timeout)
                        .and_then(|mut server| server.list_threads(limit))
                    {
                        Ok(result) => (result, false),
                        Err(_) => (collect_codex_local_threads(limit)?, true),
                    };
                    let payload = serde_json::to_string(&result)?;
                    let mut context = CodexThreadContext::default();
                    if local {
                        codex::normalize_local_thread_list_result(
                            &payload,
                            &meta,
                            &redactor,
                            &mut context,
                        )?
                    } else {
                        codex::normalize_thread_list_result(
                            &payload,
                            &meta,
                            &redactor,
                            &mut context,
                        )?
                    }
                }
                Agent::Antigravity => {
                    let _ = binary;
                    let payload = collect_antigravity_sessions(include_completed)?;
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
                            .and_then(chrono::DateTime::from_timestamp_millis);
                        let last_activity_at = session
                            .get("lastActivityAt")
                            .and_then(|v| v.as_i64())
                            .and_then(chrono::DateTime::from_timestamp_millis);
                        let steps_val = session
                            .get("steps")
                            .cloned()
                            .unwrap_or(serde_json::json!([]));
                        let steps: Vec<agent_pulse_runner::adapters::antigravity::AntigravityStep> =
                            serde_json::from_value(steps_val).unwrap_or_default();
                        let session_meta =
                            agent_pulse_runner::adapters::antigravity::AntigravitySessionMetadata {
                                session_id,
                                cwd,
                                is_process_alive: true,
                                started_at,
                                last_activity_at,
                            };
                        if let Some(obs) =
                            agent_pulse_runner::adapters::antigravity::normalize_transcript_steps(
                                &steps,
                                &session_meta,
                                &meta,
                                &redactor,
                            )
                        {
                            observations.push(obs);
                        }
                    }
                    observations
                }
                Agent::Grok => {
                    let _ = binary;
                    let payload = collect_grok_sessions(include_completed)?;
                    grok::normalize_sessions_json(&payload, &meta, &redactor)?
                }
            };
            let mut outbox = Outbox::open(db)?;
            let sequences = enqueue_all(&mut outbox, &observations)?;
            println!(
                "{}",
                serde_json::json!({
                    "accepted": observations.len(),
                    "first_sequence": sequences.first(),
                    "last_sequence": sequences.last()
                })
            );
        }
        Command::Run {
            agent,
            db,
            claude_binary,
            codex_binary,
            grok_binary,
            include_completed,
            codex_limit,
            active_interval_seconds,
            idle_interval_seconds,
            failure_initial_seconds,
            failure_max_seconds,
            collector_timeout_seconds,
            sync_server,
            gateway_ca_cert,
            sync_interval_seconds,
            heartbeat_interval_seconds,
            auth_interval_seconds,
            sync_failure_initial_seconds,
            sync_failure_max_seconds,
            sync_limit,
            sync_max_batches,
            sync_timeout_seconds,
            max_attempts,
        } => {
            let redactor = load_redactor(&fingerprint_key_env, runtime_identity.as_ref())?;
            let policy = PollPolicy {
                active_interval: nonzero_duration("active interval", active_interval_seconds)?,
                idle_interval: nonzero_duration("idle interval", idle_interval_seconds)?,
                failure_initial: nonzero_duration(
                    "initial failure backoff",
                    failure_initial_seconds,
                )?,
                failure_max: nonzero_duration("maximum failure backoff", failure_max_seconds)?,
            };
            if policy.failure_initial > policy.failure_max {
                bail!("initial failure backoff cannot exceed maximum failure backoff");
            }
            if max_attempts == Some(0) {
                bail!("max attempts must be greater than zero");
            }
            let collector_timeout =
                nonzero_duration("collector timeout", collector_timeout_seconds)?;
            let sync_policy = SyncPolicy {
                interval: nonzero_duration("sync interval", sync_interval_seconds)?,
                heartbeat_interval: nonzero_duration(
                    "heartbeat interval",
                    heartbeat_interval_seconds,
                )?,
                failure_initial: nonzero_duration(
                    "initial sync failure backoff",
                    sync_failure_initial_seconds,
                )?,
                failure_max: nonzero_duration(
                    "maximum sync failure backoff",
                    sync_failure_max_seconds,
                )?,
            };
            if sync_policy.heartbeat_interval > Duration::from_secs(30) {
                bail!("heartbeat interval cannot exceed 30 seconds");
            }
            if sync_policy.failure_initial > sync_policy.failure_max {
                bail!("initial sync failure backoff cannot exceed maximum sync failure backoff");
            }
            if !(1..=MAX_SYNC_BATCH_EVENTS).contains(&sync_limit) {
                bail!("sync limit must be between 1 and {MAX_SYNC_BATCH_EVENTS}");
            }
            if sync_max_batches == 0 {
                bail!("sync max batches must be greater than zero");
            }
            let sync_timeout = nonzero_duration("sync timeout", sync_timeout_seconds)?;
            let auth_interval = nonzero_duration("auth interval", auth_interval_seconds)?;

            let claude_binary = claude_binary.unwrap_or_else(|| PathBuf::from("claude"));
            let codex_binary = codex_binary.unwrap_or_else(|| PathBuf::from("codex"));
            let _grok_binary = grok_binary.unwrap_or_else(|| PathBuf::from("grok"));

            let mut sources: Vec<Box<dyn SessionSource>> = Vec::new();
            if matches!(agent, AgentSelection::All | AgentSelection::Claude) {
                sources.push(Box::new(ClaudeSessionSource::new(include_completed)));
            }
            if matches!(agent, AgentSelection::All | AgentSelection::Codex) {
                sources.push(Box::new(CodexSessionSource::new_with_timeout(
                    codex_binary.clone(),
                    codex_limit,
                    collector_timeout,
                )));
            }
            if matches!(agent, AgentSelection::All | AgentSelection::Antigravity) {
                sources.push(Box::new(AntigravitySessionSource::new(include_completed)));
            }
            if matches!(agent, AgentSelection::All | AgentSelection::Grok) {
                sources.push(Box::new(GrokSessionSource::new(include_completed)));
            }

            let stop = Arc::new(AtomicBool::new(false));
            let stop_on_signal = Arc::clone(&stop);
            ctrlc::set_handler(move || {
                stop_on_signal.store(true, Ordering::Relaxed);
            })
            .context("failed to install Ctrl-C handler")?;
            let mut runtime =
                RunnerRuntime::new(device_id, redactor, Outbox::open(&db)?, policy, sources);
            let auth_worker = if allow_credential_access {
                if let Some(server) = sync_server.as_ref() {
                    let auth_identity_path = identity_path.clone().ok_or_else(|| {
                        anyhow::anyhow!(
                            "background auth sync requires --identity or AGENT_PLATFORM_IDENTITY"
                        )
                    })?;
                    let auth_identity =
                        DeviceIdentity::load(&auth_identity_path).with_context(|| {
                            format!("failed to load {}", auth_identity_path.display())
                        })?;
                    let auth_client =
                        gateway_client(server, sync_timeout, gateway_ca_cert.as_deref())?;
                    Some(
                        AuthWorker::start(
                            auth_client,
                            auth_identity,
                            auth_cache.clone(),
                            claude_binary.clone(),
                            codex_binary.clone(),
                            auth_interval,
                            sync_timeout,
                            Arc::clone(&stop),
                        )
                        .map_err(|_| anyhow::anyhow!("auth_thread_start_failed"))?,
                    )
                } else {
                    None
                }
            } else {
                None
            };
            let sync_worker = if let Some(server) = sync_server {
                let device_identity = runtime_identity.ok_or_else(|| {
                    anyhow::anyhow!(
                        "background sync requires --identity or AGENT_PLATFORM_IDENTITY"
                    )
                })?;
                let client = gateway_client(&server, sync_timeout, gateway_ca_cert.as_deref())?;
                let worker = SyncWorker::start(
                    db,
                    client,
                    device_identity,
                    sync_policy,
                    sync_limit,
                    sync_max_batches,
                    Arc::clone(&stop),
                )
                .map_err(|_| anyhow::anyhow!("sync_thread_start_failed"))?;
                runtime = runtime.with_sync_trigger(worker.trigger_sender());
                Some(worker)
            } else {
                None
            };
            let run_result = runtime.run(Arc::clone(&stop), max_attempts);
            stop.store(true, Ordering::Relaxed);
            let sync_result = sync_worker.map(SyncWorker::join).transpose();
            let auth_result = auth_worker.map(AuthWorker::join).transpose();
            run_result?;
            sync_result.map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
            auth_result.map_err(|error| anyhow::anyhow!("auth_thread_failed: {error}"))?;
        }
        Command::Outbox { command } => match command {
            OutboxCommand::Stats { db } => {
                let outbox = Outbox::open(db)?;
                println!("{}", serde_json::to_string_pretty(&outbox.stats()?)?);
            }
            OutboxCommand::Pending {
                db,
                device_id,
                limit,
            } => {
                let outbox = Outbox::open(db)?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&outbox.pending(&device_id, limit)?)?
                );
            }
            OutboxCommand::Ack {
                db,
                device_id,
                through,
            } => {
                let mut outbox = Outbox::open(db)?;
                let removed = outbox.acknowledge_through(&device_id, through)?;
                println!("{}", serde_json::json!({ "acknowledged": removed }));
            }
            OutboxCommand::Health { db } => {
                let outbox = Outbox::open(db)?;
                let mut health = Vec::new();
                for agent_type in [
                    AgentType::ClaudeCode,
                    AgentType::Codex,
                    AgentType::GrokBuild,
                ] {
                    if let Some(adapter_health) = outbox.adapter_health(agent_type)? {
                        health.push(adapter_health);
                    }
                }
                println!("{}", serde_json::to_string_pretty(&health)?);
            }
        },
        Command::Device { command } => match command {
            DeviceCommand::Init { identity } => {
                let device_identity = DeviceIdentity::generate();
                device_identity
                    .save_new(&identity)
                    .with_context(|| format!("failed to create {}", identity.display()))?;
                println!("{}", serde_json::to_string_pretty(&device_identity.info())?);
            }
            DeviceCommand::Show { identity } => {
                let device_identity = DeviceIdentity::load(&identity)
                    .with_context(|| format!("failed to load {}", identity.display()))?;
                println!("{}", serde_json::to_string_pretty(&device_identity.info())?);
            }
            DeviceCommand::Bind {
                identity,
                server,
                binding_code,
                timeout_seconds,
                gateway_ca_cert,
            } => {
                let device_identity = DeviceIdentity::load(&identity)
                    .with_context(|| format!("failed to load {}", identity.display()))?;
                let client = gateway_client(
                    &server,
                    nonzero_duration("gateway timeout", timeout_seconds)?,
                    gateway_ca_cert.as_deref(),
                )?;
                let binding = client
                    .bind(&device_identity, &binding_code)
                    .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
                println!("{}", serde_json::to_string_pretty(&binding)?);
            }
        },
        Command::Sync { command } => match command {
            SyncCommand::Prepare {
                identity,
                db,
                limit,
            } => {
                let device_identity = DeviceIdentity::load(&identity)
                    .with_context(|| format!("failed to load {}", identity.display()))?;
                let outbox = Outbox::open(db)?;
                if let Some(batch) = prepare_sync_batch(&outbox, &device_identity, limit)? {
                    println!("{}", serde_json::to_string_pretty(&batch)?);
                } else {
                    println!("{}", serde_json::json!({ "pending": 0 }));
                }
            }
            SyncCommand::Verify { batch } => {
                let batch: SyncBatch = read_json_file(&batch)?;
                batch.verify()?;
                println!(
                    "{}",
                    serde_json::json!({
                        "valid": true,
                        "device_id": batch.device_id,
                        "batch_id": batch.batch_id,
                        "first_sequence": batch.first_sequence,
                        "last_sequence": batch.last_sequence
                    })
                );
            }
            SyncCommand::Ack {
                db,
                batch,
                acknowledgement,
            } => {
                let batch: SyncBatch = read_json_file(&batch)?;
                let acknowledgement: SyncAck = read_json_file(&acknowledgement)?;
                let mut outbox = Outbox::open(db)?;
                let removed = apply_sync_ack(&mut outbox, &batch, &acknowledgement)?;
                println!("{}", serde_json::json!({ "acknowledged": removed }));
            }
            SyncCommand::Push {
                identity,
                db,
                server,
                limit,
                max_batches,
                timeout_seconds,
                gateway_ca_cert,
            } => {
                let device_identity = DeviceIdentity::load(&identity)
                    .with_context(|| format!("failed to load {}", identity.display()))?;
                let client = gateway_client(
                    &server,
                    nonzero_duration("gateway timeout", timeout_seconds)?,
                    gateway_ca_cert.as_deref(),
                )?;
                let mut outbox = Outbox::open(db)?;
                let summary =
                    push_outbox(&client, &mut outbox, &device_identity, limit, max_batches)
                        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
                if summary.batches_sent == 0 && summary.pending_events > 0 {
                    eprintln!(
                        "warning: {} pending events belong to a different device ID; \
                         they were likely collected without --identity",
                        summary.pending_events
                    );
                }
                println!("{}", serde_json::to_string_pretty(&summary)?);
            }
        },
        Command::Auth {
            claude_binary,
            codex_binary,
            command,
        } => {
            let mut manager = AuthManager::from_identity(
                runtime_identity.as_ref(),
                auth_cache,
                allow_credential_access,
                claude_binary,
                codex_binary,
                Duration::from_secs(15),
            )
            .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
            match command {
                AuthCommand::Status { agent } => {
                    print_auth_snapshots(&mut manager, agent, false)?;
                }
                AuthCommand::Refresh { agent } => {
                    print_auth_snapshots(&mut manager, agent, true)?;
                }
                AuthCommand::Login { agent } => {
                    let result = manager
                        .login(auth_provider(agent)?)
                        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                AuthCommand::Logout { agent } => {
                    let result = manager
                        .logout(auth_provider(agent)?)
                        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                AuthCommand::Quota { agent } => {
                    print_auth_snapshots(&mut manager, agent, false)?;
                }
                AuthCommand::ResetCodex { credit_id } => {
                    let result = manager
                        .consume_codex_reset(credit_id.as_deref())
                        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                AuthCommand::Token { agent, reveal } => {
                    if !reveal {
                        bail!("raw token output requires --reveal and --allow-credential-access");
                    }
                    let result = manager
                        .tokens(auth_provider(agent)?)
                        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                AuthCommand::ClearCache { agent } => {
                    for provider in auth_providers(agent)? {
                        manager
                            .clear_cache(provider)
                            .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
                    }
                    println!("{}", serde_json::json!({ "cleared": true }));
                }
            }
        }
    }

    Ok(())
}

fn auth_provider(agent: Agent) -> Result<AuthProvider> {
    match agent {
        Agent::Claude => Ok(AuthProvider::ClaudeCode),
        Agent::Codex => Ok(AuthProvider::Codex),
        Agent::Antigravity => Ok(AuthProvider::Antigravity),
        Agent::Grok => Ok(AuthProvider::GrokBuild),
    }
}

fn auth_providers(selection: AgentSelection) -> Result<Vec<AuthProvider>> {
    match selection {
        AgentSelection::All => Ok(vec![
            AuthProvider::ClaudeCode,
            AuthProvider::Codex,
            AuthProvider::Antigravity,
            AuthProvider::GrokBuild,
        ]),
        AgentSelection::Claude => Ok(vec![AuthProvider::ClaudeCode]),
        AgentSelection::Codex => Ok(vec![AuthProvider::Codex]),
        AgentSelection::Antigravity => Ok(vec![AuthProvider::Antigravity]),
        AgentSelection::Grok => Ok(vec![AuthProvider::GrokBuild]),
    }
}

fn print_auth_snapshots(
    manager: &mut AuthManager,
    selection: AgentSelection,
    refresh: bool,
) -> Result<()> {
    let mut snapshots = Vec::new();
    for provider in auth_providers(selection)? {
        let snapshot = if refresh {
            manager.refresh(provider)
        } else {
            manager.status(provider)
        }
        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
        snapshots.push(snapshot);
    }
    println!("{}", serde_json::to_string_pretty(&snapshots)?);
    Ok(())
}

fn gateway_client(
    server: &str,
    timeout: Duration,
    ca_certificate_path: Option<&std::path::Path>,
) -> Result<GatewayClient> {
    let ca_certificate = ca_certificate_path
        .map(fs::read)
        .transpose()
        .map_err(|_| anyhow::anyhow!("gateway_ca_cert_unreadable"))?;
    GatewayClient::new_with_ca_certificate(server, timeout, ca_certificate.as_deref())
        .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))
}

fn read_json_file<T: serde::de::DeserializeOwned>(path: &PathBuf) -> Result<T> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn nonzero_duration(name: &str, seconds: u64) -> Result<Duration> {
    if seconds == 0 {
        bail!("{name} must be greater than zero");
    }
    Ok(Duration::from_secs(seconds))
}

fn load_redactor(
    environment_variable: &str,
    identity: Option<&DeviceIdentity>,
) -> Result<ProjectRedactor> {
    if let Some(identity) = identity {
        return Ok(ProjectRedactor::new(identity.fingerprint_key())?);
    }
    let key = env::var(environment_variable).with_context(|| {
        format!("missing fingerprint key environment variable {environment_variable}")
    })?;
    Ok(ProjectRedactor::new(key.as_bytes())?)
}

fn enqueue_all(outbox: &mut Outbox, observations: &[SessionObservation]) -> Result<Vec<i64>> {
    observations
        .iter()
        .map(|observation| outbox.enqueue(observation).map_err(Into::into))
        .collect()
}

fn normalize(
    format: InputFormat,
    input: &str,
    meta: &ObservationMeta,
    redactor: &ProjectRedactor,
) -> Result<Vec<SessionObservation>> {
    match format {
        InputFormat::ClaudeAgents => Ok(claude::normalize_agents_json(input, meta, redactor)?),
        InputFormat::ClaudeHook => Ok(vec![claude::normalize_hook(input, meta, redactor)?]),
        InputFormat::CodexMessage => {
            let mut context = CodexThreadContext::default();
            Ok(
                codex::normalize_app_server_message(input, meta, redactor, &mut context)?
                    .into_iter()
                    .collect(),
            )
        }
        InputFormat::CodexThreadList => {
            let mut context = CodexThreadContext::default();
            Ok(codex::normalize_thread_list_result(
                input,
                meta,
                redactor,
                &mut context,
            )?)
        }
        InputFormat::CodexHook => Ok(vec![codex::normalize_hook(input, meta, redactor)?]),
        InputFormat::AntigravityBrain => {
            let steps = agent_pulse_runner::adapters::antigravity::parse_transcript_lines(input)?;
            let meta_session =
                agent_pulse_runner::adapters::antigravity::AntigravitySessionMetadata {
                    session_id: "preview-session".into(),
                    cwd: ".".into(),
                    is_process_alive: true,
                    started_at: None,
                    last_activity_at: None,
                };
            Ok(
                agent_pulse_runner::adapters::antigravity::normalize_transcript_steps(
                    &steps,
                    &meta_session,
                    meta,
                    redactor,
                )
                .into_iter()
                .collect(),
            )
        }
        InputFormat::AntigravityHook => Ok(vec![
            agent_pulse_runner::adapters::antigravity::normalize_hook(input, meta, redactor)?,
        ]),
        InputFormat::GrokSessions => Ok(grok::normalize_sessions_json(input, meta, redactor)?),
        InputFormat::GrokHook => Ok(vec![grok::normalize_hook(input, meta, redactor)?]),
    }
    .and_then(|observations| {
        if observations.is_empty() {
            bail!("input produced no session observations");
        }
        Ok(observations)
    })
}
