use super::*;

pub struct RemoteJournal {
    pub(crate) connection: Connection,
    pub(crate) max_local_events: i64,
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
        connection.busy_timeout(Duration::from_secs(5))?;
        // One daemon per journal. Holding SQLite's exclusive lock prevents two
        // independent Hosts from dispatching the same persisted command.
        connection.execute_batch(
            "PRAGMA locking_mode=EXCLUSIVE;
             PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;",
        )?;
        // Enable incremental auto-vacuum so trimmed events and acknowledged outbox
        // rows return freed SQLite pages without full-file `VACUUM` stalls.
        let auto_vacuum: i64 = connection.query_row("PRAGMA auto_vacuum;", [], |row| row.get(0))?;
        if auto_vacuum != 2 {
            connection.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
        }
        connection.execute_batch(
            "BEGIN IMMEDIATE;
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
            COMMIT;",
        )?;
        reclaim_freelist_if_needed(&connection, 1, AUTO_VACUUM_PAGE_BATCH);
        let _ = connection.execute_batch("PRAGMA shrink_memory;");
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
            // If re-opening in local/offline mode, keep the existing database without error.
            if (stored.1 == "local://agenthub" || stored.1.trim().is_empty())
                && (server == "local://agenthub" || server.trim().is_empty())
            {
                transaction.execute(
                    "UPDATE remote_identity SET device_id=?1 WHERE singleton=1",
                    params![device_id],
                )?;
                transaction.commit()?;
                return Ok(());
            }
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

    pub fn unbind_server(&mut self) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE remote_identity SET server='local://agenthub' WHERE singleton=1",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn rebind_device_and_server(
        &mut self,
        new_device_id: &str,
        new_server: &str,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE remote_identity SET device_id=?1, server=?2 WHERE singleton=1",
            params![new_device_id, new_server],
        )?;
        transaction.execute("DELETE FROM remote_outbox", [])?;
        transaction.execute("DELETE FROM sqlite_sequence WHERE name='remote_outbox'", [])?;
        transaction.execute(
            "UPDATE remote_watermarks SET sent=0, acked=0 WHERE singleton=1",
            [],
        )?;
        transaction.execute("UPDATE remote_inbound SET received=0 WHERE singleton=1", [])?;
        transaction.execute("DELETE FROM remote_inbound_commands", [])?;
        transaction.commit()?;
        reclaim_freelist_if_needed(
            &self.connection,
            AUTO_VACUUM_FREELIST_THRESHOLD,
            AUTO_VACUUM_PAGE_BATCH,
        );
        Ok(())
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
        let (earliest, latest): (Option<i64>, Option<i64>) = self
            .connection
            .prepare_cached("SELECT MIN(cursor),MAX(cursor) FROM local_events")?
            .query_row([], |row| Ok((row.get(0)?, row.get(1)?)))?;
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

        let mut statement = self.connection.prepare_cached(
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

    pub(crate) fn latest_event_cursor(&self) -> Result<i64> {
        Ok(self
            .connection
            .prepare_cached("SELECT COALESCE(MAX(cursor),0) FROM local_events")?
            .query_row([], |row| row.get(0))?)
    }

    pub(crate) fn stored_bindings(&self, detached: bool) -> Result<Vec<SessionBinding>> {
        let mut statement = self
            .connection
            .prepare_cached("SELECT binding_json FROM remote_sessions ORDER BY hub_id")?;
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
        let mut statement = self.connection.prepare_cached(
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
        Ok(self
            .connection
            .prepare_cached("SELECT sent,acked FROM remote_watermarks WHERE singleton=1")?
            .query_row([], |row| Ok((row.get(0)?, row.get(1)?)))?)
    }

    pub fn received_command_seq(&self) -> Result<i64> {
        Ok(self
            .connection
            .prepare_cached("SELECT received FROM remote_inbound WHERE singleton=1")?
            .query_row([], |row| row.get(0))?)
    }

    pub fn mark_sent(&mut self, seq: i64) -> Result<()> {
        let (sent, _) = self.watermarks()?;
        if seq <= sent {
            return Ok(());
        }
        if seq != sent + 1 {
            return Err(JournalError::InvalidAck);
        }
        self.mark_sent_through(seq)
    }

    /// Advances the durable `sent` watermark across a contiguous batch `(sent..=seq]`
    /// in a single SQLite write instead of fsyncing once per outbox row.
    pub fn mark_sent_through(&mut self, seq: i64) -> Result<()> {
        if seq < 0 {
            return Err(JournalError::InvalidAck);
        }
        let transaction = self.connection.transaction()?;
        let sent: i64 = transaction.query_row(
            "SELECT sent FROM remote_watermarks WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if seq <= sent {
            return Ok(());
        }
        let max_outbox_seq: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM remote_outbox",
            [],
            |row| row.get(0),
        )?;
        if seq > max_outbox_seq {
            return Err(JournalError::InvalidAck);
        }
        let expected = seq - sent;
        let present: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM remote_outbox WHERE seq>?1 AND seq<=?2",
            params![sent, seq],
            |row| row.get(0),
        )?;
        if present != expected {
            return Err(JournalError::InvalidAck);
        }
        transaction.execute(
            "UPDATE remote_watermarks SET sent=?1 WHERE singleton=1",
            [seq],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Reconciles `sent` up to `server_seq` on reconnect after validating in O(1)
    /// that `server_seq <= MAX(seq)` in `remote_outbox`.
    pub fn reconcile_sent_through(&mut self, server_seq: i64) -> Result<()> {
        self.mark_sent_through(server_seq)
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
        reclaim_freelist_if_needed(
            &self.connection,
            AUTO_VACUUM_FREELIST_THRESHOLD,
            AUTO_VACUUM_PAGE_BATCH,
        );
        Ok(())
    }

    pub fn reset_watermarks_and_outbox(&mut self, server_seq: i64) -> Result<()> {
        if server_seq < 0 {
            return Err(JournalError::InvalidAck);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM remote_outbox", [])?;
        transaction.execute("DELETE FROM sqlite_sequence WHERE name='remote_outbox'", [])?;
        if server_seq > 0 {
            transaction.execute(
                "INSERT INTO sqlite_sequence (name, seq) VALUES ('remote_outbox', ?1)",
                [server_seq],
            )?;
        }
        transaction.execute(
            "UPDATE remote_watermarks SET sent=?1, acked=?1 WHERE singleton=1",
            [server_seq],
        )?;
        transaction.commit()?;
        reclaim_freelist_if_needed(
            &self.connection,
            AUTO_VACUUM_FREELIST_THRESHOLD,
            AUTO_VACUUM_PAGE_BATCH,
        );
        Ok(())
    }

    /// Moves the applied-command watermark to `seq` and forgets everything above it.
    ///
    /// Used when the Gateway's command journal is authoritative about a shorter
    /// history than this Host remembers — a rebuilt Gateway database, a moved
    /// server, or a re-pair. Commands above the new watermark were written for a
    /// journal that no longer exists, so keeping their rows would only make the
    /// next command at those sequences look like a conflict.
    pub fn reset_inbound_command_seq(&mut self, seq: i64) -> Result<()> {
        if seq < 0 {
            return Err(JournalError::InvalidAck);
        }
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE remote_inbound SET received=?1 WHERE singleton=1",
            [seq],
        )?;
        transaction.execute("DELETE FROM remote_inbound_commands WHERE seq>?1", [seq])?;
        transaction.commit()?;
        reclaim_freelist_if_needed(
            &self.connection,
            AUTO_VACUUM_FREELIST_THRESHOLD,
            AUTO_VACUUM_PAGE_BATCH,
        );
        Ok(())
    }

    /// Reclaims up to `max_pages` freed pages and releases SQLite page-cache memory.
    pub fn reclaim_space(&mut self, max_pages: usize) -> Result<()> {
        reclaim_incremental_pages(&self.connection, max_pages)
    }
}

pub(crate) fn persist_binding(connection: &Connection, binding: &SessionBinding) -> Result<()> {
    connection.execute(
        "DELETE FROM remote_sessions WHERE native_id=?1 AND hub_id!=?2",
        params![binding.provider_session_id, binding.hub_session_id],
    )?;
    connection.execute(
        "INSERT INTO remote_sessions(hub_id,native_id,binding_json) VALUES (?1,?2,?3)
        ON CONFLICT(hub_id) DO UPDATE SET native_id=excluded.native_id,binding_json=excluded.binding_json",
        params![
            binding.hub_session_id,
            binding.provider_session_id,
            serde_json::to_string(binding)?
        ],
    )?;
    Ok(())
}

pub(crate) fn append_local_event(
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

pub(crate) const AUTO_VACUUM_FREELIST_THRESHOLD: i64 = 64;
pub(crate) const AUTO_VACUUM_PAGE_BATCH: usize = 64;

/// Drains `PRAGMA incremental_vacuum(pages)` row-by-row so all freed pages are reclaimed,
/// then passively checkpoints WAL and shrinks SQLite's page cache.
pub(crate) fn reclaim_incremental_pages(connection: &Connection, pages: usize) -> Result<()> {
    {
        let mut stmt = connection.prepare(&format!("PRAGMA incremental_vacuum({pages});"))?;
        let mut rows = stmt.query([])?;
        while rows.next()?.is_some() {}
    }
    connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE); PRAGMA shrink_memory;")?;
    Ok(())
}

/// Runs incremental vacuum and shrinks SQLite heap cache once `freelist_count` reaches `min_pages`.
/// Must be called outside an active SQLite transaction.
pub(crate) fn reclaim_freelist_if_needed(connection: &Connection, min_pages: i64, pages: usize) {
    let freelist: i64 = connection
        .query_row("PRAGMA freelist_count;", [], |row| row.get(0))
        .unwrap_or(0);
    if freelist >= min_pages {
        let _ = reclaim_incremental_pages(connection, pages);
    }
}

/// Drops events beyond the retention window, keeping the newest `max` rows.
pub(crate) fn trim_local_events(
    connection: &Connection,
    cursor: i64,
    max_local_events: i64,
) -> Result<()> {
    connection.execute(
        "DELETE FROM local_events WHERE cursor<=?1",
        [cursor.saturating_sub(max_local_events)],
    )?;
    Ok(())
}
