#![windows_subsystem = "windows"]

use std::{
    fs,
    io::{self, Read},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use agenthub_runner::{
    adapters::{claude, codex},
    auth_worker::AuthWorker,
    device::DeviceIdentity,
    domain::ObservationMeta,
    outbox::Outbox,
    runtime::{
        AntigravitySessionSource, ClaudeSessionSource, CodexSessionSource, PollPolicy,
        RunnerRuntime, SessionSource,
    },
    sync::MAX_SYNC_BATCH_EVENTS,
    sync_worker::{SyncPolicy, SyncWorker},
};
use anyhow::{Context, Result, bail};
use clap::Parser;

mod runner_cli;
use runner_cli::*;

fn main() -> Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        // If launched from an existing terminal/command prompt, attach to parent console so CLI output works.
        // If double-clicked in Explorer, AttachConsole returns false, and with windows_subsystem = "windows"
        // no console window is created at all.
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }

    let cli = Cli::parse();
    if let Some(Command::Ui {
        url,
        idle_seconds,
        title,
    }) = cli.command
    {
        return agenthub_runner::tray::run_ui_window(&url, idle_seconds, &title);
    }
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
    // The device name is read from the file beside the identity, which is
    // also where the rename endpoint writes it. The source is kept rather
    // than the resolved string so a rename takes effect on the running
    // process instead of only after a restart.
    let device_name_path = identity_path
        .clone()
        .unwrap_or_else(agenthub_runner::device_name::default_identity_path);
    let device_name = agenthub_runner::device_name::DeviceNameSource::new(
        cli.device_name.clone(),
        Some(device_name_path.clone()),
    );

    let command = cli.command.unwrap_or(Command::Web {
        codex_binary: PathBuf::from("codex"),
        workspace: vec![PathBuf::from(".")],
        db: None,
        port: 4200,
        open_browser: true,
        hide_console: true,
    });

    match command {
        Command::Daemon {
            codex_binary,
            workspace,
            db,
            ipc_endpoint,
            web,
            web_port,
            open_browser,
        } => {
            let identity = runtime_identity
                .ok_or_else(|| anyhow::anyhow!("daemon_requires_device_identity"))?;
            let db = db.unwrap_or_else(resolve_default_db_path);
            let server = resolve_bound_gateway_server();
            agenthub_runner::daemon::run(agenthub_runner::daemon::DaemonConfig {
                codex_binary,
                workspaces: workspace,
                database: db,
                identity,
                server,
                ipc_endpoint,
                enable_web: web,
                web_port: Some(web_port),
                open_browser,
                device_name: device_name.clone(),
                identity_path: device_name_path.clone(),
            })?;
        }
        Command::Web {
            codex_binary,
            workspace,
            db,
            port,
            open_browser,
            hide_console,
        } => {
            if hide_console {
                hide_windows_console();
            }
            let db = db.unwrap_or_else(resolve_default_db_path);
            let identity = runtime_identity.unwrap_or_else(resolve_or_create_default_identity);
            let server = resolve_bound_gateway_server();
            agenthub_runner::daemon::run(agenthub_runner::daemon::DaemonConfig {
                codex_binary,
                workspaces: workspace,
                database: db,
                identity,
                server,
                ipc_endpoint: None,
                enable_web: true,
                web_port: Some(port),
                open_browser,
                device_name: device_name.clone(),
                identity_path: device_name_path.clone(),
            })?;
        }
        Command::RemoteRun {
            codex_binary,
            workspace,
            db,
            server,
            ipc_endpoint,
            web,
            web_port,
            open_browser,
        } => {
            let identity = runtime_identity
                .ok_or_else(|| anyhow::anyhow!("remote_requires_bound_device_identity"))?;
            agenthub_runner::daemon::run(agenthub_runner::daemon::DaemonConfig {
                codex_binary,
                workspaces: workspace,
                database: db,
                identity,
                server: Some(server),
                ipc_endpoint,
                enable_web: web,
                web_port: Some(web_port),
                open_browser,
                device_name: device_name.clone(),
                identity_path: device_name_path.clone(),
            })?;
        }
        Command::RemoteHost {
            codex_binary,
            workspace,
        } => {
            agenthub_runner::remote_ipc::run(codex_binary, workspace)?;
        }
        Command::Ui { .. } => unreachable!("handled early before loading identity"),
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
                Agent::Antigravity => agenthub_runner::adapters::antigravity::normalize_hook(
                    &input, &meta, &redactor,
                )?,
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
            let observations = collect_once_observations(
                agent,
                binary,
                include_completed,
                limit,
                timeout,
                &meta,
                &redactor,
            )?;
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
        Command::Outbox { command } => handle_outbox(command)?,
        Command::Device { command } => handle_device(command, identity_path.as_ref())?,
        Command::Sync { command } => handle_sync(command, &device_id, runtime_identity.as_ref())?,
        Command::Auth {
            claude_binary,
            codex_binary,
            command,
        } => handle_auth(
            runtime_identity.as_ref(),
            auth_cache,
            allow_credential_access,
            claude_binary,
            codex_binary,
            command,
        )?,
        Command::Plugin { command } => handle_plugin(command)?,
    }

    Ok(())
}
