use super::super::*;
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
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
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

pub(crate) struct SecurityDescriptor(pub(crate) PSECURITY_DESCRIPTOR);
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub(crate) fn serve(
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
    while !stopped.load(Ordering::Acquire) {
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

pub(crate) fn request(
    endpoint: &str,
    request: &DaemonRequestEnvelope,
    timeout: Duration,
) -> Result<DaemonResponseEnvelope> {
    let mut stream = connect(endpoint, timeout)?;
    write_frame(&mut stream, request)?;
    read_frame(&mut stream)?.ok_or_else(|| anyhow::anyhow!("ipc_response_missing"))
}

pub(crate) fn cancel_request(worker: &thread::JoinHandle<()>) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::CancelSynchronousIo;

    unsafe {
        CancelSynchronousIo(worker.as_raw_handle() as HANDLE);
    }
}

pub(crate) fn connect(endpoint: &str, _timeout: Duration) -> Result<Box<dyn IpcStream>> {
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

pub(crate) fn current_user_descriptor() -> Result<SecurityDescriptor> {
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
