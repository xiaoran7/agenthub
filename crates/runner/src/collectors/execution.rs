use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use super::*;

/// Resume an existing local Agent session and return only the child process'
/// bounded stdout. Claude/Codex prompts go through stdin; Grok Build uses a
/// temporary `--prompt-file` so the prompt never appears in the process list.
/// Callers must keep the returned content local; unlike normal collection, this
/// function intentionally handles conversation content after an explicit user
/// action.
pub fn run_agent_session_turn(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: Option<&Path>,
    session_id: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String, CollectorError> {
    run_agent_session_turn_streaming(
        agent,
        binary,
        project_path,
        session_id,
        prompt,
        TurnOptions::default(),
        timeout,
        &AtomicBool::new(false),
        |_| {},
    )
    .map(|outcome| outcome.response)
}

/// Resume an Agent session while forwarding safe, user-visible progress.
/// Conversation text is never logged or persisted by this function. The
/// caller owns the callback destination and cancellation flag.
///
/// Resumes an existing agent conversation and streams the turn's updates.
///
/// When the chosen binary reports that it cannot read the session, the turn is
/// retried against the remaining local builds of the same CLI. A recorded
/// session-to-binary affinity is tried first, so the second attempt is usually
/// the binary that already worked for this session.
///
/// The retry is only safe because an unreadable-session failure happens before
/// the model is ever called: no prompt was delivered, so no other binary can
/// observe a partially executed turn. The affinity map records which binary
/// worked so later resumes of the same session skip the failed candidate.
#[allow(clippy::too_many_arguments)]
/// The callback is `Clone` because a retried attempt needs a second copy: the
/// first is moved into the failed process's reader thread. Callers already
/// build these as `Arc`-based handles, so cloning is a refcount bump.
pub fn run_agent_session_turn_streaming(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    project_path: Option<&Path>,
    session_id: &str,
    prompt: &str,
    options: TurnOptions<'_>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + Clone + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    let preferred = binary.as_ref();
    // Both attempts get their own copy: whichever runs first moves its copy
    // into that process's reader thread, so a retry still has one to hand.
    let first_attempt = on_update.clone();
    let retry_attempt = on_update.clone();
    match run_agent_session_turn_streaming_once(
        agent.clone(),
        preferred,
        project_path,
        session_id,
        prompt,
        options.clone(),
        timeout,
        cancelled,
        first_attempt,
    ) {
        Ok(outcome) => Ok(outcome),
        Err(CollectorError::SessionUnreadable { program }) => {
            let Some(candidate) = retry_with_alternate_binary(agent.clone(), preferred, session_id)
            else {
                return Err(CollectorError::SessionUnreadable { program });
            };
            match run_agent_session_turn_streaming_once(
                agent,
                &candidate,
                project_path,
                session_id,
                prompt,
                options.clone(),
                timeout,
                cancelled,
                retry_attempt,
            ) {
                Ok(outcome) => Ok(outcome),
                // A second unreadable-session failure means no local build can
                // read this transcript; the original error is the honest one
                // to report.
                Err(_) => Err(CollectorError::SessionUnreadable { program }),
            }
        }
        Err(other) => Err(other),
    }
}

/// Picks the binary to retry a failed resume with: a previously-recorded
/// working binary for this session, else the best remaining candidate.
///
/// Returns `None` when discovery offers nothing beyond the one that just
/// failed, so a single-install machine reports the error instead of paying for
/// a pointless second process.
fn retry_with_alternate_binary(
    agent: crate::domain::AgentType,
    failed: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    let (candidates, prefix) = match agent {
        crate::domain::AgentType::ClaudeCode => (claude_binary_candidates(), "claude"),
        crate::domain::AgentType::Antigravity => (antigravity_binary_candidates(), "antigravity"),
        _ => return None,
    };
    let remaining: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| candidate.path != failed)
        .collect();
    if remaining.is_empty() {
        return None;
    }
    let session_key = format!("{prefix}:{session_id}");
    let mut affinity = crate::agent_binary::BinaryAffinity::load();
    if let Some(known) = affinity.lookup(&session_key, &remaining) {
        return Some(known);
    }
    let best = remaining[0].path.clone();
    affinity.record(&session_key, &best);
    Some(best)
}

#[allow(clippy::too_many_arguments)]
fn run_agent_session_turn_streaming_once(
    agent: crate::domain::AgentType,
    binary: &Path,
    project_path: Option<&Path>,
    session_id: &str,
    prompt: &str,
    options: TurnOptions<'_>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    let program = binary.display().to_string();
    if !session_id.is_empty() && !validate_safe_session_id(session_id) {
        return Err(CollectorError::Protocol {
            program,
            code: "invalid_session_id".to_owned(),
            message: "session id contains disallowed characters".to_owned(),
        });
    }
    let mut command = command_for_binary(binary);
    if is_cmd_shim_path(command.get_program().as_ref())
        && has_cmd_metacharacters(prompt)
        && matches!(agent, crate::domain::AgentType::Antigravity)
    {
        return Err(CollectorError::Protocol {
            program,
            code: "unsafe_cmd_shim_argument".to_owned(),
            message: "prompt contains characters unsafe for Windows .cmd wrapper".to_owned(),
        });
    }
    let prompt_file: Option<tempfile::NamedTempFile> = match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            let session_exists = match options.session_exists {
                Some(exists) => exists,
                None => {
                    !session_id.is_empty()
                        && find_claude_session_info(session_id)
                            .ok()
                            .flatten()
                            .is_some()
                }
            };
            if session_exists {
                command.args([
                    "--resume",
                    session_id,
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                ]);
            } else {
                command.args([
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                ]);
            }
            None
        }
        crate::domain::AgentType::Codex => {
            command.args([
                "exec",
                "resume",
                session_id,
                "--json",
                "--color",
                "never",
                "--skip-git-repo-check",
                "-",
            ]);
            None
        }
        crate::domain::AgentType::Antigravity => {
            let session_exists = options.session_exists.unwrap_or_else(|| {
                !session_id.is_empty()
                    && find_antigravity_session_info(session_id)
                        .ok()
                        .flatten()
                        .is_some()
            });
            if session_exists {
                command.args([
                    "--conversation",
                    session_id,
                    "--prompt",
                    prompt,
                    "--output-format",
                    "stream-json",
                ]);
            } else {
                command.args(["--prompt", prompt, "--output-format", "stream-json"]);
            }
            None
        }
        crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => {
            return Err(CollectorError::Protocol {
                program,
                code: "unsupported_execution_agent".to_owned(),
                message: format!(
                    "agent {} does not use one-shot CLI execution",
                    agent.as_str()
                ),
            });
        }
    };
    let mut effective_model = options.model.map(str::to_string);
    match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            if effective_model.is_none() && options.session_exists != Some(false) {
                effective_model = find_claude_session_model(session_id);
            }
            if let Some(gateway) =
                crate::claude_gateway::discover_claude_gateway_for_client(options.claude_desktop)
            {
                gateway.inject_into_command_with_session(
                    &mut command,
                    &mut effective_model,
                    Some(session_id),
                );
            }
        }
        crate::domain::AgentType::Codex => {
            if let Some(gateway) = crate::claude_gateway::discover_codex_gateway() {
                gateway.inject_into_command_with_session(
                    &mut command,
                    &mut effective_model,
                    Some(session_id),
                );
            }
        }
        crate::domain::AgentType::Antigravity => {
            if effective_model.is_none() && options.session_exists != Some(false) {
                effective_model = find_antigravity_session_model(session_id);
            }
            if let Some(gateway) = crate::claude_gateway::discover_antigravity_gateway() {
                gateway.inject_into_command_with_session(
                    &mut command,
                    &mut effective_model,
                    Some(session_id),
                );
            }
        }
        _ => {}
    }
    let effective_model_ref = effective_model.as_deref();
    let effective_options = TurnOptions {
        model: effective_model_ref,
        ..options
    };
    apply_turn_options(&mut command, &agent, &effective_options);
    if let Some(project_path) = project_path {
        command.current_dir(project_path);
    }
    run_agent_stream_process(
        agent,
        command,
        program,
        prompt,
        prompt_file,
        timeout,
        cancelled,
        on_update,
    )
}

/// Starts a brand-new agent conversation in `cwd` and streams the first turn.
/// The session id is recovered from the CLI's stream output so the panel can
/// switch to the real conversation immediately.
#[allow(clippy::too_many_arguments)] // Legacy one-shot API; retained until client migration.
pub fn run_agent_session_start_streaming(
    agent: crate::domain::AgentType,
    binary: impl AsRef<Path>,
    cwd: &Path,
    prompt: &str,
    options: TurnOptions<'_>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    let binary = binary.as_ref();
    let program = binary.display().to_string();
    let mut command = command_for_binary(binary);
    let still_cmd_shim = is_cmd_shim_path(command.get_program().as_ref());
    if still_cmd_shim
        && has_cmd_metacharacters(prompt)
        && matches!(agent, crate::domain::AgentType::Antigravity)
    {
        return Err(CollectorError::Protocol {
            program,
            code: "unsafe_cmd_shim_argument".to_owned(),
            message: "prompt contains characters unsafe for Windows .cmd wrapper".to_owned(),
        });
    }
    let prompt_file: Option<tempfile::NamedTempFile> = match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            command.args([
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ]);
            None
        }
        crate::domain::AgentType::Antigravity => {
            command.args(["--prompt", prompt, "--output-format", "stream-json"]);
            None
        }
        crate::domain::AgentType::Codex => {
            command.args([
                "exec",
                "--json",
                "--color",
                "never",
                "--skip-git-repo-check",
                "-",
            ]);
            None
        }
        crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => {
            return Err(CollectorError::Protocol {
                program,
                code: "unsupported_execution_agent".to_owned(),
                message: format!(
                    "agent {} does not use one-shot CLI execution",
                    agent.as_str()
                ),
            });
        }
    };
    let mut effective_model = options.model.map(str::to_string);
    let trimmed_prompt = prompt.trim();
    let implicit_sid = if !trimmed_prompt.is_empty() {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(trimmed_prompt.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        Some(format!("magpie-{}", &hash[..12]))
    } else {
        None
    };

    match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            if let Some(gateway) =
                crate::claude_gateway::discover_claude_gateway_for_client(options.claude_desktop)
            {
                gateway.inject_into_command_with_session(
                    &mut command,
                    &mut effective_model,
                    implicit_sid.as_deref(),
                );
            }
        }
        crate::domain::AgentType::Codex => {
            if let Some(gateway) = crate::claude_gateway::discover_codex_gateway() {
                gateway.inject_into_command_with_session(
                    &mut command,
                    &mut effective_model,
                    implicit_sid.as_deref(),
                );
            }
        }
        crate::domain::AgentType::Antigravity => {
            if let Some(gateway) = crate::claude_gateway::discover_antigravity_gateway() {
                gateway.inject_into_command_with_session(
                    &mut command,
                    &mut effective_model,
                    implicit_sid.as_deref(),
                );
            }
        }
        _ => {}
    }
    let effective_model_ref = effective_model.as_deref();
    let effective_options = TurnOptions {
        model: effective_model_ref,
        ..options
    };
    apply_turn_options(&mut command, &agent, &effective_options);
    command.current_dir(cwd);
    run_agent_stream_process(
        agent,
        command,
        program,
        prompt,
        prompt_file,
        timeout,
        cancelled,
        on_update,
    )
}

pub(crate) fn apply_turn_options(
    command: &mut Command,
    agent: &crate::domain::AgentType,
    options: &TurnOptions<'_>,
) {
    let model = options
        .model
        .map(str::trim)
        .filter(|value| is_safe_model_id(value));
    let permission_mode = options.permission_mode.and_then(sanitize_permission_mode);
    let effort = options.reasoning_effort.and_then(sanitize_effort);
    // The reasoning flag differs per CLI, so it is applied even when no model
    // override was picked: effort alone is a valid per-turn override.
    if let Some(effort) = effort {
        match agent {
            crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
                command.args(["--effort", effort]);
            }
            // Codex has no --effort flag; reasoning effort is a config key.
            crate::domain::AgentType::Codex => {
                command.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
            }
            crate::domain::AgentType::Antigravity => {
                command.args(["--effort", effort]);
            }
            crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => {}
        }
    }
    match agent {
        crate::domain::AgentType::ClaudeCode | crate::domain::AgentType::ClaudeDesktop => {
            if let Some(model) = model {
                command.args(["--model", model]);
            }
            if let Some(mode) = permission_mode {
                command.args(["--permission-mode", mode]);
            }
        }
        crate::domain::AgentType::Codex => {
            if let Some(model) = model {
                command.args(["-m", model]);
            }
            if let Some(mode) = permission_mode {
                let sandbox = match mode {
                    "read-only" | "plan" => "read-only",
                    "bypassPermissions" | "danger-full-access" | "yolo" => "danger-full-access",
                    _ => "workspace-write",
                };
                command.args(["--sandbox", sandbox]);
            }
        }
        crate::domain::AgentType::Antigravity => {
            if let Some(model) = model {
                command.args(["--model", model]);
            }
            if let Some(mode) = permission_mode {
                for flag in
                    crate::collectors::antigravity::process::map_antigravity_permission_args(mode)
                {
                    command.arg(flag);
                }
            }
        }
        crate::domain::AgentType::DeepseekHarness | crate::domain::AgentType::Plugin(_) => {}
    }
}

#[allow(clippy::too_many_arguments)] // Shared by the legacy CLI wrappers.
fn run_agent_stream_process(
    agent: crate::domain::AgentType,
    mut command: Command,
    program: String,
    prompt: &str,
    prompt_file: Option<tempfile::NamedTempFile>,
    timeout: Duration,
    cancelled: &AtomicBool,
    on_update: impl FnMut(AgentTurnUpdate) + Send + 'static,
) -> Result<AgentStreamOutcome, CollectorError> {
    let write_prompt_to_stdin =
        prompt_file.is_none() && !matches!(agent, crate::domain::AgentType::Antigravity);
    command
        .stdin(if write_prompt_to_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn().map_err(|source| CollectorError::Start {
        program: program.clone(),
        source,
    })?;
    let mut job = Some(ChildJob::attach(&child).map_err(|source| {
        let _ = child.kill();
        let _ = child.wait();
        CollectorError::Io {
            program: program.clone(),
            source,
        }
    })?);
    let stdin = if write_prompt_to_stdin {
        match child.stdin.take() {
            Some(stdin) => Some(stdin),
            None => {
                drop(job.take());
                let _ = child.kill();
                let _ = child.wait();
                return Err(CollectorError::MissingStdio {
                    program: program.clone(),
                });
            }
        }
    } else {
        drop(child.stdin.take());
        None
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            drop(stdin);
            drop(job.take());
            let _ = child.kill();
            let _ = child.wait();
            return Err(CollectorError::MissingStdio {
                program: program.clone(),
            });
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            drop(stdin);
            drop(job.take());
            let _ = child.kill();
            let _ = child.wait();
            return Err(CollectorError::MissingStdio {
                program: program.clone(),
            });
        }
    };
    let stdout_reader = read_agent_turn_output(stdout, agent.clone(), on_update);
    let stderr_reader = read_child_output(stderr);
    let stdin_writer = stdin.map(|mut stdin| {
        let payload = prompt.as_bytes().to_vec();
        thread::spawn(move || -> std::io::Result<()> {
            if let Err(err) = stdin
                .write_all(&payload)
                .and_then(|_| stdin.write_all(b"\n"))
                && err.kind() != std::io::ErrorKind::BrokenPipe
            {
                return Err(err);
            }
            drop(stdin);
            Ok(())
        })
    });
    let status =
        wait_for_child_with_cancel(&mut child, &mut job, &program, timeout, Some(cancelled));
    drop(job.take());
    let stdin_res = match stdin_writer {
        Some(handle) => match handle.join() {
            Ok(res) => res.map_err(|source| CollectorError::Io {
                program: program.clone(),
                source,
            }),
            Err(_) => Err(CollectorError::ReaderStopped {
                program: program.clone(),
            }),
        },
        None => Ok(()),
    };
    let stdout_res = join_child_output(stdout_reader, &program);
    let stderr_res = join_child_output(stderr_reader, &program);
    drop(prompt_file);
    let status = status?;
    let stdout = stdout_res?;
    let stderr_bytes = stderr_res?;
    if !status.success() {
        // Distinguish "this build cannot read that transcript" from a generic
        // non-zero exit before discarding stdout. Only the former is worth
        // retrying against a different binary; replaying a turn that may have
        // partially executed is exactly the duplicate side effect the remote
        // command contract forbids.
        if let Ok(output) = String::from_utf8(stdout) {
            if crate::agent_binary::is_unreadable_session_failure(&program, &output) {
                return Err(CollectorError::SessionUnreadable { program });
            }
            // Propagate explicit API errors (e.g. rate limit / quota exhaustion / auth failures)
            // as a protocol failure so the turn ends cleanly with an explicit error banner.
            if let Some(err_msg) = final_agent_message(&agent, &output) {
                let scrubbed = crate::privacy::redact_paths_inside_text(&err_msg);
                return Err(CollectorError::Protocol {
                    program,
                    code: "provider_turn_error".to_owned(),
                    message: scrubbed,
                });
            }
        }
        let stderr_summary = String::from_utf8_lossy(&stderr_bytes);
        let scrubbed_stderr = crate::privacy::redact_paths_inside_text(stderr_summary.trim());
        let bounded_stderr: String = scrubbed_stderr.chars().take(512).collect();
        let detail = if bounded_stderr.is_empty() {
            status.to_string()
        } else {
            format!("{status}: {bounded_stderr}")
        };
        return Err(CollectorError::Exit {
            program,
            status: detail,
        });
    }
    stdin_res?;
    if stdout.len() > MAX_AGENT_TURN_OUTPUT_BYTES {
        return Err(CollectorError::OutputTooLarge {
            program,
            limit: MAX_AGENT_TURN_OUTPUT_BYTES,
        });
    }
    let output = String::from_utf8(stdout).map_err(|source| CollectorError::InvalidUtf8 {
        program: program.clone(),
        source,
    })?;
    let response =
        final_agent_message(&agent, &output).unwrap_or_else(|| "（任务执行完成）".to_owned());
    let session_id = extract_session_id_from_output(&agent, &output);
    Ok(AgentStreamOutcome {
        response,
        session_id,
        usage: extract_context_usage_from_output(&agent, &output),
    })
}

pub(crate) fn read_child_output(
    mut stream: impl Read + Send + 'static,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut output = Vec::with_capacity(MAX_AGENT_TURN_OUTPUT_BYTES + 1);
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            let read = stream.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            let remaining = (MAX_AGENT_TURN_OUTPUT_BYTES + 1).saturating_sub(output.len());
            if remaining > 0 {
                output.extend_from_slice(&chunk[..read.min(remaining)]);
            }
        }
        Ok(output)
    })
}

fn join_child_output(
    reader: JoinHandle<std::io::Result<Vec<u8>>>,
    program: &str,
) -> Result<Vec<u8>, CollectorError> {
    reader
        .join()
        .map_err(|_| CollectorError::ReaderStopped {
            program: program.to_owned(),
        })?
        .map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })
}

fn wait_for_child_with_cancel(
    child: &mut Child,
    job: &mut Option<ChildJob>,
    program: &str,
    timeout: Duration,
    cancelled: Option<&AtomicBool>,
) -> Result<std::process::ExitStatus, CollectorError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|source| CollectorError::Io {
            program: program.to_owned(),
            source,
        })? {
            return Ok(status);
        }
        if cancelled.is_some_and(|cancelled| cancelled.load(Ordering::Relaxed)) {
            drop(job.take());
            let _ = child.kill();
            let _ = child.wait();
            return Err(CollectorError::Cancelled {
                program: program.to_owned(),
            });
        }
        if Instant::now() >= deadline {
            drop(job.take());
            let _ = child.kill();
            let _ = child.wait();
            return Err(CollectorError::Timeout {
                program: program.to_owned(),
                timeout_ms: timeout.as_millis(),
            });
        }
        thread::sleep(CHILD_WAIT_INTERVAL);
    }
}

pub(crate) enum ProtocolRead {
    Message(Value),
    InvalidJson(serde_json::Error),
    LineTooLarge,
    Io(std::io::Error),
    Eof,
}
