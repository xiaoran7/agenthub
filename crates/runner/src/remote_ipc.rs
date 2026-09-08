//! Bounded JSONL control surface for the native Host. Owned by the local Runner.
//! stdout is an explicit content channel; diagnostic stderr never contains payloads.

use crate::{
    collectors::CodexAppServer,
    remote_host::{NativeSessionHost, RemoteCommand},
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const MAX_INPUT_BYTES: usize = 1024 * 1024;

pub fn run(binary: PathBuf, workspaces: Vec<PathBuf>) -> Result<()> {
    let server = CodexAppServer::start_interactive(binary, Duration::from_secs(20))
        .map_err(|_| anyhow::anyhow!("native_host_start_failed"))?;
    let mut host =
        NativeSessionHost::new(server, workspaces).map_err(|error| anyhow::anyhow!(error.code))?;
    let stopped = Arc::new(AtomicBool::new(false));
    let stop_signal = stopped.clone();
    ctrlc::set_handler(move || stop_signal.store(true, Ordering::Relaxed))?;
    let (sender, receiver) = mpsc::sync_channel(32);
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            let mut bytes = Vec::new();
            let result = io::Read::take(&mut reader, (MAX_INPUT_BYTES + 1) as u64)
                .read_until(b'\n', &mut bytes);
            match result {
                Ok(0) => break,
                Ok(_) if bytes.len() > MAX_INPUT_BYTES => {
                    let _ = sender.send(Err("input_too_large"));
                    break;
                }
                Ok(_) => {
                    let parsed = serde_json::from_slice::<RemoteCommand>(&bytes)
                        .map_err(|_| "invalid_command");
                    if sender.send(parsed).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = sender.send(Err("input_read_failed"));
                    break;
                }
            }
        }
    });
    let stdout = io::stdout();
    let mut output = stdout.lock();
    emit(
        &mut output,
        json!({"type": "ready", "ipc_version": 1, "provider": "codex_app_server",
        "host_generation": host.generation(), "attach_running": false,
        "durable_recovery": false}),
    )?;
    // Probe reports provider data, never fabricated installed/model values.
    match host.models() {
        Ok(models) => emit(
            &mut output,
            json!({"type": "models", "source": "model/list", "result": models}),
        )?,
        Err(error) => emit(
            &mut output,
            json!({"type": "capability_error", "code": error.code}),
        )?,
    }
    while !stopped.load(Ordering::Relaxed) {
        // Consume native completions before accepting the next input command.
        match host.poll() {
            Ok(events) => {
                for event in events {
                    emit(&mut output, json!({"type": "event", "event": event}))?;
                }
            }
            Err(error) => {
                emit(
                    &mut output,
                    json!({"type": "host_lost", "code": error.code, "sessions": host.sessions()}),
                )?;
                bail!("native_host_lost");
            }
        }
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(Ok(command)) => {
                let receipt = host.handle_command(command);
                emit(&mut output, json!({"type": "receipt", "receipt": receipt}))?;
                if !host.is_alive() {
                    emit(
                        &mut output,
                        json!({"type": "host_lost", "sessions": host.sessions()}),
                    )?;
                    bail!("native_host_lost");
                }
            }
            Ok(Err(code)) => emit(&mut output, json!({"type": "input_error", "code": code}))?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn emit(output: &mut impl Write, value: Value) -> Result<()> {
    serde_json::to_writer(&mut *output, &value).context("ipc_write_failed")?;
    output.write_all(b"\n").context("ipc_write_failed")?;
    output.flush().context("ipc_write_failed")
}
