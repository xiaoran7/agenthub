use super::*;

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

pub(crate) fn serve_connection<S>(
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
