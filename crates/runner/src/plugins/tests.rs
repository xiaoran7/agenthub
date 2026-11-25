use std::{ffi::OsString, fs, sync::Mutex};

use sha2::{Digest, Sha256};

use super::*;

static PLUGIN_ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    prev: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let prev = std::env::var_os(key);
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        Self { key, prev }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            match self.prev.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn write_test_plugin_bundle(root: &std::path::Path, plugin_id: &str, prefix: &str) -> String {
    let dir = root.join("repo").join("plugins").join(plugin_id);
    fs::create_dir_all(&dir).unwrap();

    let adapter = r#"
import readline from "node:readline";
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
function out(obj) { process.stdout.write(JSON.stringify(obj) + "\n"); }
rl.on("line", (line) => {
  const t = line.trim();
  if (!t.startsWith("{")) return;
  const msg = JSON.parse(t);
  const { id, method, params = {} } = msg;
  if (method === "initialize") {
    out({ jsonrpc: "2.0", id, result: { protocolVersion: "1.0", agentInfo: { name: "test-acp", version: "1.0.0" } } });
  } else if (method === "agenthub/probe") {
    out({ jsonrpc: "2.0", id, result: { available: true, binary_path: "node", version: "1.0.0" } });
  } else if (method === "agenthub/discover") {
    out({ jsonrpc: "2.0", id, result: { sessions: [{ id: "sess_demo", cwd: "/tmp/demo", title: "Demo Plugin Session", model: "plugin-m1", last_activity_at_ms: 1700000000000 }] } });
  } else if (method === "agenthub/models") {
    out({ jsonrpc: "2.0", id, result: { models: [{ id: "plugin-m1", name: "plugin-m1", display_name: "Plugin M1" }] } });
  } else if (method === "agenthub/history") {
    out({ jsonrpc: "2.0", id, result: { items: [{ id: "h1", type: "userMessage", text: "hi plugin" }, { id: "h2", type: "agentMessage", text: "hello from plugin" }] } });
  } else if (method === "session/new") {
    out({ jsonrpc: "2.0", id, result: { sessionId: "demo_created_1" } });
  } else if (method === "session/prompt") {
    out({ jsonrpc: "2.0", method: "session/update", params: { sessionId: params.sessionId, update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text: "streamed reply" } } } });
    out({ jsonrpc: "2.0", id, result: { stopReason: "end_turn" } });
  } else if (id !== undefined) {
    out({ jsonrpc: "2.0", id, error: { code: -32601, message: "Method not found" } });
  }
});
"#;
    let icon = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6"/></svg>"#;
    let manifest = serde_json::json!({
        "schema_version": 1,
        "id": plugin_id,
        "name": "Test ACP",
        "display_name": "Test ACP Agent",
        "version": "1.2.3",
        "category": "agent",
        "description": "Test ACP plugin",
        "session_prefix": prefix,
        "icon": "icon.svg",
        "aliases": ["test_alias"],
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
    let adapter_bytes = adapter.as_bytes().to_vec();
    let icon_bytes = icon.as_bytes().to_vec();
    fs::write(dir.join("plugin.json"), &manifest_bytes).unwrap();
    fs::write(dir.join("adapter.mjs"), &adapter_bytes).unwrap();
    fs::write(dir.join("icon.svg"), &icon_bytes).unwrap();

    let sha = |b: &[u8]| hex::encode(Sha256::digest(b));
    let registry = serde_json::json!({
        "schema_version": 1,
        "plugins": [{
            "id": plugin_id,
            "name": "Test ACP",
            "display_name": "Test ACP Agent",
            "version": "1.2.3",
            "category": "agent",
            "description": "Test ACP plugin",
            "session_prefix": prefix,
            "icon": "icon.svg",
            "aliases": ["test_alias"],
            "files": [
                { "path": "plugin.json", "url": format!("plugins/{plugin_id}/plugin.json"), "sha256": sha(&manifest_bytes), "size": manifest_bytes.len() },
                { "path": "adapter.mjs", "url": format!("plugins/{plugin_id}/adapter.mjs"), "sha256": sha(&adapter_bytes), "size": adapter_bytes.len() },
                { "path": "icon.svg", "url": format!("plugins/{plugin_id}/icon.svg"), "sha256": sha(&icon_bytes), "size": icon_bytes.len() }
            ]
        }]
    });
    let reg_path = root.join("repo").join("registry.json");
    fs::write(&reg_path, serde_json::to_vec_pretty(&registry).unwrap()).unwrap();
    reg_path.to_string_lossy().to_string()
}

#[test]
fn rejects_native_agent_ids_and_unsafe_paths() {
    for native in [
        "antigravity",
        "codex",
        "claude_desktop",
        "claude_code",
        "deepseek_harness",
        "acp",
    ] {
        assert!(
            validate_plugin_id(native).is_err(),
            "{native} must be reserved"
        );
    }
    assert!(validate_plugin_id("zcode").is_ok());
    assert!(types::validate_relative_path("../escape.js").is_err());
    assert!(types::validate_relative_path("/etc/passwd").is_err());
    assert!(types::validate_relative_path("a\\b.js").is_err());
    assert!(types::validate_relative_path("adapter.mjs").is_ok());
}

#[test]
fn rejects_sha256_tampering_on_install() {
    let dir = tempfile::tempdir().unwrap();
    let reg_path = write_test_plugin_bundle(dir.path(), "tamper_test", "tamp_");
    let adapter_path = dir
        .path()
        .join("repo")
        .join("plugins")
        .join("tamper_test")
        .join("adapter.mjs");
    let mut tampered = fs::read(&adapter_path).unwrap();
    tampered[0] = if tampered[0] == b'/' { b' ' } else { b'/' };
    fs::write(&adapter_path, tampered).unwrap();

    let manager = PluginManager::with_dir(dir.path().join("state"));
    manager.set_registry_url(Some(&reg_path)).unwrap();
    let err = manager
        .install_plugin("tamper_test", None, true)
        .unwrap_err();
    assert!(
        err.contains("plugin_sha256_mismatch"),
        "unexpected error: {err}"
    );
    assert!(manager.list_installed_enabled().is_empty());
}

#[test]
fn installs_verifies_persists_queries_and_uninstalls_live_acp_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let reg_path = write_test_plugin_bundle(dir.path(), "sample_acp", "samp_");
    let state_dir = dir.path().join("runner-plugins");

    let manager = PluginManager::with_dir(state_dir.clone());
    manager.set_registry_url(Some(&reg_path)).unwrap();

    let (before_views, _, err) = manager.list_plugins_view(true);
    assert!(err.is_none(), "unexpected registry err: {err:?}");
    assert_eq!(before_views.len(), 1);
    assert!(!before_views[0].installed);
    assert_eq!(before_views[0].runtime_status, "not_installed");

    let installed = manager.install_plugin("sample_acp", None, true).unwrap();
    assert_eq!(installed.id, "sample_acp");
    assert!(installed.enabled);

    // Persistence across fresh PluginManager instance
    let reloaded = PluginManager::with_dir(state_dir);
    assert!(reloaded.is_plugin_enabled("sample_acp"));
    assert!(reloaded.get_enabled_plugin("test_alias").is_some());
    assert!(
        reloaded
            .find_enabled_plugin_for_session("samp_123")
            .is_some()
    );

    let sessions = reloaded.discover_all_sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["id"], "samp_sess_demo");
    assert_eq!(sessions[0]["provider"], "sample_acp");

    let models = reloaded.collect_all_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].0, "sample_acp");
    assert_eq!(models[0].1[0]["id"], "plugin-m1");

    let history = reloaded.load_conversation("sample_acp", "samp_sess_demo", None);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0]["role"], "user");
    assert_eq!(history[1]["role"], "assistant");
    assert_eq!(history[1]["text"], "hello from plugin");

    let (icon_bytes, icon_mime) = reloaded.read_icon("sample_acp").unwrap();
    assert_eq!(icon_mime, "image/svg+xml");
    assert!(String::from_utf8_lossy(&icon_bytes).contains("<svg"));

    // Disable stops discovery & models
    reloaded.toggle_plugin("sample_acp", Some(false)).unwrap();
    assert!(!reloaded.is_plugin_enabled("sample_acp"));
    assert!(reloaded.discover_all_sessions().is_empty());

    // Uninstall removes from state & disk
    reloaded.uninstall_plugin("sample_acp").unwrap();
    let (after_views, _, _) = reloaded.list_plugins_view(false);
    assert_eq!(after_views.len(), 1);
    assert!(!after_views[0].installed);
}

#[test]
fn validates_plugin_id_bounds_and_session_prefixes() {
    let long_valid = "a".repeat(64);
    let too_long = "a".repeat(65);
    assert!(validate_plugin_id(&long_valid).is_ok());
    assert!(validate_plugin_id(&too_long).is_err());

    let manifest = PluginManifest {
        schema_version: 1,
        id: "zcode".into(),
        name: "ZCode".into(),
        display_name: None,
        version: "1.0.1".into(),
        category: "agent".into(),
        description: String::new(),
        author: None,
        license: None,
        homepage: None,
        min_runner_version: None,
        session_prefix: Some("zcode_".into()),
        session_prefixes: vec!["zcode_".into(), "sess_".into()],
        icon: None,
        aliases: vec![],
        capabilities: PluginCapabilities::default(),
        runtime: PluginRuntimeSpec {
            protocol: "acp".into(),
            command: "node".into(),
            args: vec!["adapter.mjs".into()],
            binary_hints: vec![],
            env: Default::default(),
        },
    };
    assert!(manifest.matches_session_id("zcode_abc"));
    assert!(manifest.matches_session_id("sess_legacy_123"));
    assert!(!manifest.matches_session_id("codex_123"));
}

#[test]
fn rejects_unsafe_runtime_commands_node_flags_env_and_svg_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let plug_dir = dir.path().join("plug");
    fs::create_dir_all(&plug_dir).unwrap();
    fs::write(plug_dir.join("adapter.mjs"), "console.log('hi');\n").unwrap();

    let bash_spec = PluginRuntimeSpec {
        protocol: "acp".into(),
        command: "bash".into(),
        args: vec!["-c".into(), "echo pwn".into()],
        binary_hints: vec![],
        env: Default::default(),
    };
    assert!(installer::validate_runtime_spec(&bash_spec, &plug_dir).is_err());

    let eval_spec = PluginRuntimeSpec {
        protocol: "acp".into(),
        command: "node".into(),
        args: vec!["adapter.mjs".into(), "--require".into(), "evil.js".into()],
        binary_hints: vec![],
        env: Default::default(),
    };
    assert!(installer::validate_runtime_spec(&eval_spec, &plug_dir).is_err());

    let mut env_map = std::collections::BTreeMap::new();
    env_map.insert(
        "NODE_OPTIONS".to_string(),
        "--require ./evil.js".to_string(),
    );
    let env_spec = PluginRuntimeSpec {
        protocol: "acp".into(),
        command: "node".into(),
        args: vec!["adapter.mjs".into()],
        binary_hints: vec![],
        env: env_map,
    };
    assert!(installer::validate_runtime_spec(&env_spec, &plug_dir).is_err());

    let store = PluginStore::new(dir.path().join("store"));
    let xss_svg_dir = dir.path().join("svgplug");
    fs::create_dir_all(&xss_svg_dir).unwrap();
    fs::write(
        xss_svg_dir.join("icon.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"><script>alert(1)</script></svg>"#,
    )
    .unwrap();
    let rec = InstalledPluginRecord {
        id: "svg_xss".into(),
        version: "1.0.0".into(),
        enabled: true,
        install_dir: xss_svg_dir.to_string_lossy().to_string(),
        installed_at: String::new(),
        source_url: None,
        runtime_status: "ready".into(),
        error: None,
        manifest: PluginManifest {
            schema_version: 1,
            id: "svg_xss".into(),
            name: "XSS".into(),
            display_name: None,
            version: "1.0.0".into(),
            category: "agent".into(),
            description: String::new(),
            author: None,
            license: None,
            homepage: None,
            min_runner_version: None,
            session_prefix: None,
            session_prefixes: vec![],
            icon: Some("icon.svg".into()),
            aliases: vec![],
            capabilities: PluginCapabilities::default(),
            runtime: PluginRuntimeSpec {
                protocol: "acp".into(),
                command: "node".into(),
                args: vec!["adapter.mjs".into()],
                binary_hints: vec![],
                env: Default::default(),
            },
        },
    };
    assert!(
        store.read_icon(&rec).is_none(),
        "XSS SVG icon must be rejected"
    );
}

#[test]
fn rejects_remote_ssrf_and_source_url_commands() {
    let _env_lock = PLUGIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _no_insecure = EnvVarGuard::set("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY", None);
    let _no_allowed = EnvVarGuard::set("AGENTHUB_ALLOWED_PLUGIN_REGISTRIES", None);
    assert!(installer::validate_remote_url("file:///etc/passwd", true).is_err());
    assert!(installer::validate_remote_url("http://example.com/registry.json", true).is_err());
    assert!(installer::validate_remote_url("https://127.0.0.1/registry.json", true).is_err());
    assert!(installer::validate_remote_url("https://169.254.169.254/latest", true).is_err());
    assert!(installer::validate_remote_url("https://10.0.0.1/registry.json", true).is_err());
    assert!(
        installer::validate_remote_url("https://user:pass@example.com/registry.json", true)
            .is_err()
    );
    assert!(
        installer::validate_remote_url(
            "https://raw.githubusercontent.com/org/repo/main/registry.json",
            true
        )
        .is_ok()
    );

    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_dir(dir.path().join("state"));
    let src_res = manager.handle_remote_command(&RemotePluginCommand {
        request_id: Some("r1".into()),
        action: "install".into(),
        plugin_id: Some("demo".into()),
        enabled: None,
        source_url: Some("/tmp/local-plugin".into()),
        registry_url: None,
    });
    assert!(!src_res.ok);
    assert_eq!(
        src_res.error.as_deref(),
        Some("remote_source_url_unsupported")
    );

    let ssrf_res = manager.handle_remote_command(&RemotePluginCommand {
        request_id: Some("r2".into()),
        action: "set_registry".into(),
        plugin_id: None,
        enabled: None,
        source_url: None,
        registry_url: Some("https://127.0.0.1:8443/registry.json".into()),
    });
    assert!(!ssrf_res.ok);
}

#[test]
fn local_install_validates_id_and_skips_symlinks_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_dir(dir.path().join("state"));
    let src_alpha = dir.path().join("src_alpha");
    fs::create_dir_all(&src_alpha).unwrap();
    fs::write(src_alpha.join("adapter.mjs"), "console.log('ok');\n").unwrap();
    fs::write(
        src_alpha.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "id": "alpha",
            "name": "Alpha",
            "version": "1.0.0",
            "runtime": { "protocol": "acp", "command": "node", "args": ["adapter.mjs"] }
        }))
        .unwrap(),
    )
    .unwrap();
    let rec = manager
        .install_plugin("alpha", Some(src_alpha.to_str().unwrap()), false)
        .unwrap();
    assert_eq!(rec.version, "1.0.0");

    // Attempt to overwrite/install with mismatched manifest id ("alpha" manifest under "beta" request)
    let src_mismatch = dir.path().join("src_mismatch");
    fs::create_dir_all(&src_mismatch).unwrap();
    fs::write(src_mismatch.join("adapter.mjs"), "console.log('bad');\n").unwrap();
    fs::write(
        src_mismatch.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "id": "alpha",
            "name": "Malicious Alpha",
            "version": "9.9.9",
            "runtime": { "protocol": "acp", "command": "node", "args": ["adapter.mjs"] }
        }))
        .unwrap(),
    )
    .unwrap();
    let err = manager
        .install_plugin("beta", Some(src_mismatch.to_str().unwrap()), false)
        .unwrap_err();
    assert!(err.contains("plugin_id_mismatch"), "unexpected err: {err}");
    let intact = fs::read_to_string(Path::new(&rec.install_dir).join("adapter.mjs")).unwrap();
    assert!(
        intact.contains("ok"),
        "existing alpha install was clobbered"
    );

    #[cfg(unix)]
    {
        let secret = dir.path().join("secret.txt");
        fs::write(&secret, "TOP_SECRET").unwrap();
        let src_sym = dir.path().join("src_sym");
        fs::create_dir_all(&src_sym).unwrap();
        fs::write(src_sym.join("adapter.mjs"), "console.log('gamma');\n").unwrap();
        fs::write(
            src_sym.join("plugin.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "id": "gamma",
                "name": "Gamma",
                "version": "1.0.0",
                "runtime": { "protocol": "acp", "command": "node", "args": ["adapter.mjs"] }
            }))
            .unwrap(),
        )
        .unwrap();
        std::os::unix::fs::symlink(&secret, src_sym.join("leak.txt")).unwrap();
        let gamma = manager
            .install_plugin("gamma", Some(src_sym.to_str().unwrap()), false)
            .unwrap();
        assert!(!Path::new(&gamma.install_dir).join("leak.txt").exists());
    }
}

#[test]
fn uppercase_registry_schemes_normalize_and_fetch_cleanly() {
    let _env_lock = PLUGIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _no_allowed = EnvVarGuard::set("AGENTHUB_ALLOWED_PLUGIN_REGISTRIES", None);
    let _no_insecure = EnvVarGuard::set("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY", None);

    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_dir(dir.path().join("state"));

    let normalized_remote = manager
        .set_remote_registry_url(Some(
            "HTTPS://Raw.GitHubUserContent.com/xiaoran7/agenthub-plugins/main/registry.json",
        ))
        .unwrap();
    assert_eq!(
        normalized_remote,
        "https://raw.githubusercontent.com/xiaoran7/agenthub-plugins/main/registry.json"
    );
    assert!(!installer::is_local_source(&normalized_remote));
    assert!(!installer::is_local_source(
        "HTTPS://EXAMPLE.COM/registry.json"
    ));
    assert!(installer::is_local_source("FILE:///tmp/registry.json"));

    let child = installer::resolve_child_url(
        "HTTPS://Raw.GitHubUserContent.com/xiaoran7/agenthub-plugins/main/registry.json",
        "plugins/demo/plugin.json",
    )
    .unwrap();
    assert_eq!(
        child,
        "https://raw.githubusercontent.com/xiaoran7/agenthub-plugins/main/plugins/demo/plugin.json"
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let body = r#"{"schema_version":1,"plugins":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let _allow_insecure = EnvVarGuard::set("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY", Some("1"));
    let upper_local_http = format!("HTTP://127.0.0.1:{port}/registry.json");
    let saved = manager.set_registry_url(Some(&upper_local_http)).unwrap();
    assert_eq!(saved, format!("http://127.0.0.1:{port}/registry.json"));
    let (doc, fetched_url) = manager.refresh_registry().unwrap();
    assert_eq!(fetched_url, saved);
    assert_eq!(doc.schema_version, 1);
    let _ = handle.join();
}

#[test]
fn plugin_view_exposes_effective_session_prefixes() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_dir(dir.path().join("state"));
    let src = dir.path().join("zcode_src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("adapter.mjs"), "console.log('z');\n").unwrap();
    fs::write(
        src.join("plugin.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "id": "zcode",
            "name": "ZCode",
            "version": "1.0.0",
            "session_prefix": "zcode_",
            "session_prefixes": ["zcode_", "sess_"],
            "runtime": { "protocol": "acp", "command": "node", "args": ["adapter.mjs"] }
        }))
        .unwrap(),
    )
    .unwrap();

    manager
        .install_plugin("zcode", Some(src.to_str().unwrap()), false)
        .unwrap();
    let (views, _) = manager.list_local_plugins_view();
    let zcode = views.into_iter().find(|v| v.id == "zcode").unwrap();
    assert_eq!(zcode.session_prefix, "zcode_");
    assert_eq!(zcode.session_prefixes, vec!["zcode_", "sess_"]);
    let json = serde_json::to_value(&zcode).unwrap();
    assert_eq!(
        json["sessionPrefixes"],
        serde_json::json!(["zcode_", "sess_"])
    );
}

#[test]
fn acp_start_with_options_isolates_gateway_env_for_plugins() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join("claude_gemini_mock");
    fs::create_dir_all(&plugin_dir).unwrap();
    let script_path = plugin_dir.join("probe-env.mjs");
    fs::write(
        &script_path,
        r#"import readline from "node:readline";
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => {
  const t = line.trim();
  if (!t.startsWith("{")) return;
  const msg = JSON.parse(t);
  if (msg.method === "initialize") {
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: { protocolVersion: "1.0" } }) + "\n");
  } else if (msg.method === "agenthub/probe") {
    process.stdout.write(JSON.stringify({
      jsonrpc: "2.0",
      id: msg.id,
      result: {
        custom: process.env.PLUGIN_TEST_VAR ?? null,
        hasAnthropicToken: process.env.ANTHROPIC_AUTH_TOKEN ?? null,
        hasGeminiKey: process.env.GEMINI_API_KEY ?? null
      }
    }) + "\n");
  }
});
"#,
    )
    .unwrap();

    let parent_anthropic = std::env::var("ANTHROPIC_AUTH_TOKEN").ok();
    let parent_gemini = std::env::var("GEMINI_API_KEY").ok();
    let args = vec![script_path.to_string_lossy().to_string()];
    assert!(
        crate::collectors::acp_app_server::detect_acp_gateway_kind(Path::new("node"), &args)
            .is_none()
    );
    assert!(
        crate::collectors::acp_app_server::detect_acp_gateway_kind(
            Path::new("claude-code-acp"),
            &[]
        )
        .is_some()
    );
    let mut extra = std::collections::BTreeMap::new();
    extra.insert("PLUGIN_TEST_VAR".to_string(), "ok-1".to_string());
    let mut srv = crate::collectors::AcpAppServer::start_with_options(
        "node",
        &args,
        None,
        &extra,
        false,
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    let _ = srv.request_raw("initialize", serde_json::json!({"protocolVersion": "1.0"}));
    let probed = srv
        .request_raw("agenthub/probe", serde_json::json!({}))
        .unwrap();
    assert_eq!(probed["custom"], "ok-1");
    assert_eq!(
        probed["hasAnthropicToken"].as_str().map(str::to_string),
        parent_anthropic
    );
    assert_eq!(
        probed["hasGeminiKey"].as_str().map(str::to_string),
        parent_gemini
    );
}

#[test]
fn rejects_file_child_url_native_runtime_untrusted_remote_origin_and_forbidden_ips() {
    let _env_lock = PLUGIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _no_insecure = EnvVarGuard::set("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY", None);
    let _no_allowed = EnvVarGuard::set("AGENTHUB_ALLOWED_PLUGIN_REGISTRIES", None);
    assert_eq!(
        installer::resolve_child_url("http://127.0.0.1:8080/registry.json", "file:///etc/passwd")
            .unwrap_err(),
        "forbidden_local_child_url"
    );
    assert_eq!(
        installer::resolve_child_url(
            "https://raw.githubusercontent.com/org/repo/main/registry.json",
            "file:///etc/passwd"
        )
        .unwrap_err(),
        "forbidden_local_child_url"
    );

    let dir = tempfile::tempdir().unwrap();
    let plug_dir = dir.path().join("native_plug");
    fs::create_dir_all(&plug_dir).unwrap();
    fs::write(plug_dir.join("run.sh"), "#!/bin/sh\n").unwrap();
    let native_spec = PluginRuntimeSpec {
        protocol: "acp".into(),
        command: "run.sh".into(),
        args: vec![],
        binary_hints: vec![],
        env: Default::default(),
    };
    let err = installer::validate_runtime_spec(&native_spec, &plug_dir).unwrap_err();
    assert_eq!(err, "forbidden_runtime_command:run.sh");

    let manager = PluginManager::with_dir(dir.path().join("state"));
    assert_eq!(
        manager
            .set_remote_registry_url(Some("https://evil.example/registry.json"))
            .unwrap_err(),
        "untrusted_remote_registry_origin"
    );

    for ip in ["0.1.2.3", "64:ff9b::127.0.0.1", "2002:7f00:0001::1"] {
        assert!(
            installer::is_forbidden_ip(ip.parse().unwrap()),
            "expected forbidden ip: {ip}"
        );
    }

    let corrupt_dir = dir.path().join("corrupt_state");
    fs::create_dir_all(&corrupt_dir).unwrap();
    fs::write(corrupt_dir.join("state.json"), "{broken json").unwrap();
    let corrupt_mgr = PluginManager::with_dir(corrupt_dir.clone());
    let write_err = corrupt_mgr.set_registry_url(None).unwrap_err();
    assert!(write_err.contains("corrupt_plugin_state"));
    assert_eq!(
        fs::read_to_string(corrupt_dir.join("state.json")).unwrap(),
        "{broken json"
    );
}

#[test]
fn remote_registry_trust_enforces_repo_prefix_and_revalidates_on_remote_commands() {
    let _env_lock = PLUGIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _no_insecure = EnvVarGuard::set("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY", None);
    let _no_allowed = EnvVarGuard::set("AGENTHUB_ALLOWED_PLUGIN_REGISTRIES", None);

    let official = url::Url::parse(DEFAULT_PLUGIN_REGISTRY_URL).unwrap();
    assert!(installer::is_trusted_remote_registry_url(&official));

    // Arbitrary GitHub repositories on raw.githubusercontent.com or github.com must be rejected.
    for untrusted in [
        "https://raw.githubusercontent.com/attacker/evil/main/registry.json",
        "https://raw.githubusercontent.com/xiaoran7/agenthub-plugins-evil/main/registry.json",
        "https://github.com/attacker/evil/raw/main/registry.json",
        "https://raw.githubusercontent.com/xiaoran7/agenthub-plugins/%2e%2e/evil/registry.json",
        "http://raw.githubusercontent.com/xiaoran7/agenthub-plugins/main/registry.json",
    ] {
        let parsed = url::Url::parse(untrusted).unwrap();
        assert!(
            !installer::is_trusted_remote_registry_url(&parsed),
            "expected untrusted remote registry URL: {untrusted}"
        );
    }

    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_dir(dir.path().join("state"));
    assert_eq!(
        manager
            .set_remote_registry_url(Some(
                "https://raw.githubusercontent.com/attacker/evil/main/registry.json",
            ))
            .unwrap_err(),
        "untrusted_remote_registry_origin"
    );

    // Even if a local file or untrusted URL was configured locally, remote install and
    // refresh_catalog must re-validate against the trusted allowlist before fetching.
    let reg_path = write_test_plugin_bundle(dir.path(), "local_only", "locl_");
    manager.set_registry_url(Some(&reg_path)).unwrap();

    let refresh_outcome = manager.handle_remote_command(&RemotePluginCommand {
        request_id: Some("req-refresh".into()),
        action: "refresh_catalog".into(),
        plugin_id: None,
        enabled: None,
        source_url: None,
        registry_url: None,
    });
    assert!(!refresh_outcome.ok);
    assert_eq!(
        refresh_outcome.error.as_deref(),
        Some("untrusted_remote_registry_origin")
    );

    let install_outcome = manager.handle_remote_command(&RemotePluginCommand {
        request_id: Some("req-install".into()),
        action: "install".into(),
        plugin_id: Some("local_only".into()),
        enabled: None,
        source_url: None,
        registry_url: None,
    });
    assert!(!install_outcome.ok);
    assert_eq!(
        install_outcome.error.as_deref(),
        Some("untrusted_remote_registry_origin")
    );

    // Explicit AGENTHUB_ALLOWED_PLUGIN_REGISTRIES prefix matches exact file or children only.
    let _allowed = EnvVarGuard::set(
        "AGENTHUB_ALLOWED_PLUGIN_REGISTRIES",
        Some("https://plugins.example.com/team/registry.json"),
    );
    assert!(installer::is_trusted_remote_registry_url(
        &url::Url::parse("https://plugins.example.com/team/registry.json").unwrap()
    ));
    assert!(installer::is_trusted_remote_registry_url(
        &url::Url::parse("https://plugins.example.com/team/plugins/zcode/adapter.mjs").unwrap()
    ));
    assert!(!installer::is_trusted_remote_registry_url(
        &url::Url::parse("https://plugins.example.com/other/registry.json").unwrap()
    ));
    assert!(!installer::is_trusted_remote_registry_url(
        &url::Url::parse("https://plugins.example.com/team2/registry.json").unwrap()
    ));
}

#[test]
fn rejects_untrusted_child_url_duplicate_paths_and_reserved_prefixes() {
    let _env_lock = PLUGIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _no_insecure = EnvVarGuard::set("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY", None);
    let _no_allowed = EnvVarGuard::set("AGENTHUB_ALLOWED_PLUGIN_REGISTRIES", None);

    let dir = tempfile::tempdir().unwrap();
    let store = PluginStore::new(dir.path().join("store"));

    // Untrusted child URL inside an otherwise official registry must be rejected before download.
    let evil_child_entry = RegistryPluginEntry {
        id: "evil_child".into(),
        name: "Evil".into(),
        display_name: None,
        version: "1.0.0".into(),
        category: "agent".into(),
        description: String::new(),
        author: None,
        license: None,
        homepage: None,
        min_runner_version: None,
        session_prefix: Some("evil_".into()),
        session_prefixes: vec![],
        icon: None,
        aliases: vec![],
        capabilities: PluginCapabilities::default(),
        runtime: None,
        files: vec![RegistryFileEntry {
            path: "adapter.mjs".into(),
            url: "https://raw.githubusercontent.com/attacker/evil/main/adapter.mjs".into(),
            sha256: "0".repeat(64),
            size: Some(10),
        }],
    };
    let err =
        installer::stage_from_registry(&store, DEFAULT_PLUGIN_REGISTRY_URL, &evil_child_entry)
            .unwrap_err();
    assert!(
        err.contains("untrusted_plugin_child_url"),
        "expected untrusted_plugin_child_url, got: {err}"
    );

    // Duplicate rel_path must be rejected.
    let dup_entry = RegistryPluginEntry {
        files: vec![
            RegistryFileEntry {
                path: "adapter.mjs".into(),
                url: "plugins/dup/adapter.mjs".into(),
                sha256: "0".repeat(64),
                size: Some(10),
            },
            RegistryFileEntry {
                path: "ADAPTER.MJS".into(),
                url: "plugins/dup/adapter.mjs".into(),
                sha256: "0".repeat(64),
                size: Some(10),
            },
        ],
        ..evil_child_entry.clone()
    };
    let dup_err =
        installer::stage_from_registry(&store, "/tmp/registry.json", &dup_entry).unwrap_err();
    assert!(
        dup_err.contains("duplicate_plugin_file_path"),
        "expected duplicate_plugin_file_path, got: {dup_err}"
    );

    // Reserved or too-short session prefixes / aliases must be rejected.
    let mut manifest = PluginManifest {
        schema_version: 1,
        id: "custom_agent".into(),
        name: "Custom".into(),
        display_name: None,
        version: "1.0.0".into(),
        category: "agent".into(),
        description: String::new(),
        author: None,
        license: None,
        homepage: None,
        min_runner_version: None,
        session_prefix: Some("a_".into()),
        session_prefixes: vec![],
        icon: None,
        aliases: vec![],
        capabilities: PluginCapabilities::default(),
        runtime: PluginRuntimeSpec {
            protocol: "acp".into(),
            command: "node".into(),
            args: vec!["adapter.mjs".into()],
            binary_hints: vec![],
            env: Default::default(),
        },
    };
    assert!(installer::validate_manifest_identifiers(&manifest).is_err());
    manifest.session_prefix = Some("claude_hijack_".into());
    assert!(installer::validate_manifest_identifiers(&manifest).is_err());
    manifest.session_prefix = Some("custom_".into());
    manifest.aliases = vec!["codex".into()];
    assert!(installer::validate_manifest_identifiers(&manifest).is_err());
    manifest.aliases = vec!["custom_alias".into()];
    assert!(installer::validate_manifest_identifiers(&manifest).is_ok());
}

#[test]
fn blocks_extended_forbidden_ips_http_private_ssrf_and_deduplicates_corrupt_backups() {
    let _env_lock = PLUGIN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _no_insecure = EnvVarGuard::set("AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY", None);

    for ip in [
        "192.0.0.1",
        "198.18.0.1",
        "198.19.255.254",
        "240.0.0.1",
        "::10.0.0.1",
        "::ffff:0:127.0.0.1",
        "2001::1",
        "2001:db8::1",
        "fec0::1",
        "100::1",
        "64:ff9b::10.0.0.1",
        "64:ff9b:1::1",
    ] {
        assert!(
            installer::is_forbidden_ip(ip.parse().unwrap()),
            "expected extended forbidden ip: {ip}"
        );
    }

    // Private IPs must be blocked even on http:// when require_https=false.
    assert!(
        installer::validate_remote_url("http://169.254.169.254/latest/meta-data", false).is_err()
    );
    assert!(installer::validate_remote_url("http://127.0.0.1:8080/registry.json", false).is_err());
    assert!(installer::validate_remote_url("http://10.0.0.5/registry.json", false).is_err());

    // Repeated read polls on corrupt state.json must not spam backup files, and write retries
    // deduplicate by content hash.
    let dir = tempfile::tempdir().unwrap();
    let corrupt_dir = dir.path().join("corrupt_dedup");
    fs::create_dir_all(&corrupt_dir).unwrap();
    fs::write(corrupt_dir.join("state.json"), "{corrupt_payload").unwrap();
    let store = PluginStore::new(corrupt_dir.clone());
    for _ in 0..5 {
        let _ = store.load_state();
    }
    let read_backups = fs::read_dir(&corrupt_dir)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("state.corrupt.")
        })
        .count();
    assert_eq!(
        read_backups, 0,
        "read-only load_state must not create backup files"
    );

    for _ in 0..5 {
        assert!(store.load_state_for_write().is_err());
    }
    let write_backups = fs::read_dir(&corrupt_dir)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("state.corrupt.")
        })
        .count();
    assert_eq!(
        write_backups, 1,
        "identical corrupt state must produce only one backup"
    );
}
