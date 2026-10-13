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

pub(crate) const METADATA_MAX_LINE_BYTES: usize = 64 * 1024;
pub(crate) const LONG_LINE_HEAD_BYTES: usize = 64 * 1024;
pub(crate) const LONG_LINE_TAIL_BYTES: usize = 8 * 1024;

pub(crate) enum SplicedLine {
    Line {
        text: String,
        bytes_consumed: u64,
    },
    Oversized {
        head: String,
        tail: String,
        bytes_consumed: u64,
    },
    Eof,
    Io(std::io::Error),
}

/// Reads one newline-terminated JSONL record while capping retained memory.
/// If the line fits within `METADATA_MAX_LINE_BYTES`, returns the full `Line`.
/// If the line exceeds `METADATA_MAX_LINE_BYTES` (for example a base64 pasted image
/// or huge tool output), drains until the newline while retaining only the first
/// `LONG_LINE_HEAD_BYTES` (64 KiB) and the last `LONG_LINE_TAIL_BYTES` (8 KiB).
pub(crate) fn read_spliced_line(reader: &mut impl BufRead) -> SplicedLine {
    let mut line = Vec::new();
    let mut tail = Vec::new();
    let mut dropping_long = false;
    let mut bytes_consumed = 0_u64;

    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return SplicedLine::Io(error),
        };
        if buffer.is_empty() {
            if bytes_consumed == 0 {
                return SplicedLine::Eof;
            }
            return finish_spliced(line, tail, dropping_long, bytes_consumed);
        }
        let (take, found_newline) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(position) => (position + 1, true),
            None => (buffer.len(), false),
        };
        let chunk = &buffer[..take];
        bytes_consumed = bytes_consumed.saturating_add(take as u64);

        if dropping_long {
            keep_tail_window(&mut tail, chunk);
        } else if line.len() + take > METADATA_MAX_LINE_BYTES {
            let head_need = LONG_LINE_HEAD_BYTES.saturating_sub(line.len());
            if head_need > 0 {
                line.extend_from_slice(&chunk[..head_need.min(chunk.len())]);
            }
            let combined_len = line.len();
            if combined_len > 0 {
                let start = combined_len.saturating_sub(LONG_LINE_TAIL_BYTES);
                tail.extend_from_slice(&line[start..]);
            }
            if chunk.len() > head_need {
                keep_tail_window(&mut tail, &chunk[head_need..]);
            }
            dropping_long = true;
        } else {
            line.extend_from_slice(chunk);
        }

        reader.consume(take);
        if found_newline {
            return finish_spliced(line, tail, dropping_long, bytes_consumed);
        }
    }
}

fn keep_tail_window(tail: &mut Vec<u8>, incoming: &[u8]) {
    if incoming.is_empty() {
        return;
    }
    if incoming.len() >= LONG_LINE_TAIL_BYTES {
        tail.clear();
        tail.extend_from_slice(&incoming[incoming.len() - LONG_LINE_TAIL_BYTES..]);
        return;
    }
    let total = tail.len() + incoming.len();
    if total > LONG_LINE_TAIL_BYTES {
        let drop_count = total - LONG_LINE_TAIL_BYTES;
        tail.drain(..drop_count);
    }
    tail.extend_from_slice(incoming);
}

fn finish_spliced(
    line: Vec<u8>,
    tail: Vec<u8>,
    dropping_long: bool,
    bytes_consumed: u64,
) -> SplicedLine {
    if !dropping_long {
        return SplicedLine::Line {
            text: String::from_utf8_lossy(&line).into_owned(),
            bytes_consumed,
        };
    }
    SplicedLine::Oversized {
        head: String::from_utf8_lossy(&line).into_owned(),
        tail: String::from_utf8_lossy(&tail).into_owned(),
        bytes_consumed,
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
