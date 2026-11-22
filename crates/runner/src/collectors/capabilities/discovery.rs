use super::*;

type CapCacheKey = (Option<PathBuf>, Option<PathBuf>);

fn capabilities_cache() -> &'static RwLock<HashMap<CapCacheKey, (u64, CapabilitiesSummary)>> {
    static CACHE: OnceLock<RwLock<HashMap<CapCacheKey, (u64, CapabilitiesSummary)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn invalidate_capabilities_cache() {
    if let Ok(mut guard) = capabilities_cache().write() {
        guard.clear();
    }
}

fn capabilities_fingerprint(home: Option<&Path>, workspace_opt: Option<&Path>) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut feed = |p: &Path| {
        if let Ok(meta) = fs::metadata(p) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            hash ^= meta.len().wrapping_add(1);
            hash = hash.wrapping_mul(0x100000001b3);
            hash ^= mtime;
            hash = hash.wrapping_mul(0x100000001b3);
        } else {
            hash ^= 0xff;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    };
    if let Some(h) = home {
        for rel in [
            ".claude/skills",
            ".codex/skills",
            ".agenthub/skills",
            ".agents/skills",
            ".claude.json",
            ".claude/settings.json",
            ".codex/config.toml",
            ".codex/hooks.json",
            ".agenthub/mcp.json",
            ".agenthub/hooks.json",
            ".gemini/config/config.json",
            ".gemini/config/mcp_config.json",
            ".gemini/config/skills",
            ".gemini/settings.json",
            ".gemini/antigravity/skills",
            ".gemini/antigravity/mcp_config.json",
        ] {
            feed(&h.join(rel));
        }
    }
    if let Some(ws) = workspace_opt {
        feed(&ws.join(".mcp.json"));
        feed(&ws.join(".claude").join("skills"));
        feed(&ws.join(".agents").join("skills"));
        feed(&ws.join(".gemini").join("skills"));
    } else {
        feed(Path::new(".mcp.json"));
    }
    hash
}

/// Discovers all available skills, MCP servers, and hooks on the host,
/// reusing cached results whenever configuration files and skill directories
/// have not changed on disk.
pub fn discover_capabilities(workspace_opt: Option<&Path>) -> CapabilitiesSummary {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let key: CapCacheKey = (home.clone(), workspace_opt.map(Path::to_path_buf));
    let fingerprint = capabilities_fingerprint(home.as_deref(), workspace_opt);
    if let Ok(guard) = capabilities_cache().read()
        && let Some((cached_fp, summary)) = guard.get(&key)
        && *cached_fp == fingerprint
    {
        return summary.clone();
    }

    let mut skills = Vec::new();
    let mut mcp_servers = Vec::new();
    let mut hooks = Vec::new();

    if let Some(ref h) = home {
        // 1. Discover Claude Code skills (~/.claude/skills/)
        let claude_skills_dir = h.join(".claude").join("skills");
        if claude_skills_dir.is_dir() {
            scan_skills_in_dir(&claude_skills_dir, "claude", "Claude Code", &mut skills);
        }

        // 2. Discover Codex skills (~/.codex/skills/)
        let codex_skills_dir = h.join(".codex").join("skills");
        if codex_skills_dir.is_dir() {
            scan_skills_in_dir(&codex_skills_dir, "codex", "Codex", &mut skills);
        }

        // 3. Discover AgentHub central skills (~/.agenthub/skills/)
        let agenthub_skills_dir = h.join(".agenthub").join("skills");
        if agenthub_skills_dir.is_dir() {
            scan_skills_in_dir(&agenthub_skills_dir, "agenthub", "AgentHub", &mut skills);
        }

        // 4. Discover Shared skills (~/.agents/skills/)
        let shared_skills_dir = h.join(".agents").join("skills");
        if shared_skills_dir.is_dir() {
            scan_skills_in_dir(&shared_skills_dir, "shared", "Multi-Agent", &mut skills);
        }

        // 5. Discover Claude Code MCP servers (~/.claude.json)
        let claude_json = h.join(".claude.json");
        if claude_json.is_file() {
            parse_mcp_from_json_file(&claude_json, "claude", "Claude Code", &mut mcp_servers);
        }

        // 6. Discover Claude Code settings (~/.claude/settings.json)
        let claude_settings = h.join(".claude").join("settings.json");
        if claude_settings.is_file() {
            parse_mcp_from_json_file(&claude_settings, "claude", "Claude Code", &mut mcp_servers);
            parse_hooks_from_claude_settings(&claude_settings, &mut hooks);
        }

        // 7. Discover Codex config (~/.codex/config.toml)
        let codex_config = h.join(".codex").join("config.toml");
        if codex_config.is_file() {
            parse_mcp_from_codex_toml(&codex_config, &mut mcp_servers);
        }

        // 8. Discover Codex hooks (~/.codex/hooks.json)
        let codex_hooks = h.join(".codex").join("hooks.json");
        if codex_hooks.is_file() {
            parse_hooks_from_codex_file(&codex_hooks, &mut hooks);
        }

        // 9. Discover AgentHub central MCP & Hooks (~/.agenthub/mcp.json, ~/.agenthub/hooks.json)
        let ah_mcp = h.join(".agenthub").join("mcp.json");
        if ah_mcp.is_file() {
            parse_mcp_from_json_file(&ah_mcp, "agenthub", "AgentHub", &mut mcp_servers);
        }
        let ah_hooks = h.join(".agenthub").join("hooks.json");
        if ah_hooks.is_file() {
            parse_hooks_from_claude_settings(&ah_hooks, &mut hooks);
        }
    }

    // 10. Workspace-level discovery
    if let Some(ws) = workspace_opt {
        // Workspace .mcp.json
        let ws_mcp = ws.join(".mcp.json");
        if ws_mcp.is_file() {
            parse_mcp_from_json_file(&ws_mcp, "project", "Workspace", &mut mcp_servers);
        }
        // Workspace .claude/skills/
        let ws_skills = ws.join(".claude").join("skills");
        if ws_skills.is_dir() {
            scan_skills_in_dir(&ws_skills, "workspace", "Workspace", &mut skills);
        }
    } else {
        // Check current working directory .mcp.json
        let cwd_mcp = Path::new(".mcp.json");
        if cwd_mcp.is_file() {
            parse_mcp_from_json_file(cwd_mcp, "project", "Workspace", &mut mcp_servers);
        }
    }

    // Deduplicate and aggregate agents
    if let Some(ref home) = home {
        super::antigravity::discover_antigravity_capabilities(
            home,
            workspace_opt,
            &mut skills,
            &mut mcp_servers,
            &mut hooks,
        );
    }

    deduplicate_skills(&mut skills);
    deduplicate_mcp_servers(&mut mcp_servers);
    deduplicate_hooks(&mut hooks);

    let total_skills = skills.len();
    let total_mcp_servers = mcp_servers.len();
    let total_hooks = hooks.len();

    let summary = CapabilitiesSummary {
        skills,
        mcp_servers,
        hooks,
        total_skills,
        total_mcp_servers,
        total_hooks,
    };
    if let Ok(mut guard) = capabilities_cache().write() {
        if guard.len() > 32 {
            guard.clear();
        }
        guard.insert(key, (fingerprint, summary.clone()));
    }
    summary
}

/// Scans a directory for skill subdirectories containing SKILL.md or script files.
fn scan_skills_in_dir(dir: &Path, source: &str, agent_name: &str, out: &mut Vec<SkillItem>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let folder_name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };

        if folder_name.starts_with('.') || folder_name == "node_modules" {
            continue;
        }

        // Look for SKILL.md or readme or prompt files
        let skill_md = path.join("SKILL.md");
        let (name, description) = if skill_md.is_file() {
            parse_skill_markdown(&skill_md, &folder_name)
        } else {
            (
                folder_name.clone(),
                format!("智能体扩展技能 [{}]", folder_name),
            )
        };

        let has_scripts = has_executable_scripts(&path);

        out.push(SkillItem {
            id: format!("{}:{}", source, folder_name),
            name,
            description,
            source: source.to_string(),
            path: path.to_string_lossy().to_string(),
            agents: vec![agent_name.to_string()],
            enabled: true,
            has_scripts,
        });
    }
}
