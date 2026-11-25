use super::*;

pub trait DaemonRequestHandler: Send + Sync + 'static {
    fn handle(&self, request: DaemonRequest) -> DaemonResponse;
}

pub(crate) trait IpcStream: Read + Write + Send {}
impl<T: Read + Write + Send> IpcStream for T {}

pub struct DaemonSubscription {
    pub(crate) stream: Box<dyn IpcStream>,
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

#[cfg(unix)]
pub(crate) fn ensure_private_unix_dir(dir: &std::path::Path, uid: u32) -> bool {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    if let Ok(meta) = std::fs::symlink_metadata(dir) {
        if !meta.file_type().is_dir() || meta.file_type().is_symlink() || meta.uid() != uid {
            return false;
        }
    } else if std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .is_err()
    {
        return false;
    }
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return false;
    };
    if !meta.file_type().is_dir() || meta.file_type().is_symlink() || meta.uid() != uid {
        return false;
    }
    if std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).is_err() {
        return false;
    }
    std::fs::symlink_metadata(dir).is_ok_and(|m| {
        m.file_type().is_dir()
            && !m.file_type().is_symlink()
            && m.uid() == uid
            && (m.mode() & 0o077) == 0
    })
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
        let uid = unsafe { libc::geteuid() };
        let file_name = format!("agenthubd-{suffix}.sock");
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
            candidates.push(std::path::PathBuf::from(xdg).join("agenthub"));
        }
        if let Some(home) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
            candidates.push(std::path::PathBuf::from(home).join(".agenthub").join("run"));
        }
        candidates.push(std::path::PathBuf::from(format!("/tmp/agenthub-{uid}")));
        let env_tmp = std::env::temp_dir().join(format!("agenthub-{uid}"));
        if !candidates.contains(&env_tmp) {
            candidates.push(env_tmp);
        }

        for dir in candidates {
            let candidate = dir.join(&file_name);
            if candidate.as_os_str().len() <= 100 && ensure_private_unix_dir(&dir, uid) {
                return candidate.to_string_lossy().into_owned();
            }
        }
        // Fail closed: never return an unverified `/tmp/...` directory that an attacker
        // might pre-create or own. Both `serve` and `connect` reject paths under `/dev/null`.
        format!("/dev/null/agenthubd-{suffix}.sock")
    }
}

pub(crate) fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>> {
    let mut prefix = [0u8; 4];
    let mut read = 0;
    while read < prefix.len() {
        match reader.read(&mut prefix[read..]) {
            Ok(0) if read == 0 => return Ok(None),
            Ok(0) => bail!("ipc_truncated_frame"),
            Ok(count) => read += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            #[cfg(windows)]
            Err(error) if read == 0 && matches!(error.raw_os_error(), Some(109) | Some(232)) => {
                return Ok(None);
            }
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

pub(crate) fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<()> {
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
