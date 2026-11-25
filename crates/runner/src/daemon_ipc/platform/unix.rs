use super::super::*;
use std::{
    fs,
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::atomic::AtomicUsize,
};

const MAX_ACTIVE_IPC_CONNECTIONS: usize = 64;
const IPC_SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(30);

struct SocketGuard(PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct ConnectionPermit(Arc<AtomicUsize>);
impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(target_os = "linux")]
fn peer_matches_euid(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast::<libc::c_void>(),
            &mut len,
        )
    };
    if rc != 0 || (len as usize) < std::mem::size_of::<libc::ucred>() {
        return false;
    }
    cred.uid == unsafe { libc::geteuid() }
}

#[cfg(any(target_os = "macos", target_os = "freebsd"))]
fn peer_matches_euid(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd;
    let mut euid: libc::uid_t = libc::uid_t::MAX;
    let mut egid: libc::gid_t = libc::gid_t::MAX;
    let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut euid, &mut egid) };
    rc == 0 && euid == unsafe { libc::geteuid() }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
fn peer_matches_euid(_stream: &UnixStream) -> bool {
    false
}

pub(crate) fn serve(
    endpoint: &str,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
    mut ready: Option<SyncSender<Result<(), String>>>,
) -> Result<()> {
    let path = Path::new(endpoint);
    if path.as_os_str().len() > 100 {
        if let Some(ready) = ready.take() {
            let _ = ready.send(Err("invalid_ipc_endpoint".into()));
        }
        bail!("invalid_ipc_endpoint");
    }
    let uid = unsafe { libc::geteuid() };
    let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        if let Some(ready) = ready.take() {
            let _ = ready.send(Err("invalid_ipc_endpoint".into()));
        }
        bail!("invalid_ipc_endpoint");
    };
    if !super::super::types::ensure_private_unix_dir(parent, uid) {
        if let Some(ready) = ready.take() {
            let _ = ready.send(Err("ipc_socket_dir_insecure".into()));
        }
        bail!("ipc_socket_dir_insecure");
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
    let active_connections = Arc::new(AtomicUsize::new(0));
    while !stopped.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if !peer_matches_euid(&stream) {
                    continue;
                }
                let prev = active_connections.fetch_add(1, Ordering::AcqRel);
                if prev >= MAX_ACTIVE_IPC_CONNECTIONS {
                    active_connections.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(IPC_SOCKET_IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IPC_SOCKET_IO_TIMEOUT));
                let permit = ConnectionPermit(active_connections.clone());
                let worker_handler = handler.clone();
                let worker_stopped = stopped.clone();
                thread::spawn(move || {
                    let _permit = permit;
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
