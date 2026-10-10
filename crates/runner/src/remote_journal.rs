//! Dedicated content journal. Never mixed with the v1 metadata-only Outbox.
//! Intent commits precede native side effects; receipts and snapshots commit together.

use crate::remote_host::{
    CommandReceipt, ExecutionState, NativeSessionHost, NativeTransport, PendingApprovalSnapshot,
    RemoteCommand, SessionBinding,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

const DEFAULT_MAX_LOCAL_EVENTS: i64 = 10_000;
const MAX_CONFIGURED_LOCAL_EVENTS: i64 = 100_000;
/// Bound on a projected provider error message. A stack-trace-sized payload would
/// otherwise travel to every subscribed client on every reconnect.
const MAX_ERROR_MESSAGE_CHARS: usize = 2_048;

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

pub enum BeginInboundCommand {
    New,
    CompleteExisting(CommandReceipt),
    AlreadyCompleted(CommandReceipt),
}

/// One host event queued for a batched journal write.
///
/// A poll tick drains many events at once. Committing each one separately means
/// one `synchronous=FULL` fsync per event, so a burst costs a fsync storm.
pub struct BatchedAppend<'a> {
    pub local: &'a Value,
    /// Remote projection; `None` keeps this event local-only.
    pub remote: Option<&'a Value>,
    pub bindings: &'a [SessionBinding],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalSnapshot {
    pub sessions: Vec<SessionBinding>,
    pub pending_approvals: Vec<PendingApprovalSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_questions: Vec<crate::remote_host::PendingQuestionSnapshot>,
    pub cursor: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEvent {
    pub cursor: i64,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EventsAfter {
    Events {
        events: Vec<JournalEvent>,
        cursor: i64,
    },
    SnapshotRequired {
        snapshot: JournalSnapshot,
    },
}

pub struct RemoteJournal {
    connection: Connection,
    max_local_events: i64,
}

impl RemoteJournal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_event_retention(path, DEFAULT_MAX_LOCAL_EVENTS as usize)
    }

    pub fn open_with_event_retention(
        path: impl AsRef<Path>,
        max_local_events: usize,
    ) -> Result<Self> {
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
            CREATE TABLE IF NOT EXISTS local_events (
                cursor INTEGER PRIMARY KEY AUTOINCREMENT, payload_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS remote_watermarks (
                singleton INTEGER PRIMARY KEY CHECK(singleton=1), sent INTEGER NOT NULL, acked INTEGER NOT NULL);
            INSERT OR IGNORE INTO remote_watermarks VALUES (1,0,0);
            CREATE TABLE IF NOT EXISTS remote_inbound (singleton INTEGER PRIMARY KEY CHECK(singleton=1), received INTEGER NOT NULL);
            INSERT OR IGNORE INTO remote_inbound VALUES (1,0);
            CREATE TABLE IF NOT EXISTS remote_inbound_commands (
                seq INTEGER PRIMARY KEY, cmd_id TEXT NOT NULL, command_json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS remote_identity (singleton INTEGER PRIMARY KEY CHECK(singleton=1), device_id TEXT NOT NULL, server TEXT NOT NULL);
            COMMIT;")?;
        Ok(Self {
            connection,
            max_local_events: i64::try_from(max_local_events)
                .unwrap_or(MAX_CONFIGURED_LOCAL_EVENTS)
                .clamp(1, MAX_CONFIGURED_LOCAL_EVENTS),
        })
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
        if stored.0 != device_id {
            return Err(JournalError::IdentityMismatch);
        }
        // If re-opening in local/offline mode, keep the existing database without error.
        if server == "local://agenthub" || server.trim().is_empty() {
            transaction.commit()?;
            return Ok(());
        }
        if stored.1 == "local://agenthub" || stored.1.trim().is_empty() {
            transaction.execute(
                "UPDATE remote_identity SET server=?1 WHERE singleton=1",
                params![server],
            )?;
        } else if stored.1 != server {
            return Err(JournalError::IdentityMismatch);
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn rebind_server_if_local(&mut self, new_server: &str) -> Result<()> {
        let transaction = self.connection.transaction()?;
        let stored: Option<(String, String)> = transaction
            .query_row(
                "SELECT device_id,server FROM remote_identity WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((_device_id, server)) = stored {
            if server == new_server {
                transaction.commit()?;
                return Ok(());
            }
            if server != "local://agenthub" && !server.trim().is_empty() {
                return Err(JournalError::IdentityMismatch);
            }
            transaction.execute(
                "UPDATE remote_identity SET server=?1 WHERE singleton=1",
                params![new_server],
            )?;
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
                return Err(JournalError::InvalidAck);
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
        Ok(last)
    }

    pub fn enqueue_remote(&mut self, payload: &Value) -> Result<i64> {
        self.connection.execute(
            "INSERT INTO remote_outbox(payload_json) VALUES (?1)",
            [serde_json::to_string(payload)?],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub fn bindings(&self) -> Result<Vec<SessionBinding>> {
        self.stored_bindings(true)
    }

    pub fn snapshot(&self) -> Result<JournalSnapshot> {
        Ok(JournalSnapshot {
            sessions: self.stored_bindings(true)?,
            pending_approvals: Vec::new(),
            pending_questions: Vec::new(),
            cursor: self.latest_event_cursor()?,
        })
    }

    pub fn events_after(&self, after_cursor: i64, limit: usize) -> Result<EventsAfter> {
        let (earliest, latest): (Option<i64>, Option<i64>) = self.connection.query_row(
            "SELECT MIN(cursor),MAX(cursor) FROM local_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let latest = latest.unwrap_or(0);
        let earliest = earliest.unwrap_or(latest + 1);
        if after_cursor < 0
            || after_cursor > latest
            || (earliest <= latest && after_cursor < earliest - 1)
        {
            return Ok(EventsAfter::SnapshotRequired {
                snapshot: self.snapshot()?,
            });
        }

        let mut statement = self.connection.prepare(
            "SELECT cursor,payload_json FROM local_events WHERE cursor>?1 ORDER BY cursor LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![after_cursor, limit.clamp(1, 256) as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
        let mut events = Vec::new();
        for row in rows {
            let (cursor, payload) = row?;
            events.push(JournalEvent {
                cursor,
                payload: serde_json::from_str(&payload)?,
            });
        }
        let cursor = events.last().map_or(after_cursor, |event| event.cursor);
        Ok(EventsAfter::Events { events, cursor })
    }

    fn latest_event_cursor(&self) -> Result<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(cursor),0) FROM local_events",
            [],
            |row| row.get(0),
        )?)
    }

    fn stored_bindings(&self, detached: bool) -> Result<Vec<SessionBinding>> {
        let mut statement = self
            .connection
            .prepare("SELECT binding_json FROM remote_sessions ORDER BY hub_id")?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|row| {
                let mut binding: SessionBinding = serde_json::from_str(&row?)?;
                if detached {
                    // A registry row does not prove the previous native process is alive.
                    binding.control_connected = false;
                    binding.execution_state = ExecutionState::Unknown;
                }
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

fn append_local_event(
    connection: &Connection,
    payload: &Value,
    max_local_events: i64,
) -> Result<i64> {
    connection.execute(
        "INSERT INTO local_events(payload_json) VALUES (?1)",
        [serde_json::to_string(payload)?],
    )?;
    let cursor = connection.last_insert_rowid();
    trim_local_events(connection, cursor, max_local_events)?;
    Ok(cursor)
}

/// Drops events beyond the retention window, keeping the newest `max` rows.
fn trim_local_events(connection: &Connection, cursor: i64, max_local_events: i64) -> Result<()> {
    connection.execute(
        "DELETE FROM local_events WHERE cursor<=?1",
        [cursor.saturating_sub(max_local_events)],
    )?;
    Ok(())
}

fn projected_remote_receipt(receipt: &CommandReceipt) -> Value {
    let result = receipt.result.as_ref().and_then(|result| {
        let mut projected = serde_json::Map::new();
        if let Some(binding) = result.get("binding") {
            projected.insert("binding".into(), binding.clone());
        }
        if let Some(turn_id) = result.pointer("/turn/id").and_then(Value::as_str) {
            projected.insert("turn".into(), json!({"id":turn_id}));
        }
        for key in ["approval_id", "response_sent"] {
            if let Some(value) = result.get(key) {
                projected.insert(key.into(), value.clone());
            }
        }
        (!projected.is_empty()).then_some(Value::Object(projected))
    });
    let mut projected = json!({
        "client_cmd_id": receipt.client_cmd_id,
        "hub_session_id": receipt.hub_session_id,
        "status": receipt.status,
        "code": receipt.code,
        "result": result,
    });
    crate::privacy::sanitize_workspace_paths_in_json(&mut projected);
    projected
}

fn projected_remote_event(event: &crate::remote_host::HostEvent) -> Result<Value> {
    let mut projected = serde_json::to_value(event)?;
    if let Some(object) = projected.as_object_mut() {
        object.remove("raw");
        if let Some(raw) = projected_event_raw(event) {
            object.insert("raw".into(), raw);
        }
    }
    crate::privacy::sanitize_workspace_paths_in_json(&mut projected);
    Ok(json!({"type":"event","event":projected}))
}

/// One-line summary of a `codexErrorInfo` kind object, e.g.
/// `{"usageLimitReached":{"httpStatusCode":429}}`.
///
/// Only the small scalars that say what to do about the failure are kept, so an
/// internal detail object cannot ride along with them. Anything unrecognised is
/// forwarded as-is rather than guessed at.
fn codex_error_summary(detail: &Value) -> Value {
    const KEEP: [&str; 4] = ["httpStatusCode", "retryAfterSeconds", "code", "message"];
    let Some(object) = detail.as_object() else {
        return detail.clone();
    };
    let mut summary = serde_json::Map::new();
    for key in KEEP {
        if let Some(value) = object.get(key)
            && (value.is_string() || value.is_number() || value.is_boolean())
        {
            let bounded = value
                .as_str()
                .map(|text| {
                    text.chars()
                        .take(MAX_ERROR_MESSAGE_CHARS)
                        .collect::<String>()
                })
                .map(Value::String)
                .unwrap_or_else(|| value.clone());
            summary.insert(key.into(), bounded);
        }
    }
    if summary.is_empty() {
        return detail.clone();
    }
    Value::Object(summary)
}

fn projected_event_raw(event: &crate::remote_host::HostEvent) -> Option<Value> {
    let params = event.raw.get("params")?;
    match event.kind.as_str() {
        "history/snapshot" => params
            .get("items")
            .and_then(Value::as_array)
            .map(|items| json!({"params":{"items":items}})),
        "turn/started" | "turn/completed" => params
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(|id| json!({"params":{"turn":{"id":id}}})),
        "item/agentMessage/delta" => {
            let item_id = params.get("itemId").and_then(Value::as_str)?;
            let delta = params.get("delta").and_then(Value::as_str)?;
            Some(json!({"params":{"itemId":item_id,"delta":delta}}))
        }
        // A turn that died must say why. Without this the remote console shows
        // "执行出错" with no cause, and the only way to learn the real reason is
        // to walk back to the machine. Only the already allowlisted, bounded
        // fields are projected; the message is then scrubbed token by token so
        // an absolute path inside the prose does not take the whole explanation
        // down with it.
        "error" => {
            let error = params.get("error")?;
            let mut projected_error = serde_json::Map::new();
            if let Some(message) = error.get("message").and_then(Value::as_str) {
                let scrubbed = crate::privacy::redact_paths_inside_text(message);
                let bounded = scrubbed
                    .chars()
                    .take(MAX_ERROR_MESSAGE_CHARS)
                    .collect::<String>();
                projected_error.insert("message".into(), json!(bounded));
            }
            // `codexErrorInfo` is an object keyed by kind (`{"usageLimitReached":{...}}`),
            // not a string, so it is projected as its kind plus whatever the kind
            // object can say in one line. An unknown shape keeps its own shape
            // rather than being guessed at.
            if let Some(info) = error.get("codexErrorInfo") {
                match info {
                    Value::Object(kinds) => {
                        let mut projected_info = serde_json::Map::new();
                        for (kind, detail) in kinds {
                            projected_info.insert(kind.clone(), json!(codex_error_summary(detail)));
                        }
                        if !projected_info.is_empty() {
                            projected_error
                                .insert("codexErrorInfo".into(), Value::Object(projected_info));
                        }
                    }
                    Value::String(text) => {
                        projected_error.insert("codexErrorInfo".into(), json!(text));
                    }
                    _ => {}
                }
            }
            if let Some(value) = error.get("type").and_then(Value::as_str) {
                projected_error.insert("type".into(), json!(value));
            }
            // An error object with nothing a human can act on is not a cause;
            // dropping it keeps the console from rendering an empty banner.
            if projected_error.is_empty() {
                return None;
            }
            let mut projected_params = serde_json::Map::new();
            projected_params.insert("error".into(), Value::Object(projected_error));
            if let Some(turn_id) = params.get("turnId").and_then(Value::as_str) {
                projected_params.insert("turnId".into(), json!(turn_id));
            }
            // `willRetry: true` means the provider is still working on it, so the
            // console must not present this as the turn's final outcome.
            if let Some(will_retry) = params.get("willRetry").and_then(Value::as_bool) {
                projected_params.insert("willRetry".into(), json!(will_retry));
            }
            Some(json!({"params": Value::Object(projected_params)}))
        }

        "item/completed" => {
            let item = params.get("item")?;
            let id = item.get("id").and_then(Value::as_str)?;
            match item.get("type").and_then(Value::as_str)? {
                "userMessage" => {
                    let mut user_item = json!({"id": id, "type": "userMessage"});
                    if let Some(content) = item.get("content") {
                        user_item["content"] = content.clone();
                    }
                    if let Some(text) = item.get("text").and_then(Value::as_str) {
                        user_item["text"] = json!(text);
                    }
                    Some(json!({"params":{"item": user_item}}))
                }
                "agentMessage" => Some(json!({"params":{"item":{
                    "id":id,"type":"agentMessage","text":item.get("text").and_then(Value::as_str).unwrap_or_default()
                }}})),
                "commandExecution" => {
                    let mut cmd_item = json!({"id": id, "type": "commandExecution"});
                    if let Some(cmd) = item.get("command") {
                        cmd_item["command"] = cmd.clone();
                    }
                    if let Some(output) = item.get("output") {
                        cmd_item["output"] = output.clone();
                    }
                    if let Some(status) = item.get("status") {
                        cmd_item["status"] = status.clone();
                    }
                    if let Some(exit_code) = item.get("exitCode") {
                        cmd_item["exitCode"] = exit_code.clone();
                    }
                    Some(json!({"params":{"item": cmd_item}}))
                }
                "thinking" => {
                    let text = item
                        .get("text")
                        .or_else(|| item.get("thought"))
                        .or_else(|| item.get("thinking"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    Some(json!({"params":{"item": {
                        "id": id,
                        "type": "thinking",
                        "text": text
                    }}}))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
#[test]
fn remote_receipt_projection_keeps_only_hub_results() {
    let receipt = CommandReceipt {
        client_cmd_id: "command-one".into(),
        hub_session_id: "hub-one".into(),
        status: "accepted".into(),
        code: None,
        result: Some(json!({
            "binding": {
                "hub_session_id": "hub-one",
                "provider_session_id": "native-one",
                "workspace_path": "C:\\Users\\alice\\private",
                "host_generation": "generation-one",
                "control_connected": true,
                "execution_state": "idle",
                "current_turn_id": null
            },
            "turn": {"id":"turn-one","status":{"type":"inProgress"}},
            "provider_response": {
                "thread":{"id":"native-one","cwd":"C:\\Users\\alice\\private"},
                "access_token":"usable-secret"
            }
        })),
    };

    let projected = projected_remote_receipt(&receipt);

    assert_eq!(projected["result"]["turn"], json!({"id":"turn-one"}));
    assert!(projected["result"].get("provider_response").is_none());
    assert!(
        projected["result"]["binding"]["workspace_path"]
            .as_str()
            .is_some_and(|workspace| workspace.starts_with("ws-"))
    );
    let serialized = projected.to_string();
    for forbidden in ["alice", "usable-secret", "provider_response", "inProgress"] {
        assert!(!serialized.contains(forbidden));
    }
    assert_eq!(
        receipt.result.as_ref().unwrap()["provider_response"]["access_token"],
        "usable-secret"
    );
}

#[cfg(test)]
#[test]
fn approval_remote_projection_omits_provider_request_payload() {
    let approval = PendingApprovalSnapshot {
        approval_id: format!("approval-{}", "a".repeat(64)),
        hub_session_id: "hub-one".into(),
        host_generation: "generation-one".into(),
        turn_id: "turn-one".into(),
        can_approve: true,
        can_decline: false,
        tool_name: Some("Bash".into()),
        command_preview: Some("cargo test".into()),
        path: None,
        description: Some("Run unit tests".into()),
        risk_level: Some("high".into()),
    };
    let event = crate::remote_host::HostEvent {
        event_id: "event-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("native-one".into()),
        kind: "approval_requested".into(),
        raw: json!({
            "id": 7,
            "method": "item/commandExecution/requestApproval",
            "params": {"command":"private","reason":"secret","diff":"sensitive"}
        }),
        binding: None,
        approval_id: Some(approval.approval_id.clone()),
        approval: Some(approval.clone()),
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    assert!(projected["event"].get("raw").is_none());
    assert_eq!(projected["event"]["approval"], json!(approval));
    for forbidden in ["private", "secret", "sensitive", "\"id\":7"] {
        assert!(!projected.to_string().contains(forbidden));
    }
    let local = json!({"type":"event","event":event});
    assert_eq!(local["event"]["raw"]["id"], 7);
    assert_eq!(local["event"]["raw"]["params"]["command"], "private");
}

#[cfg(test)]
#[test]
fn history_snapshot_projects_normalized_items_without_raw_paths() {
    let event = crate::remote_host::HostEvent {
        event_id: "history-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("sess-one".into()),
        kind: "history/snapshot".into(),
        raw: json!({"method":"history/snapshot","params":{
            "items":[{"role":"user","kind":"message","text":"检查 C:\\Users\\alice\\secret.txt","status":"completed"}],
            "provider_private":"do-not-send"
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };
    let projected = projected_remote_event(&event).unwrap();
    assert_eq!(
        projected["event"]["raw"]["params"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let encoded = projected.to_string();
    assert!(!encoded.contains("alice"));
    assert!(!encoded.contains("do-not-send"));
}

#[cfg(test)]
#[test]
fn error_events_project_the_cause_without_raw_provider_fields() {
    let event = crate::remote_host::HostEvent {
        event_id: "error-one".into(),
        host_generation: "generation-one".into(),
        hub_session_id: Some("hub-one".into()),
        provider_session_id: Some("native-one".into()),
        kind: "error".into(),
        raw: json!({"method":"error","params":{
            "threadId":"native-one",
            "turnId":"turn-one",
            "willRetry":false,
            "error":{
                "message":"The 'gpt-6-luna' model is not supported; check C:\\Users\\alice\\codex\\config.toml",
                "codexErrorInfo":{"usageLimitReached":{"httpStatusCode":429,"internalStack":"do-not-send"}},
                "additionalDetails":{"internalStack":"do-not-send"}
            }
        }}),
        binding: None,
        approval_id: None,
        approval: None,
        question_id: None,
        question: None,
    };

    let projected = projected_remote_event(&event).unwrap();

    // The console can now explain the failure instead of showing a bare state.
    let params = &projected["event"]["raw"]["params"];
    assert_eq!(params["turnId"], json!("turn-one"));
    assert_eq!(params["willRetry"], json!(false));
    let message = params["error"]["message"].as_str().unwrap();
    assert!(message.contains("gpt-6-luna"));
    // `codexErrorInfo` is an object keyed by kind, not a string: the kind survives
    // along with the one scalar that says what to do, and nothing else.
    assert_eq!(
        params["error"]["codexErrorInfo"],
        json!({"usageLimitReached":{"httpStatusCode":429}})
    );
    // Absolute paths in the provider text are rewritten, never forwarded raw.
    assert!(!message.contains("alice"));
    assert!(!message.contains("config.toml"));
    // Fields outside the allowlist stay on the machine.
    let encoded = projected.to_string();
    assert!(!encoded.contains("do-not-send"));
    assert!(!encoded.contains("internalStack"));
    assert!(!encoded.contains("additionalDetails"));
    // The local copy keeps the original for the operator's own diagnostics.
    assert_eq!(
        event.raw["params"]["error"]["additionalDetails"]["internalStack"],
        json!("do-not-send")
    );
}

#[cfg(test)]
#[test]
fn error_events_without_a_known_shape_are_dropped_rather_than_guessed() {
    for kind in [
        json!({"method":"error"}),                       // no params
        json!({"method":"error","params":{}}),           // no error object
        json!({"method":"error","params":{"error":{}}}), // no message
    ] {
        let event = crate::remote_host::HostEvent {
            event_id: "error-one".into(),
            host_generation: "generation-one".into(),
            hub_session_id: Some("hub-one".into()),
            provider_session_id: Some("native-one".into()),
            kind: "error".into(),
            raw: kind,
            binding: None,
            approval_id: None,
            approval: None,
            question_id: None,
            question: None,
        };
        let projected = projected_remote_event(&event).unwrap();
        assert!(
            projected["event"].get("raw").is_none(),
            "an unrecognised error must not invent a cause"
        );
    }
}

pub struct DurableHost<T: NativeTransport> {
    pub host: NativeSessionHost<T>,
    pub journal: RemoteJournal,
}

impl<T: NativeTransport> DurableHost<T> {
    pub fn new(mut host: NativeSessionHost<T>, journal: RemoteJournal) -> Result<Self> {
        host.restore_detached(journal.bindings()?);
        let detached = host.sessions();
        for binding in &detached {
            persist_binding(&journal.connection, binding)?;
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
