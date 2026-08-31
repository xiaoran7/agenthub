//! AgentHub Local Command Router & Process Dispatcher (v2.0)
//! Reference: docs/SYNC_PROTOCOL.md §5

use crate::v2_protocol::{CommandPayload, EventPayload, LaunchProfile};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ActiveLocalSession {
    pub hub_session_id: String,
    pub provider_session_id: String,
    pub launch_profile: LaunchProfile,
    pub current_turn_id: Option<String>,
    pub execution_state: String,
}

#[derive(Debug, Clone, Default)]
pub struct CommandRouter {
    sessions: Arc<Mutex<HashMap<String, ActiveLocalSession>>>,
}

impl CommandRouter {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn handle_command(
        &self,
        hub_session_id: Option<String>,
        _cmd_id: Option<String>,
        command: CommandPayload,
    ) -> Result<Vec<EventPayload>, String> {
        let mut events = Vec::new();

        match command {
            CommandPayload::LaunchSession { launch_profile } => {
                let session_id =
                    hub_session_id.unwrap_or_else(|| format!("hub_sess_{}", uuid::Uuid::new_v4()));
                let provider_sess_id = format!(
                    "{}_native_{}",
                    launch_profile.provider,
                    uuid::Uuid::new_v4()
                );

                let active_sess = ActiveLocalSession {
                    hub_session_id: session_id.clone(),
                    provider_session_id: provider_sess_id.clone(),
                    launch_profile: launch_profile.clone(),
                    current_turn_id: None,
                    execution_state: "idle".to_string(),
                };

                self.sessions
                    .lock()
                    .unwrap()
                    .insert(session_id.clone(), active_sess);

                events.push(EventPayload::SessionLaunched {
                    hub_session_id: session_id,
                    provider_session_id: provider_sess_id,
                    launch_profile,
                });
            }

            CommandPayload::SendPrompt {
                text,
                client_turn_id,
            } => {
                let sess_id = hub_session_id
                    .ok_or_else(|| "hub_session_id is required for send_prompt".to_string())?;
                let mut sessions = self.sessions.lock().unwrap();
                let session = sessions
                    .get_mut(&sess_id)
                    .ok_or_else(|| format!("session {} not found", sess_id))?;

                let turn_id =
                    client_turn_id.unwrap_or_else(|| format!("turn_{}", uuid::Uuid::new_v4()));
                session.current_turn_id = Some(turn_id.clone());
                session.execution_state = "running".to_string();

                events.push(EventPayload::TurnStarted {
                    turn_id,
                    user_prompt: text,
                });
            }

            CommandPayload::ResolveApproval {
                approval_id: _,
                decision: _,
                custom_feedback: _,
            } => {
                let sess_id = hub_session_id
                    .ok_or_else(|| "hub_session_id is required for resolve_approval".to_string())?;
                let mut sessions = self.sessions.lock().unwrap();
                let session = sessions
                    .get_mut(&sess_id)
                    .ok_or_else(|| format!("session {} not found", sess_id))?;
                session.execution_state = "running".to_string();

                events.push(EventPayload::SessionStateChanged {
                    previous_state: "awaiting_permission".to_string(),
                    current_state: "running".to_string(),
                    reason: None,
                });
            }

            CommandPayload::InterruptTurn { reason: _ } => {
                let sess_id = hub_session_id
                    .ok_or_else(|| "hub_session_id is required for interrupt_turn".to_string())?;
                let mut sessions = self.sessions.lock().unwrap();
                let session = sessions
                    .get_mut(&sess_id)
                    .ok_or_else(|| format!("session {} not found", sess_id))?;
                session.execution_state = "idle".to_string();

                events.push(EventPayload::SessionStateChanged {
                    previous_state: "running".to_string(),
                    current_state: "idle".to_string(),
                    reason: None,
                });
            }

            CommandPayload::ImportNativeSession {
                provider,
                native_session_id,
                workspace_path,
            } => {
                let session_id =
                    hub_session_id.unwrap_or_else(|| format!("hub_sess_{}", uuid::Uuid::new_v4()));

                let active_sess = ActiveLocalSession {
                    hub_session_id: session_id.clone(),
                    provider_session_id: native_session_id.clone(),
                    launch_profile: LaunchProfile {
                        workspace_path,
                        workspace_display_name: None,
                        target_runner_id: "local".to_string(),
                        provider,
                        model: crate::v2_protocol::ModelConfig {
                            model_id: "imported".to_string(),
                            reasoning: None,
                        },
                        permissions: crate::v2_protocol::SessionPermissions {
                            approval_mode: crate::v2_protocol::PermissionMode::AskAllEdits,
                            allow_tool_execution: Some(true),
                            tool_rules: None,
                        },
                        initial_prompt: None,
                    },
                    current_turn_id: None,
                    execution_state: "idle".to_string(),
                };

                self.sessions
                    .lock()
                    .unwrap()
                    .insert(session_id.clone(), active_sess.clone());

                events.push(EventPayload::SessionLaunched {
                    hub_session_id: session_id,
                    provider_session_id: native_session_id,
                    launch_profile: active_sess.launch_profile,
                });
            }
        }

        Ok(events)
    }

    pub fn get_session(&self, hub_session_id: &str) -> Option<ActiveLocalSession> {
        self.sessions.lock().unwrap().get(hub_session_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2_protocol::*;

    #[test]
    fn routes_launch_and_prompt_commands() {
        let router = CommandRouter::new();

        let profile = LaunchProfile {
            workspace_path: "d:/test".to_string(),
            workspace_display_name: Some("test".to_string()),
            target_runner_id: "run_01".to_string(),
            provider: "claude_code".to_string(),
            model: ModelConfig {
                model_id: "claude-3-7-sonnet-20250219".to_string(),
                reasoning: None,
            },
            permissions: SessionPermissions {
                approval_mode: PermissionMode::AskAllEdits,
                allow_tool_execution: Some(true),
                tool_rules: None,
            },
            initial_prompt: None,
        };

        let launch_events = router
            .handle_command(
                Some("hub_sess_01".to_string()),
                Some("cmd_01".to_string()),
                CommandPayload::LaunchSession {
                    launch_profile: profile,
                },
            )
            .unwrap();

        assert_eq!(launch_events.len(), 1);
        match &launch_events[0] {
            EventPayload::SessionLaunched { hub_session_id, .. } => {
                assert_eq!(hub_session_id, "hub_sess_01");
            }
            _ => panic!("Expected SessionLaunched event"),
        }

        let prompt_events = router
            .handle_command(
                Some("hub_sess_01".to_string()),
                Some("cmd_02".to_string()),
                CommandPayload::SendPrompt {
                    text: "Hello".to_string(),
                    client_turn_id: Some("turn_01".to_string()),
                },
            )
            .unwrap();

        assert_eq!(prompt_events.len(), 1);
        match &prompt_events[0] {
            EventPayload::TurnStarted {
                turn_id,
                user_prompt,
            } => {
                assert_eq!(turn_id, "turn_01");
                assert_eq!(user_prompt, "Hello");
            }
            _ => panic!("Expected TurnStarted event"),
        }
    }
}
