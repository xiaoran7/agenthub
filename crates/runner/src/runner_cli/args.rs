use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "agenthub-runner")]
#[command(about = "AgentHub native session host and metadata collector")]
pub struct Cli {
    #[arg(long, env = "AGENT_PLATFORM_DEVICE_ID", default_value = "local-dev")]
    pub device_id: String,

    #[arg(
        long,
        env = "AGENT_PLATFORM_IDENTITY",
        help = "Use a persisted device identity for the device ID and project fingerprint key"
    )]
    pub identity: Option<PathBuf>,

    #[arg(
        long,
        default_value = "AGENT_PLATFORM_FINGERPRINT_KEY",
        help = "Environment variable containing the project fingerprint key"
    )]
    pub fingerprint_key_env: String,

    #[arg(
        long,
        env = "AGENT_PLATFORM_ALLOW_CREDENTIAL_ACCESS",
        help = "Allow the Runner to read provider auth files, keyrings and credential environment variables"
    )]
    pub allow_credential_access: bool,

    #[arg(
        long,
        env = "AGENTHUB_DEVICE_NAME",
        help = "Name for this computer shown in the console; defaults to the system computer name"
    )]
    pub device_name: Option<String>,

    #[arg(
        long,
        env = "AGENT_PLATFORM_AUTH_CACHE",
        help = "Encrypted local credential cache path"
    )]
    pub auth_cache: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the local agenthubd control plane without requiring Gateway connectivity.
    Daemon {
        #[arg(long)]
        codex_binary: PathBuf,
        #[arg(long, required = true)]
        workspace: Vec<PathBuf>,
        #[arg(long, help = "Database path (defaults to ~/.agenthub/agenthub.db)")]
        db: Option<PathBuf>,
        #[arg(long, help = "Override the per-device named pipe or Unix socket")]
        ipc_endpoint: Option<String>,
        #[arg(long, default_value_t = true, help = "Enable embedded CDS web console")]
        web: bool,
        #[arg(long, default_value_t = 4200, help = "Embedded web console port")]
        web_port: u16,
        #[arg(
            long,
            default_value_t = false,
            action = clap::ArgAction::Set,
            help = "Auto open desktop console window"
        )]
        open_browser: bool,
    },
    /// Launch the native runtime with an embedded CDS web console and auto-open browser.
    Web {
        #[arg(long, default_value = "codex")]
        codex_binary: PathBuf,
        #[arg(long, default_value = ".")]
        workspace: Vec<PathBuf>,
        #[arg(long, help = "Database path (defaults to ~/.agenthub/agenthub.db)")]
        db: Option<PathBuf>,
        #[arg(long, default_value_t = 4200)]
        port: u16,
        #[arg(
            long,
            default_value_t = true,
            action = clap::ArgAction::Set,
            help = "Auto open desktop console window"
        )]
        open_browser: bool,
        #[arg(
            long,
            default_value_t = true,
            action = clap::ArgAction::Set,
            help = "Hide Windows console window (run in background with system tray)"
        )]
        hide_console: bool,
    },
    /// Connect the local agenthubd control plane to the authenticated v3 Gateway.
    RemoteRun {
        #[arg(long)]
        codex_binary: PathBuf,
        #[arg(long, required = true)]
        workspace: Vec<PathBuf>,
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        server: String,
        #[arg(long, help = "Override the per-device named pipe or Unix socket")]
        ipc_endpoint: Option<String>,
        #[arg(long, default_value_t = true, help = "Enable embedded CDS web console")]
        web: bool,
        #[arg(long, default_value_t = 4200, help = "Embedded web console port")]
        web_port: u16,
        #[arg(
            long,
            default_value_t = false,
            action = clap::ArgAction::Set,
            help = "Auto open web console in default browser"
        )]
        open_browser: bool,
    },
    /// Open the isolated native WebView2 console window for an active local server.
    Ui {
        #[arg(long)]
        url: String,
        #[arg(long, default_value_t = 300)]
        idle_seconds: u64,
        #[arg(long, default_value = "AgentHub")]
        title: String,
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
        #[arg(long, help = "Override the Codex executable path")]
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
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List installed plugins and locally cached registry entries.
    List,
    /// Fetch the latest catalog from the configured plugin registry.
    Catalog,
    /// Download and install a plugin by ID (or from a local directory via --source).
    Install {
        id: String,
        #[arg(
            long,
            help = "Install from a local plugin directory instead of the remote registry"
        )]
        source: Option<String>,
    },
    /// Remove an installed plugin.
    Uninstall { id: String },
    /// Enable an installed plugin.
    Enable { id: String },
    /// Disable an installed plugin without removing it.
    Disable { id: String },
    /// Override or inspect the plugin registry JSON URL.
    SetRegistry { url: String },
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
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
pub enum InputFormat {
    ClaudeAgents,
    ClaudeHook,
    CodexMessage,
    CodexThreadList,
    CodexHook,
    AntigravityBrain,
    AntigravityHook,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Agent {
    Claude,
    Codex,
    Antigravity,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum AgentSelection {
    All,
    Claude,
    Codex,
    Antigravity,
}

impl std::fmt::Display for AgentSelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::All => "all",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
        })
    }
}

#[derive(Debug, Subcommand)]
pub enum OutboxCommand {
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
pub enum DeviceCommand {
    Init {
        #[arg(long)]
        identity: PathBuf,
    },
    Show {
        #[arg(long)]
        identity: PathBuf,
    },
    /// Name this computer the way it should appear in the console.
    Name {
        #[arg(long)]
        identity: PathBuf,
        /// The name to show; pass an empty string to fall back to the system name.
        name: String,
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
pub enum SyncCommand {
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
