#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let mut args = std::env::args_os();
    let _executable = args.next();
    if args.next().as_deref() == Some(std::ffi::OsStr::new("--agenthubd")) {
        let data_dir = args
            .next()
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        if let Err(error) = agent_pulse_desktop_lib::run_daemon_process(data_dir) {
            // Dedicated structured startup channel. Ignore a closed parent pipe:
            // the background process is independent of the Desktop lifetime.
            use std::io::Write;
            let code = agent_pulse_desktop_lib::safe_daemon_code(&error);
            let _ = writeln!(std::io::stdout(), "{{\"code\":\"{code}\"}}");
            std::process::exit(1);
        }
        return;
    }
    agent_pulse_desktop_lib::run();
}
