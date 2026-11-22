use std::fs;

use rusqlite::Connection;

use super::*;

#[test]
fn empty_context_returns_no_hardcoded_agents() {
    let ctx = RouteDiscoveryContext::default();
    let summaries = discover_client_route_summaries_from(&ctx);
    assert!(
        summaries.is_empty(),
        "expected empty summaries, got {summaries:?}"
    );
}

#[test]
fn claude_desktop_prefers_live_config_library_over_stale_cc_switch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("Claude-3p");
    let lib_dir = root.join("configLibrary");
    fs::create_dir_all(&lib_dir).unwrap();
    fs::write(
        lib_dir.join("_meta.json"),
        r#"{"appliedId":"00000000-0000-4000-8000-6d6167706965","entries":[{"id":"00000000-0000-4000-8000-6d6167706965","name":"magpie"}]}"#,
    )
    .unwrap();
    fs::write(
        lib_dir.join("00000000-0000-4000-8000-6d6167706965.json"),
        r#"{"inferenceGatewayBaseUrl":"http://127.0.0.1:3425","inferenceProvider":"gateway"}"#,
    )
    .unwrap();

    let stale_cc = CcSwitchActiveProvider {
        id: "old-id".to_string(),
        name: "LegacyRelay".to_string(),
        app_type: "claude-desktop".to_string(),
        upstream_base_url: Some("https://old-relay.example.com:8317".to_string()),
        upstream_auth_token: None,
        models: Vec::new(),
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: false,
    };

    let ctx = RouteDiscoveryContext {
        claude_roots: vec![root],
        cc_providers: vec![stale_cc],
        ..RouteDiscoveryContext::default()
    };
    let summaries = discover_client_route_summaries_from(&ctx);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].agent_id, "claude_desktop");
    assert_eq!(summaries[0].provider_name, "magpie");
    assert!(summaries[0].is_routed);
}

#[test]
fn claude_desktop_uses_cc_switch_when_proxied_or_taken_over() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("Claude-3p");
    let lib_dir = root.join("configLibrary");
    fs::create_dir_all(&lib_dir).unwrap();
    fs::write(
        lib_dir.join("_meta.json"),
        r#"{"appliedId":"p1","entries":[{"id":"p1","name":"LocalEntry"}]}"#,
    )
    .unwrap();
    fs::write(
        lib_dir.join("p1.json"),
        r#"{"inferenceGatewayBaseUrl":"http://127.0.0.1:15721/claude-desktop"}"#,
    )
    .unwrap();

    let cc_proxied = CcSwitchActiveProvider {
        id: "p1".to_string(),
        name: "RelayAlpha".to_string(),
        app_type: "claude-desktop".to_string(),
        upstream_base_url: Some("https://upstream.example.com".to_string()),
        upstream_auth_token: None,
        models: Vec::new(),
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: false,
    };
    let ctx = RouteDiscoveryContext {
        claude_roots: vec![root.clone()],
        cc_providers: vec![cc_proxied],
        ..RouteDiscoveryContext::default()
    };
    let summaries = discover_client_route_summaries_from(&ctx);
    assert_eq!(summaries[0].provider_name, "RelayAlpha");
    assert!(summaries[0].is_routed);

    fs::write(
        lib_dir.join("p1.json"),
        r#"{"inferenceGatewayBaseUrl":"https://gateway.example.com/v1"}"#,
    )
    .unwrap();
    let cc_takeover = CcSwitchActiveProvider {
        id: "p2".to_string(),
        name: "RelayBeta".to_string(),
        app_type: "claude-desktop".to_string(),
        upstream_base_url: Some("https://other.example.com".to_string()),
        upstream_auth_token: None,
        models: Vec::new(),
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: true,
    };
    let ctx2 = RouteDiscoveryContext {
        claude_roots: vec![root],
        cc_providers: vec![cc_takeover],
        ..RouteDiscoveryContext::default()
    };
    let summaries2 = discover_client_route_summaries_from(&ctx2);
    assert_eq!(summaries2[0].provider_name, "RelayBeta");
    assert!(summaries2[0].is_routed);
}

#[test]
fn claude_code_reads_live_settings_and_matches_magpie_gateway() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(
        home.join(".claude").join("settings.json"),
        r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:3425"}}"#,
    )
    .unwrap();
    fs::create_dir_all(home.join(".config").join("magpie")).unwrap();
    fs::write(
        home.join(".config").join("magpie").join("gateway.json"),
        r#"{"addr":"127.0.0.1:3425","pid":123}"#,
    )
    .unwrap();

    let stale_default = CcSwitchActiveProvider {
        id: "default".to_string(),
        name: "default".to_string(),
        app_type: "claude".to_string(),
        upstream_base_url: Some("https://stale.example.com".to_string()),
        upstream_auth_token: None,
        models: Vec::new(),
        upstream_models: Vec::new(),
        is_routed: false,
        proxy_takeover: false,
    };

    let ctx = RouteDiscoveryContext {
        home: Some(home.to_path_buf()),
        cc_providers: vec![stale_default],
        ..RouteDiscoveryContext::default()
    };
    let summaries = discover_client_route_summaries_from(&ctx);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].agent_id, "claude_code");
    assert_eq!(summaries[0].provider_name, "Magpie");
    assert!(summaries[0].is_routed);
}

#[test]
fn codex_toml_ignores_unselected_sections_and_resolves_selected_provider() {
    let toml_unselected = r#"
model = "gpt-5"

[model_providers.custom]
name = "custom"
base_url = "https://unused.example.com:15721/v1"

[model_providers.magpie]
name = "magpie"
base_url = "http://127.0.0.1:3425/v1"
"#;
    let parsed_unselected = parse_codex_toml_route(toml_unselected);
    assert_eq!(parsed_unselected.active_provider_key, None);
    assert_eq!(parsed_unselected.active_base_url, None);
    assert_eq!(parsed_unselected.model.as_deref(), Some("gpt-5"));

    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    fs::create_dir_all(home.join(".codex")).unwrap();
    fs::write(home.join(".codex").join("config.toml"), toml_unselected).unwrap();
    fs::create_dir_all(home.join(".config").join("magpie")).unwrap();
    fs::write(
        home.join(".config").join("magpie").join("gateway.json"),
        r#"{"addr":"127.0.0.1:3425"}"#,
    )
    .unwrap();

    let ctx = RouteDiscoveryContext {
        home: Some(home.to_path_buf()),
        installed: InstalledAgentFlags {
            codex: true,
            ..InstalledAgentFlags::default()
        },
        ..RouteDiscoveryContext::default()
    };
    let summaries = discover_client_route_summaries_from(&ctx);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].agent_id, "codex");
    assert_eq!(summaries[0].provider_name, "OpenAI Official");
    assert!(!summaries[0].is_routed);

    let toml_selected = format!("model_provider = \"magpie\"\n{toml_unselected}");
    fs::write(home.join(".codex").join("config.toml"), &toml_selected).unwrap();
    let summaries_routed = discover_client_route_summaries_from(&ctx);
    assert_eq!(summaries_routed[0].provider_name, "Magpie");
    assert!(summaries_routed[0].is_routed);

    let toml_subtable = r#"
model_provider = "my-gw"
[model_providers."my-gw"]
name = "My Gateway"
base_url = "https://gw.example.com/v1" # inline comment
[model_providers.my-gw.http_headers]
base_url = "https://should-be-ignored.example.com"
"#;
    let parsed_sub = parse_codex_toml_route(toml_subtable);
    assert_eq!(parsed_sub.active_provider_key.as_deref(), Some("my-gw"));
    assert_eq!(
        parsed_sub.active_provider_name.as_deref(),
        Some("My Gateway")
    );
    assert_eq!(
        parsed_sub.active_base_url.as_deref(),
        Some("https://gw.example.com/v1")
    );
}

#[test]
fn antigravity_does_not_inherit_gemini_cc_switch_row() {
    let gemini_cc = CcSwitchActiveProvider {
        id: "gem-1".to_string(),
        name: "GeminiRelay".to_string(),
        app_type: "gemini".to_string(),
        upstream_base_url: Some("https://gemini-gw.example.com".to_string()),
        upstream_auth_token: None,
        models: Vec::new(),
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: true,
    };

    let ctx = RouteDiscoveryContext {
        cc_providers: vec![gemini_cc],
        installed: InstalledAgentFlags {
            antigravity: true,
            ..InstalledAgentFlags::default()
        },
        ..RouteDiscoveryContext::default()
    };
    let summaries = discover_client_route_summaries_from(&ctx);
    let ag = summaries
        .iter()
        .find(|s| s.agent_id == "antigravity")
        .expect("antigravity should appear when installed");
    assert_eq!(ag.provider_name, "Google Official");
    assert!(!ag.is_routed);

    let gem = summaries
        .iter()
        .find(|s| s.agent_id == "gemini_cli")
        .expect("gemini should appear when proxy_takeover is true");
    assert_eq!(gem.provider_name, "GeminiRelay");
    assert!(gem.is_routed);
}

#[test]
fn dynamic_cc_switch_app_type_admitted_only_under_active_takeover() {
    let inactive = CcSwitchActiveProvider {
        id: "h1".to_string(),
        name: "HermesGW".to_string(),
        app_type: "hermes".to_string(),
        upstream_base_url: Some("https://hermes.example.com".to_string()),
        upstream_auth_token: None,
        models: Vec::new(),
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: false,
    };
    let ctx_inactive = RouteDiscoveryContext {
        cc_providers: vec![inactive.clone()],
        ..RouteDiscoveryContext::default()
    };
    assert!(discover_client_route_summaries_from(&ctx_inactive).is_empty());

    let mut active = inactive;
    active.proxy_takeover = true;
    let ctx_active = RouteDiscoveryContext {
        cc_providers: vec![active],
        ..RouteDiscoveryContext::default()
    };
    let summaries = discover_client_route_summaries_from(&ctx_active);
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].agent_id, "hermes");
    assert_eq!(summaries[0].title, "Hermes");
    assert_eq!(summaries[0].provider_name, "HermesGW");
    assert!(summaries[0].is_routed);
}

#[test]
fn discover_all_cc_switch_active_providers_in_reads_takeover_flags() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("cc-switch.db");
    let conn = Connection::open(&db_path).expect("sqlite open");
    conn.execute_batch(
        "CREATE TABLE providers (
            id TEXT,
            name TEXT,
            app_type TEXT,
            settings_config TEXT,
            meta TEXT,
            is_current INTEGER
         );
         CREATE TABLE proxy_config (
            app_type TEXT,
            enabled INTEGER,
            proxy_enabled INTEGER,
            live_takeover_active INTEGER
         );
         CREATE TABLE proxy_live_backup (
            app_type TEXT,
            original_config TEXT,
            backed_up_at TEXT
         );
         CREATE TABLE settings (key TEXT, value TEXT);",
    )
    .unwrap();

    conn.execute(
        "INSERT INTO providers VALUES ('p-claude', 'StaleClaude', 'claude', '{\"env\":{\"ANTHROPIC_BASE_URL\":\"https://stale.example.com\"}}', '{}', 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO providers VALUES ('p-codex', 'CodexLive', 'codex', '{\"base_url\":\"https://codex.example.com\"}', '{}', 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO providers VALUES ('p-hermes', 'HermesBackup', 'hermes', '{}', '{}', 1)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO proxy_config VALUES ('claude', 0, 0, 0)", [])
        .unwrap();
    conn.execute("INSERT INTO proxy_config VALUES ('codex', 0, 0, 1)", [])
        .unwrap();
    conn.execute(
        "INSERT INTO proxy_live_backup VALUES ('hermes', '{}', '2026-11-19')",
        [],
    )
    .unwrap();
    drop(conn);

    let rows = discover_all_cc_switch_active_providers_in(&db_path);
    assert_eq!(rows.len(), 3);
    let find_row = |app: &str| rows.iter().find(|r| r.app_type == app).unwrap();
    assert!(!find_row("claude").proxy_takeover);
    assert!(find_row("codex").proxy_takeover);
    assert!(find_row("hermes").proxy_takeover);
}
