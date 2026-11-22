use super::*;

/// How often the stream sends a comment frame when nothing changed.
///
/// A comment costs a few bytes and keeps NATs and proxies from dropping an
/// idle connection; it is never a reason for the page to re-read anything.
const CHANGE_STREAM_KEEPALIVE: Duration = Duration::from_secs(15);

/// Answers `/api/events`: one `text/event-stream` per open console.
///
/// The frame carries only a version, never a payload: the page answers it by
/// re-reading `/api/dashboard` under its ETag, so the stream can never show
/// state the query endpoints would refuse, and an unchanged snapshot still
/// costs a 304 rather than a full payload.
pub(crate) fn serve_change_stream(
    stream: &mut TcpStream,
    last_event_id: Option<&str>,
    stopped: &AtomicBool,
) -> anyhow::Result<()> {
    let subscription = crate::live::subscribe();
    // A page that reconnects with Last-Event-ID reports what it already has;
    // anything newer than that is worth one immediate notification.
    let resume_from = last_event_id
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nCache-Control: no-store\r\n{SECURITY_HEADERS}Connection: keep-alive\r\nX-Accel-Buffering: no\r\n\r\n"
        )
        .as_bytes(),
    )?;
    // `retry` tells the browser how long to wait before reconnecting; one
    // second is the same cadence the poll it replaces ran at.
    stream.write_all(b"retry: 1000\n\n")?;
    stream.write_all(
        format!(
            "event: ready\ndata: {{\"version\":{}}}\n\n",
            subscription.current
        )
        .as_bytes(),
    )?;
    if subscription.current > resume_from {
        stream.write_all(
            format!(
                "id: {}\nevent: change\ndata: {{\"version\":{}}}\n\n",
                subscription.current, subscription.current
            )
            .as_bytes(),
        )?;
    }
    stream.flush()?;

    // This connection is meant to sit idle between changes, and the 10 s
    // timeout every other response is written under would close it between
    // two quiet scans.
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    loop {
        if stopped.load(Ordering::Relaxed) {
            return Ok(());
        }
        match subscription.receiver.recv_timeout(CHANGE_STREAM_KEEPALIVE) {
            Ok(version) => {
                if stream
                    .write_all(
                        format!(
                            "id: {version}\nevent: change\ndata: {{\"version\":{version}}}\n\n"
                        )
                        .as_bytes(),
                    )
                    .is_err()
                {
                    return Ok(());
                }
                stream.flush()?;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if stream.write_all(b": keepalive\n\n").is_err() {
                    return Ok(());
                }
                stream.flush()?;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}
