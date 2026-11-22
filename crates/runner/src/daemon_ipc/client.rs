use super::*;

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
