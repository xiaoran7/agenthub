use super::*;
use tempfile::tempdir;

#[test]
fn test_discover_capabilities_smoke() {
    // Smoke test: host-level discovery safely executes without panic in any environment (including clean CI)
    let caps = discover_capabilities(None);
    println!(
        "Discovered: {} skills, {} MCP servers, {} hooks",
        caps.total_skills, caps.total_mcp_servers, caps.total_hooks
    );
    for s in &caps.skills {
        println!(
            "  [Skill] {} ({}) - agents: {:?}",
            s.name, s.source, s.agents
        );
    }
    for m in &caps.mcp_servers {
        println!(
            "  [MCP] {} ({}, {}) - agents: {:?}",
            m.name, m.transport, m.source, m.agents
        );
    }
    for h in &caps.hooks {
        println!("  [Hook] {} ({}) - agents: {:?}", h.name, h.event, h.agents);
    }
    // Verify internal invariants hold even when clean CI has zero host configs
    assert_eq!(caps.total_skills, caps.skills.len());
    assert_eq!(caps.total_mcp_servers, caps.mcp_servers.len());
    assert_eq!(caps.total_hooks, caps.hooks.len());
}

#[test]
fn test_discover_capabilities_in_workspace() {
    let dir = tempdir().unwrap();
    let ws_path = dir.path();

    // 1. Mock workspace skills (.claude/skills/demo_skill/SKILL.md)
    let skill_dir = ws_path.join(".claude").join("skills").join("demo_skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: Demo Skill\ndescription: Test description\n---\nPrompt body",
    )
    .unwrap();

    // 2. Mock workspace MCP servers (.mcp.json)
    fs::write(
        ws_path.join(".mcp.json"),
        r#"{ "mcpServers": { "demo_mcp": { "command": "node", "args": ["app.js"] } } }"#,
    )
    .unwrap();

    let caps = discover_capabilities(Some(ws_path));
    assert!(caps.total_skills >= 1);
    assert!(caps.skills.iter().any(|s| s.name == "Demo Skill"));
    assert!(caps.total_mcp_servers >= 1);
    assert!(caps.mcp_servers.iter().any(|m| m.name == "demo_mcp"));
}

#[test]
fn test_parse_skill_markdown_with_frontmatter() {
    let dir = tempdir().unwrap();
    let skill_file = dir.path().join("SKILL.md");
    fs::write(
        &skill_file,
        "---\nname: Frontend Design\ndescription: Aozora modern UI tokens & components\n---\n# Docs\nBody text",
    )
    .unwrap();

    let (name, desc) = parse_skill_markdown(&skill_file, "fallback");
    assert_eq!(name, "Frontend Design");
    assert_eq!(desc, "Aozora modern UI tokens & components");
}

#[test]
fn test_parse_mcp_from_json_file() {
    let dir = tempdir().unwrap();
    let mcp_file = dir.path().join(".mcp.json");
    fs::write(
        &mcp_file,
        r#"{
            "mcpServers": {
                "sample_stdio": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": { "PORT": "3000", "API_KEY": "sk-secret-123" },
                    "headers": { "Authorization": "Bearer secret-tok", "X-Client": "agenthub" }
                },
                "sample_http": {
                    "url": "https://api.example.com/mcp/sse"
                }
            }
        }"#,
    )
    .unwrap();

    let mut servers = Vec::new();
    parse_mcp_from_json_file(&mcp_file, "project", "Workspace", &mut servers);
    assert_eq!(servers.len(), 2);

    let stdio = servers.iter().find(|s| s.name == "sample_stdio").unwrap();
    assert_eq!(stdio.transport, "stdio");
    assert_eq!(stdio.command.as_deref(), Some("node"));
    assert_eq!(stdio.args, vec!["server.js"]);
    assert_eq!(stdio.env.get("PORT").map(|s| s.as_str()), Some("3000"));
    assert_eq!(
        stdio.env.get("API_KEY").map(|s| s.as_str()),
        Some("[redacted]")
    );
    assert_eq!(
        stdio.headers.get("Authorization").map(|s| s.as_str()),
        Some("[redacted]")
    );
    assert_eq!(
        stdio.headers.get("X-Client").map(|s| s.as_str()),
        Some("agenthub")
    );

    let http = servers.iter().find(|s| s.name == "sample_http").unwrap();
    assert_eq!(http.transport, "sse");
    assert_eq!(http.url.as_deref(), Some("https://api.example.com/mcp/sse"));
}

#[test]
fn test_parse_mcp_from_codex_toml() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("config.toml");
    fs::write(
        &config_file,
        r#"
[mcp_servers.node_repl]
command = "C:\\runtimes\\node_repl.exe"
args = ["--trusted"]

[mcp_servers.node_repl.env]
PORT = "4000"

[mcp_servers.web_search]
url = "https://mcp.example.com/search"

[mcp_servers.claude_desktop_repl]
command = 'C:\Users\demo\AppData\Local\AnthropicClaude\app-1.3561.0\resources\claude-code\node_repl.exe'
args = ['--config', 'C:\temp\new_rule.json']
"#,
    )
    .unwrap();

    let mut servers = Vec::new();
    parse_mcp_from_codex_toml(&config_file, &mut servers);
    assert_eq!(servers.len(), 3);

    let node_repl = servers.iter().find(|s| s.name == "node_repl").unwrap();
    assert_eq!(
        node_repl.command.as_deref(),
        Some("C:\\runtimes\\node_repl.exe")
    );
    assert_eq!(node_repl.args, vec!["--trusted"]);
    assert_eq!(node_repl.transport, "stdio");

    let web_search = servers.iter().find(|s| s.name == "web_search").unwrap();
    assert_eq!(
        web_search.url.as_deref(),
        Some("https://mcp.example.com/search")
    );
    assert_eq!(web_search.transport, "http");

    let literal_win = servers
        .iter()
        .find(|s| s.name == "claude_desktop_repl")
        .unwrap();
    assert_eq!(
        literal_win.command.as_deref(),
        Some(
            r"C:\Users\demo\AppData\Local\AnthropicClaude\app-1.3561.0\resources\claude-code\node_repl.exe"
        )
    );
    assert_eq!(literal_win.args, vec!["--config", r"C:\temp\new_rule.json"]);
}

#[test]
fn test_parse_hooks_from_claude_settings() {
    let dir = tempdir().unwrap();
    let settings_file = dir.path().join("settings.json");
    fs::write(
        &settings_file,
        r#"{
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash|Write",
                        "hooks": [
                            { "type": "command", "command": "node check.js", "timeout": 5 }
                        ]
                    }
                ]
            }
        }"#,
    )
    .unwrap();

    let mut hooks = Vec::new();
    parse_hooks_from_claude_settings(&settings_file, &mut hooks);
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].event, "PreToolUse");
    assert_eq!(hooks[0].matcher.as_deref(), Some("Bash|Write"));
    assert_eq!(hooks[0].command, "node check.js");
    assert_eq!(hooks[0].timeout_sec, 5);
}
#[test]
fn discovers_antigravity_bom_mcp_plugins_skills_and_hooks() {
    let dir = tempdir().unwrap();
    let home = dir.path();
    let cfg = home.join(".gemini").join("config");
    let skill_dir = cfg.join("skills").join("ui-design");
    let plugin_single = cfg
        .join("plugins")
        .join("android-cli-plugin")
        .join("skills");
    let plugin_nested = cfg
        .join("plugins")
        .join("chrome-devtools-plugin")
        .join("skills")
        .join("chrome-devtools");
    let disabled_plugin = cfg
        .join("plugins")
        .join("disabled-plugin")
        .join("skills")
        .join("hidden-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::create_dir_all(&plugin_single).unwrap();
    fs::create_dir_all(&plugin_nested).unwrap();
    fs::create_dir_all(&disabled_plugin).unwrap();

    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: ui-design\ndescription: Visual tokens\n---\nBody",
    )
    .unwrap();
    fs::write(
        plugin_single.join("SKILL.md"),
        "---\nname: android-cli\ndescription: Android tooling\n---\nBody",
    )
    .unwrap();
    fs::write(
        plugin_nested.join("SKILL.md"),
        "---\nname: chrome-devtools\ndescription: DevTools MCP\n---\nBody",
    )
    .unwrap();
    fs::write(
        disabled_plugin.join("SKILL.md"),
        "---\nname: hidden-skill\ndescription: Disabled\n---\nBody",
    )
    .unwrap();

    fs::write(
        cfg.join("config.json"),
        r#"{"plugins":{"android-cli-plugin":{"enabled":true},"chrome-devtools-plugin":{"enabled":true},"disabled-plugin":{"enabled":false}}}"#,
    )
    .unwrap();
    fs::write(
        cfg.join("mcp_config.json"),
        "\u{feff}{\"mcpServers\":{\"github-mcp\":{\"command\":\"npx\",\"args\":[\"-y\",\"server\"]}}}",
    )
    .unwrap();
    fs::write(
        home.join(".gemini").join("settings.json"),
        r#"{"hooks":{"BeforeAgent":[{"hooks":[{"type":"command","command":"node guard.js","timeout":3}]}]}}"#,
    )
    .unwrap();

    let ws = dir.path().join("workspace");
    let ws_agents_skill = ws.join(".agents").join("skills").join("ws-ag-skill");
    let ws_gemini_skill = ws.join(".gemini").join("skills").join("ws-gem-skill");
    fs::create_dir_all(&ws_agents_skill).unwrap();
    fs::create_dir_all(&ws_gemini_skill).unwrap();
    fs::write(
        ws_agents_skill.join("SKILL.md"),
        "---\nname: ws-ag-skill\ndescription: Workspace Antigravity skill\n---\nBody",
    )
    .unwrap();
    fs::write(
        ws_gemini_skill.join("SKILL.md"),
        "---\nname: ws-gem-skill\ndescription: Workspace Gemini skill\n---\nBody",
    )
    .unwrap();

    let mut skills = Vec::new();
    let mut mcp = Vec::new();
    let mut hooks = Vec::new();
    super::antigravity::discover_antigravity_capabilities(
        home,
        Some(&ws),
        &mut skills,
        &mut mcp,
        &mut hooks,
    );

    assert!(skills.iter().any(|s| s.name == "ui-design"));
    assert!(skills.iter().any(|s| s.name == "android-cli"));
    assert!(skills.iter().any(|s| s.name == "chrome-devtools"));
    assert!(skills.iter().any(|s| s.name == "ws-ag-skill"));
    assert!(skills.iter().any(|s| s.name == "ws-gem-skill"));
    assert!(!skills.iter().any(|s| s.name == "hidden-skill"));
    assert_eq!(mcp.len(), 1);
    assert_eq!(mcp[0].name, "github-mcp");
    assert_eq!(mcp[0].agents, vec!["Antigravity"]);
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].event, "BeforeAgent");
    assert!(hooks[0].agents.contains(&"Antigravity".to_string()));
}
