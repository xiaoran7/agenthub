use super::*;

impl RemoteJournal {
    pub fn begin_command(&mut self, command: &RemoteCommand) -> Result<BeginCommand> {
        let transaction = self.connection.transaction()?;
        let previous: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT command_json,receipt_json FROM remote_commands WHERE cmd_id=?1",
                [&command.client_cmd_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((stored, receipt)) = previous {
            let stored: RemoteCommand = serde_json::from_str(&stored)?;
            let receipt = if stored != *command {
                CommandReceipt {
                    client_cmd_id: command.client_cmd_id.clone(),
                    hub_session_id: command.hub_session_id.clone(),
                    status: "rejected".into(),
                    code: Some("command_id_conflict".into()),
                    result: None,
                }
            } else if let Some(receipt) = receipt {
                serde_json::from_str(&receipt)?
            } else {
                CommandReceipt {
                    client_cmd_id: command.client_cmd_id.clone(),
                    hub_session_id: command.hub_session_id.clone(),
                    status: "indeterminate".into(),
                    code: Some("previous_execution_unconfirmed".into()),
                    result: None,
                }
            };
            return Ok(BeginCommand::Existing(receipt));
        }
        transaction.execute(
            "INSERT INTO remote_commands(cmd_id,command_json) VALUES (?1,?2)",
            params![command.client_cmd_id, serde_json::to_string(command)?],
        )?;
        transaction.commit()?;
        Ok(BeginCommand::New)
    }

    pub fn begin_inbound_command(
        &mut self,
        seq: i64,
        command: &RemoteCommand,
    ) -> Result<BeginInboundCommand> {
        let transaction = self.connection.transaction()?;
        let received: i64 = transaction.query_row(
            "SELECT received FROM remote_inbound WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if seq < 1 || seq > received + 1 {
            return Err(JournalError::InvalidAck);
        }
        let serialized = serde_json::to_string(command)?;
        let inbound: Option<(String, String)> = transaction
            .query_row(
                "SELECT cmd_id,command_json FROM remote_inbound_commands WHERE seq=?1",
                [seq],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((stored_id, stored)) = inbound {
            if stored_id != command.client_cmd_id || stored != serialized {
                // The Gateway re-issued a sequence whose previous occupant this
                // journal had already *completed*. That happens when its command
                // journal was rebuilt (database reset, server move, or a
                // re-pair) and its sequence restarted while this journal still
                // remembered the old commands — the watermark was rewound but
                // the rows above it were not. The sequence, not the vanished
                // command, is what the two sides now agree on, so the newer
                // command replaces the row. Without this every reconnect
                // re-delivers the same frame, fails the same comparison, and
                // fences the transport for the life of the process.
                //
                // Two neighbouring cases must keep failing instead:
                // - a mismatch at or below the applied watermark, which is a
                //   real protocol fault about commands already acknowledged;
                // - a stored intent with no receipt, which may have already
                //   executed, so a different command at that sequence could
                //   silently double-execute.
                let completed = transaction
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM remote_commands WHERE cmd_id=?1 AND receipt_json IS NOT NULL)",
                        [&stored_id],
                        |row| row.get::<_, bool>(0),
                    )?;
                if seq <= received || !completed {
                    return Err(JournalError::InvalidAck);
                }
                transaction.execute(
                    "UPDATE remote_inbound_commands SET cmd_id=?2,command_json=?3 WHERE seq=?1",
                    params![seq, command.client_cmd_id, serialized],
                )?;
            }
        } else {
            if seq <= received {
                return Err(JournalError::InvalidAck);
            }
            transaction.execute(
                "INSERT INTO remote_inbound_commands(seq,cmd_id,command_json) VALUES (?1,?2,?3)",
                params![seq, command.client_cmd_id, serialized],
            )?;
        }

        let previous: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT command_json,receipt_json FROM remote_commands WHERE cmd_id=?1",
                [&command.client_cmd_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let result = if let Some((stored, receipt)) = previous {
            if stored != serde_json::to_string(command)? {
                return Err(JournalError::InvalidAck);
            }
            if let Some(receipt) = receipt {
                let receipt = serde_json::from_str(&receipt)?;
                if seq <= received {
                    BeginInboundCommand::AlreadyCompleted(receipt)
                } else {
                    BeginInboundCommand::CompleteExisting(receipt)
                }
            } else {
                BeginInboundCommand::CompleteExisting(CommandReceipt {
                    client_cmd_id: command.client_cmd_id.clone(),
                    hub_session_id: command.hub_session_id.clone(),
                    status: "indeterminate".into(),
                    code: Some("previous_execution_unconfirmed".into()),
                    result: None,
                })
            }
        } else {
            if seq <= received {
                return Err(JournalError::InvalidAck);
            }
            transaction.execute(
                "INSERT INTO remote_commands(cmd_id,command_json) VALUES (?1,?2)",
                params![command.client_cmd_id, serde_json::to_string(command)?],
            )?;
            BeginInboundCommand::New
        };
        transaction.commit()?;
        Ok(result)
    }

    pub fn finish_inbound_command(
        &mut self,
        seq: i64,
        receipt: &CommandReceipt,
        sessions: &[SessionBinding],
    ) -> Result<i64> {
        let transaction = self.connection.transaction()?;
        let received: i64 = transaction.query_row(
            "SELECT received FROM remote_inbound WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if seq != received + 1 {
            return Err(JournalError::InvalidAck);
        }
        let inbound_cmd_id: Option<String> = transaction
            .query_row(
                "SELECT cmd_id FROM remote_inbound_commands WHERE seq=?1",
                [seq],
                |row| row.get(0),
            )
            .optional()?;
        if inbound_cmd_id.as_deref() != Some(&receipt.client_cmd_id) {
            return Err(JournalError::InvalidAck);
        }
        transaction.execute(
            "UPDATE remote_commands SET receipt_json=?2 WHERE cmd_id=?1",
            params![receipt.client_cmd_id, serde_json::to_string(receipt)?],
        )?;
        for binding in sessions {
            persist_binding(&transaction, binding)?;
        }
        let remote_payload = json!({"type":"receipt","receipt":projected_remote_receipt(receipt)});
        transaction.execute(
            "INSERT INTO remote_outbox(payload_json) VALUES (?1)",
            [serde_json::to_string(&remote_payload)?],
        )?;
        let outbox_seq = transaction.last_insert_rowid();
        let local_payload = json!({"type":"receipt","receipt":receipt});
        append_local_event(&transaction, &local_payload, self.max_local_events)?;
        transaction.execute(
            "UPDATE remote_inbound SET received=?1 WHERE singleton=1",
            [seq],
        )?;
        transaction.commit()?;
        Ok(outbox_seq)
    }

    pub fn finish_local_command(
        &mut self,
        receipt: &CommandReceipt,
        sessions: &[SessionBinding],
    ) -> Result<i64> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE remote_commands SET receipt_json=?2 WHERE cmd_id=?1",
            params![receipt.client_cmd_id, serde_json::to_string(receipt)?],
        )?;
        for binding in sessions {
            persist_binding(&transaction, binding)?;
        }
        let payload = json!({"type":"receipt","receipt":receipt});
        let cursor = append_local_event(&transaction, &payload, self.max_local_events)?;
        transaction.commit()?;
        Ok(cursor)
    }

    pub fn append_projected(
        &mut self,
        local_payload: &Value,
        remote_payload: &Value,
        sessions: &[SessionBinding],
    ) -> Result<i64> {
        let transaction = self.connection.transaction()?;
        for binding in sessions {
            persist_binding(&transaction, binding)?;
        }
        transaction.execute(
            "INSERT INTO remote_outbox(payload_json) VALUES (?1)",
            [serde_json::to_string(remote_payload)?],
        )?;
        let seq = transaction.last_insert_rowid();
        append_local_event(&transaction, local_payload, self.max_local_events)?;
        transaction.commit()?;
        reclaim_freelist_if_needed(
            &self.connection,
            AUTO_VACUUM_FREELIST_THRESHOLD,
            AUTO_VACUUM_PAGE_BATCH,
        );
        Ok(seq)
    }

    pub fn append(&mut self, payload: &Value, sessions: &[SessionBinding]) -> Result<i64> {
        self.append_projected(payload, payload, sessions)
    }

    pub fn append_local(&mut self, payload: &Value, sessions: &[SessionBinding]) -> Result<i64> {
        let transaction = self.connection.transaction()?;
        for binding in sessions {
            persist_binding(&transaction, binding)?;
        }
        let cursor = append_local_event(&transaction, payload, self.max_local_events)?;
        transaction.commit()?;
        reclaim_freelist_if_needed(
            &self.connection,
            AUTO_VACUUM_FREELIST_THRESHOLD,
            AUTO_VACUUM_PAGE_BATCH,
        );
        Ok(cursor)
    }

    /// Commits a whole poll tick in one transaction.
    ///
    /// Returns the local event cursor of the last appended event. Trimming runs
    /// once against that final cursor, so a batch never drops events it just
    /// wrote the way per-event appends would.
    pub fn append_batch(&mut self, batch: &[BatchedAppend<'_>]) -> Result<i64> {
        if batch.is_empty() {
            return self.latest_event_cursor();
        }
        let transaction = self.connection.transaction()?;
        let mut last = 0i64;
        for item in batch {
            for binding in item.bindings {
                persist_binding(&transaction, binding)?;
            }
            if let Some(remote) = item.remote {
                transaction.execute(
                    "INSERT INTO remote_outbox(payload_json) VALUES (?1)",
                    [serde_json::to_string(remote)?],
                )?;
            }
            transaction.execute(
                "INSERT INTO local_events(payload_json) VALUES (?1)",
                [serde_json::to_string(item.local)?],
            )?;
            last = transaction.last_insert_rowid();
        }
        trim_local_events(&transaction, last, self.max_local_events)?;
        transaction.commit()?;
        reclaim_freelist_if_needed(
            &self.connection,
            AUTO_VACUUM_FREELIST_THRESHOLD,
            AUTO_VACUUM_PAGE_BATCH,
        );
        Ok(last)
    }

    pub fn enqueue_remote(&mut self, payload: &Value) -> Result<i64> {
        self.connection.execute(
            "INSERT INTO remote_outbox(payload_json) VALUES (?1)",
            [serde_json::to_string(payload)?],
        )?;
        Ok(self.connection.last_insert_rowid())
    }
}
