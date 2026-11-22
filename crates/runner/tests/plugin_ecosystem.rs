use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use agenthub_runner::{
    plugins,
    remote_host::{MultiProviderTransport, NativeTransport},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn build_mock_adapter_script() -> Vec<u8> {
    br#"import readline from "node:readline";
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
function out(obj) {
  process.stdout.write(JSON.stringify(obj) + "\n");
}
rl.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed.startsWith("{")) return;
  const msg = JSON.parse(trimmed);
  const { id, method, params = {} } = msg;
  if (method === "initialize") {
    out({ jsonrpc: "2.0", id, result: { protocolVersion: "1.0", agentInfo: { name: "demo-acp", version: "1.0.0" } } });
  } else if (method === "agenthub/probe") {
    out({ jsonrpc: "2.0", id, result: { available: true, binary_path: "node", version: "1.0.0" } });
  } else if (method === "agenthub/discover") {
    out({
      jsonrpc: "2.0",
      id,
      result: {
        sessions: [
          {
            id: "sess_1",
            cwd: "/tmp/demo",
            title: "Demo Plugin Session",
            model: "demo-model-1",
            last_activity_at_ms: 1700000000000,
          },
        ],
      },
    });
  } else if (method === "agenthub/models") {
    out({
      jsonrpc: "2.0",
      id,
      result: {
        models: [
          { id: "demo-model-1", name: "demo-model-1", display_name: "Demo Model 1" },
        ],
      },
    });
  } else if (method === "agenthub/history") {
    out({
      jsonrpc: "2.0",
      id,
      result: {
        items: [
          { id: "h1", type: "userMessage", text: "hi plugin" },
          { id: "h2", type: "agentMessage", text: "hello from demo plugin" },
        ],
      },
    });
  } else if (method === "session/new") {
    out({ jsonrpc: "2.0", id, result: { sessionId: "created_1" } });
  } else if (method === "session/load") {
    out({ jsonrpc: "2.0", id, result: { loaded: true } });
  } else if (method === "session/prompt") {
    const rawId = String(params.sessionId || "").replace(/^demo_/, "");
    out({
      jsonrpc: "2.0",
      method: "session/update",
      params: {
        sessionId: `demo_${rawId}`,
        update: {
          sessionUpdate: "agent_message_chunk",
          content: { type: "text", text: "streamed e2e reply" },
        },
      },
    });
    out({ jsonrpc: "2.0", id, result: { stopReason: "end_turn" } });
  } else if (method === "session/cancel") {
    if (id !== undefined) out({ jsonrpc: "2.0", id, result: { ok: true } });
  } else if (id !== undefined) {
    out({ jsonrpc: "2.0", id, error: { code: -32601, message: "Method not found" } });
  }
});
"#
    .to_vec()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn add_plugin_routes(
    routes: &mut HashMap<String, Vec<u8>>,
    plugin_id: &str,
    prefix: &str,
    tamper_adapter: bool,
) -> Value {
    let manifest = json!({
        "schema_version": 1,
        "id": plugin_id,
        "name": format!("Name {plugin_id}"),
        "display_name": format!("Display {plugin_id}"),
        "version": "1.2.3",
        "category": "agent",
        "description": format!("ACP plugin {plugin_id}"),
        "session_prefix": prefix,
        "icon": "icon.svg",
        "aliases": [format!("{plugin_id}_alias")],
        "runtime": {
            "protocol": "acp",
            "command": "node",
            "args": ["adapter.mjs"]
        },
        "capabilities": {
            "discover": true,
            "launch": true,
            "resume": true,
            "models": true,
            "history": true,
            "approvals": false
        }
    });
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    let adapter_bytes = build_mock_adapter_script();
    let icon_bytes =
        br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6"/></svg>"#
            .to_vec();

    let expected_adapter_sha = sha256_hex(&adapter_bytes);
    let expected_adapter_size = adapter_bytes.len();
    let served_adapter = if tamper_adapter {
        let mut altered = adapter_bytes.clone();
        altered[0] = if altered[0] == b'i' { b' ' } else { b'i' };
        altered
    } else {
        adapter_bytes
    };

    routes.insert(
        format!("/plugins/{plugin_id}/plugin.json"),
        manifest_bytes.clone(),
    );
    routes.insert(format!("/plugins/{plugin_id}/adapter.mjs"), served_adapter);
    routes.insert(format!("/plugins/{plugin_id}/icon.svg"), icon_bytes.clone());

    json!({
        "id": plugin_id,
        "name": format!("Name {plugin_id}"),
        "display_name": format!("Display {plugin_id}"),
        "version": "1.2.3",
        "category": "agent",
        "description": format!("ACP plugin {plugin_id}"),
        "session_prefix": prefix,
        "icon": "icon.svg",
        "aliases": [format!("{plugin_id}_alias")],
        "files": [
            {
                "path": "plugin.json",
                "url": format!("plugins/{plugin_id}/plugin.json"),
                "sha256": sha256_hex(&manifest_bytes),
                "size": manifest_bytes.len(),
            },
            {
                "path": "adapter.mjs",
                "url": format!("plugins/{plugin_id}/adapter.mjs"),
                "sha256": expected_adapter_sha,
                "size": expected_adapter_size,
            },
            {
                "path": "icon.svg",
                "url": format!("plugins/{plugin_id}/icon.svg"),
                "sha256": sha256_hex(&icon_bytes),
                "size": icon_bytes.len(),
            }
        ]
    })
}

fn spawn_mock_http_registry(
    routes: HashMap<String, Vec<u8>>,
) -> (String, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock registry http server");
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);

    let handle = thread::spawn(move || {
        while !stop_flag.load(Ordering::Acquire) {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            if stop_flag.load(Ordering::Acquire) {
                break;
            }
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            if let Some(body) = routes.get(path) {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
            } else {
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
            let _ = stream.flush();
        }
    });

    (format!("http://{addr}"), stop, handle)
}

#[test]
fn runner_plugin_ecosystem_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("runner-plugins");
    let workspace_dir = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace_dir).unwrap();

    // Must be set before first access to plugins::global_manager().
    unsafe {
        std::env::set_var("AGENTHUB_PLUGINS_DIR", &plugins_dir);
    }

    let mut routes = HashMap::new();
    let demo_entry = add_plugin_routes(&mut routes, "demo_acp", "demo_", false);
    let tampered_entry = add_plugin_routes(&mut routes, "tampered_acp", "tamp_", true);
    let registry_doc = json!({
        "schema_version": 1,
        "plugins": [demo_entry, tampered_entry]
    });
    routes.insert(
        "/registry.json".to_string(),
        serde_json::to_vec_pretty(&registry_doc).unwrap(),
    );

    let (base_url, stop, server_thread) = spawn_mock_http_registry(routes);
    let registry_url = format!("{base_url}/registry.json");

    // 1. Configure remote HTTP registry URL.
    let saved_url = plugins::set_registry_url(Some(&registry_url)).unwrap();
    assert_eq!(saved_url, registry_url);

    // 2. Tampered file fails SHA-256 verification and is not installed.
    let tamper_err = plugins::install_plugin("tampered_acp", None, true).unwrap_err();
    assert!(
        tamper_err.contains("plugin_sha256_mismatch"),
        "expected plugin_sha256_mismatch, got: {tamper_err}"
    );

    // 3. Valid plugin downloads over HTTP, passes SHA-256, persists state, and probes ACP runtime.
    let installed = plugins::install_plugin("demo_acp", None, true).unwrap();
    assert_eq!(installed.id, "demo_acp");
    assert!(installed.enabled);

    let (views, active_url) = plugins::list_local_plugins_view();
    assert_eq!(active_url, registry_url);
    let demo_view = views
        .iter()
        .find(|v| v.id == "demo_acp")
        .expect("demo_acp listed in local views");
    assert!(demo_view.installed);
    assert!(demo_view.enabled);
    assert_eq!(demo_view.runtime_status, "ready");
    assert_eq!(demo_view.available, Some(true));

    // 4. Discovery, models, and history over ACP JSON-RPC 2.0 + agenthub/* extensions.
    let sessions = plugins::discover_all_plugin_sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], "demo_sess_1");
    assert_eq!(sessions[0]["provider"], "demo_acp");

    let models = plugins::collect_plugin_models();
    assert!(
        models.iter().any(|(agent, list)| {
            agent == "demo_acp" && list.iter().any(|m| m["id"] == "demo-model-1")
        }),
        "expected demo-model-1 under demo_acp, got: {models:?}"
    );

    let history = plugins::load_plugin_conversation("demo_acp", "demo_sess_1", None);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["role"], "user");
    assert_eq!(history[1]["role"], "assistant");
    assert_eq!(history[1]["text"], "hello from demo plugin");

    // 5. MultiProviderTransport routes DeepSeek native contract vs dynamic ACP plugin sessions.
    let mut transport = MultiProviderTransport::new(None);
    let cwd = workspace_dir.to_string_lossy().to_string();

    let ds_err = transport
        .request(
            "thread/start",
            json!({ "agentType": "deepseek_harness", "cwd": cwd }),
        )
        .unwrap_err();
    assert_eq!(ds_err.code, "deepseek_harness_runtime_not_implemented");

    let started = transport
        .request(
            "thread/start",
            json!({ "agentType": "demo_acp", "cwd": cwd }),
        )
        .unwrap();
    let thread_id = started["thread"]["id"].as_str().unwrap().to_string();
    assert!(
        thread_id.starts_with("demo_"),
        "unexpected thread id: {thread_id}"
    );
    assert_eq!(started["thread"]["provider"], "demo_acp");

    let turn_res = transport
        .request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "cwd": cwd,
                "input": [{ "type": "text", "text": "hello e2e" }]
            }),
        )
        .unwrap();
    let expected_turn_id = turn_res["turn"]["id"].as_str().unwrap().to_string();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_delta = false;
    let mut saw_completed = false;
    while Instant::now() < deadline && !(saw_delta && saw_completed) {
        let msgs = transport.drain().unwrap();
        for msg in msgs {
            if msg.get("method").and_then(Value::as_str) == Some("item/agentMessage/delta")
                && msg["params"]["delta"] == "streamed e2e reply"
            {
                assert_eq!(msg["params"]["turnId"], expected_turn_id);
                saw_delta = true;
            }
            if msg.get("method").and_then(Value::as_str) == Some("turn/completed") {
                saw_completed = true;
            }
        }
        if !(saw_delta && saw_completed) {
            thread::sleep(Duration::from_millis(25));
        }
    }
    assert!(saw_delta, "did not observe streamed ACP delta");
    assert!(saw_completed, "did not observe ACP turn/completed");

    // 6. Disabling stops discovery, tears down active ACP servers on drain, and rejects new thread/start.
    plugins::toggle_plugin("demo_acp", Some(false)).unwrap();
    let _ = transport.drain().unwrap();
    assert!(plugins::discover_all_plugin_sessions().is_empty());
    let disabled_err = transport
        .request(
            "thread/start",
            json!({ "agentType": "demo_acp", "cwd": cwd }),
        )
        .unwrap_err();
    assert_eq!(disabled_err.code, "plugin_disabled");

    // 7. Uninstalling removes files from disk and marks not installed in registry view.
    plugins::uninstall_plugin("demo_acp").unwrap();
    let (after_views, _) = plugins::list_local_plugins_view();
    let after_demo = after_views
        .iter()
        .find(|v| v.id == "demo_acp")
        .expect("demo_acp still visible in cached registry view");
    assert!(!after_demo.installed);
    assert!(!plugins_dir.join("installed").join("demo_acp").exists());

    stop.store(true, Ordering::Release);
    let _ = TcpStream::connect(base_url.trim_start_matches("http://"));
    let _ = server_thread.join();
}
