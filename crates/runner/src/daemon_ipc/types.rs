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
