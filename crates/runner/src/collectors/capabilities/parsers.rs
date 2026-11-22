use super::*;

/// Parses SKILL.md frontmatter (YAML) or first paragraphs.
pub fn parse_skill_markdown(path: &Path, fallback_name: &str) -> (String, String) {
    let Ok(content) = fs::read_to_string(path) else {
        return (fallback_name.to_string(), "暂无详细描述".to_string());
    };

    let mut name = fallback_name.to_string();
    let mut description = String::new();

    if content.starts_with("---") {
        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() >= 3 {
            let frontmatter = parts[1];
            for line in frontmatter.lines() {
                let trimmed = line.trim();
                if let Some(val) = trimmed.strip_prefix("name:") {
                    let cleaned = val.trim().trim_matches('"').trim_matches('\'').trim();
                    if !cleaned.is_empty() {
                        name = cleaned.to_string();
                    }
                } else if let Some(val) = trimmed.strip_prefix("description:") {
                    let cleaned = val.trim().trim_matches('"').trim_matches('\'').trim();
                    if !cleaned.is_empty() {
                        description = cleaned.to_string();
                    }
                }
            }
        }
    }

    if description.is_empty() {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with("---") || trimmed.is_empty() {
                continue;
            }
            description = trimmed.chars().take(200).collect();
            break;
        }
    }

    if description.is_empty() {
        description = format!("{} 扩展技能模块", fallback_name);
    }

    (name, description)
}

/// Checks if a directory contains script files.
pub(crate) fn has_executable_scripts(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            let ext_l = ext.to_ascii_lowercase();
            if matches!(
                ext_l.as_str(),
                "sh" | "bash" | "cmd" | "bat" | "ps1" | "py" | "js" | "mjs" | "ts"
            ) {
                return true;
            }
        }
    }
    false
}

/// Parses MCP servers from a JSON file (e.g. .mcp.json, ~/.claude.json, ~/.claude/settings.json).
pub fn parse_mcp_from_json_file(
    path: &Path,
    source: &str,
    agent_name: &str,
    out: &mut Vec<McpServerItem>,
) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    let Ok(val) =
        serde_json::from_str::<Value>(crate::collectors::antigravity::strip_utf8_bom(&content))
    else {
        return;
    };

    parse_mcp_from_json_value(&val, source, agent_name, out);
}

fn redact_mcp_secret(key: &str, value: &str) -> String {
    let upper = key.to_ascii_uppercase();
    let looks_secret_key = [
        "KEY",
        "TOKEN",
        "SECRET",
        "PASS",
        "AUTH",
        "COOKIE",
        "CREDENTIAL",
        "PRIVATE",
        "BEARER",
        "SIGNATURE",
    ]
    .iter()
    .any(|kw| upper.contains(kw));
    let trimmed = value.trim();
    let looks_secret_val = trimmed.starts_with("sk-")
        || trimmed.starts_with("ghp_")
        || trimmed.starts_with("github_pat_")
        || trimmed.starts_with("Bearer ")
        || trimmed.starts_with("bearer ");
    if looks_secret_key || looks_secret_val {
        "[redacted]".to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn parse_mcp_from_json_value(
    val: &Value,
    source: &str,
    agent_name: &str,
    out: &mut Vec<McpServerItem>,
) {
    let servers_obj = val.get("mcpServers").or_else(|| val.get("mcp_servers"));
    let Some(servers_map) = servers_obj.and_then(Value::as_object) else {
        return;
    };

    for (name, srv_val) in servers_map {
        let command = srv_val
            .get("command")
            .and_then(Value::as_str)
            .map(|s| s.to_string());

        let args = srv_val
            .get("args")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let mut env = HashMap::new();
        if let Some(env_obj) = srv_val.get("env").and_then(Value::as_object) {
            for (k, v) in env_obj {
                if let Some(s) = v.as_str() {
                    env.insert(k.clone(), redact_mcp_secret(k, s));
                }
            }
        }

        let url = srv_val
            .get("url")
            .and_then(Value::as_str)
            .map(|s| s.to_string());

        let mut headers = HashMap::new();
        if let Some(h_obj) = srv_val.get("headers").and_then(Value::as_object) {
            for (k, v) in h_obj {
                if let Some(s) = v.as_str() {
                    headers.insert(k.clone(), redact_mcp_secret(k, s));
                }
            }
        }

        let transport = if url.is_some() {
            let u = url.as_deref().unwrap_or("");
            if u.contains("/sse") {
                "sse".to_string()
            } else {
                "http".to_string()
            }
        } else {
            "stdio".to_string()
        };

        out.push(McpServerItem {
            id: format!("{}:{}", source, name),
            name: name.clone(),
            transport,
            command,
            args,
            env,
            url,
            headers,
            source: source.to_string(),
            agents: vec![agent_name.to_string()],
            status: "active".to_string(),
        });
    }
}

/// Parses MCP servers from Codex TOML file (~/.codex/config.toml).
/// Lightweight line-by-line block extractor without extra external crate.
pub fn parse_mcp_from_codex_toml(path: &Path, out: &mut Vec<McpServerItem>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    let mut current_server: Option<McpServerItem> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("[mcp_servers.") && trimmed.ends_with(']') {
            let section = &trimmed[13..trimmed.len() - 1];
            // If it's a subtable like [mcp_servers.foo.env], don't treat as new server
            if section.contains('.') {
                continue;
            }

            if let Some(srv) = current_server.take() {
                out.push(srv);
            }

            let name = section.to_string();
            current_server = Some(McpServerItem {
                id: format!("codex:{}", name),
                name,
                transport: "stdio".to_string(),
                command: None,
                args: Vec::new(),
                env: HashMap::new(),
                url: None,
                headers: HashMap::new(),
                source: "codex".to_string(),
                agents: vec!["Codex".to_string()],
                status: "active".to_string(),
            });
            continue;
        }

        if let Some(ref mut srv) = current_server {
            if let Some(val) = trimmed.strip_prefix("command") {
                let eq = val.trim_start().strip_prefix('=').unwrap_or(val).trim();
                srv.command = Some(parse_toml_string_literal(eq));
            } else if let Some(val) = trimmed.strip_prefix("url") {
                let eq = val.trim_start().strip_prefix('=').unwrap_or(val).trim();
                srv.url = Some(parse_toml_string_literal(eq));
                srv.transport = "http".to_string();
            } else if let Some(val) = trimmed.strip_prefix("args") {
                let eq = val.trim_start().strip_prefix('=').unwrap_or(val).trim();
                if eq.starts_with('[') && eq.ends_with(']') {
                    let inner = &eq[1..eq.len() - 1];
                    srv.args = inner
                        .split(',')
                        .map(|s| parse_toml_string_literal(s.trim()))
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
        }
    }

    if let Some(srv) = current_server {
        out.push(srv);
    }
}

fn parse_toml_string_literal(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        // Single-quoted TOML strings are literal: backslashes in Windows paths
        // such as `'C:\Users\demo\...\resources\...\node_repl.exe'` must stay intact.
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return unescape_toml_str(&trimmed[1..trimmed.len() - 1]);
    }
    trimmed.to_string()
}

fn unescape_toml_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    other => {
                        out.push('\\');
                        out.push(other);
                    }
                }
            } else {
                out.push('\\');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parses lifecycle execution hooks from Claude Code settings (~/.claude/settings.json).
pub fn parse_hooks_from_claude_settings(path: &Path, out: &mut Vec<HookItem>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    let Ok(val) =
        serde_json::from_str::<Value>(crate::collectors::antigravity::strip_utf8_bom(&content))
    else {
        return;
    };

    let Some(hooks_obj) = val.get("hooks").and_then(Value::as_object) else {
        return;
    };

    for (event_key, event_val) in hooks_obj {
        let Some(rule_arr) = event_val.as_array() else {
            continue;
        };

        for (idx, rule) in rule_arr.iter().enumerate() {
            let matcher = rule
                .get("matcher")
                .and_then(Value::as_str)
                .map(|s| s.to_string());

            let hooks_inner = rule.get("hooks").and_then(Value::as_array);
            if let Some(inner_arr) = hooks_inner {
                for (sub_idx, h) in inner_arr.iter().enumerate() {
                    let command = h
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if command.is_empty() {
                        continue;
                    }
                    let timeout = h.get("timeout").and_then(Value::as_u64).unwrap_or(10) as u32;

                    let (name, display) = friendly_hook_name(event_key);

                    out.push(HookItem {
                        id: format!("claude:{}:{}_{}", event_key, idx, sub_idx),
                        name,
                        event: event_key.clone(),
                        event_display: display,
                        matcher: matcher.clone(),
                        command,
                        timeout_sec: timeout,
                        source: "claude".to_string(),
                        agents: vec!["Claude Code".to_string()],
                        enabled: true,
                    });
                }
            }
        }
    }
}

/// Parses hooks from Codex hooks.json (~/.codex/hooks.json).
pub fn parse_hooks_from_codex_file(path: &Path, out: &mut Vec<HookItem>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    let Ok(val) = serde_json::from_str::<Value>(&content) else {
        return;
    };

    let Some(hooks_obj) = val.get("hooks").and_then(Value::as_object) else {
        return;
    };

    for (event_key, event_val) in hooks_obj {
        if let Some(arr) = event_val.as_array() {
            for (idx, item) in arr.iter().enumerate() {
                let command = item
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if command.is_empty() {
                    continue;
                }
                let timeout = item.get("timeout").and_then(Value::as_u64).unwrap_or(10) as u32;
                let matcher = item
                    .get("matcher")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());

                let (name, display) = friendly_hook_name(event_key);

                out.push(HookItem {
                    id: format!("codex:{}:{}", event_key, idx),
                    name,
                    event: event_key.clone(),
                    event_display: display,
                    matcher,
                    command,
                    timeout_sec: timeout,
                    source: "codex".to_string(),
                    agents: vec!["Codex".to_string()],
                    enabled: true,
                });
            }
        }
    }
}

/// Translates raw hook event keys into friendly titles and descriptions.
fn friendly_hook_name(event: &str) -> (String, String) {
    match event {
        "PreToolUse" => (
            "工具执行前拦截".to_string(),
            "在智能体执行 Shell 或文件变更前进行权限核验与前置检查".to_string(),
        ),
        "PostToolUse" => (
            "工具执行后校验".to_string(),
            "在工具成功执行后捕获输出、执行格式化或记录审计日志".to_string(),
        ),
        "PostToolUseFailure" => (
            "工具执行失败处置".to_string(),
            "在工具执行报错时触发告警或注入自动降级建议".to_string(),
        ),
        "SessionStart" => (
            "会话启动初始化".to_string(),
            "在新会话建立时加载环境变数、初始化工程或播报连接信息".to_string(),
        ),
        "UserPromptSubmit" => (
            "用户提问前置处理".to_string(),
            "在用户提交 Prompt 后进行敏感词拦截、语义增强或用量预估".to_string(),
        ),
        "Stop" => (
            "会话完成收尾".to_string(),
            "在任务或会话优雅完成时发送完成通知或触发工作流".to_string(),
        ),
        "StopFailure" => (
            "会话异常中断告警".to_string(),
            "在会话异常退出或被强行 Kill 时捕获栈现场并通报".to_string(),
        ),
        "SubagentStart" => (
            "子智能体派生注入".to_string(),
            "在主智能体启动子代理协同任务时注入系统级准则".to_string(),
        ),
        "SubagentStop" => (
            "子智能体结束回收".to_string(),
            "在子代理退出时释放临时沙箱或汇总子任务耗时".to_string(),
        ),
        "TeammateIdle" => (
            "团队空闲看门狗".to_string(),
            "在多代理协作处于空闲状态时调度下一步任务或提醒人工接管".to_string(),
        ),
        "PermissionRequest" => (
            "权限审批请求拦截".to_string(),
            "在触发高危权限审批时向移动端或监控面板发送实时推送".to_string(),
        ),
        "PostCompact" => (
            "上下文压缩后对齐".to_string(),
            "在触发自动上下文压缩后补齐关键基线与核心实体信息".to_string(),
        ),
        other => (
            format!("{} 钩子", other),
            format!("生命周期点 [{}] 触发执行", other),
        ),
    }
}

/// Deduplicates skills with the same name across agents and aggregates their agent tags.
pub(crate) fn deduplicate_skills(skills: &mut Vec<SkillItem>) {
    let mut map: HashMap<String, SkillItem> = HashMap::new();

    for item in skills.drain(..) {
        let key = item.name.to_ascii_lowercase();
        if let Some(existing) = map.get_mut(&key) {
            for agent in item.agents {
                if !existing.agents.contains(&agent) {
                    existing.agents.push(agent);
                }
            }
        } else {
            map.insert(key, item);
        }
    }

    let mut result: Vec<SkillItem> = map.into_values().collect();
    result.sort_by_key(|a| a.name.to_ascii_lowercase());
    *skills = result;
}

/// Deduplicates MCP servers by name and merges agents.
pub(crate) fn deduplicate_mcp_servers(servers: &mut Vec<McpServerItem>) {
    let mut map: HashMap<String, McpServerItem> = HashMap::new();

    for item in servers.drain(..) {
        let key = item.name.to_ascii_lowercase();
        if let Some(existing) = map.get_mut(&key) {
            for agent in item.agents {
                if !existing.agents.contains(&agent) {
                    existing.agents.push(agent);
                }
            }
        } else {
            map.insert(key, item);
        }
    }

    let mut result: Vec<McpServerItem> = map.into_values().collect();
    result.sort_by_key(|a| a.name.to_ascii_lowercase());
    *servers = result;
}

/// Deduplicates hooks by command + event.
pub(crate) fn deduplicate_hooks(hooks: &mut Vec<HookItem>) {
    let mut map: HashMap<String, HookItem> = HashMap::new();

    for item in hooks.drain(..) {
        let key = format!("{}:{}", item.event, item.command);
        if let Some(existing) = map.get_mut(&key) {
            for agent in item.agents {
                if !existing.agents.contains(&agent) {
                    existing.agents.push(agent);
                }
            }
        } else {
            map.insert(key, item);
        }
    }

    let mut result: Vec<HookItem> = map.into_values().collect();
    result.sort_by(|a, b| a.event.cmp(&b.event).then_with(|| a.name.cmp(&b.name)));
    *hooks = result;
}

/// Registers or updates an MCP server into ~/.agenthub/mcp.json.
pub fn register_agenthub_mcp_server(
    name: &str,
    command: Option<String>,
    args: Vec<String>,
    url: Option<String>,
    headers: HashMap<String, String>,
) -> Result<McpServerItem, String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| "home_directory_unavailable".to_string())?;

    let agenthub_dir = home.join(".agenthub");
    if !agenthub_dir.exists() {
        let _ = fs::create_dir_all(&agenthub_dir);
    }

    let mcp_path = agenthub_dir.join("mcp.json");
    let mut doc: Value = if mcp_path.is_file() {
        fs::read_to_string(&mcp_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| json!({ "mcpServers": {} }))
    } else {
        json!({ "mcpServers": {} })
    };

    let servers = doc
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "invalid_mcp_json_structure".to_string())?;

    let transport = if url.is_some() {
        let u = url.as_deref().unwrap_or("");
        if u.contains("/sse") { "sse" } else { "http" }
    } else {
        "stdio"
    };

    let mut srv_val = json!({});
    if let Some(ref c) = command {
        srv_val["command"] = json!(c);
    }
    if !args.is_empty() {
        srv_val["args"] = json!(args);
    }
    if let Some(ref u) = url {
        srv_val["url"] = json!(u);
    }
    if !headers.is_empty() {
        srv_val["headers"] = json!(headers);
    }

    servers.insert(name.to_string(), srv_val);

    let serialized =
        serde_json::to_string_pretty(&doc).map_err(|e| format!("json_serialize_error: {}", e))?;
    fs::write(&mcp_path, serialized).map_err(|e| format!("write_mcp_json_error: {}", e))?;
    invalidate_capabilities_cache();

    Ok(McpServerItem {
        id: format!("agenthub:{}", name),
        name: name.to_string(),
        transport: transport.to_string(),
        command,
        args,
        env: HashMap::new(),
        url,
        headers,
        source: "agenthub".to_string(),
        agents: vec![
            "AgentHub".to_string(),
            "Claude Code".to_string(),
            "Codex".to_string(),
        ],
        status: "active".to_string(),
    })
}
