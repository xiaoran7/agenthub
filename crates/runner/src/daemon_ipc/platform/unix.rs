use super::super::*;
use std::{
    fs,
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
};

struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn serve(
    endpoint: &str,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
    mut ready: Option<SyncSender<Result<(), String>>>,
) -> Result<()> {
    let path = Path::new(endpoint);
    if path.as_os_str().len() > 100 {
        bail!("invalid_ipc_endpoint");
    }
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            if std::os::unix::net::UnixStream::connect(path).is_ok() {
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Err("daemon_already_running".into()));
                }
                return Err(error).context("daemon_already_running");
            }
            let file_type = fs::symlink_metadata(path)
                .context("ipc_stale_socket_metadata_failed")?
                .file_type();
            if !file_type.is_socket() {
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Err("ipc_endpoint_not_socket".into()));
                }
                bail!("ipc_endpoint_not_socket");
            }
            fs::remove_file(path).context("ipc_stale_socket_remove_failed")?;
            UnixListener::bind(path).context("ipc_socket_bind_failed")?
        }
        Err(error) => {
            if let Some(ready) = ready.take() {
                let _ = ready.send(Err("ipc_socket_bind_failed".into()));
            }
            return Err(error).context("ipc_socket_bind_failed");
        }
    };
    let _guard = SocketGuard(path.to_path_buf());
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .context("ipc_socket_permissions_failed")?;
    listener
        .set_nonblocking(true)
        .context("ipc_socket_nonblocking_failed")?;
    if let Some(ready) = ready.take() {
        let _ = ready.send(Ok(()));
    }
    while !stopped.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let worker_handler = handler.clone();
                let worker_stopped = stopped.clone();
                thread::spawn(move || {
                    let _ = serve_connection(stream, worker_handler, worker_stopped);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error).context("ipc_socket_accept_failed"),
        }
    }
    Ok(())
}

pub(crate) fn request(
    endpoint: &str,
    request: &DaemonRequestEnvelope,
    timeout: Duration,
) -> Result<DaemonResponseEnvelope> {
    let mut stream = connect(endpoint, timeout)?;
    write_frame(&mut stream, request)?;
    read_frame(&mut stream)?.ok_or_else(|| anyhow::anyhow!("ipc_response_missing"))
}

pub(crate) fn cancel_request(_worker: &thread::JoinHandle<()>) {}

pub(crate) fn connect(endpoint: &str, timeout: Duration) -> Result<Box<dyn IpcStream>> {
    let stream = std::os::unix::net::UnixStream::connect(endpoint).context("ipc_connect_failed")?;
    stream
        .set_read_timeout(Some(timeout))
        .context("ipc_timeout_failed")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("ipc_timeout_failed")?;
    Ok(Box::new(stream))
}
