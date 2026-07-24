use std::{
    env, fs,
    io::{self, Read},
    path::PathBuf,
};

use agent_platform_runner::{
    adapters::{
        claude,
        codex::{self, CodexThreadContext},
    },
    collectors::{CodexAppServer, collect_claude_agents},
    domain::{ObservationMeta, SessionObservation},
    outbox::Outbox,
    privacy::ProjectRedactor,
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
        },
    }

    Ok(())
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
