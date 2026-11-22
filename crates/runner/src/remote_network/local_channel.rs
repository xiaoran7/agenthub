use super::*;

const LOCAL_HOST_SEND_TIMEOUT: Duration = Duration::from_secs(2);
const LOCAL_HOST_QUERY_TIMEOUT: Duration = Duration::from_secs(25);

fn send_local_command(
    sender: &SyncSender<LocalHostCommand>,
    mut command: LocalHostCommand,
) -> Result<(), String> {
    let deadline = Instant::now() + LOCAL_HOST_SEND_TIMEOUT;
    loop {
        match sender.try_send(command) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(returned)) if Instant::now() < deadline => {
                command = returned;
                thread::sleep(Duration::from_millis(5));
            }
            Err(mpsc::TrySendError::Full(_)) => return Err("host_queue_timeout".into()),
            Err(mpsc::TrySendError::Disconnected(_)) => return Err("host_not_running".into()),
        }
    }
}

pub enum LocalHostCommand {
    Execute {
        command: RemoteCommand,
        respond_to: SyncSender<Result<CommandReceipt, String>>,
    },
    QuerySessions {
        respond_to: SyncSender<Vec<SessionBinding>>,
    },
    QuerySnapshot {
        respond_to: SyncSender<Result<JournalSnapshot, String>>,
    },
    QueryEvents {
        after_cursor: i64,
        limit: usize,
        respond_to: SyncSender<Result<EventsAfter, String>>,
    },
    QueryWorkspaces {
        respond_to: SyncSender<Vec<PathBuf>>,
    },
    AuthorizeWorkspace {
        workspace: PathBuf,
        respond_to: SyncSender<Result<PathBuf, String>>,
    },
    QueryGeneration {
        respond_to: SyncSender<String>,
    },
    QueryModels {
        respond_to: SyncSender<Result<Value, String>>,
    },
    Discover {
        respond_to: SyncSender<Result<Value, String>>,
    },
    SetRemoteEnabled {
        server_url: Option<String>,
        identity: DeviceIdentity,
        respond_to: SyncSender<Result<bool, String>>,
    },
    QueryStatus {
        respond_to: SyncSender<(String, Vec<PathBuf>, bool, bool)>,
    },
}

pub struct LocalHostChannel {
    pub receiver: Receiver<LocalHostCommand>,
    pub listeners: Arc<Mutex<HashMap<u64, SyncSender<HostEvent>>>>,
}

#[derive(Clone)]
pub struct LocalHostClient {
    pub(crate) sender: SyncSender<LocalHostCommand>,
    listeners: Arc<Mutex<HashMap<u64, SyncSender<HostEvent>>>>,
    next_listener_id: Arc<AtomicU64>,
}

pub struct LocalEventSubscription {
    listeners: Arc<Mutex<HashMap<u64, SyncSender<HostEvent>>>>,
    id: u64,
}

impl Drop for LocalEventSubscription {
    fn drop(&mut self) {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.remove(&self.id);
        }
    }
}

impl LocalHostClient {
    pub fn execute(&self, command: RemoteCommand) -> Result<CommandReceipt, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::Execute {
                command,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "command_timeout".to_string())?
    }

    pub fn query_sessions(&self) -> Result<Vec<SessionBinding>, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QuerySessions { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn snapshot(&self) -> Result<JournalSnapshot, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QuerySnapshot { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn events_after(&self, after_cursor: i64, limit: usize) -> Result<EventsAfter, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryEvents {
                after_cursor,
                limit,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn query_workspaces(&self) -> Result<Vec<PathBuf>, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryWorkspaces { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn authorize_workspace(&self, workspace: PathBuf) -> Result<PathBuf, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::AuthorizeWorkspace {
                workspace,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "authorize_timeout".to_string())?
    }

    pub fn query_generation(&self) -> Result<String, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryGeneration { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn query_models(&self) -> Result<Value, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryModels { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn discover(&self) -> Result<Value, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(&self.sender, LocalHostCommand::Discover { respond_to: tx })?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())?
    }

    pub fn set_remote_enabled(
        &self,
        server_url: Option<String>,
        identity: DeviceIdentity,
    ) -> Result<bool, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::SetRemoteEnabled {
                server_url,
                identity,
                respond_to: tx,
            },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "set_remote_timeout".to_string())?
    }

    pub fn status(&self) -> Result<(String, Vec<PathBuf>, bool, bool), String> {
        let (tx, rx) = mpsc::sync_channel(1);
        send_local_command(
            &self.sender,
            LocalHostCommand::QueryStatus { respond_to: tx },
        )?;
        rx.recv_timeout(LOCAL_HOST_QUERY_TIMEOUT)
            .map_err(|_| "query_timeout".to_string())
    }

    pub fn enable_remote(
        &self,
        server_url: String,
        identity: DeviceIdentity,
    ) -> Result<(), String> {
        self.set_remote_enabled(Some(server_url), identity)
            .map(|_| ())
    }

    pub fn subscribe(&self, capacity: usize) -> (LocalEventSubscription, Receiver<HostEvent>) {
        let (tx, rx) = mpsc::sync_channel(capacity);
        let id = self.next_listener_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.insert(id, tx);
        }
        (
            LocalEventSubscription {
                listeners: self.listeners.clone(),
                id,
            },
            rx,
        )
    }
}

pub fn create_local_host_channel() -> (LocalHostClient, LocalHostChannel) {
    let (tx, rx) = mpsc::sync_channel(128);
    let listeners = Arc::new(Mutex::new(HashMap::new()));
    let client = LocalHostClient {
        sender: tx,
        listeners: listeners.clone(),
        next_listener_id: Arc::new(AtomicU64::new(1)),
    };
    let channel = LocalHostChannel {
        receiver: rx,
        listeners,
    };
    (client, channel)
}
