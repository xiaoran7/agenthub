use agent_pulse_runner::{
    daemon_ipc,
    daemon_protocol::{
        DAEMON_PROTOCOL_VERSION, DaemonRequest, DaemonRequestEnvelope, DaemonResponse,
        DaemonResponseEnvelope,
    },
    device::DeviceIdentity,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn profile_path(data_dir: &std::path::Path, device_id: &str) -> PathBuf {
    let scope = format!(
        "{:x}",
        Sha256::digest(format!("agenthub_native_host_v1\n{device_id}").as_bytes())
    );
    data_dir.join(format!("remote-{}.json", &scope[..16]))
}

#[test]
fn hidden_daemon_failure_emits_only_safe_code_in_isolated_data_dir() {
    let dir = tempfile::tempdir().unwrap();
    let identity = DeviceIdentity::generate();
    identity
        .save_new(dir.path().join("device-identity.json"))
        .unwrap();
    fs::write(
        profile_path(dir.path(), identity.device_id()),
        serde_json::to_vec(&serde_json::json!({
            "workspace_paths": [dir.path()],
            "codex_binary": dir.path().join("SECRET-nonexistent-provider.exe"),
            "remote_enabled": false
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-pulse-desktop"))
        .arg("--agenthubd")
        .arg(dir.path())
        .env("HOME", dir.path())
        .env("USERPROFILE", dir.path())
        .env("CODEX_HOME", dir.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let diagnostic: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        diagnostic,
        serde_json::json!({"code": "native_host_start_failed"})
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SECRET"));
}

#[test]
#[ignore = "requires AGENTHUB_TEST_CODEX_BINARY pointing to a working native Codex"]
fn hidden_agenthubd_mode_starts_the_process_external_host() {
    let codex_binary = PathBuf::from(
        std::env::var_os("AGENTHUB_TEST_CODEX_BINARY").expect("native Codex binary required"),
    );
    assert!(codex_binary.is_file());
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let identity = DeviceIdentity::generate();
    identity
        .save_new(dir.path().join("device-identity.json"))
        .unwrap();
    fs::write(
        profile_path(dir.path(), identity.device_id()),
        serde_json::to_vec(&serde_json::json!({
            "workspace_paths": [workspace],
            "codex_binary": codex_binary,
            "remote_enabled": false
        }))
        .unwrap(),
    )
    .unwrap();

    let endpoint = daemon_ipc::default_endpoint(identity.device_id());
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_agent-pulse-desktop"))
            .arg("--agenthubd")
            .arg(dir.path())
            .env("HOME", dir.path())
            .env("USERPROFILE", dir.path())
            .env("CODEX_HOME", dir.path())
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        assert_eq!(
            child.0.try_wait().unwrap(),
            None,
            "hidden daemon process stopped during startup"
        );
        let response = daemon_ipc::request(
            &endpoint,
            &DaemonRequestEnvelope {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                request_id: "desktop-hidden-status".into(),
                request: DaemonRequest::GetStatus,
            },
        );
        if let Ok(DaemonResponseEnvelope {
            response: DaemonResponse::Status { workspaces, .. },
            ..
        }) = response
        {
            break workspaces;
        }
        assert!(
            Instant::now() < deadline,
            "hidden daemon process startup timed out"
        );
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(status, vec![dunce::canonicalize(&workspace).unwrap()]);

    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert!(
        daemon_ipc::request(
            &endpoint,
            &DaemonRequestEnvelope {
                protocol_version: DAEMON_PROTOCOL_VERSION,
                request_id: "desktop-hidden-stopped".into(),
                request: DaemonRequest::GetStatus,
            }
        )
        .is_err()
    );
}
