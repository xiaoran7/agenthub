use super::*;
use agent_pulse_runner::remote_network::{RemoteControl, run_with_control};

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct RemoteProfile {
    pub workspace_paths: Vec<PathBuf>,
    pub codex_binary: String,
}

pub(crate) struct RemoteHandle {
    control: Arc<RemoteControl>,
    thread: JoinHandle<()>,
    failed: Arc<AtomicBool>,
    profile: RemoteProfile,
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
    if slot
        .as_ref()
        .is_some_and(|handle| !handle.thread.is_finished())
    {
        return Err("本机连接仍在运行，请先停止后再更改目录".to_owned());
    }
    fs::write(
        profile_path,
        serde_json::to_vec(&profile).map_err(|_| "无法保存连接设置".to_owned())?,
    )
    .map_err(|_| "无法保存连接设置".to_owned())?;
    let control = Arc::new(RemoteControl::default());
    let worker_control = control.clone();
    // A panic must not look like an intentional stop when the thread finishes.
    let failed = Arc::new(AtomicBool::new(true));
    let worker_failed = failed.clone();
    let worker_profile = profile.clone();
    let worker = thread::spawn(move || {
        let result = run_with_control(
            PathBuf::from(worker_profile.codex_binary),
            worker_profile.workspace_paths,
            database,
            identity,
            server,
            worker_control,
        );
        worker_failed.store(result.is_err(), Ordering::Relaxed);
    });
    *slot = Some(RemoteHandle {
        control,
        thread: worker,
        failed,
        profile,
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
}
