use super::*;

pub struct DurableHost<T: NativeTransport> {
    pub host: NativeSessionHost<T>,
    pub journal: RemoteJournal,
}

impl<T: NativeTransport> DurableHost<T> {
    pub fn new(mut host: NativeSessionHost<T>, mut journal: RemoteJournal) -> Result<Self> {
        host.restore_detached(journal.bindings()?);
        let detached = host.sessions();
        if !detached.is_empty() {
            let tx = journal.connection.transaction()?;
            for binding in &detached {
                persist_binding(&tx, binding)?;
            }
            tx.commit()?;
        }
        Ok(Self { host, journal })
    }

    pub fn snapshot(&self) -> Result<JournalSnapshot> {
        let mut snapshot = self.journal.snapshot()?;
        snapshot.sessions = self.host.sessions();
        snapshot.pending_approvals = self.host.pending_approvals();
        snapshot.pending_questions = self.host.pending_questions();
        Ok(snapshot)
    }

    pub fn events_after(&self, after_cursor: i64, limit: usize) -> Result<EventsAfter> {
        let mut result = self.journal.events_after(after_cursor, limit)?;
        if let EventsAfter::SnapshotRequired { snapshot } = &mut result {
            snapshot.sessions = self.host.sessions();
            snapshot.pending_approvals = self.host.pending_approvals();
            snapshot.pending_questions = self.host.pending_questions();
        }
        Ok(result)
    }

    pub fn handle_inbound_command(
        &mut self,
        seq: i64,
        command: RemoteCommand,
    ) -> Result<CommandReceipt> {
        match self.journal.begin_inbound_command(seq, &command)? {
            BeginInboundCommand::AlreadyCompleted(receipt) => Ok(receipt),
            BeginInboundCommand::CompleteExisting(receipt) => {
                self.journal
                    .finish_inbound_command(seq, &receipt, &self.host.sessions())?;
                Ok(receipt)
            }
            BeginInboundCommand::New => {
                let receipt = self.host.handle_command(command);
                self.journal
                    .finish_inbound_command(seq, &receipt, &self.host.sessions())?;
                Ok(receipt)
            }
        }
    }

    pub fn handle_local_command(&mut self, command: RemoteCommand) -> Result<CommandReceipt> {
        match self.journal.begin_command(&command)? {
            BeginCommand::Existing(receipt) => {
                self.journal
                    .append_local(&json!({"type":"receipt","receipt":receipt}), &[])?;
                Ok(receipt)
            }
            BeginCommand::New => {
                let receipt = self.host.handle_command(command);
                self.journal
                    .finish_local_command(&receipt, &self.host.sessions())?;
                Ok(receipt)
            }
        }
    }

    pub fn poll(&mut self, remote_enabled: bool) -> Result<Vec<crate::remote_host::HostEvent>> {
        match self.host.poll() {
            Ok(events) => {
                if events.is_empty() {
                    return Ok(events);
                }
                // One transaction for the whole tick: per-event commits would
                // fsync once per drained event under `synchronous=FULL`.
                let mut local_payloads = Vec::with_capacity(events.len());
                let mut remote_payloads = Vec::with_capacity(events.len());
                let mut binding_refs = Vec::with_capacity(events.len());
                for event in &events {
                    let bindings: Vec<SessionBinding> = event.binding.iter().cloned().collect();
                    local_payloads.push(json!({"type":"event","event":event}));
                    remote_payloads.push(if remote_enabled {
                        Some(projected_remote_event(event)?)
                    } else {
                        None
                    });
                    binding_refs.push(bindings);
                }
                let batch: Vec<BatchedAppend<'_>> = local_payloads
                    .iter()
                    .zip(remote_payloads.iter())
                    .zip(binding_refs.iter())
                    .map(|((local, remote), bindings)| BatchedAppend {
                        local,
                        remote: remote.as_ref(),
                        bindings,
                    })
                    .collect();
                self.journal.append_batch(&batch)?;
                Ok(events)
            }
            Err(error) => {
                let payload = json!({"type":"host_lost","code":error.code});
                if remote_enabled {
                    self.journal.append(&payload, &self.host.sessions())?;
                } else {
                    self.journal.append_local(&payload, &self.host.sessions())?;
                }
                Err(JournalError::HostLost(error.code))
            }
        }
    }
}
