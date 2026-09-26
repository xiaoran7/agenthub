use std::{
    io::BufRead,
    process::{Child, Command},
};

use super::MAX_PROTOCOL_LINE_BYTES;

pub(crate) enum BoundedLine {
    Line(String),
    TooLarge,
    Eof,
    Io(std::io::Error),
}

/// Read one newline-terminated line while enforcing the size limit during the
/// read. `read_line` would buffer an arbitrarily long line before any length
/// check could run, letting a misbehaving child exhaust memory.
pub(crate) fn read_bounded_line(reader: &mut impl BufRead) -> BoundedLine {
    let mut line = Vec::new();
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return BoundedLine::Io(error),
        };
        if buffer.is_empty() {
            if line.is_empty() {
                return BoundedLine::Eof;
            }
            return finish_line(line);
        }
        let (take, found_newline) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (buffer.len(), false),
        };
        if line.len() + take > MAX_PROTOCOL_LINE_BYTES {
            return BoundedLine::TooLarge;
        }
        line.extend_from_slice(&buffer[..take]);
        reader.consume(take);
        if found_newline {
            return finish_line(line);
        }
    }
}

pub(crate) fn finish_line(line: Vec<u8>) -> BoundedLine {
    match String::from_utf8(line) {
        Ok(line) => BoundedLine::Line(line),
        Err(error) => BoundedLine::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            error.utf8_error(),
        )),
    }
}

#[cfg(windows)]
pub(crate) struct ChildJob(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: ChildJob exclusively owns a Windows kernel handle. Kernel handles
// are valid across threads in the owning process, and Drop only calls
// CloseHandle on that same owned value.
#[cfg(windows)]
unsafe impl Send for ChildJob {}

#[cfg(windows)]
impl ChildJob {
    pub(crate) fn attach(child: &Child) -> std::io::Result<Self> {
        use std::{
            ffi::c_void,
            mem::{size_of, zeroed},
            os::windows::io::AsRawHandle,
            ptr,
        };
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
        };

        // SAFETY: All pointers refer to initialized local data for the duration of each
        // call. Every successful job handle is owned by ChildJob and closed once.
        unsafe {
            let handle = CreateJobObjectW(ptr::null(), ptr::null());
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut information: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const information).cast::<c_void>(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .expect("job information size fits u32"),
            ) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            if AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(handle);
                return Err(error);
            }
            Ok(Self(handle))
        }
    }
}

#[cfg(windows)]
impl Drop for ChildJob {
    fn drop(&mut self) {
        // SAFETY: The handle is created and exclusively owned by this ChildJob.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Place the child in its own process group on Unix so the whole tree can be
/// signalled at teardown; on Windows, detach console windows so processes run silently.
pub(crate) fn configure_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // CREATE_NO_WINDOW: prevent popping up console windows on desktop.
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = command;
    }
}

#[cfg(not(windows))]
pub(crate) struct ChildJob {
    process_group: i32,
}

#[cfg(not(windows))]
impl ChildJob {
    pub(crate) fn attach(child: &Child) -> std::io::Result<Self> {
        // `configure_process_group` made the child a process-group leader, so
        // its PID doubles as the process-group ID.
        Ok(Self {
            process_group: i32::try_from(child.id())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?,
        })
    }
}

#[cfg(not(windows))]
impl Drop for ChildJob {
    fn drop(&mut self) {
        // SAFETY: kill(2) with a negative PID signals the process group the
        // runner created for this collector child; it never signals our own
        // group because the child was spawned as a group leader.
        unsafe {
            libc::kill(-self.process_group, libc::SIGKILL);
        }
    }
}

pub(crate) fn sanitize_protocol_message(message: &str) -> String {
    let sanitized: String = message
        .chars()
        .take(160)
        .filter(|ch| !ch.is_control())
        .collect();
    if sanitized.is_empty() {
        "protocol error".into()
    } else {
        sanitized
    }
}
