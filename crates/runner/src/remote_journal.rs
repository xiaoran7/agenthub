//! Dedicated content journal. Never mixed with the v1 metadata-only Outbox.
//! Intent commits precede native side effects; receipts and snapshots commit together.

use crate::remote_host::{
    CommandReceipt, ExecutionState, NativeSessionHost, NativeTransport, RemoteCommand,
    SessionBinding,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

type Result<T> = std::result::Result<T, JournalError>;

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("remote_journal_io")]
    Sqlite(#[from] rusqlite::Error),
    #[error("remote_journal_invalid_data")]
    Json(#[from] serde_json::Error),
    #[error("remote_journal_invalid_ack")]
    InvalidAck,
    #[error("remote_journal_identity_mismatch")]
    IdentityMismatch,
    #[error("{0}")]
    HostLost(String),
}

pub enum BeginCommand {
    New,
    Existing(CommandReceipt),
}

pub struct RemoteJournal {
    connection: Connection,
}

impl RemoteJournal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_millis(250))?;
        // One daemon per journal. Holding SQLite's exclusive lock prevents two
        // independent Hosts from dispatching the same persisted command.
        connection.execute_batch("PRAGMA locking_mode=EXCLUSIVE;
            PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS remote_commands (
                cmd_id TEXT PRIMARY KEY, command_json TEXT NOT NULL, receipt_json TEXT);
            CREATE TABLE IF NOT EXISTS remote_sessions (
                hub_id TEXT PRIMARY KEY, native_id TEXT NOT NULL UNIQUE, binding_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS remote_outbox (
                seq INTEGER PRIMARY KEY AUTOINCREMENT, payload_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS remote_watermarks (
                singleton INTEGER PRIMARY KEY CHECK(singleton=1), sent INTEGER NOT NULL, acked INTEGER NOT NULL);
            INSERT OR IGNORE INTO remote_watermarks VALUES (1,0,0);
            CREATE TABLE IF NOT EXISTS remote_inbound (singleton INTEGER PRIMARY KEY CHECK(singleton=1), received INTEGER NOT NULL);
            INSERT OR IGNORE INTO remote_inbound VALUES (1,0);
            CREATE TABLE IF NOT EXISTS remote_identity (singleton INTEGER PRIMARY KEY CHECK(singleton=1), device_id TEXT NOT NULL, server TEXT NOT NULL);
            COMMIT;")?;
        Ok(Self { connection })
    }

    pub fn bind_identity(&mut self, device_id: &str, server: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT OR IGNORE INTO remote_identity VALUES (1,?1,?2)",
            params![device_id, server],
        )?;
        let stored: (String, String) = transaction.query_row(
            "SELECT device_id,server FROM remote_identity WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if stored != (device_id.to_owned(), server.to_owned()) {
            return Err(JournalError::IdentityMismatch);
        }
        transaction.commit()?;
        Ok(())
    }

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

    pub fn finish_command(
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
        transaction.execute(
            "INSERT INTO remote_outbox(payload_json) VALUES (?1)",
            [serde_json::to_string(&payload)?],
        )?;
        let seq = transaction.last_insert_rowid();
        transaction.commit()?;
        Ok(seq)
    }

    pub fn append(&mut self, payload: &Value, sessions: &[SessionBinding]) -> Result<i64> {
        let transaction = self.connection.transaction()?;
        for binding in sessions {
            persist_binding(&transaction, binding)?;
        }
        transaction.execute(
            "INSERT INTO remote_outbox(payload_json) VALUES (?1)",
            [serde_json::to_string(payload)?],
        )?;
        let seq = transaction.last_insert_rowid();
        transaction.commit()?;
        Ok(seq)
    }

    pub fn bindings(&self) -> Result<Vec<SessionBinding>> {
        let mut statement = self
            .connection
            .prepare("SELECT binding_json FROM remote_sessions ORDER BY hub_id")?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|row| {
                let mut binding: SessionBinding = serde_json::from_str(&row?)?;
                // A registry row does not prove the previous native process is alive.
                binding.control_connected = false;
                binding.execution_state = ExecutionState::Unknown;
                Ok(binding)
            })
            .collect()
    }

    pub fn pending(&self, after: i64, limit: usize) -> Result<Vec<(i64, Value)>> {
        let mut statement = self.connection.prepare(
            "SELECT seq,payload_json FROM remote_outbox WHERE seq>?1 ORDER BY seq LIMIT ?2",
        )?;
        statement
            .query_map(params![after, limit.min(256) as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .map(|row| {
                let (seq, payload) = row?;
                Ok((seq, serde_json::from_str(&payload)?))
            })
            .collect()
    }

    pub fn watermarks(&self) -> Result<(i64, i64)> {
        Ok(self.connection.query_row(
            "SELECT sent,acked FROM remote_watermarks WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    pub fn received_command_seq(&self) -> Result<i64> {
        Ok(self.connection.query_row(
            "SELECT received FROM remote_inbound WHERE singleton=1",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn confirm_command_seq(&mut self, seq: i64) -> Result<()> {
        let received = self.received_command_seq()?;
        if seq <= received {
            return Ok(());
        }
        if seq != received + 1 {
            return Err(JournalError::InvalidAck);
        }
        self.connection.execute(
            "UPDATE remote_inbound SET received=?1 WHERE singleton=1",
            [seq],
        )?;
        Ok(())
    }

    pub fn mark_sent(&mut self, seq: i64) -> Result<()> {
        let (sent, _) = self.watermarks()?;
        if seq <= sent {
            return Ok(());
        }
        if seq != sent + 1
            || !self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM remote_outbox WHERE seq=?1)",
                [seq],
                |row| row.get::<_, bool>(0),
            )?
        {
            return Err(JournalError::InvalidAck);
        }
        self.connection.execute(
            "UPDATE remote_watermarks SET sent=?1 WHERE singleton=1",
            [seq],
        )?;
        Ok(())
    }

    pub fn acknowledge(&mut self, seq: i64) -> Result<()> {
        let (sent, acked) = self.watermarks()?;
        if seq < 0 || seq > sent {
            return Err(JournalError::InvalidAck);
        }
        if seq <= acked {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE remote_watermarks SET acked=?1 WHERE singleton=1",
            [seq],
        )?;
        transaction.execute("DELETE FROM remote_outbox WHERE seq<=?1", [seq])?;
        transaction.commit()?;
        Ok(())
    }
}

fn persist_binding(connection: &Connection, binding: &SessionBinding) -> Result<()> {
    connection.execute("INSERT INTO remote_sessions(hub_id,native_id,binding_json) VALUES (?1,?2,?3)
        ON CONFLICT(hub_id) DO UPDATE SET native_id=excluded.native_id,binding_json=excluded.binding_json",
        params![binding.hub_session_id,binding.provider_session_id,serde_json::to_string(binding)?])?;
    Ok(())
}

pub struct DurableHost<T: NativeTransport> {
    pub host: NativeSessionHost<T>,
    pub journal: RemoteJournal,
}

impl<T: NativeTransport> DurableHost<T> {
    pub fn new(mut host: NativeSessionHost<T>, journal: RemoteJournal) -> Result<Self> {
        host.restore_detached(journal.bindings()?);
        Ok(Self { host, journal })
    }

    pub fn handle_command(&mut self, command: RemoteCommand) -> Result<(CommandReceipt, i64)> {
        match self.journal.begin_command(&command)? {
            BeginCommand::Existing(receipt) => {
                let seq = self
                    .journal
                    .append(&json!({"type":"receipt","receipt":receipt}), &[])?;
                Ok((receipt, seq))
            }
            BeginCommand::New => {
                let receipt = self.host.handle_command(command);
                let seq = self
                    .journal
                    .finish_command(&receipt, &self.host.sessions())?;
                Ok((receipt, seq))
            }
        }
    }

    pub fn poll(&mut self) -> Result<Vec<crate::remote_host::HostEvent>> {
        match self.host.poll() {
            Ok(events) => {
                for event in &events {
                    let bindings = event.binding.iter().cloned().collect::<Vec<_>>();
                    self.journal
                        .append(&json!({"type":"event","event":event}), &bindings)?;
                }
                Ok(events)
            }
            Err(error) => {
                self.journal.append(
                    &json!({"type":"host_lost","code":error.code}),
                    &self.host.sessions(),
                )?;
                Err(JournalError::HostLost(error.code))
            }
        }
    }
}
