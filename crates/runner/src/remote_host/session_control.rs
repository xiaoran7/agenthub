use super::*;

const BORROWED_IDLE_TIMEOUT: Duration = Duration::from_secs(45);
const RELEASE_RECHECK_DELAY: Duration = Duration::from_secs(5);

pub(super) fn is_codex_thread_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

pub(super) fn configure_codex_unload(params: &mut Value) {
    // This override applies to this app-server thread only.
    params["config"] = json!({ "thread_unload_delay_secs": 1 });
}

impl<T: NativeTransport> NativeSessionHost<T> {
    pub(super) fn take_control_for_prompt(
        &mut self,
        command: &RemoteCommand,
        binding: &SessionBinding,
        model: Option<String>,
        permission_mode: Option<String>,
    ) -> Result<(), NativeError> {
        let mut acquisition = command.clone();
        acquisition.command = RemoteAction::Connect {
            mode: ConnectMode::TakeControl,
            workspace_path: binding.workspace_path.clone(),
            provider_session_id: Some(binding.provider_session_id.clone()),
            model,
            sandbox: CodexSandbox::ReadOnly,
            approval_policy: CodexApprovalPolicy::OnRequest,
            agent_type: None,
            permission_mode,
        };
        self.execute(&acquisition)?;
        Ok(())
    }

    pub(super) fn release_codex_control(
        &mut self,
        binding: &SessionBinding,
    ) -> Result<SessionBinding, NativeError> {
        if !is_codex_thread_id(&binding.provider_session_id) {
            return Err(NativeError::rejected("session_control_unsupported"));
        }
        if !binding.control_connected {
            return Ok(binding.clone());
        }
        if self.newly_created.contains(&binding.hub_session_id) {
            return Err(NativeError::rejected("session_not_started"));
        }
        if !self.can_release(binding) {
            return Err(NativeError::rejected("session_busy_or_unknown"));
        }
        let state = self.request_codex_release(binding)?;
        Ok(self.record_codex_release(&binding.hub_session_id, state, Instant::now()))
    }

    pub(super) fn recheck_codex_release(&mut self, now: Instant) {
        let Some(id) = self
            .control_release_checks
            .iter()
            .find(|(_, due)| **due <= now)
            .map(|(id, _)| id.clone())
        else {
            return;
        };
        let Some(binding) = self.sessions.get(&id).cloned() else {
            self.control_release_checks.remove(&id);
            return;
        };
        match self.request_codex_release(&binding) {
            Ok(ExecutionState::Unknown) => {
                self.record_codex_release(&id, ExecutionState::Unknown, now);
            }
            _ => {
                self.control_release_checks
                    .insert(id, now + RELEASE_RECHECK_DELAY);
            }
        }
    }

    fn request_codex_release(
        &mut self,
        binding: &SessionBinding,
    ) -> Result<ExecutionState, NativeError> {
        match self.transport.request(
            "thread/unsubscribe",
            json!({ "threadId": binding.provider_session_id }),
        ) {
            Ok(response) => match response["status"].as_str() {
                Some("notLoaded") => Ok(ExecutionState::Unknown),
                _ => Ok(ExecutionState::ReleasingControl),
            },
            Err(error) if error.outcome_unknown => Ok(ExecutionState::ReleasingControl),
            Err(error) => Err(error),
        }
    }

    fn record_codex_release(
        &mut self,
        id: &str,
        state: ExecutionState,
        now: Instant,
    ) -> SessionBinding {
        self.control_idle_since.remove(id);
        if state == ExecutionState::ReleasingControl {
            self.control_release_checks
                .insert(id.into(), now + RELEASE_RECHECK_DELAY);
        } else {
            self.control_release_checks.remove(id);
        }
        let current = self.sessions.get_mut(id).expect("binding checked");
        current.control_connected = false;
        current.execution_state = state;
        let updated = current.clone();
        self.enqueue_control_change(&updated);
        updated
    }

    pub(super) fn release_idle_control(&mut self, now: Instant) {
        let due: Vec<_> = self
            .control_idle_since
            .iter()
            .filter_map(|(id, since)| {
                let binding = self.sessions.get(id)?;
                (now.saturating_duration_since(*since) >= BORROWED_IDLE_TIMEOUT
                    && self.can_release(binding))
                .then(|| binding.clone())
            })
            .collect();
        for binding in due {
            if self.release_codex_control(&binding).is_err() {
                self.control_idle_since.insert(binding.hub_session_id, now);
            }
        }
    }

    fn can_release(&self, binding: &SessionBinding) -> bool {
        binding.control_connected
            && binding.current_turn_id.is_none()
            && matches!(
                binding.execution_state,
                ExecutionState::Idle | ExecutionState::Completed | ExecutionState::Error
            )
            && !self
                .approvals
                .values()
                .any(|item| item.hub_session_id == binding.hub_session_id)
            && !self
                .questions
                .values()
                .any(|item| item.hub_session_id == binding.hub_session_id)
    }

    pub(super) fn enqueue_control_change(&mut self, binding: &SessionBinding) {
        self.pending_historical_events.push_front(HostEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            host_generation: self.generation.clone(),
            hub_session_id: Some(binding.hub_session_id.clone()),
            provider_session_id: Some(binding.provider_session_id.clone()),
            binding: Some(binding.clone()),
            kind: "session_control_changed".into(),
            raw: json!({ "method": "session_control_changed" }),
            approval_id: None,
            approval: None,
            question_id: None,
            question: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct ReleaseTransport {
        released: Vec<String>,
        lose_first_response: bool,
    }

    impl NativeTransport for ReleaseTransport {
        fn request(&mut self, method: &str, params: Value) -> Result<Value, NativeError> {
            assert_eq!(method, "thread/unsubscribe");
            self.released
                .push(params["threadId"].as_str().unwrap().to_owned());
            if self.lose_first_response {
                if self.released.len() == 1 {
                    return Err(NativeError::uncertain("provider_transport_lost"));
                }
                return Ok(json!({ "status": "notLoaded" }));
            }
            Ok(json!({ "status": "unsubscribed" }))
        }
        fn respond(&mut self, _: Value, _: Value) -> Result<(), NativeError> {
            unreachable!()
        }
        fn drain(&mut self) -> Result<Vec<Value>, NativeError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn idle_return_never_unsubscribes_a_running_or_waiting_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut host =
            NativeSessionHost::new(ReleaseTransport::default(), vec![dir.path().to_owned()])
                .unwrap();
        let start = Instant::now();
        for (id, state) in [
            ("idle", ExecutionState::Idle),
            ("running", ExecutionState::Running),
            ("approval", ExecutionState::AwaitingPermission),
            ("question", ExecutionState::AwaitingInput),
        ] {
            let native_id = uuid::Uuid::new_v4().to_string();
            host.control_idle_since.insert(id.into(), start);
            host.sessions.insert(
                id.into(),
                SessionBinding {
                    hub_session_id: id.into(),
                    provider_session_id: native_id,
                    workspace_path: dir.path().to_owned(),
                    host_generation: host.generation().to_owned(),
                    control_connected: true,
                    execution_state: state,
                    current_turn_id: None,
                },
            );
        }
        host.release_idle_control(start + BORROWED_IDLE_TIMEOUT - Duration::from_secs(1));
        assert!(host.transport.released.is_empty());
        host.release_idle_control(start + BORROWED_IDLE_TIMEOUT);
        assert_eq!(
            host.transport.released,
            [host.sessions["idle"].provider_session_id.clone()]
        );
        assert_eq!(
            host.sessions["idle"].execution_state,
            ExecutionState::ReleasingControl
        );
        assert!(host.sessions["running"].control_connected);
        assert!(host.sessions["approval"].control_connected);
        assert!(host.sessions["question"].control_connected);
    }
    #[test]
    fn a_lost_release_reply_keeps_control_disconnected_until_native_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let transport = ReleaseTransport {
            lose_first_response: true,
            ..ReleaseTransport::default()
        };
        let mut host = NativeSessionHost::new(transport, vec![dir.path().to_owned()]).unwrap();
        let binding = SessionBinding {
            hub_session_id: "hub".into(),
            provider_session_id: uuid::Uuid::new_v4().to_string(),
            workspace_path: dir.path().to_owned(),
            host_generation: host.generation().to_owned(),
            control_connected: true,
            execution_state: ExecutionState::Idle,
            current_turn_id: None,
        };
        host.sessions.insert("hub".into(), binding.clone());
        host.newly_created.insert("hub".into());
        assert_eq!(
            host.release_codex_control(&binding).unwrap_err().code,
            "session_not_started"
        );
        assert!(host.transport.released.is_empty());
        host.newly_created.remove("hub");
        let pending = host.release_codex_control(&binding).unwrap();
        assert_eq!(pending.execution_state, ExecutionState::ReleasingControl);
        assert!(!pending.control_connected);
        host.recheck_codex_release(Instant::now() + RELEASE_RECHECK_DELAY);
        assert_eq!(
            host.sessions["hub"].execution_state,
            ExecutionState::Unknown
        );
        assert!(host.control_release_checks.is_empty());
    }
}
