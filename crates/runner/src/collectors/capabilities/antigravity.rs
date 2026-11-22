use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::parsers::{has_executable_scripts, parse_mcp_from_json_value, parse_skill_markdown};
use super::{HookItem, McpServerItem, SkillItem};
use crate::collectors::antigravity::{read_json_bom_safe, strip_utf8_bom};

fn collect_skill_entry(
    skill_dir: &Path,
    source: &str,
    agents: Vec<String>,
    out: &mut Vec<SkillItem>,
) {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.is_file() {
        return;
    }
    let folder_name = skill_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    let (name, description) = parse_skill_markdown(&skill_md, &folder_name);
    let has_scripts = has_executable_scripts(&skill_dir.join("scripts"));
    out.push(SkillItem {
        id: format!("{source}:{folder_name}"),
        name,
        description,
        source: source.to_string(),
        path: skill_dir.to_string_lossy().to_string(),
        agents,
        enabled: true,
        has_scripts,
    });
}

fn scan_skills_directory(dir: &Path, source: &str, agents: &[&str], out: &mut Vec<SkillItem>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let agent_vec: Vec<String> = agents.iter().map(|s| (*s).to_string()).collect();
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_skill_entry(&p, source, agent_vec.clone(), out);
        }
    }
}

fn friendly_gemini_hook_name(event: &str) -> (String, String) {
    match event {
        "BeforeAgent" => ("Agent 执行前".into(), "BeforeAgent".into()),
        "AfterAgent" => ("Agent 完成后".into(), "AfterAgent".into()),
        "BeforeTool" => ("工具调用前".into(), "BeforeTool".into()),
        "AfterTool" => ("工具调用后".into(), "AfterTool".into()),
        "PreToolUse" => ("工具调用前".into(), "PreToolUse".into()),
        "PostToolUse" => ("工具调用后".into(), "PostToolUse".into()),
        "SessionStart" => ("会话启动".into(), "SessionStart".into()),
        other => (other.to_string(), other.to_string()),
    }
}

fn parse_gemini_settings_hooks(path: &Path, out: &mut Vec<HookItem>) {
    let Some(val) = read_json_bom_safe(path) else {
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
                .map(str::to_string);
            if let Some(inner_arr) = rule.get("hooks").and_then(Value::as_array) {
                for (sub_idx, h) in inner_arr.iter().enumerate() {
                    let command = h
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if command.is_empty() {
                        continue;
                    }
                    let timeout = h.get("timeout").and_then(Value::as_u64).unwrap_or(10) as u32;
                    let (name, display) = friendly_gemini_hook_name(event_key);
                    out.push(HookItem {
                        id: format!("antigravity:{event_key}:{idx}_{sub_idx}"),
                        name,
                        event: event_key.clone(),
                        event_display: display,
                        matcher: matcher.clone(),
                        command,
                        timeout_sec: timeout,
                        source: "antigravity".to_string(),
                        agents: vec!["Antigravity".to_string(), "Gemini CLI".to_string()],
                        enabled: true,
                    });
                }
            } else if let Some(cmd) = rule.get("command").and_then(Value::as_str) {
                let trimmed = cmd.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let timeout = rule.get("timeout").and_then(Value::as_u64).unwrap_or(10) as u32;
                let (name, display) = friendly_gemini_hook_name(event_key);
                out.push(HookItem {
                    id: format!("antigravity:{event_key}:{idx}"),
                    name,
                    event: event_key.clone(),
                    event_display: display,
                    matcher: matcher.clone(),
                    command: trimmed.to_string(),
                    timeout_sec: timeout,
                    source: "antigravity".to_string(),
                    agents: vec!["Antigravity".to_string(), "Gemini CLI".to_string()],
                    enabled: true,
                });
            }
        }
    }
}

/// Discovers Antigravity skills, installed plugin skills, BOM-safe MCP configs, and hooks.
pub(crate) fn discover_antigravity_capabilities(
    home: &Path,
    workspace: Option<&Path>,
    skills: &mut Vec<SkillItem>,
    mcp_servers: &mut Vec<McpServerItem>,
    hooks: &mut Vec<HookItem>,
) {
    let gemini_dir = home.join(".gemini");
    let cfg_dir = gemini_dir.join("config");

    // 1. User skills (`~/.gemini/config/skills/*`) & Antigravity skills (`~/.gemini/antigravity/skills/*`)
    scan_skills_directory(
        &cfg_dir.join("skills"),
        "antigravity",
        &["Antigravity"],
        skills,
    );
    scan_skills_directory(
        &gemini_dir.join("antigravity").join("skills"),
        "antigravity",
        &["Antigravity"],
        skills,
    );

    // 2. Installed plugin skills (`~/.gemini/config/plugins/*/skills/`), respecting enabled flags in config.json
    let mut plugin_enabled: HashMap<String, bool> = HashMap::new();
    if let Some(cfg_val) = read_json_bom_safe(&cfg_dir.join("config.json"))
        && let Some(plugins_obj) = cfg_val.get("plugins").and_then(Value::as_object)
    {
        for (name, v) in plugins_obj {
            let enabled = v.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            plugin_enabled.insert(name.clone(), enabled);
        }
    }
    let plugins_dir = cfg_dir.join("plugins");
    if let Ok(plugin_entries) = fs::read_dir(&plugins_dir) {
        for entry in plugin_entries.flatten() {
            let p_dir = entry.path();
            if !p_dir.is_dir() {
                continue;
            }
            let plugin_name = entry.file_name().to_string_lossy().to_string();
            if plugin_enabled.get(&plugin_name) == Some(&false) {
                continue;
            }
            let skills_root = p_dir.join("skills");
            if !skills_root.is_dir() {
                continue;
            }
            // Some plugins have `skills/SKILL.md` directly; others have `skills/<subskill>/SKILL.md`.
            if skills_root.join("SKILL.md").is_file() {
                let (name, description) =
                    parse_skill_markdown(&skills_root.join("SKILL.md"), &plugin_name);
                let has_scripts = has_executable_scripts(&skills_root.join("scripts"));
                skills.push(SkillItem {
                    id: format!("antigravity_plugin:{plugin_name}"),
                    name,
                    description,
                    source: "antigravity".to_string(),
                    path: skills_root.to_string_lossy().to_string(),
                    agents: vec!["Antigravity".to_string()],
                    enabled: true,
                    has_scripts,
                });
            }
            scan_skills_directory(&skills_root, "antigravity", &["Antigravity"], skills);
        }
    }

    // 3. BOM-safe MCP servers (`~/.gemini/config/mcp_config.json`, legacy `~/.gemini/antigravity/mcp*.json`)
    for mcp_path in [
        cfg_dir.join("mcp_config.json"),
        gemini_dir.join("antigravity").join("mcp_config.json"),
        gemini_dir.join("antigravity").join("mcp.json"),
    ] {
        if let Ok(raw) = fs::read_to_string(&mcp_path)
            && let Ok(val) = serde_json::from_str::<Value>(strip_utf8_bom(&raw))
        {
            parse_mcp_from_json_value(&val, "antigravity", "Antigravity", mcp_servers);
        }
    }

    // 4. Hooks (`~/.gemini/settings.json`)
    let gemini_settings = gemini_dir.join("settings.json");
    if gemini_settings.is_file() {
        parse_gemini_settings_hooks(&gemini_settings, hooks);
    }

    // 5. Workspace `.agents/skills` & `.gemini/skills` if present
    if let Some(ws) = workspace {
        for dir in [
            ws.join(".agents").join("skills"),
            ws.join(".gemini").join("skills"),
        ] {
            scan_skills_directory(&dir, "workspace", &["Workspace", "Antigravity"], skills);
        }
    }
}
