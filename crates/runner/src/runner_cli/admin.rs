use std::{path::PathBuf, time::Duration};

use agenthub_runner::{
    auth::{AuthManager, AuthProvider},
    device::DeviceIdentity,
    domain::AgentType,
    gateway::push_outbox,
    outbox::Outbox,
    sync::{SyncAck, SyncBatch, apply_sync_ack, prepare_sync_batch},
};
use anyhow::{Context, Result, bail};
use serde_json::json;

use super::{
    args::{
        Agent, AgentSelection, AuthCommand, DeviceCommand, OutboxCommand, PluginCommand,
        SyncCommand,
    },
    helpers::{gateway_client, nonzero_duration, read_json_file},
};

pub fn handle_outbox(command: OutboxCommand) -> Result<()> {
    match command {
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
                AgentType::Antigravity,
            ] {
                if let Some(adapter_health) = outbox.adapter_health(agent_type)? {
                    health.push(adapter_health);
                }
            }
            println!("{}", serde_json::to_string_pretty(&health)?);
        }
    }
    Ok(())
}

pub fn handle_device(
    command: DeviceCommand,
    _default_identity_path: Option<&PathBuf>,
) -> Result<()> {
    match command {
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
        DeviceCommand::Name { identity, name } => {
            let name = name.trim();
            agenthub_runner::device_name::write_configured_name(&identity, name).with_context(
                || {
                    format!(
                        "failed to write the device name beside {}",
                        identity.display()
                    )
                },
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "device_name": agenthub_runner::device_name::resolve(
                        if name.is_empty() { None } else { Some(name) },
                        Some(&identity),
                    ),
                }))?
            );
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
            let dev_name = agenthub_runner::device_name::resolve(None, Some(&identity));
            let binding = client
                .bind(&device_identity, &binding_code, Some(&dev_name))
                .map_err(|error| anyhow::anyhow!("{}: {error}", error.code()))?;
            println!("{}", serde_json::to_string_pretty(&binding)?);
        }
    }
    Ok(())
}

pub fn handle_sync(
    command: SyncCommand,
    _device_id: &str,
    _runtime_identity: Option<&DeviceIdentity>,
) -> Result<()> {
    match command {
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
            let summary = push_outbox(&client, &mut outbox, &device_identity, limit, max_batches)
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
    }
    Ok(())
}

pub fn handle_auth(
    runtime_identity: Option<&DeviceIdentity>,
    auth_cache: Option<PathBuf>,
    allow_credential_access: bool,
    claude_binary: PathBuf,
    codex_binary: PathBuf,
    command: AuthCommand,
) -> Result<()> {
    let mut manager = AuthManager::from_identity(
        runtime_identity,
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
    Ok(())
}

pub fn auth_provider(agent: Agent) -> Result<AuthProvider> {
    match agent {
        Agent::Claude => Ok(AuthProvider::ClaudeCode),
        Agent::Codex => Ok(AuthProvider::Codex),
        Agent::Antigravity => Ok(AuthProvider::Antigravity),
    }
}

pub fn auth_providers(selection: AgentSelection) -> Result<Vec<AuthProvider>> {
    match selection {
        AgentSelection::All => Ok(vec![
            AuthProvider::ClaudeCode,
            AuthProvider::Codex,
            AuthProvider::Antigravity,
        ]),
        AgentSelection::Claude => Ok(vec![AuthProvider::ClaudeCode]),
        AgentSelection::Codex => Ok(vec![AuthProvider::Codex]),
        AgentSelection::Antigravity => Ok(vec![AuthProvider::Antigravity]),
    }
}

pub fn handle_plugin(command: PluginCommand) -> Result<()> {
    match command {
        PluginCommand::List => {
            let (plugins, registry_url) = agenthub_runner::plugins::list_local_plugins_view();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "registry_url": registry_url,
                    "plugins": plugins,
                }))?
            );
        }
        PluginCommand::Catalog => {
            let (plugins, registry_url, error) = agenthub_runner::plugins::list_plugins_view(true);
            if let Some(err) = error {
                bail!("failed to fetch plugin catalog ({registry_url}): {err}");
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "registry_url": registry_url,
                    "plugins": plugins,
                }))?
            );
        }
        PluginCommand::Install { id, source } => {
            let rec = agenthub_runner::plugins::install_plugin(&id, source.as_deref(), true)
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&rec)?);
        }
        PluginCommand::Uninstall { id } => {
            agenthub_runner::plugins::uninstall_plugin(&id).map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::json!({ "uninstalled": id }));
        }
        PluginCommand::Enable { id } => {
            let rec = agenthub_runner::plugins::toggle_plugin(&id, Some(true))
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&rec)?);
        }
        PluginCommand::Disable { id } => {
            let rec = agenthub_runner::plugins::toggle_plugin(&id, Some(false))
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&rec)?);
        }
        PluginCommand::SetRegistry { url } => {
            let effective = agenthub_runner::plugins::set_registry_url(Some(&url))
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::json!({ "registry_url": effective }));
        }
    }
    Ok(())
}

pub fn print_auth_snapshots(
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
