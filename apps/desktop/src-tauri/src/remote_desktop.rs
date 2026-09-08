use super::*;
use agent_pulse_runner::remote_network::{
    LocalHostClient, RemoteControl, create_local_host_channel, run_with_local_control,
};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct RemoteProfile {
    pub workspace_paths: Vec<PathBuf>,
    pub codex_binary: String,
}

pub(crate) struct RemoteHandle {
    pub(crate) control: Arc<RemoteControl>,
    pub(crate) thread: JoinHandle<()>,
    pub(crate) failed: Arc<AtomicBool>,
    pub(crate) profile: RemoteProfile,
    pub(crate) client: LocalHostClient,
}

pub(crate) fn stop(state: &DesktopState) {
    if let Ok(handle) = state.remote.lock()
        && let Some(handle) = handle.as_ref()
    {
        handle.control.stopped.store(true, Ordering::Relaxed);
    }
}

fn scoped_paths(data_dir: &Path, server: &str, device: &str) -> (PathBuf, PathBuf) {
    let scope = format!(
        "{:x}",
        Sha256::digest(format!("{server}\n{device}").as_bytes())
    );
    (
        data_dir.join(format!("remote-{}.sqlite", &scope[..16])),
        data_dir.join(format!("remote-{}.json", &scope[..16])),
    )
}

fn local_scoped_paths(data_dir: &Path, device: &str) -> (PathBuf, PathBuf) {
    scoped_paths(data_dir, "local://agenthub", device)
}

pub(crate) fn get_host_client(state: &DesktopState) -> Option<LocalHostClient> {
    if let Ok(slot) = state.remote.lock()
        && let Some(handle) = slot.as_ref()
        && !handle.thread.is_finished()
        && !handle.control.stopped.load(Ordering::Relaxed)
        && !handle.failed.load(Ordering::Relaxed)
        && handle.control.ready.load(Ordering::Relaxed)
    {
        return Some(handle.client.clone());
    }
    None
}

static LAUNCH_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn ensure_host_client(
    state: &DesktopState,
    required_workspace: Option<&Path>,
) -> Result<LocalHostClient, DesktopError> {
    // 1. Fast-path: Host is running, check or dynamically authorize workspace without restart
    {
        let mut slot = state.remote.lock().map_err(|_| DesktopError::AgentFailed)?;

        if let Some(handle) = slot.as_mut()
            && !handle.thread.is_finished()
            && !handle.failed.load(Ordering::Relaxed)
            && handle.control.ready.load(Ordering::Relaxed)
        {
            if let Some(workspace) = required_workspace {
                let canonical = workspace
                    .canonicalize()
                    .unwrap_or_else(|_| workspace.to_path_buf());
                if handle
                    .profile
                    .workspace_paths
                    .iter()
                    .any(|w| w == &canonical)
                {
                    return Ok(handle.client.clone());
                }
                // Dynamically authorize new workspace in running host - NEVER stop or kill active sessions!
                match handle.client.authorize_workspace(canonical.clone()) {
                    Ok(authorized) => {
                        if !handle.profile.workspace_paths.contains(&authorized) {
                            handle.profile.workspace_paths.push(authorized);
                            let config = read_config(&state.data_dir).ok();
                            let identity = load_or_create_identity(&state.data_dir)
                                .map_err(|_| DesktopError::Identity)?;
                            let (_, profile_path) = match config
                                .as_ref()
                                .map(|c| c.server_url.as_str())
                            {
                                Some(url) => {
                                    scoped_paths(&state.data_dir, url, identity.device_id())
                                }
                                None => local_scoped_paths(&state.data_dir, identity.device_id()),
                            };
                            let _ = fs::write(
                                &profile_path,
                                serde_json::to_vec(&handle.profile).unwrap_or_default(),
                            );
                        }
                        return Ok(handle.client.clone());
                    }
                    Err(_) => return Err(DesktopError::AgentFailed),
                }
            } else {
                return Ok(handle.client.clone());
            }
        }
    }

    // 2. Slow-path: serialize host launches with LAUNCH_LOCK to prevent double-spawn races
    let _launch_guard = LAUNCH_LOCK.lock().map_err(|_| DesktopError::AgentFailed)?;

    // Re-check after acquiring launch lock
    {
        let slot = state.remote.lock().map_err(|_| DesktopError::AgentFailed)?;
        if let Some(handle) = slot.as_ref()
            && !handle.thread.is_finished()
            && !handle.failed.load(Ordering::Relaxed)
            && handle.control.ready.load(Ordering::Relaxed)
        {
            if let Some(workspace) = required_workspace {
                let canonical = workspace
                    .canonicalize()
                    .unwrap_or_else(|_| workspace.to_path_buf());
                if handle.profile.workspace_paths.contains(&canonical) {
                    return Ok(handle.client.clone());
                }
                if handle.client.authorize_workspace(canonical).is_ok() {
                    return Ok(handle.client.clone());
                }
            } else {
                return Ok(handle.client.clone());
            }
        }
    }

    let mut workspaces = Vec::new();
    if let Some(workspace) = required_workspace
        && let Ok(canonical) = workspace.canonicalize()
        && canonical.is_dir()
    {
        workspaces.push(canonical);
    }
    start_host_with_workspaces(state, workspaces)
}

fn start_host_with_workspaces(
    state: &DesktopState,
    mut workspace_paths: Vec<PathBuf>,
) -> Result<LocalHostClient, DesktopError> {
    let data_dir = &state.data_dir;
    let config = read_config(data_dir).ok();
    let codex_binary = config
        .as_ref()
        .map(|c| c.codex_binary.clone())
        .unwrap_or_else(default_codex_binary);

    let identity = load_or_create_identity(data_dir).map_err(|_| DesktopError::Identity)?;

    let server_url = config.as_ref().map(|c| c.server_url.as_str());
    let (database, profile_path) = match server_url {
        Some(url) => scoped_paths(data_dir, url, identity.device_id()),
        None => local_scoped_paths(data_dir, identity.device_id()),
    };

    if let Ok(bytes) = fs::read(&profile_path)
        && let Ok(saved_profile) = serde_json::from_slice::<RemoteProfile>(&bytes)
    {
        for path in saved_profile.workspace_paths {
            if workspace_paths.len() < 32 && !workspace_paths.contains(&path) && path.is_dir() {
                workspace_paths.push(path);
            }
        }
    }

    if workspace_paths.is_empty()
        && let Ok(cwd) = std::env::current_dir().and_then(|p| p.canonicalize())
        && cwd.is_dir()
    {
        workspace_paths.push(cwd);
    }

    let profile = RemoteProfile {
        workspace_paths: workspace_paths.clone(),
        codex_binary: codex_binary.clone(),
    };
    let _ = fs::write(
        &profile_path,
        serde_json::to_vec(&profile).unwrap_or_default(),
    );

    let server_for_run = match server_url {
        Some(url) => {
            let mut u = Url::parse(url).ok();
            if let Some(ref mut u) = u {
                let scheme = if u.scheme() == "https" { "wss" } else { "ws" };
                let _ = u.set_scheme(scheme);
                u.set_path("/v3/remote/ws");
            }
            u.map(|u| u.to_string())
        }
        None => None,
    };

    let control = Arc::new(RemoteControl::default());
    let worker_control = control.clone();
    let failed = Arc::new(AtomicBool::new(false));
    let worker_failed = failed.clone();
    let worker_profile = profile.clone();
    let worker_identity = identity.clone();

    let (client, channel) = create_local_host_channel();
    let client_for_state = client.clone();

    let worker = thread::spawn(move || {
        let result = run_with_local_control(
            PathBuf::from(worker_profile.codex_binary),
            worker_profile.workspace_paths,
            database,
            worker_identity,
            server_for_run,
            worker_control.clone(),
            Some(channel),
        );
        worker_failed.store(result.is_err(), Ordering::Relaxed);
        worker_control.ready.store(false, Ordering::Relaxed);
    });

    // Wait up to 20 seconds for host to report ready (aligning with 20s interactive handshake)
    let start = Instant::now();
    while !control.ready.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(20) {
        if worker.is_finished() {
            let _ = worker.join();
            return Err(DesktopError::AgentFailed);
        }
        thread::sleep(Duration::from_millis(20));
    }

    if !control.ready.load(Ordering::Relaxed) {
        control.stopped.store(true, Ordering::Relaxed);
        let _ = worker.join();
        return Err(DesktopError::AgentFailed);
    }

    let mut slot = state.remote.lock().map_err(|_| DesktopError::AgentFailed)?;
    *slot = Some(RemoteHandle {
        control,
        thread: worker,
        failed,
        profile,
        client: client_for_state,
    });

    Ok(client)
}

#[tauri::command]
pub(crate) async fn start_remote_host(
    input: RemoteProfile,
    state: State<'_, DesktopState>,
) -> Result<(), String> {
    let data_dir = state.data_dir.clone();
    let (profile, identity, server, database, profile_path) =
        tauri::async_runtime::spawn_blocking(move || {
            if input.workspace_paths.is_empty()
                || input.workspace_paths.len() > 32
                || !valid_binary(&input.codex_binary)
            {
                return Err("请选择工作目录和有效的 Codex 可执行文件".to_owned());
            }
            let workspace_paths = input
                .workspace_paths
                .iter()
                .map(|path| {
                    path.canonicalize()
                        .ok()
                        .filter(|path| path.is_dir())
                        .ok_or_else(|| "工作目录不存在".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let config = read_config(&data_dir).map_err(|_| "请先登录".to_owned())?;
            let cookie = load_cookie(&config.server_url).map_err(|_| "请重新登录".to_owned())?;
            // Check Remote enablement/account access before starting a native process.
            request_json(
                "GET",
                &config.server_url,
                "/v3/remote/sessions",
                Some(&cookie),
                None,
            )
            .map_err(|_| "Gateway 尚未启用远程连接，或当前登录已失效".to_owned())?;
            let identity =
                load_or_create_identity(&data_dir).map_err(|_| "本机设备身份不可用".to_owned())?;
            ensure_device_bound(&config.server_url, &cookie, &identity)
                .map_err(|_| "设备绑定已失效".to_owned())?;
            let (database, profile_path) =
                scoped_paths(&data_dir, &config.server_url, identity.device_id());
            let mut server =
                Url::parse(&config.server_url).map_err(|_| "服务器地址无效".to_owned())?;
            let scheme = if server.scheme() == "https" {
                "wss"
            } else {
                "ws"
            };
            server
                .set_scheme(scheme)
                .map_err(|_| "服务器地址无效".to_owned())?;
            server.set_path("/v3/remote/ws");
            Ok((
                RemoteProfile {
                    workspace_paths,
                    codex_binary: input.codex_binary,
                },
                identity,
                server.to_string(),
                database,
                profile_path,
            ))
        })
        .await
        .map_err(|_| "无法启动本机连接".to_owned())??;
    let mut slot = state
        .remote
        .lock()
        .map_err(|_| "本机连接正在切换".to_owned())?;
    if let Some(handle) = slot.as_mut()
        && !handle.thread.is_finished()
        && !handle.failed.load(Ordering::Relaxed)
        && handle.control.ready.load(Ordering::Relaxed)
    {
        // Host is already running: dynamically authorize any new workspaces without stopping active sessions
        for ws in &profile.workspace_paths {
            let _ = handle.client.authorize_workspace(ws.clone());
            if !handle.profile.workspace_paths.contains(ws) {
                handle.profile.workspace_paths.push(ws.clone());
            }
        }
        let _ = fs::write(
            profile_path,
            serde_json::to_vec(&handle.profile).map_err(|_| "无法保存连接设置".to_owned())?,
        );
        return Ok(());
    }
    if slot
        .as_ref()
        .is_some_and(|handle| !handle.thread.is_finished())
    {
        return Err("本机连接正在关闭或状态异常，请稍候再试".to_owned());
    }
    fs::write(
        profile_path,
        serde_json::to_vec(&profile).map_err(|_| "无法保存连接设置".to_owned())?,
    )
    .map_err(|_| "无法保存连接设置".to_owned())?;
    let control = Arc::new(RemoteControl::default());
    let worker_control = control.clone();
    let failed = Arc::new(AtomicBool::new(false));
    let worker_failed = failed.clone();
    let worker_profile = profile.clone();
    let (client, channel) = create_local_host_channel();
    let client_for_state = client.clone();
    let worker = thread::spawn(move || {
        let result = run_with_local_control(
            PathBuf::from(worker_profile.codex_binary),
            worker_profile.workspace_paths,
            database,
            identity,
            Some(server),
            worker_control.clone(),
            Some(channel),
        );
        worker_failed.store(result.is_err(), Ordering::Relaxed);
        worker_control.ready.store(false, Ordering::Relaxed);
    });
    *slot = Some(RemoteHandle {
        control,
        thread: worker,
        failed,
        profile,
        client: client_for_state,
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn remote_host_status(state: State<'_, DesktopState>) -> Result<Value, String> {
    let slot = state
        .remote
        .lock()
        .map_err(|_| "本机连接状态不可用".to_owned())?;
    if let Some(handle) = slot.as_ref() {
        let phase = if handle.thread.is_finished() {
            if handle.failed.load(Ordering::Relaxed) {
                "failed"
            } else {
                "stopped"
            }
        } else if handle.control.stopped.load(Ordering::Relaxed) {
            "stopping"
        } else if handle.control.connected.load(Ordering::Relaxed) {
            "connected"
        } else {
            "connecting"
        };
        return Ok(json!({"phase":phase,"profile":handle.profile}));
    }
    let config = read_config(&state.data_dir).map_err(|_| "请先登录".to_owned())?;
    let identity =
        load_or_create_identity(&state.data_dir).map_err(|_| "设备身份不可用".to_owned())?;
    let (_, path) = scoped_paths(&state.data_dir, &config.server_url, identity.device_id());
    let profile = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RemoteProfile>(&bytes).ok())
        .unwrap_or(RemoteProfile {
            workspace_paths: Vec::new(),
            codex_binary: config.codex_binary,
        });
    Ok(json!({"phase":"stopped","profile":profile}))
}

#[tauri::command]
pub(crate) async fn stop_remote_host(state: State<'_, DesktopState>) -> Result<(), String> {
    stop(&state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_content_and_profile_are_scoped_to_server_and_device() {
        let dir = Path::new("example");
        let a = scoped_paths(dir, "https://one.test", "a");
        assert_eq!(a, scoped_paths(dir, "https://one.test", "a"));
        assert_ne!(a, scoped_paths(dir, "https://one.test", "b"));
        assert_ne!(a, scoped_paths(dir, "https://two.test", "a"));
        assert!(a.0.starts_with(dir));
    }

    #[test]
    fn offline_scoped_paths_are_deterministic_and_distinct_per_device() {
        let dir = Path::new("test-dir");
        let p1 = local_scoped_paths(dir, "dev-1");
        let p2 = local_scoped_paths(dir, "dev-1");
        let p3 = local_scoped_paths(dir, "dev-2");
        assert_eq!(p1, p2);
        assert_ne!(p1, p3);
        assert!(p1.0.starts_with(dir));
    }

    #[test]
    fn running_host_handle_reports_ready_and_provides_client() {
        let dir = tempfile::tempdir().unwrap();
        let (client, _channel) = create_local_host_channel();
        let control = Arc::new(RemoteControl::default());
        control.ready.store(true, Ordering::Relaxed);
        let worker = thread::spawn(|| {
            thread::sleep(Duration::from_millis(50));
        });
        let handle = RemoteHandle {
            control: control.clone(),
            thread: worker,
            failed: Arc::new(AtomicBool::new(false)),
            profile: RemoteProfile {
                workspace_paths: Vec::new(),
                codex_binary: "codex".to_owned(),
            },
            client,
        };
        let state = DesktopState {
            remote: Mutex::new(Some(handle)),
            data_dir: dir.path().to_path_buf(),
            engine: Mutex::new(None),
            realtime: Mutex::new(None),
            active_turns: Mutex::new(HashMap::new()),
            session_contexts: Mutex::new(SessionContextCache::default()),
        };
        assert!(get_host_client(&state).is_some());

        // When not ready, get_host_client must strictly reject and return None
        control.ready.store(false, Ordering::Relaxed);
        assert!(get_host_client(&state).is_none());
    }
}
