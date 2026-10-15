use agenthub_runner::{
    device::DeviceIdentity,
    remote_journal::RemoteJournal,
    remote_network::{RemoteControl, run_with_control},
};
use std::{
    net::TcpListener,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires AGENTHUB_TEST_CODEX_BINARY pointing to a working native Codex"]
fn desktop_owner_can_stop_native_host_without_a_process_signal() {
    let binary = PathBuf::from(
        std::env::var_os("AGENTHUB_TEST_CODEX_BINARY").expect("native binary required"),
    );
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("remote.db");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = format!("ws://{}/v3/remote/ws", listener.local_addr().unwrap());
    let control = Arc::new(RemoteControl::default());
    let worker_control = control.clone();
    let workspace = dir.path().to_path_buf();
    let database = db.clone();
    let worker = thread::spawn(move || {
        run_with_control(
            binary,
            vec![workspace],
            database,
            DeviceIdentity::generate(),
            server,
            worker_control,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    // The network starts only after native initialize, model/list and discovery.
    let socket = loop {
        if let Ok((socket, _)) = listener.accept() {
            break socket;
        }
        assert!(!worker.is_finished(), "native startup failed");
        assert!(Instant::now() < deadline, "native startup timed out");
        thread::sleep(Duration::from_millis(25));
    };
    assert!(
        !control.connected.load(Ordering::Relaxed),
        "TCP alone is not a signed connection"
    );
    control.stopped.store(true, Ordering::Relaxed);
    drop(socket);
    while !worker.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    assert!(worker.is_finished(), "embedded shutdown timed out");
    worker.join().unwrap().unwrap();
    assert!(!control.connected.load(Ordering::Relaxed));
    // Shutdown has released the exclusive journal and its native process owner.
    assert!(RemoteJournal::open(db).is_ok());
}
