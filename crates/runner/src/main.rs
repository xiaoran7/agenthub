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

use agent_platform_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
    },
    collectors::{CodexAppServer, collect_claude_agents},
    domain::{AgentType, ObservationMeta, SessionObservation},
    outbox::Outbox,
    privacy::ProjectRedactor,
    runtime::{ClaudeSessionSource, CodexSessionSource, PollPolicy, RunnerRuntime, SessionSource},
};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "agent-platform-runner")]
#[command(about = "Local metadata collector for Coding Agent sessions")]
struct Cli {
    #[arg(long, env = "AGENT_PLATFORM_DEVICE_ID", default_value = "local-dev")]
    device_id: String,

    #[arg(
        long,
        default_value = "AGENT_PLATFORM_FINGERPRINT_KEY",
        help = "Environment variable containing the project fingerprint key"
    )]
    fingerprint_key_env: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
        #[arg(long, help = "Override the claude or codex executable path")]
        binary: Option<PathBuf>,
        #[arg(long, help = "Include completed Claude sessions")]
        include_completed: bool,
        #[arg(long, default_value_t = 100, help = "Maximum Codex threads to request")]
        limit: u32,
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
        #[arg(long, help = "Include completed Claude sessions")]
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
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InputFormat {
    ClaudeAgents,
    ClaudeHook,
    CodexMessage,
    CodexThreadList,
    CodexHook,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Agent {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AgentSelection {
    All,
    Claude,
    Codex,
}

impl std::fmt::Display for AgentSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::All => "all",
            Self::Claude => "claude",
            Self::Codex => "codex",
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let device_id = cli.device_id;
    let fingerprint_key_env = cli.fingerprint_key_env;

    match cli.command {
        Command::Normalize { format, input } => {
            let redactor = load_redactor(&fingerprint_key_env)?;
            let meta = ObservationMeta::now(&device_id);
            let input = fs::read_to_string(&input)
                .with_context(|| format!("failed to read {}", input.display()))?;
            let observations = normalize(format, &input, &meta, &redactor)?;
            println!("{}", serde_json::to_string_pretty(&observations)?);
        }
        Command::IngestHook { agent, db } => {
            let redactor = load_redactor(&fingerprint_key_env)?;
            let meta = ObservationMeta::now(&device_id);
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            let observation = match agent {
                Agent::Claude => claude::normalize_hook(&input, &meta, &redactor)?,
                Agent::Codex => codex::normalize_hook(&input, &meta, &redactor)?,
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
        } => {
            let redactor = load_redactor(&fingerprint_key_env)?;
            let meta = ObservationMeta::now(&device_id);
            let observations = match agent {
                Agent::Claude => {
                    let binary = binary.unwrap_or_else(|| PathBuf::from("claude"));
                    let payload = collect_claude_agents(binary, include_completed)?;
                    claude::normalize_agents_json(&payload, &meta, &redactor)?
                }
                Agent::Codex => {
                    let binary = binary.unwrap_or_else(|| PathBuf::from("codex"));
                    let result = CodexAppServer::start(binary)?.list_threads(limit)?;
                    let payload = serde_json::to_string(&result)?;
                    let mut context = CodexThreadContext::default();
                    codex::normalize_thread_list_result(&payload, &meta, &redactor, &mut context)?
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
            include_completed,
            codex_limit,
            active_interval_seconds,
            idle_interval_seconds,
            failure_initial_seconds,
            failure_max_seconds,
            max_attempts,
        } => {
            let redactor = load_redactor(&fingerprint_key_env)?;
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

            let mut sources: Vec<Box<dyn SessionSource>> = Vec::new();
            if matches!(agent, AgentSelection::All | AgentSelection::Claude) {
                sources.push(Box::new(ClaudeSessionSource::new(
                    claude_binary.unwrap_or_else(|| PathBuf::from("claude")),
                    include_completed,
                )));
            }
            if matches!(agent, AgentSelection::All | AgentSelection::Codex) {
                sources.push(Box::new(CodexSessionSource::new(
                    codex_binary.unwrap_or_else(|| PathBuf::from("codex")),
                    codex_limit,
                )));
            }

            let stop = Arc::new(AtomicBool::new(false));
            let stop_on_signal = Arc::clone(&stop);
            ctrlc::set_handler(move || {
                stop_on_signal.store(true, Ordering::Relaxed);
            })
            .context("failed to install Ctrl-C handler")?;
            let mut runtime =
                RunnerRuntime::new(device_id, redactor, Outbox::open(db)?, policy, sources);
            runtime.run(stop, max_attempts)?;
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
                for agent_type in [AgentType::ClaudeCode, AgentType::Codex] {
                    if let Some(adapter_health) = outbox.adapter_health(agent_type)? {
                        health.push(adapter_health);
                    }
                }
                println!("{}", serde_json::to_string_pretty(&health)?);
            }
        },
    }

    Ok(())
}

fn nonzero_duration(name: &str, seconds: u64) -> Result<Duration> {
    if seconds == 0 {
        bail!("{name} must be greater than zero");
    }
    Ok(Duration::from_secs(seconds))
}

fn load_redactor(environment_variable: &str) -> Result<ProjectRedactor> {
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
    }
    .and_then(|observations| {
        if observations.is_empty() {
            bail!("input produced no session observations");
        }
        Ok(observations)
    })
}
