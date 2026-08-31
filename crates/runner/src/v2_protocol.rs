//! AgentHub Sync Protocol v2.0 - Rust Data Structures & Slash Router
//! Reference: docs/SYNC_PROTOCOL.md & docs/CAPABILITY_AND_LAUNCH_SPEC.md

use serde::{Deserialize, Serialize};

pub const V2_PROTOCOL_VERSION: u32 = 2;
pub const MAX_REPLAY_WINDOW: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameType {
    Handshake,
    HandshakeAck,
    Heartbeat,
    HeartbeatAck,
    Event,
    Cmd,
    CmdAck,
    Ack,
    ReconcileCheck,
    ReconcileSync,
    Presence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameEnvelope<T> {
    pub ver: u32,
    pub frame_id: String,
    pub r#type: FrameType,
    pub seq: u64,
    pub ack_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hub_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_replay: Option<bool>,
    pub timestamp: u64,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportedModel {
    pub id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_thinking_budget: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_thinking_budget: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_reasoning_effort: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_efforts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapability {
    pub provider_id: String,
    pub display_name: String,
    pub cli_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    pub available: bool,
    pub supported_models: Vec<SupportedModel>,
    pub supported_permission_modes: Vec<String>,
    pub supported_slash_commands: Vec<String>,
    pub supports_stream_cancellation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub runner_id: String,
    pub hostname: String,
    pub os: String,
    pub architecture: String,
    pub providers: Vec<ProviderCapability>,
    pub reported_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    PlanOnly,
    AskAllEdits,
    AcceptEdits,
    BypassPermissions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRule {
    pub tool_name: String,
    pub action: String, // allow, ask, deny
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whitelist_commands: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPermissions {
    pub approval_mode: PermissionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_tool_execution: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_rules: Option<Vec<ToolRule>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConfig {
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchProfile {
    pub workspace_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_display_name: Option<String>,
    pub target_runner_id: String,
    pub provider: String,
    pub model: ModelConfig,
    pub permissions: SessionPermissions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CommandPayload {
    LaunchSession {
        launch_profile: LaunchProfile,
    },
    SendPrompt {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_turn_id: Option<String>,
    },
    ResolveApproval {
        approval_id: String,
        decision: String, // approved, rejected
        #[serde(skip_serializing_if = "Option::is_none")]
        custom_feedback: Option<String>,
    },
    InterruptTurn {
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    ImportNativeSession {
        provider: String,
        native_session_id: String,
        workspace_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum EventPayload {
    SessionLaunched {
        hub_session_id: String,
        provider_session_id: String,
        launch_profile: LaunchProfile,
    },
    StreamDelta {
        turn_id: String,
        block_index: u32,
        delta_type: String, // text, thinking, tool_input, tool_output
        delta_content: String,
    },
    SessionStateChanged {
        previous_state: String,
        current_state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<serde_json::Value>,
    },
    ToolApprovalRequested {
        approval_id: String,
        tool_name: String,
        tool_params: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff_preview: Option<String>,
    },
    TurnStarted {
        turn_id: String,
        user_prompt: String,
    },
    TurnCompleted {
        turn_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        token_usage: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashCommandKind {
    Hub {
        command: String,
        args: Vec<String>,
        raw: String,
    },
    Native {
        command: String,
        args: Vec<String>,
        raw: String,
        supported_providers: Vec<String>,
    },
    PromptFallback {
        raw: String,
        command_candidate: Option<String>,
    },
}

pub fn parse_slash_command(input: &str) -> SlashCommandKind {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return SlashCommandKind::PromptFallback {
            raw: input.to_string(),
            command_candidate: None,
        };
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let command = parts[0].to_ascii_lowercase();
    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

    match command.as_str() {
        "/cancel" | "/stop" | "/permission" | "/clear" | "/reconnect" | "/switch" => {
            SlashCommandKind::Hub {
                command,
                args,
                raw: input.to_string(),
            }
        }
        "/compact" | "/cost" | "/doctor" | "/init" => SlashCommandKind::Native {
            command,
            args,
            raw: input.to_string(),
            supported_providers: vec!["claude_code".to_string()],
        },
        "/undo" | "/reset" | "/approval" => SlashCommandKind::Native {
            command,
            args,
            raw: input.to_string(),
            supported_providers: vec!["codex".to_string(), "codex_app_server".to_string()],
        },
        "/review" => SlashCommandKind::Native {
            command,
            args,
            raw: input.to_string(),
            supported_providers: vec![
                "claude_code".to_string(),
                "codex".to_string(),
                "codex_app_server".to_string(),
            ],
        },
        _ => SlashCommandKind::PromptFallback {
            raw: input.to_string(),
            command_candidate: Some(command),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_slash_commands_accurately() {
        // Hub command
        match parse_slash_command("/cancel") {
            SlashCommandKind::Hub { command, .. } => assert_eq!(command, "/cancel"),
            _ => panic!("Expected Hub command"),
        }

        // Native command
        match parse_slash_command("/compact") {
            SlashCommandKind::Native {
                command,
                supported_providers,
                ..
            } => {
                assert_eq!(command, "/compact");
                assert!(supported_providers.contains(&"claude_code".to_string()));
            }
            _ => panic!("Expected Native command"),
        }

        // Prompt fallback
        match parse_slash_command("plain prompt") {
            SlashCommandKind::PromptFallback {
                raw,
                command_candidate,
            } => {
                assert_eq!(raw, "plain prompt");
                assert!(command_candidate.is_none());
            }
            _ => panic!("Expected PromptFallback"),
        }
    }
}
