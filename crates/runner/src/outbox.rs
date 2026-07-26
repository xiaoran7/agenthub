use std::path::Path;

use crate::domain::{AgentType, SessionObservation, merge_observations};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("observation serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid persisted timestamp: {0}")]
    Timestamp(#[from] chrono::ParseError),
}

impl OutboxError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => "outbox_sqlite_failed",
            Self::Json(_) => "outbox_serialization_failed",
            Self::Timestamp(_) => "outbox_timestamp_invalid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxRecord {
    pub sequence: i64,
    pub observation: SessionObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxStats {
    pub pending_events: u64,
    pub session_snapshots: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterHealth {
    pub agent_type: AgentType,
    pub status: String,
    pub consecutive_failures: u32,
    pub last_attempt_at: DateTime<Utc>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub next_attempt_at: DateTime<Utc>,
    pub last_error_code: Option<String>,
}

pub struct Outbox {
    connection: Connection,
}

impl Outbox {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, OutboxError> {
        let connection = Connection::open(path)?;
        let outbox = Self { connection };
        outbox.initialize()?;
        Ok(outbox)
    }

    pub fn open_in_memory() -> Result<Self, OutboxError> {
        let connection = Connection::open_in_memory()?;
        let outbox = Self { connection };
        outbox.initialize()?;
        Ok(outbox)
    }

    fn initialize(&self) -> Result<(), OutboxError> {
        // The run daemon and hook ingest processes share this database; wait
        // for a writer to finish instead of failing on the first SQLITE_BUSY.
        self.connection
            .busy_timeout(std::time::Duration::from_secs(5))?;
        self.connection.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS outbox_events (
                event_id TEXT PRIMARY KEY,
                device_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                observed_at TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(device_id, sequence)
            );

            CREATE INDEX IF NOT EXISTS idx_outbox_device_sequence
                ON outbox_events(device_id, sequence);

            CREATE TABLE IF NOT EXISTS device_sequences (
                device_id TEXT PRIMARY KEY,
                last_sequence INTEGER NOT NULL
            );

            INSERT INTO device_sequences(device_id, last_sequence)
            SELECT device_id, MAX(sequence)
            FROM outbox_events
            GROUP BY device_id
            ON CONFLICT(device_id) DO UPDATE SET
                last_sequence = MAX(device_sequences.last_sequence, excluded.last_sequence);

            CREATE TABLE IF NOT EXISTS session_snapshots (
                session_key TEXT PRIMARY KEY,
                device_id TEXT NOT NULL,
                agent_type TEXT NOT NULL,
                agent_session_id TEXT NOT NULL,
                observed_at TEXT NOT NULL,
                payload TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_snapshots_device
                ON session_snapshots(device_id);

            CREATE TABLE IF NOT EXISTS adapter_health (
                agent_type TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                consecutive_failures INTEGER NOT NULL,
                last_attempt_at TEXT NOT NULL,
                last_success_at TEXT,
                next_attempt_at TEXT NOT NULL,
                last_error_code TEXT
            );
            "#,
        )?;
        Ok(())
    }

    pub fn enqueue(&mut self, observation: &SessionObservation) -> Result<i64, OutboxError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let event_id = observation.event_id.to_string();

        if let Some(existing_sequence) = transaction
            .query_row(
                "SELECT sequence FROM outbox_events WHERE event_id = ?1",
                [&event_id],
                |row| row.get(0),
            )
            .optional()?
        {
            transaction.commit()?;
            return Ok(existing_sequence);
        }

        let last_sequence = transaction
            .query_row(
                "SELECT last_sequence FROM device_sequences WHERE device_id = ?1",
                [&observation.device_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        let next_sequence = last_sequence + 1;
        transaction.execute(
            r#"
            INSERT INTO device_sequences(device_id, last_sequence)
            VALUES (?1, ?2)
            ON CONFLICT(device_id) DO UPDATE SET last_sequence = excluded.last_sequence
            "#,
            params![observation.device_id, next_sequence],
        )?;

        // Merge with the previous snapshot before persisting so the synced
        // event stream carries the same preserved state as the local snapshot
        // (for example a Codex notLoaded probe keeps the last known activity
        // instead of erasing it in the cloud view).
        let session_key = observation.session_key();
        let previous = transaction
            .query_row(
                "SELECT payload FROM session_snapshots WHERE session_key = ?1",
                [&session_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str::<SessionObservation>(&payload))
            .transpose()?;
        let snapshot = previous
            .as_ref()
            .map(|previous| merge_observations(previous, observation))
            .unwrap_or_else(|| observation.clone());
        let snapshot_payload = serde_json::to_string(&snapshot)?;
        // The event keeps this observation's identity and time; only the
        // session state fields come from the merge. Reusing the merged
        // snapshot verbatim could smuggle the previous event's ID into a new
        // sequence when an out-of-order observation arrives.
        let mut event_observation = snapshot.clone();
        event_observation.event_id = observation.event_id;
        event_observation.observed_at = observation.observed_at;
        let event_payload = serde_json::to_string(&event_observation)?;
        transaction.execute(
            r#"
            INSERT INTO outbox_events(event_id, device_id, sequence, observed_at, payload)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                event_id,
                event_observation.device_id,
                next_sequence,
                event_observation.observed_at.to_rfc3339(),
                event_payload
            ],
        )?;
        transaction.execute(
            r#"
            INSERT INTO session_snapshots(
                session_key, device_id, agent_type, agent_session_id, observed_at, payload
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(session_key) DO UPDATE SET
                observed_at = excluded.observed_at,
                payload = excluded.payload
            "#,
            params![
                session_key,
                snapshot.device_id,
                snapshot.agent_type.as_str(),
                snapshot.agent_session_id,
                snapshot.observed_at.to_rfc3339(),
                snapshot_payload
            ],
        )?;

        transaction.commit()?;
        Ok(next_sequence)
    }

    pub fn enqueue_if_changed(
        &mut self,
        observation: &SessionObservation,
    ) -> Result<Option<i64>, OutboxError> {
        if let Some(previous) = self.snapshot(&observation.session_key())? {
            let merged = merge_observations(&previous, observation);
            if previous.has_same_snapshot_payload(&merged) {
                return Ok(None);
            }
        }
        self.enqueue(observation).map(Some)
    }

    pub fn pending(&self, device_id: &str, limit: usize) -> Result<Vec<OutboxRecord>, OutboxError> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT sequence, payload
            FROM outbox_events
            WHERE device_id = ?1
            ORDER BY sequence ASC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![device_id, limit as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        rows.map(|row| {
            let (sequence, payload) = row?;
            Ok(OutboxRecord {
                sequence,
                observation: serde_json::from_str(&payload)?,
            })
        })
        .collect()
    }

    pub fn acknowledge_through(
        &mut self,
        device_id: &str,
        sequence: i64,
    ) -> Result<usize, OutboxError> {
        Ok(self.connection.execute(
            "DELETE FROM outbox_events WHERE device_id = ?1 AND sequence <= ?2",
            params![device_id, sequence],
        )?)
    }

    pub fn snapshot(&self, session_key: &str) -> Result<Option<SessionObservation>, OutboxError> {
        self.connection
            .query_row(
                "SELECT payload FROM session_snapshots WHERE session_key = ?1",
                [session_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|payload| serde_json::from_str(&payload))
            .transpose()
            .map_err(Into::into)
    }

    pub fn stats(&self) -> Result<OutboxStats, OutboxError> {
        let pending_events = u64::try_from(self.connection.query_row(
            "SELECT COUNT(*) FROM outbox_events",
            [],
            |row| row.get::<_, i64>(0),
        )?)
        .expect("SQLite COUNT cannot be negative");
        let session_snapshots = u64::try_from(self.connection.query_row(
            "SELECT COUNT(*) FROM session_snapshots",
            [],
            |row| row.get::<_, i64>(0),
        )?)
        .expect("SQLite COUNT cannot be negative");
        Ok(OutboxStats {
            pending_events,
            session_snapshots,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_adapter_health(
        &self,
        agent_type: AgentType,
        attempted_at: DateTime<Utc>,
        succeeded: bool,
        consecutive_failures: u32,
        next_attempt_at: DateTime<Utc>,
        error_code: Option<&str>,
    ) -> Result<(), OutboxError> {
        let status = if succeeded { "healthy" } else { "backoff" };
        let last_success_at = succeeded.then(|| attempted_at.to_rfc3339());
        self.connection.execute(
            r#"
            INSERT INTO adapter_health(
                agent_type,
                status,
                consecutive_failures,
                last_attempt_at,
                last_success_at,
                next_attempt_at,
                last_error_code
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(agent_type) DO UPDATE SET
                status = excluded.status,
                consecutive_failures = excluded.consecutive_failures,
                last_attempt_at = excluded.last_attempt_at,
                last_success_at = COALESCE(
                    excluded.last_success_at,
                    adapter_health.last_success_at
                ),
                next_attempt_at = excluded.next_attempt_at,
                last_error_code = excluded.last_error_code
            "#,
            params![
                agent_type.as_str(),
                status,
                consecutive_failures,
                attempted_at.to_rfc3339(),
                last_success_at,
                next_attempt_at.to_rfc3339(),
                error_code,
            ],
        )?;
        Ok(())
    }

    pub fn adapter_health(
        &self,
        agent_type: AgentType,
    ) -> Result<Option<AdapterHealth>, OutboxError> {
        self.connection
            .query_row(
                r#"
                SELECT
                    status,
                    consecutive_failures,
                    last_attempt_at,
                    last_success_at,
                    next_attempt_at,
                    last_error_code
                FROM adapter_health
                WHERE agent_type = ?1
                "#,
                [agent_type.as_str()],
                |row| {
                    let consecutive_failures = row.get::<_, u32>(1)?;
                    let last_attempt_at = row.get::<_, String>(2)?;
                    let last_success_at = row.get::<_, Option<String>>(3)?;
                    let next_attempt_at = row.get::<_, String>(4)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        consecutive_failures,
                        last_attempt_at,
                        last_success_at,
                        next_attempt_at,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?
            .map(
                |(
                    status,
                    consecutive_failures,
                    last_attempt_at,
                    last_success_at,
                    next_attempt_at,
                    last_error_code,
                )|
                 -> Result<AdapterHealth, chrono::ParseError> {
                    Ok(AdapterHealth {
                        agent_type,
                        status,
                        consecutive_failures,
                        last_attempt_at: DateTime::parse_from_rfc3339(&last_attempt_at)?
                            .with_timezone(&Utc),
                        last_success_at: last_success_at
                            .map(|value| DateTime::parse_from_rfc3339(&value))
                            .transpose()?
                            .map(|value| value.with_timezone(&Utc)),
                        next_attempt_at: DateTime::parse_from_rfc3339(&next_attempt_at)?
                            .with_timezone(&Utc),
                        last_error_code,
                    })
                },
            )
            .transpose()
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Duration;

    use crate::domain::{
        ActivityStatus, AgentType, Confidence, ObservationMeta, ObservationSource, Presence,
        ProjectRef, SessionState,
    };

    use super::*;

    fn observation(status: ActivityStatus) -> SessionObservation {
        let meta = ObservationMeta::now("device-1");
        SessionObservation::new(
            &meta,
            AgentType::ClaudeCode,
            "session-1",
            ProjectRef {
                display_name: "demo".into(),
                fingerprint: "abc".into(),
            },
            SessionState {
                presence: Presence::Live,
                activity_status: status,
                raw_status: "fixture".into(),
                raw_status_detail: BTreeMap::new(),
                waiting_reason: None,
            },
            ObservationSource::ClaudeHook,
            Confidence::Observed,
        )
    }

    #[test]
    fn enqueues_orders_and_acknowledges_events() {
        let mut outbox = Outbox::open_in_memory().unwrap();
        let first = observation(ActivityStatus::Working);
        let mut second = observation(ActivityStatus::Completed);
        second.observed_at = first.observed_at + Duration::seconds(1);

        assert_eq!(outbox.enqueue(&first).unwrap(), 1);
        assert_eq!(outbox.enqueue(&second).unwrap(), 2);
        assert_eq!(outbox.pending("device-1", 10).unwrap().len(), 2);

        assert_eq!(outbox.acknowledge_through("device-1", 1).unwrap(), 1);
        let pending = outbox.pending("device-1", 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sequence, 2);

        let mut third = observation(ActivityStatus::Idle);
        third.observed_at = second.observed_at + Duration::seconds(1);
        assert_eq!(outbox.enqueue(&third).unwrap(), 3);
    }

    #[test]
    fn enqueue_is_idempotent_by_event_id() {
        let mut outbox = Outbox::open_in_memory().unwrap();
        let observation = observation(ActivityStatus::Working);

        assert_eq!(outbox.enqueue(&observation).unwrap(), 1);
        assert_eq!(outbox.enqueue(&observation).unwrap(), 1);
        assert_eq!(outbox.stats().unwrap().pending_events, 1);
    }

    #[test]
    fn unknown_activity_does_not_erase_snapshot_activity() {
        let mut outbox = Outbox::open_in_memory().unwrap();
        let first = observation(ActivityStatus::Working);
        let mut second = observation(ActivityStatus::Unknown);
        second.observed_at = first.observed_at + Duration::seconds(1);
        second.state.presence = Presence::Persisted;

        outbox.enqueue(&first).unwrap();
        outbox.enqueue(&second).unwrap();

        let snapshot = outbox.snapshot(&first.session_key()).unwrap().unwrap();
        assert_eq!(snapshot.state.activity_status, ActivityStatus::Working);
        assert_eq!(snapshot.state.presence, Presence::Persisted);
    }

    #[test]
    fn enqueued_events_carry_the_merged_state_with_their_own_identity() {
        let mut outbox = Outbox::open_in_memory().unwrap();
        let first = observation(ActivityStatus::Working);
        let mut second = observation(ActivityStatus::Unknown);
        second.observed_at = first.observed_at + Duration::seconds(1);
        second.state.presence = Presence::Unknown;

        outbox.enqueue(&first).unwrap();
        outbox.enqueue(&second).unwrap();

        let pending = outbox.pending("device-1", 10).unwrap();
        assert_eq!(pending.len(), 2);
        // The synced event stream preserves the last known activity exactly
        // like the local snapshot, so the cloud view cannot regress to
        // UNKNOWN while the local view still shows Working.
        let uploaded = &pending[1].observation;
        assert_eq!(uploaded.event_id, second.event_id);
        assert_eq!(uploaded.observed_at, second.observed_at);
        assert_eq!(uploaded.state.activity_status, ActivityStatus::Working);
        assert_eq!(uploaded.state.presence, Presence::Unknown);
    }

    #[test]
    fn stats_start_empty() {
        let outbox = Outbox::open_in_memory().unwrap();
        assert_eq!(
            outbox.stats().unwrap(),
            OutboxStats {
                pending_events: 0,
                session_snapshots: 0
            }
        );
    }

    #[test]
    fn adapter_health_preserves_last_success_across_failures() {
        let outbox = Outbox::open_in_memory().unwrap();
        let attempted_at = Utc::now();
        let next_attempt_at = attempted_at + Duration::seconds(10);
        outbox
            .record_adapter_health(
                AgentType::Codex,
                attempted_at,
                true,
                0,
                next_attempt_at,
                None,
            )
            .unwrap();
        let failed_at = attempted_at + Duration::seconds(10);
        outbox
            .record_adapter_health(
                AgentType::Codex,
                failed_at,
                false,
                1,
                failed_at + Duration::seconds(2),
                Some("permission_denied"),
            )
            .unwrap();

        let health = outbox.adapter_health(AgentType::Codex).unwrap().unwrap();
        assert_eq!(health.status, "backoff");
        assert_eq!(health.consecutive_failures, 1);
        assert_eq!(health.last_success_at, Some(attempted_at));
        assert_eq!(health.last_error_code.as_deref(), Some("permission_denied"));
    }

    #[test]
    fn unchanged_poll_observation_does_not_grow_outbox() {
        let mut outbox = Outbox::open_in_memory().unwrap();
        let first = observation(ActivityStatus::Working);
        let mut repeated = first.clone();
        repeated.event_id = uuid::Uuid::new_v4();
        repeated.observed_at += Duration::seconds(2);

        assert_eq!(outbox.enqueue_if_changed(&first).unwrap(), Some(1));
        assert_eq!(outbox.enqueue_if_changed(&repeated).unwrap(), None);
        assert_eq!(outbox.stats().unwrap().pending_events, 1);
    }
}
