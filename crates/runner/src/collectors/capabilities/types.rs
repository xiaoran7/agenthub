use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub path: String,
    pub agents: Vec<String>,
    pub enabled: bool,
    pub has_scripts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerItem {
    pub id: String,
    pub name: String,
    pub transport: String, // "stdio", "sse", "http"
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub url: Option<String>,
    pub headers: HashMap<String, String>,
    pub source: String,
    pub agents: Vec<String>,
    pub status: String, // "active", "configured", "error"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookItem {
    pub id: String,
    pub name: String,
    pub event: String,
    pub event_display: String,
    pub matcher: Option<String>,
    pub command: String,
    pub timeout_sec: u32,
    pub source: String,
    pub agents: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CapabilitiesSummary {
    pub skills: Vec<SkillItem>,
    pub mcp_servers: Vec<McpServerItem>,
    pub hooks: Vec<HookItem>,
    pub total_skills: usize,
    pub total_mcp_servers: usize,
    pub total_hooks: usize,
}
