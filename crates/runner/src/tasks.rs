use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    collectors::{CollectorError, run_agent_task},
    device::DeviceIdentity,
    domain::AgentType,
    gateway::{GatewayClient, GatewayError},
};

const TASK_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskExecutionPolicy {
    ObserveOnly,
    LocalConfirmation,
    TrustedExecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteTaskStatus {
    Queued,
    Delivered,
    Accepted,
    Running,
    WaitingUser,
    Succeeded,
    Failed,
    Canceled,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteTask {
    pub task_id: String,
    pub target_device_id: String,
    pub agent_type: AgentType,
    pub prompt: String,
    pub project_fingerprint: Option<String>,
    pub idempotency_key: String,
    pub execution_policy: TaskExecutionPolicy,
    pub status: RemoteTaskStatus,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub agent_session_id: Option<String>,
    pub result_summary: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceTaskClaim {
    protocol_version: u16,
    device_id: String,
    public_key: String,
    requested_at: DateTime<Utc>,
    signature: String,
}

#[derive(Serialize)]
struct UnsignedDeviceTaskClaim<'a> {
    protocol_version: u16,
    device_id: &'a str,
    public_key: &'a str,
    requested_at: DateTime<Utc>,
}

impl DeviceTaskClaim {
    pub fn signed(identity: &DeviceIdentity) -> Result<Self, GatewayError> {
        let public_key = identity.public_key_hex();
        let requested_at = Utc::now();
        let unsigned = UnsignedDeviceTaskClaim {
            protocol_version: TASK_PROTOCOL_VERSION,
            device_id: identity.device_id(),
            public_key: &public_key,
            requested_at,
        };
        let bytes = serde_json::to_vec(&unsigned).map_err(|_| GatewayError::InvalidResponse)?;
        Ok(Self {
            protocol_version: TASK_PROTOCOL_VERSION,
            device_id: identity.device_id().to_owned(),
            public_key,
            requested_at,
            signature: hex::encode(identity.sign(&bytes).to_bytes()),
        })
    }
}

#[derive(Debug, Serialize)]
pub struct DeviceTaskUpdate {
    protocol_version: u16,
    pub task_id: String,
    device_id: String,
    public_key: String,
    expected_version: u32,
    status: RemoteTaskStatus,
    updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    signature: String,
}

#[derive(Serialize)]
struct UnsignedDeviceTaskUpdate<'a> {
    protocol_version: u16,
    task_id: &'a str,
    device_id: &'a str,
    public_key: &'a str,
    expected_version: u32,
    status: RemoteTaskStatus,
    updated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_session_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_summary: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<&'a str>,
}

impl DeviceTaskUpdate {
    pub fn signed(
        identity: &DeviceIdentity,
        task: &RemoteTask,
        status: RemoteTaskStatus,
        error_code: Option<&str>,
    ) -> Result<Self, GatewayError> {
        let public_key = identity.public_key_hex();
        let updated_at = Utc::now();
        let unsigned = UnsignedDeviceTaskUpdate {
            protocol_version: TASK_PROTOCOL_VERSION,
            task_id: &task.task_id,
            device_id: identity.device_id(),
            public_key: &public_key,
            expected_version: task.version,
            status,
            updated_at,
            agent_session_id: None,
            result_summary: None,
            error_code,
        };
        let bytes = serde_json::to_vec(&unsigned).map_err(|_| GatewayError::InvalidResponse)?;
        Ok(Self {
            protocol_version: TASK_PROTOCOL_VERSION,
            task_id: task.task_id.clone(),
            device_id: identity.device_id().to_owned(),
            public_key,
            expected_version: task.version,
            status,
            updated_at,
            agent_session_id: None,
            result_summary: None,
            error_code: error_code.map(str::to_owned),
            signature: hex::encode(identity.sign(&bytes).to_bytes()),
        })
    }
}

#[derive(Debug, Error)]
pub enum TaskWorkerError {
    #[error(transparent)]
    Gateway(#[from] GatewayError),
    #[error("failed to read local confirmation")]
    ConfirmationIo,
}

pub struct TaskWorker<'a> {
    client: &'a GatewayClient,
    identity: &'a DeviceIdentity,
    projects: BTreeMap<String, PathBuf>,
    claude_binary: PathBuf,
    codex_binary: PathBuf,
    execution_timeout: Duration,
}

impl<'a> TaskWorker<'a> {
    pub fn new(
        client: &'a GatewayClient,
        identity: &'a DeviceIdentity,
        projects: BTreeMap<String, PathBuf>,
        claude_binary: PathBuf,
        codex_binary: PathBuf,
        execution_timeout: Duration,
    ) -> Self {
        Self {
            client,
            identity,
            projects,
            claude_binary,
            codex_binary,
            execution_timeout,
        }
    }

    pub fn poll_once(
        &self,
        input: &mut impl BufRead,
        output: &mut impl Write,
    ) -> Result<bool, TaskWorkerError> {
        let Some(task) = self.client.claim_task(self.identity)? else {
            return Ok(false);
        };
        self.process(task, input, output)?;
        Ok(true)
    }

    fn process(
        &self,
        task: RemoteTask,
        input: &mut impl BufRead,
        output: &mut impl Write,
    ) -> Result<(), TaskWorkerError> {
        if task.status == RemoteTaskStatus::Running {
            self.transition(
                &task,
                RemoteTaskStatus::Failed,
                Some("execution_interrupted"),
            )?;
            return Ok(());
        }
        let task = if task.status == RemoteTaskStatus::Delivered {
            self.transition(&task, RemoteTaskStatus::Accepted, None)?
        } else {
            task
        };
        if task.execution_policy != TaskExecutionPolicy::LocalConfirmation {
            self.transition(
                &task,
                RemoteTaskStatus::Failed,
                Some("execution_policy_denied"),
            )?;
            return Ok(());
        }
        let Some(fingerprint) = task.project_fingerprint.as_ref() else {
            self.transition(&task, RemoteTaskStatus::Failed, Some("project_not_allowed"))?;
            return Ok(());
        };
        let Some(project_path) = self.projects.get(fingerprint) else {
            self.transition(&task, RemoteTaskStatus::Failed, Some("project_not_allowed"))?;
            return Ok(());
        };
        if !project_path.is_dir() {
            self.transition(&task, RemoteTaskStatus::Failed, Some("project_unavailable"))?;
            return Ok(());
        }
        let waiting = if task.status == RemoteTaskStatus::WaitingUser {
            task
        } else {
            self.transition(&task, RemoteTaskStatus::WaitingUser, None)?
        };
        writeln!(output, "\nAgentPulse remote task {}", waiting.task_id)
            .map_err(|_| TaskWorkerError::ConfirmationIo)?;
        writeln!(output, "Agent: {}", waiting.agent_type.as_str())
            .map_err(|_| TaskWorkerError::ConfirmationIo)?;
        writeln!(output, "Project: {}", project_path.display())
            .map_err(|_| TaskWorkerError::ConfirmationIo)?;
        writeln!(output, "Prompt:\n{}", waiting.prompt)
            .map_err(|_| TaskWorkerError::ConfirmationIo)?;
        write!(output, "Execute locally? [y/N] ")
            .and_then(|_| output.flush())
            .map_err(|_| TaskWorkerError::ConfirmationIo)?;
        let mut answer = String::new();
        input
            .read_line(&mut answer)
            .map_err(|_| TaskWorkerError::ConfirmationIo)?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            self.transition(&waiting, RemoteTaskStatus::Canceled, None)?;
            return Ok(());
        }
        let running = self.transition(&waiting, RemoteTaskStatus::Running, None)?;
        let binary: &Path = match running.agent_type {
            AgentType::ClaudeCode => &self.claude_binary,
            AgentType::Codex => &self.codex_binary,
        };
        match run_agent_task(
            running.agent_type,
            binary,
            project_path,
            &running.prompt,
            self.execution_timeout,
        ) {
            Ok(()) => {
                self.transition(&running, RemoteTaskStatus::Succeeded, None)?;
            }
            Err(error) => {
                self.transition(&running, RemoteTaskStatus::Failed, Some(error.code()))?;
            }
        }
        Ok(())
    }

    fn transition(
        &self,
        task: &RemoteTask,
        status: RemoteTaskStatus,
        error_code: Option<&str>,
    ) -> Result<RemoteTask, GatewayError> {
        let update = DeviceTaskUpdate::signed(self.identity, task, status, error_code)?;
        self.client.update_task(&update)
    }
}

impl TaskWorkerError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Gateway(error) => error.code(),
            Self::ConfirmationIo => "task_confirmation_io_failed",
        }
    }
}

pub fn parse_project_mapping(value: &str) -> Result<(String, PathBuf), &'static str> {
    let (fingerprint, path) = value.split_once('=').ok_or("expected fingerprint=path")?;
    if fingerprint.len() != 64
        || !fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || path.is_empty()
    {
        return Err("expected lowercase 64-character fingerprint and non-empty path");
    }
    Ok((fingerprint.to_owned(), PathBuf::from(path)))
}

pub fn collector_error_code(error: &CollectorError) -> &'static str {
    error.code()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Cursor, Read},
        net::{TcpListener, TcpStream},
        sync::{Arc, Mutex},
        thread::{self, JoinHandle},
    };

    use chrono::Duration as ChronoDuration;
    use serde_json::Value;

    use super::*;

    const PROJECT_FINGERPRINT: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn task_worker_completes_claim_confirmation_execution_and_status_loop() {
        let identity = DeviceIdentity::generate();
        let task = remote_task(&identity, RemoteTaskStatus::Delivered, 2);
        let expected = [
            RemoteTaskStatus::Accepted,
            RemoteTaskStatus::WaitingUser,
            RemoteTaskStatus::Running,
            RemoteTaskStatus::Succeeded,
        ];
        let (server, updates, server_thread) =
            spawn_task_gateway(task, identity.device_id(), &expected);
        let client = GatewayClient::new(&server, Duration::from_secs(2)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let arguments_path = directory.path().join("arguments.txt");
        let prompt_path = directory.path().join("prompt.txt");
        let binary = recording_script(&directory, &arguments_path, &prompt_path);
        let worker = task_worker(&client, &identity, directory.path(), &binary);
        let mut input = Cursor::new(b"yes\n");
        let mut output = Vec::new();

        assert!(worker.poll_once(&mut input, &mut output).unwrap());
        server_thread.join().unwrap();

        assert_eq!(*updates.lock().unwrap(), expected);
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("private task prompt")
        );
        assert!(
            !fs::read_to_string(arguments_path)
                .unwrap()
                .contains("private task prompt")
        );
        assert_eq!(
            fs::read_to_string(prompt_path).unwrap().trim_end(),
            "private task prompt"
        );
    }

    #[test]
    fn task_worker_reports_local_rejection_without_starting_the_cli() {
        let identity = DeviceIdentity::generate();
        let task = remote_task(&identity, RemoteTaskStatus::Delivered, 2);
        let expected = [
            RemoteTaskStatus::Accepted,
            RemoteTaskStatus::WaitingUser,
            RemoteTaskStatus::Canceled,
        ];
        let (server, updates, server_thread) =
            spawn_task_gateway(task, identity.device_id(), &expected);
        let client = GatewayClient::new(&server, Duration::from_secs(2)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let missing_binary = directory.path().join("must-not-run");
        let worker = task_worker(&client, &identity, directory.path(), &missing_binary);
        let mut input = Cursor::new(b"no\n");
        let mut output = Vec::new();

        assert!(worker.poll_once(&mut input, &mut output).unwrap());
        server_thread.join().unwrap();

        assert_eq!(*updates.lock().unwrap(), expected);
        assert!(!missing_binary.exists());
    }

    #[test]
    fn task_worker_fails_a_recovered_running_task_without_reexecution() {
        let identity = DeviceIdentity::generate();
        let task = remote_task(&identity, RemoteTaskStatus::Running, 5);
        let expected = [RemoteTaskStatus::Failed];
        let (server, updates, server_thread) =
            spawn_task_gateway(task, identity.device_id(), &expected);
        let client = GatewayClient::new(&server, Duration::from_secs(2)).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let missing_binary = directory.path().join("must-not-run");
        let worker = task_worker(&client, &identity, directory.path(), &missing_binary);
        let mut input = Cursor::new(Vec::<u8>::new());
        let mut output = Vec::new();

        assert!(worker.poll_once(&mut input, &mut output).unwrap());
        server_thread.join().unwrap();

        assert_eq!(*updates.lock().unwrap(), expected);
        assert!(!missing_binary.exists());
    }

    #[test]
    fn claim_rejects_a_task_addressed_to_another_device() {
        let identity = DeviceIdentity::generate();
        let other = DeviceIdentity::generate();
        let task = remote_task(&other, RemoteTaskStatus::Delivered, 2);
        let (server, _, server_thread) = spawn_task_gateway(task, identity.device_id(), &[]);
        let client = GatewayClient::new(&server, Duration::from_secs(2)).unwrap();

        assert!(matches!(
            client.claim_task(&identity),
            Err(GatewayError::DeviceMismatch)
        ));
        server_thread.join().unwrap();
    }

    fn task_worker<'a>(
        client: &'a GatewayClient,
        identity: &'a DeviceIdentity,
        project_path: &Path,
        binary: &Path,
    ) -> TaskWorker<'a> {
        TaskWorker::new(
            client,
            identity,
            BTreeMap::from([(PROJECT_FINGERPRINT.to_owned(), project_path.to_owned())]),
            binary.to_owned(),
            binary.to_owned(),
            Duration::from_secs(2),
        )
    }

    fn remote_task(
        identity: &DeviceIdentity,
        status: RemoteTaskStatus,
        version: u32,
    ) -> RemoteTask {
        let now = Utc::now();
        RemoteTask {
            task_id: "task_test_1".into(),
            target_device_id: identity.device_id().to_owned(),
            agent_type: AgentType::Codex,
            prompt: "private task prompt".into(),
            project_fingerprint: Some(PROJECT_FINGERPRINT.into()),
            idempotency_key: "task-test-1".into(),
            execution_policy: TaskExecutionPolicy::LocalConfirmation,
            status,
            version,
            created_at: now,
            expires_at: now + ChronoDuration::minutes(5),
            updated_at: now,
            delivered_at: Some(now),
            started_at: (status == RemoteTaskStatus::Running).then_some(now),
            completed_at: None,
            agent_session_id: None,
            result_summary: None,
            error_code: None,
        }
    }

    fn spawn_task_gateway(
        initial_task: RemoteTask,
        expected_claim_device_id: &str,
        expected_statuses: &[RemoteTaskStatus],
    ) -> (String, Arc<Mutex<Vec<RemoteTaskStatus>>>, JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let expected_claim_device_id = expected_claim_device_id.to_owned();
        let expected_statuses = expected_statuses.to_vec();
        let updates = Arc::new(Mutex::new(Vec::new()));
        let recorded_updates = Arc::clone(&updates);
        let server_thread = thread::spawn(move || {
            let mut task = initial_task;
            let (mut stream, _) = listener.accept().unwrap();
            let (path, claim) = read_json_request(&mut stream);
            assert_eq!(path, "/v1/device-tasks/claim");
            assert_eq!(claim["device_id"], expected_claim_device_id);
            assert_eq!(claim["protocol_version"], TASK_PROTOCOL_VERSION);
            assert_eq!(claim["signature"].as_str().unwrap().len(), 128);
            respond_json(&mut stream, &task);

            for expected_status in expected_statuses {
                let (mut stream, _) = listener.accept().unwrap();
                let (path, update) = read_json_request(&mut stream);
                assert_eq!(path, format!("/v1/device-tasks/{}/status", task.task_id));
                assert_eq!(update["device_id"], task.target_device_id);
                assert_eq!(update["expected_version"], task.version);
                assert_eq!(
                    update["status"],
                    serde_json::to_value(expected_status).unwrap()
                );
                if expected_status == RemoteTaskStatus::Failed
                    && task.status == RemoteTaskStatus::Running
                {
                    assert_eq!(update["error_code"], "execution_interrupted");
                }
                task.status = expected_status;
                task.version += 1;
                task.updated_at = Utc::now();
                recorded_updates.lock().unwrap().push(expected_status);
                respond_json(&mut stream, &task);
            }
        });
        (format!("http://{address}"), updates, server_thread)
    }

    fn read_json_request(stream: &mut TcpStream) -> (String, Value) {
        let mut request = Vec::new();
        let body_start = loop {
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if let Some(position) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&request[..body_start]).unwrap();
        let path = headers
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .to_owned();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while request.len() < body_start + content_length {
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        (
            path,
            serde_json::from_slice(&request[body_start..body_start + content_length]).unwrap(),
        )
    }

    fn respond_json(stream: &mut TcpStream, value: &impl Serialize) {
        let response = serde_json::to_vec(value).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        )
        .unwrap();
        stream.write_all(&response).unwrap();
        stream.flush().unwrap();
    }

    fn recording_script(
        directory: &tempfile::TempDir,
        arguments_path: &Path,
        input_path: &Path,
    ) -> PathBuf {
        #[cfg(windows)]
        {
            let path = directory.path().join("agent.cmd");
            fs::write(
                &path,
                format!(
                    "@echo off\r\necho %* > \"{}\"\r\nmore > \"{}\"\r\n",
                    arguments_path.display(),
                    input_path.display(),
                ),
            )
            .unwrap();
            path
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let path = directory.path().join("agent");
            fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s' \"$*\" > '{}'\ncat > '{}'\n",
                    arguments_path.display(),
                    input_path.display(),
                ),
            )
            .unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&path, permissions).unwrap();
            path
        }
    }
}
