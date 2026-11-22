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

    // Two legitimate timeout outcomes race here, and which one wins is pure
    // scheduler jitter: the caller's `recv_timeout` deadline and the worker
    // thread's own 50ms Unix stream timeout both start at the same instant, and
    // `platform::cancel_request` is a no-op on Unix, so neither can pre-empt the
    // other. Whichever fires first decides the rendered error
    // (`ipc_response_timeout` from the caller side, `ipc_read_failed` from the
    // platform side). Both mean the same thing -- the call gave up instead of
    // blocking on the 350ms handler. On a loaded runner (several CI jobs sharing
    // the cores) the platform side wins often enough that the previous
    // single-string assertion flaked; the promptness assertion below is what
    // actually pins the behaviour under test.
    let rendered = error.to_string();
    assert!(
        rendered.contains("ipc_response_timeout") || rendered.contains("ipc_read_failed"),
        "expected a timeout error, got: {rendered}"
    );
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
