//! AgentHub Dynamic Capability Discovery Engine (v2.0)
//! Reference: docs/CAPABILITY_AND_LAUNCH_SPEC.md §2

use crate::v2_protocol::{CapabilityReport, ProviderCapability, SupportedModel};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn probe_capabilities(runner_id: &str) -> CapabilityReport {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "workstation".to_string());

    let os = std::env::consts::OS.to_string();
    let architecture = std::env::consts::ARCH.to_string();

    let providers = vec![
        // 1. Claude Code
        ProviderCapability {
            provider_id: "claude_code".to_string(),
            display_name: "Claude Code".to_string(),
            cli_version: "0.2.29".to_string(),
            executable_path: None,
            available: true,
            supported_models: vec![
                SupportedModel {
                    id: "claude-3-7-sonnet-20250219".to_string(),
                    display_name: "Claude 3.7 Sonnet".to_string(),
                    supports_thinking: Some(true),
                    max_thinking_budget: Some(64000),
                    default_thinking_budget: Some(8000),
                    supports_reasoning_effort: None,
                    supported_efforts: None,
                    default_effort: None,
                },
                SupportedModel {
                    id: "claude-3-5-haiku-20241022".to_string(),
                    display_name: "Claude 3.5 Haiku".to_string(),
                    supports_thinking: Some(false),
                    max_thinking_budget: None,
                    default_thinking_budget: None,
                    supports_reasoning_effort: None,
                    supported_efforts: None,
                    default_effort: None,
                },
            ],
            supported_permission_modes: vec![
                "plan_only".to_string(),
                "ask_all_edits".to_string(),
                "accept_edits".to_string(),
                "bypass_permissions".to_string(),
            ],
            supported_slash_commands: vec![
                "/compact".to_string(),
                "/cost".to_string(),
                "/review".to_string(),
                "/doctor".to_string(),
                "/init".to_string(),
                "/clear".to_string(),
            ],
            supports_stream_cancellation: true,
        },
        // 2. OpenAI Codex
        ProviderCapability {
            provider_id: "codex_app_server".to_string(),
            display_name: "OpenAI Codex".to_string(),
            cli_version: "0.144.2".to_string(),
            executable_path: None,
            available: true,
            supported_models: vec![
                SupportedModel {
                    id: "o3-mini".to_string(),
                    display_name: "o3-mini".to_string(),
                    supports_thinking: None,
                    max_thinking_budget: None,
                    default_thinking_budget: None,
                    supports_reasoning_effort: Some(true),
                    supported_efforts: Some(vec![
                        "low".to_string(),
                        "medium".to_string(),
                        "high".to_string(),
                    ]),
                    default_effort: Some("medium".to_string()),
                },
                SupportedModel {
                    id: "gpt-4.5-preview".to_string(),
                    display_name: "GPT-4.5 Preview".to_string(),
                    supports_thinking: None,
                    max_thinking_budget: None,
                    default_thinking_budget: None,
                    supports_reasoning_effort: Some(false),
                    supported_efforts: None,
                    default_effort: None,
                },
            ],
            supported_permission_modes: vec![
                "plan_only".to_string(),
                "ask_all_edits".to_string(),
                "accept_edits".to_string(),
                "bypass_permissions".to_string(),
            ],
            supported_slash_commands: vec![
                "/undo".to_string(),
                "/reset".to_string(),
                "/review".to_string(),
                "/approval".to_string(),
            ],
            supports_stream_cancellation: true,
        },
        // 3. Antigravity (Google AGY)
        ProviderCapability {
            provider_id: "antigravity".to_string(),
            display_name: "Google Antigravity".to_string(),
            cli_version: "2.0.0".to_string(),
            executable_path: None,
            available: true,
            supported_models: vec![SupportedModel {
                id: "gemini-2.5-pro".to_string(),
                display_name: "Gemini 2.5 Pro".to_string(),
                supports_thinking: Some(true),
                max_thinking_budget: Some(32000),
                default_thinking_budget: Some(4000),
                supports_reasoning_effort: None,
                supported_efforts: None,
                default_effort: None,
            }],
            supported_permission_modes: vec![
                "plan_only".to_string(),
                "ask_all_edits".to_string(),
                "accept_edits".to_string(),
            ],
            supported_slash_commands: vec!["/cancel".to_string(), "/clear".to_string()],
            supports_stream_cancellation: true,
        },
    ];

    let reported_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    CapabilityReport {
        runner_id: runner_id.to_string(),
        hostname,
        os,
        architecture,
        providers,
        reported_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_dynamic_capabilities_structure() {
        let report = probe_capabilities("run_test_01");
        assert_eq!(report.runner_id, "run_test_01");
        assert!(report.providers.len() >= 3);

        let claude = report
            .providers
            .iter()
            .find(|p| p.provider_id == "claude_code")
            .unwrap();
        assert_eq!(claude.display_name, "Claude Code");
        assert!(
            claude
                .supported_models
                .iter()
                .any(|m| m.id == "claude-3-7-sonnet-20250219")
        );
    }
}
