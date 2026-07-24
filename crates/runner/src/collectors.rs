use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};
use thiserror::Error;

const MAX_PROTOCOL_LINE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum CollectorError {
    #[error("failed to start {program}: {source}")]
    Start {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} exited unsuccessfully with status {status}")]
    Exit { program: String, status: String },
    #[error("failed to communicate with {program}: {source}")]
    Io {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} did not expose the expected stdio handles")]
    MissingStdio { program: String },
    #[error("invalid JSON from {program}: {source}")]
    InvalidJson {
        program: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{program} returned output that was not UTF-8: {source}")]
    InvalidUtf8 {
        program: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("{program} protocol line exceeded {limit} bytes")]
    ProtocolLineTooLarge { program: String, limit: usize },
    #[error("{program} closed stdout before responding to request {request_id}")]
    UnexpectedEof { program: String, request_id: u64 },
    #[error("{program} returned protocol error {code}: {message}")]
    Protocol {
        program: String,
        code: String,
        message: String,
    },
}

pub fn collect_claude_agents(
    binary: impl AsRef<Path>,
    include_completed: bool,
) -> Result<String, CollectorError> {
    let binary = binary.as_ref();
    let mut command = Command::new(binary);
    command.args(["agents", "--json"]);
    if include_completed {
        command.arg("--all");
    }
    command.stdin(Stdio::null());
    command.stderr(Stdio::null());

    let output = command.output().map_err(|source| CollectorError::Start {
        program: binary.display().to_string(),
        source,
    })?;
    if !output.status.success() {
        return Err(CollectorError::Exit {
            program: binary.display().to_string(),
            status: output.status.to_string(),
        });
    }
    String::from_utf8(output.stdout).map_err(|source| CollectorError::InvalidUtf8 {
        program: binary.display().to_string(),
        source,
    })
}

pub struct CodexAppServer {
    program: String,
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_request_id: u64,
}

impl CodexAppServer {
    pub fn start(binary: impl AsRef<Path>) -> Result<Self, CollectorError> {
        let binary = binary.as_ref();
        let program = binary.display().to_string();
        let mut child = Command::new(binary)
            .args(["app-server", "--listen", "stdio://"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| CollectorError::Start {
                program: program.clone(),
                source,
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CollectorError::MissingStdio {
                program: program.clone(),
            })?;
        let mut server = Self {
            program,
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            next_request_id: 1,
        };
        server.initialize()?;
        Ok(server)
    }

    fn initialize(&mut self) -> Result<(), CollectorError> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "agent_platform",
                    "title": "Agent-Platform Runner",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": false,
                    "optOutNotificationMethods": [
                        "item/agentMessage/delta",
                        "item/reasoning/textDelta",
                        "item/reasoning/summaryTextDelta"
                    ]
                }
            }),
        )?;
        self.send(&json!({"method": "initialized", "params": {}}))?;
        Ok(())
    }

    pub fn list_threads(&mut self, limit: u32) -> Result<Value, CollectorError> {
        self.request(
            "thread/list",
            json!({
                "cursor": null,
                "limit": limit,
                "sortKey": "recency_at",
                "sortDirection": "desc",
                "archived": false,
                "useStateDbOnly": false,
                "sourceKinds": [
                    "cli",
                    "vscode",
                    "exec",
                    "appServer",
                    "subAgent",
                    "subAgentReview",
                    "subAgentCompact",
                    "subAgentThreadSpawn",
                    "subAgentOther",
                    "unknown"
                ]
            }),
        )
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, CollectorError> {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        self.send(&json!({
            "method": method,
            "id": request_id,
            "params": params
        }))?;

        loop {
            let message = self.read_message(request_id)?;
            if message.get("id").and_then(Value::as_u64) != Some(request_id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(CollectorError::Protocol {
                    program: self.program.clone(),
                    code: error
                        .get("code")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "unknown".into()),
                    message: sanitize_protocol_message(
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("error"),
                    ),
                });
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), CollectorError> {
        serde_json::to_writer(&mut self.stdin, message).map_err(|source| {
            CollectorError::InvalidJson {
                program: self.program.clone(),
                source,
            }
        })?;
        self.stdin
            .write_all(b"\n")
            .and_then(|_| self.stdin.flush())
            .map_err(|source| CollectorError::Io {
                program: self.program.clone(),
                source,
            })
    }

    fn read_message(&mut self, request_id: u64) -> Result<Value, CollectorError> {
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .map_err(|source| CollectorError::Io {
                program: self.program.clone(),
                source,
            })?;
        if read == 0 {
            return Err(CollectorError::UnexpectedEof {
                program: self.program.clone(),
                request_id,
            });
        }
        if line.len() > MAX_PROTOCOL_LINE_BYTES {
            return Err(CollectorError::ProtocolLineTooLarge {
                program: self.program.clone(),
                limit: MAX_PROTOCOL_LINE_BYTES,
            });
        }
        serde_json::from_str(&line).map_err(|source| CollectorError::InvalidJson {
            program: self.program.clone(),
            source,
        })
    }
}

impl Drop for CodexAppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn sanitize_protocol_message(message: &str) -> String {
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

#[derive(Debug, Clone)]
pub struct CollectorBinaries {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

impl Default for CollectorBinaries {
    fn default() -> Self {
        Self {
            claude: PathBuf::from("claude"),
            codex: PathBuf::from("codex"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_protocol_error_messages() {
        let input = "bad\0message\nwith control";
        assert_eq!(sanitize_protocol_message(input), "badmessagewith control");
    }
}
