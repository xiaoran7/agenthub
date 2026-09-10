use crate::daemon_protocol::{
    DaemonEventEnvelope, DaemonRequest, DaemonRequestEnvelope, DaemonResponse,
    DaemonResponseEnvelope, MAX_DAEMON_FRAME_BYTES, validate_request,
};
use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::Duration,
};

pub trait DaemonRequestHandler: Send + Sync + 'static {
    fn handle(&self, request: DaemonRequest) -> DaemonResponse;
}

trait IpcStream: Read + Write + Send {}
impl<T: Read + Write + Send> IpcStream for T {}

pub struct DaemonSubscription {
    stream: Box<dyn IpcStream>,
}

impl DaemonSubscription {
    pub fn next_event(&mut self) -> Result<Option<DaemonEventEnvelope>> {
        read_frame(&mut self.stream)
    }
}

impl<F> DaemonRequestHandler for F
where
    F: Fn(DaemonRequest) -> DaemonResponse + Send + Sync + 'static,
{
    fn handle(&self, request: DaemonRequest) -> DaemonResponse {
        self(request)
    }
}

pub fn default_endpoint(scope: &str) -> String {
    let digest = Sha256::digest(scope.as_bytes());
    let suffix = hex::encode(&digest[..12]);
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\agenthubd-{suffix}")
    }
    #[cfg(unix)]
    {
        // The socket itself is 0600. Including the effective UID also prevents
        // unrelated users from contending for the same fallback /tmp name.
        format!("/tmp/agenthubd-{}-{suffix}.sock", unsafe {
            libc::geteuid()
        })
    }
}

pub fn serve(
    endpoint: &str,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
) -> Result<()> {
    platform::serve(endpoint, handler, stopped, None)
}

pub(crate) fn serve_with_ready(
    endpoint: &str,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
    ready: SyncSender<Result<(), String>>,
) -> Result<()> {
    platform::serve(endpoint, handler, stopped, Some(ready))
}

pub const DAEMON_REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

pub fn request(endpoint: &str, request: &DaemonRequestEnvelope) -> Result<DaemonResponseEnvelope> {
    request_with_timeout(endpoint, request, DAEMON_REQUEST_TIMEOUT)
}

pub fn request_with_timeout(
    endpoint: &str,
    request: &DaemonRequestEnvelope,
    timeout: Duration,
) -> Result<DaemonResponseEnvelope> {
    let endpoint = endpoint.to_owned();
    let request = request.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let _ = sender.send(platform::request(&endpoint, &request, timeout));
    });
    match receiver.recv_timeout(timeout) {
        Ok(result) => {
            let _ = worker.join();
            result
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            platform::cancel_request(&worker);
            bail!("ipc_response_timeout")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let _ = worker.join();
            bail!("ipc_request_worker_stopped")
        }
    }
}

pub fn subscribe(
    endpoint: &str,
    request: &DaemonRequestEnvelope,
) -> Result<(DaemonResponseEnvelope, DaemonSubscription)> {
    if !matches!(request.request, DaemonRequest::Subscribe { .. }) {
        bail!("ipc_subscription_request_required");
    }
    let mut stream = platform::connect(endpoint, DAEMON_REQUEST_TIMEOUT)?;
    write_frame(&mut stream, request)?;
    let response =
        read_frame(&mut stream)?.ok_or_else(|| anyhow::anyhow!("ipc_response_missing"))?;
    Ok((response, DaemonSubscription { stream }))
}

fn serve_connection<S>(
    mut stream: S,
    handler: Arc<dyn DaemonRequestHandler>,
    stopped: Arc<AtomicBool>,
) -> Result<()>
where
    S: Read + Write,
{
    while !stopped.load(Ordering::Relaxed) {
        let Some(request) = read_frame::<DaemonRequestEnvelope>(&mut stream)? else {
            return Ok(());
        };
        if let Err(code) = validate_request(&request) {
            write_frame(
                &mut stream,
                &DaemonResponseEnvelope::error(request.request_id, code),
            )?;
            continue;
        }
        if let DaemonRequest::Subscribe { after_cursor } = request.request {
            let request_id = request.request_id;
            let initial = handler.handle(DaemonRequest::GetEvents {
                after_cursor,
                limit: 256,
            });
            let mut cursor = match &initial {
                DaemonResponse::Events(crate::remote_journal::EventsAfter::Events {
                    cursor,
                    ..
                }) => *cursor,
                _ => after_cursor,
            };
            let snapshot_required = matches!(
                &initial,
                DaemonResponse::Events(crate::remote_journal::EventsAfter::SnapshotRequired { .. })
            );
            write_frame(
                &mut stream,
                &DaemonResponseEnvelope::success(request_id.clone(), initial),
            )?;
            if snapshot_required {
                return Ok(());
            }
            let mut last_heartbeat = std::time::Instant::now();
            loop {
                if stopped.load(Ordering::Relaxed) {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(50));
                match handler.handle(DaemonRequest::GetEvents {
                    after_cursor: cursor,
                    limit: 256,
                }) {
                    DaemonResponse::Events(crate::remote_journal::EventsAfter::Events {
                        events,
                        cursor: next,
                    }) => {
                        for event in events {
                            write_frame(
                                &mut stream,
                                &DaemonEventEnvelope::new(
                                    request_id.clone(),
                                    event.cursor,
                                    event.payload,
                                ),
                            )?;
                        }
                        cursor = next;
                        if last_heartbeat.elapsed() >= Duration::from_secs(1) {
                            write_frame(
                                &mut stream,
                                &DaemonEventEnvelope::new(
                                    request_id.clone(),
                                    cursor,
                                    serde_json::json!({"type": "heartbeat"}),
                                ),
                            )?;
                            last_heartbeat = std::time::Instant::now();
                        }
                    }
                    DaemonResponse::Events(
                        crate::remote_journal::EventsAfter::SnapshotRequired { snapshot },
                    ) => {
                        write_frame(
                            &mut stream,
                            &DaemonEventEnvelope::new(
                                request_id.clone(),
                                snapshot.cursor,
                                serde_json::json!({
                                    "type": "snapshot_required",
                                    "snapshot": snapshot,
                                }),
                            ),
                        )?;
                        return Ok(());
                    }
                    DaemonResponse::Error { code } => {
                        write_frame(
                            &mut stream,
                            &DaemonEventEnvelope::new(
                                request_id.clone(),
                                cursor,
                                serde_json::json!({"type": "error", "code": code}),
                            ),
                        )?;
                        return Ok(());
                    }
                    _ => bail!("invalid_subscription_response"),
                }
            }
        }
        let response = handler.handle(request.request);
        write_frame(
            &mut stream,
            &DaemonResponseEnvelope::success(request.request_id, response),
        )?;
    }
    Ok(())
}

fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>> {
    let mut prefix = [0u8; 4];
    let mut read = 0;
    while read < prefix.len() {
        match reader.read(&mut prefix[read..]) {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => bail!("ipc_truncated_frame"),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("ipc_read_failed"),
        }
    }
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length > MAX_DAEMON_FRAME_BYTES {
        bail!("ipc_frame_too_large");
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .context("ipc_truncated_frame")?;
    serde_json::from_slice(&payload)
        .context("ipc_invalid_json")
        .map(Some)
}

fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value).context("ipc_encode_failed")?;
    if payload.len() > MAX_DAEMON_FRAME_BYTES {
        bail!("ipc_frame_too_large");
    }
    writer
        .write_all(&(payload.len() as u32).to_le_bytes())
        .context("ipc_write_failed")?;
    writer.write_all(&payload).context("ipc_write_failed")?;
    writer.flush().context("ipc_write_failed")
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        ffi::c_void,
        fs::File,
        os::windows::io::{FromRawHandle, RawHandle},
        ptr,
        time::Instant,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
            GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
        },
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers,
            OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
        },
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
                PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
            },
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    struct OwnedWinHandle(HANDLE);

    // Windows HANDLE values are owned by this process and closed exactly once.
    unsafe impl Send for OwnedWinHandle {}

    impl Drop for OwnedWinHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
    impl Drop for SecurityDescriptor {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    pub(super) fn serve(
        endpoint: &str,
        handler: Arc<dyn DaemonRequestHandler>,
        stopped: Arc<AtomicBool>,
        mut ready: Option<SyncSender<Result<(), String>>>,
    ) -> Result<()> {
        validate_pipe_name(endpoint)?;
        let endpoint = wide(endpoint);
        let descriptor = current_user_descriptor()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let mut first = true;
        while !stopped.load(Ordering::Relaxed) {
            let access = PIPE_ACCESS_DUPLEX
                | if first {
                    FILE_FLAG_FIRST_PIPE_INSTANCE
                } else {
                    0
                };
            let handle = unsafe {
                CreateNamedPipeW(
                    endpoint.as_ptr(),
                    access,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    16,
                    MAX_DAEMON_FRAME_BYTES as u32,
                    MAX_DAEMON_FRAME_BYTES as u32,
                    0,
                    &attributes,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                let last_error = unsafe { GetLastError() };
                if !first && last_error == ERROR_PIPE_BUSY {
                    thread::sleep(Duration::from_millis(20));
                    continue;
                }
                let code = if first {
                    "daemon_already_running"
                } else {
                    "ipc_pipe_create_failed"
                };
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Err(code.into()));
                }
                return Err(io::Error::from_raw_os_error(last_error as i32)).context(code);
            }
            if let Some(ready) = ready.take() {
                let _ = ready.send(Ok(()));
            }
            first = false;
            let handle = OwnedWinHandle(handle);
            let connected = unsafe { ConnectNamedPipe(handle.0, ptr::null_mut()) } != 0
                || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            if !connected {
                return Err(io::Error::last_os_error()).context("ipc_pipe_connect_failed");
            }
            let worker_handler = handler.clone();
            let worker_stopped = stopped.clone();
            thread::spawn(move || {
                let raw = handle.0;
                std::mem::forget(handle);
                let mut stream = unsafe { File::from_raw_handle(raw as RawHandle) };
                let _ = serve_connection(&mut stream, worker_handler, worker_stopped);
                unsafe {
                    FlushFileBuffers(raw);
                    DisconnectNamedPipe(raw);
                }
            });
        }
        Ok(())
    }

    pub(super) fn request(
        endpoint: &str,
        request: &DaemonRequestEnvelope,
        timeout: Duration,
    ) -> Result<DaemonResponseEnvelope> {
        let mut stream = connect(endpoint, timeout)?;
        write_frame(&mut stream, request)?;
        read_frame(&mut stream)?.ok_or_else(|| anyhow::anyhow!("ipc_response_missing"))
    }

    pub(super) fn cancel_request(worker: &thread::JoinHandle<()>) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::IO::CancelSynchronousIo;

        unsafe {
            CancelSynchronousIo(worker.as_raw_handle() as HANDLE);
        }
    }

    pub(super) fn connect(endpoint: &str, _timeout: Duration) -> Result<Box<dyn IpcStream>> {
        validate_pipe_name(endpoint)?;
        let endpoint = wide(endpoint);
        let deadline = Instant::now() + Duration::from_secs(2);
        let handle = loop {
            let handle = unsafe {
                CreateFileW(
                    endpoint.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    ptr::null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    ptr::null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                break handle;
            }
            if unsafe { GetLastError() } != ERROR_PIPE_BUSY || Instant::now() >= deadline {
                return Err(io::Error::last_os_error()).context("ipc_connect_failed");
            }
            thread::sleep(Duration::from_millis(20));
        };
        let stream = unsafe { File::from_raw_handle(handle as RawHandle) };
        Ok(Box::new(stream))
    }

    fn validate_pipe_name(endpoint: &str) -> Result<()> {
        if !endpoint.starts_with(r"\\.\pipe\") || endpoint.len() > 240 {
            bail!("invalid_ipc_endpoint");
        }
        Ok(())
    }

    fn current_user_descriptor() -> Result<SecurityDescriptor> {
        let mut token = ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error()).context("ipc_token_open_failed");
        }
        let token = OwnedWinHandle(token);
        let mut length = 0;
        unsafe {
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut length);
        }
        if length == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            return Err(io::Error::last_os_error()).context("ipc_token_query_failed");
        }
        let word_size = std::mem::size_of::<usize>();
        let mut buffer = vec![0usize; (length as usize).div_ceil(word_size)];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast::<c_void>(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error()).context("ipc_token_query_failed");
        }
        let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
        let mut sid = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
            return Err(io::Error::last_os_error()).context("ipc_sid_format_failed");
        }
        let sid_text = unsafe {
            let mut length = 0;
            while *sid.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(sid, length))
        };
        unsafe {
            LocalFree(sid.cast());
        }
        let sddl = wide(&format!("D:P(A;;GA;;;{sid_text})"));
        let mut descriptor = ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error()).context("ipc_acl_create_failed");
        }
        Ok(SecurityDescriptor(descriptor))
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
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

    pub(super) fn serve(
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

    pub(super) fn request(
        endpoint: &str,
        request: &DaemonRequestEnvelope,
        timeout: Duration,
    ) -> Result<DaemonResponseEnvelope> {
        let mut stream = connect(endpoint, timeout)?;
        write_frame(&mut stream, request)?;
        read_frame(&mut stream)?.ok_or_else(|| anyhow::anyhow!("ipc_response_missing"))
    }

    pub(super) fn cancel_request(_worker: &thread::JoinHandle<()>) {}

    pub(super) fn connect(endpoint: &str, timeout: Duration) -> Result<Box<dyn IpcStream>> {
        let stream =
            std::os::unix::net::UnixStream::connect(endpoint).context("ipc_connect_failed")?;
        stream
            .set_read_timeout(Some(timeout))
            .context("ipc_timeout_failed")?;
        stream
            .set_write_timeout(Some(timeout))
            .context("ipc_timeout_failed")?;
        Ok(Box::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        daemon_protocol::{DAEMON_PROTOCOL_VERSION, DaemonRequest},
        remote_journal::{EventsAfter, JournalEvent},
    };
    use serde_json::json;
    use std::{
        io::Cursor,
        sync::{
            atomic::{AtomicUsize, Ordering as AtomicOrdering},
            mpsc,
        },
    };

    fn envelope(request_id: &str) -> DaemonRequestEnvelope {
        DaemonRequestEnvelope {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            request_id: request_id.into(),
            request: DaemonRequest::GetModels,
        }
    }

    #[test]
    fn oversized_and_truncated_frames_are_rejected_before_json_decode() {
        let bytes = ((MAX_DAEMON_FRAME_BYTES + 1) as u32).to_le_bytes().to_vec();
        let error = read_frame::<DaemonRequestEnvelope>(&mut Cursor::new(bytes)).unwrap_err();
        assert!(error.to_string().contains("ipc_frame_too_large"));

        let mut truncated = 10u32.to_le_bytes().to_vec();
        truncated.extend_from_slice(b"{}");
        let error = read_frame::<DaemonRequestEnvelope>(&mut Cursor::new(truncated)).unwrap_err();
        assert!(error.to_string().contains("ipc_truncated_frame"));

        let too_large = json!({"payload": "x".repeat(MAX_DAEMON_FRAME_BYTES)});
        let error = write_frame(&mut Vec::new(), &too_large).unwrap_err();
        assert!(error.to_string().contains("ipc_frame_too_large"));
    }

    #[test]
    fn platform_ipc_roundtrips_and_rejects_a_second_server() {
        let endpoint = default_endpoint(&format!("ipc-test-{}", uuid::Uuid::new_v4()));
        let stopped = Arc::new(AtomicBool::new(false));
        let handler: Arc<dyn DaemonRequestHandler> = Arc::new(|request| match request {
            DaemonRequest::GetModels => DaemonResponse::Models(json!({"source": "test"})),
            _ => DaemonResponse::Error {
                code: "unexpected_request".into(),
            },
        });
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let server_endpoint = endpoint.clone();
        let server_stopped = stopped.clone();
        let server_handler = handler.clone();
        let server = thread::spawn(move || {
            serve_with_ready(&server_endpoint, server_handler, server_stopped, ready_tx)
        });
        assert_eq!(
            ready_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Ok(())
        );

        let response = request(&endpoint, &envelope("request-one")).unwrap();
        assert_eq!(response.protocol_version, DAEMON_PROTOCOL_VERSION);
        assert_eq!(response.request_id, "request-one");
        assert_eq!(
            response.response,
            DaemonResponse::Models(json!({"source": "test"}))
        );

        let (second_ready_tx, second_ready_rx) = mpsc::sync_channel(1);
        let second = serve_with_ready(
            &endpoint,
            handler,
            Arc::new(AtomicBool::new(false)),
            second_ready_tx,
        );
        assert!(second.is_err());
        assert_eq!(
            second_ready_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap(),
            Err("daemon_already_running".into())
        );

        stopped.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        let _ = request(&endpoint, &envelope("shutdown"));
        server.join().unwrap().unwrap();
    }

    #[test]
    fn request_timeout_returns_without_waiting_for_a_slow_handler() {
        let endpoint = default_endpoint(&format!("timeout-test-{}", uuid::Uuid::new_v4()));
        let stopped = Arc::new(AtomicBool::new(false));
        let handler: Arc<dyn DaemonRequestHandler> = Arc::new(|request| match request {
            DaemonRequest::GetModels => {
                thread::sleep(Duration::from_millis(350));
                DaemonResponse::Models(json!({"source": "slow"}))
            }
            _ => DaemonResponse::Error {
                code: "unexpected_request".into(),
            },
        });
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let server_endpoint = endpoint.clone();
        let server_stopped = stopped.clone();
        let server = thread::spawn(move || {
            serve_with_ready(&server_endpoint, handler, server_stopped, ready_tx)
        });
        assert_eq!(
            ready_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Ok(())
        );

        let started = std::time::Instant::now();
        let error = request_with_timeout(
            &endpoint,
            &envelope("request-timeout"),
            Duration::from_millis(50),
        )
        .unwrap_err();

        assert!(error.to_string().contains("ipc_response_timeout"));
        assert!(started.elapsed() < Duration::from_millis(250));
        thread::sleep(Duration::from_millis(400));
        stopped.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        let _ = request(&endpoint, &envelope("shutdown"));
        server.join().unwrap().unwrap();
    }

    #[test]
    fn platform_subscription_streams_versioned_cursor_events() {
        let endpoint = default_endpoint(&format!("subscription-test-{}", uuid::Uuid::new_v4()));
        let stopped = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = calls.clone();
        let handler: Arc<dyn DaemonRequestHandler> = Arc::new(move |request| match request {
            DaemonRequest::GetEvents { after_cursor, .. } => {
                let call = handler_calls.fetch_add(1, AtomicOrdering::Relaxed);
                if call == 0 {
                    DaemonResponse::Events(EventsAfter::Events {
                        events: vec![],
                        cursor: after_cursor,
                    })
                } else {
                    DaemonResponse::Events(EventsAfter::Events {
                        events: vec![JournalEvent {
                            cursor: after_cursor + 1,
                            payload: json!({"type": "event", "value": 7}),
                        }],
                        cursor: after_cursor + 1,
                    })
                }
            }
            _ => DaemonResponse::Error {
                code: "unexpected_request".into(),
            },
        });
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let server_endpoint = endpoint.clone();
        let server_stopped = stopped.clone();
        let server = thread::spawn(move || {
            serve_with_ready(&server_endpoint, handler, server_stopped, ready_tx)
        });
        assert_eq!(
            ready_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Ok(())
        );

        let subscribe_request = DaemonRequestEnvelope {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            request_id: "subscription-one".into(),
            request: DaemonRequest::Subscribe { after_cursor: 10 },
        };
        let (initial, mut subscription) = subscribe(&endpoint, &subscribe_request).unwrap();
        assert_eq!(initial.request_id, "subscription-one");
        assert!(matches!(
            initial.response,
            DaemonResponse::Events(EventsAfter::Events { cursor: 10, .. })
        ));
        let event = subscription.next_event().unwrap().unwrap();
        assert_eq!(event.protocol_version, DAEMON_PROTOCOL_VERSION);
        assert_eq!(event.request_id, "subscription-one");
        assert_eq!(event.cursor, 11);
        assert_eq!(event.event["value"], 7);
        drop(subscription);

        stopped.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        let _ = request(&endpoint, &envelope("shutdown"));
        server.join().unwrap().unwrap();
    }
}
