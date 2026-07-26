use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    device::{
        DeviceIdentity, DeviceIdentityError, device_id_for_public_key, signature_from_hex,
        verifying_key_from_hex,
    },
    domain::{AgentType, Confidence, ObservationSource, ProjectRef, SessionState},
    outbox::{Outbox, OutboxError, OutboxRecord},
};

pub const SYNC_PROTOCOL_VERSION: u16 = 1;
pub const MAX_SYNC_BATCH_EVENTS: usize = 1_000;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error(transparent)]
    Identity(#[from] DeviceIdentityError),
    #[error(transparent)]
    Outbox(#[from] OutboxError),
    #[error("failed to encode sync payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("sync batch limit must be between 1 and {MAX_SYNC_BATCH_EVENTS}")]
    InvalidLimit,
    #[error("sync batch contains events for a different device")]
    DeviceMismatch,
    #[error("sync batch event sequence is not contiguous")]
    NonContiguousSequence,
    #[error("sync batch signature is invalid")]
    InvalidSignature,
    #[error("unsupported sync protocol version {0}")]
    UnsupportedVersion(u16),
    #[error("sync acknowledgement does not match its batch")]
    AckMismatch,
    #[error("sync acknowledgement exceeds the transmitted sequence range")]
    AckOutOfRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncSessionPayload {
    pub project: ProjectRef,
    pub state: SessionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncEvent {
    pub sequence: i64,
    pub event_id: Uuid,
    pub schema_version: u16,
    pub device_id: String,
    pub agent_type: AgentType,
    pub agent_session_id: String,
    pub event_type: SyncEventType,
    pub observed_at: DateTime<Utc>,
    pub source: ObservationSource,
    pub confidence: Confidence,
    pub adapter_version: String,
    pub payload: SyncSessionPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncEventType {
    SessionObserved,
}

impl SyncEvent {
    fn from_record(record: OutboxRecord) -> Self {
        let observation = record.observation;
        Self {
            sequence: record.sequence,
            event_id: observation.event_id,
            schema_version: observation.schema_version,
            device_id: observation.device_id,
            agent_type: observation.agent_type,
            agent_session_id: observation.agent_session_id,
            event_type: SyncEventType::SessionObserved,
            observed_at: observation.observed_at,
            source: observation.source,
            confidence: observation.confidence,
            adapter_version: env!("CARGO_PKG_VERSION").into(),
            payload: SyncSessionPayload {
                project: observation.project,
                state: observation.state,
                started_at: observation.started_at,
                last_activity_at: observation.last_activity_at,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncBatch {
    pub protocol_version: u16,
    pub batch_id: Uuid,
    pub device_id: String,
    pub public_key: String,
    pub created_at: DateTime<Utc>,
    pub first_sequence: i64,
    pub last_sequence: i64,
    pub events: Vec<SyncEvent>,
    pub signature: String,
}

#[derive(Serialize)]
struct UnsignedSyncBatch<'a> {
    protocol_version: u16,
    batch_id: Uuid,
    device_id: &'a str,
    public_key: &'a str,
    created_at: DateTime<Utc>,
    first_sequence: i64,
    last_sequence: i64,
    events: &'a [SyncEvent],
}

impl SyncBatch {
    fn unsigned(&self) -> UnsignedSyncBatch<'_> {
        UnsignedSyncBatch {
            protocol_version: self.protocol_version,
            batch_id: self.batch_id,
            device_id: &self.device_id,
            public_key: &self.public_key,
            created_at: self.created_at,
            first_sequence: self.first_sequence,
            last_sequence: self.last_sequence,
            events: &self.events,
        }
    }

    fn signing_bytes(&self) -> Result<Vec<u8>, SyncError> {
        Ok(serde_json::to_vec(&self.unsigned())?)
    }

    pub fn verify(&self) -> Result<(), SyncError> {
        if self.protocol_version != SYNC_PROTOCOL_VERSION {
            return Err(SyncError::UnsupportedVersion(self.protocol_version));
        }
        validate_sequence_range(
            &self.device_id,
            self.first_sequence,
            self.last_sequence,
            &self.events,
        )?;

        let verifying_key = verifying_key_from_hex(&self.public_key)?;
        if device_id_for_public_key(&verifying_key.to_bytes()) != self.device_id {
            return Err(SyncError::DeviceMismatch);
        }
        verifying_key
            .verify_strict(
                &self.signing_bytes()?,
                &signature_from_hex(&self.signature)?,
            )
            .map_err(|_| SyncError::InvalidSignature)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncAck {
    pub protocol_version: u16,
    pub batch_id: Uuid,
    pub device_id: String,
    pub accepted_through: i64,
}

pub fn prepare_sync_batch(
    outbox: &Outbox,
    identity: &DeviceIdentity,
    limit: usize,
) -> Result<Option<SyncBatch>, SyncError> {
    if limit == 0 || limit > MAX_SYNC_BATCH_EVENTS {
        return Err(SyncError::InvalidLimit);
    }
    let records = outbox.pending(identity.device_id(), limit)?;
    if records.is_empty() {
        return Ok(None);
    }
    let events: Vec<_> = records.into_iter().map(SyncEvent::from_record).collect();
    let first_sequence = events.first().expect("events are not empty").sequence;
    let last_sequence = events.last().expect("events are not empty").sequence;
    validate_sequence_range(identity.device_id(), first_sequence, last_sequence, &events)?;

    let mut batch = SyncBatch {
        protocol_version: SYNC_PROTOCOL_VERSION,
        batch_id: Uuid::new_v4(),
        device_id: identity.device_id().to_owned(),
        public_key: identity.public_key_hex(),
        created_at: Utc::now(),
        first_sequence,
        last_sequence,
        events,
        signature: String::new(),
    };
    batch.signature = hex::encode(identity.sign(&batch.signing_bytes()?).to_bytes());
    Ok(Some(batch))
}

pub fn apply_sync_ack(
    outbox: &mut Outbox,
    batch: &SyncBatch,
    acknowledgement: &SyncAck,
) -> Result<usize, SyncError> {
    batch.verify()?;
    if acknowledgement.protocol_version != SYNC_PROTOCOL_VERSION {
        return Err(SyncError::UnsupportedVersion(
            acknowledgement.protocol_version,
        ));
    }
    if acknowledgement.batch_id != batch.batch_id || acknowledgement.device_id != batch.device_id {
        return Err(SyncError::AckMismatch);
    }
    if acknowledgement.accepted_through < batch.first_sequence
        || acknowledgement.accepted_through > batch.last_sequence
    {
        return Err(SyncError::AckOutOfRange);
    }
    Ok(outbox.acknowledge_through(&batch.device_id, acknowledgement.accepted_through)?)
}

fn validate_sequence_range(
    device_id: &str,
    first_sequence: i64,
    last_sequence: i64,
    events: &[SyncEvent],
) -> Result<(), SyncError> {
    if events.is_empty() || events.iter().any(|event| event.device_id != device_id) {
        return Err(SyncError::DeviceMismatch);
    }
    if first_sequence != events[0].sequence
        || last_sequence != events[events.len() - 1].sequence
        || events
            .windows(2)
            .any(|pair| pair[1].sequence != pair[0].sequence + 1)
    {
        return Err(SyncError::NonContiguousSequence);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::{
        ActivityStatus, Confidence, ObservationMeta, ObservationSource, Presence, ProjectRef,
        SessionObservation, SessionState,
    };

    use super::*;

    fn observation(identity: &DeviceIdentity, session_id: &str) -> SessionObservation {
        SessionObservation::new(
            &ObservationMeta::now(identity.device_id()),
            AgentType::Codex,
            session_id,
            ProjectRef {
                display_name: "safe-project".into(),
                fingerprint: "safe-fingerprint".into(),
            },
            SessionState {
                presence: Presence::Live,
                activity_status: ActivityStatus::Working,
                raw_status: "active".into(),
                raw_status_detail: BTreeMap::new(),
                waiting_reason: None,
            },
            ObservationSource::CodexAppServer,
            Confidence::Authoritative,
        )
    }

    #[test]
    fn signed_batch_round_trips_and_rejects_tampering() {
        let identity = DeviceIdentity::generate();
        let mut outbox = Outbox::open_in_memory().unwrap();
        outbox
            .enqueue(&observation(&identity, "session-1"))
            .unwrap();
        let batch = prepare_sync_batch(&outbox, &identity, 100)
            .unwrap()
            .unwrap();

        batch.verify().unwrap();
        let mut tampered = batch;
        tampered.events[0].payload.state.raw_status = "tampered".into();
        assert!(matches!(
            tampered.verify(),
            Err(SyncError::InvalidSignature)
        ));
    }

    #[test]
    fn acknowledgement_cannot_delete_events_beyond_the_batch() {
        let identity = DeviceIdentity::generate();
        let mut outbox = Outbox::open_in_memory().unwrap();
        outbox
            .enqueue(&observation(&identity, "session-1"))
            .unwrap();
        outbox
            .enqueue(&observation(&identity, "session-2"))
            .unwrap();
        let batch = prepare_sync_batch(&outbox, &identity, 1).unwrap().unwrap();
        let invalid_ack = SyncAck {
            protocol_version: SYNC_PROTOCOL_VERSION,
            batch_id: batch.batch_id,
            device_id: batch.device_id.clone(),
            accepted_through: batch.last_sequence + 1,
        };

        assert!(matches!(
            apply_sync_ack(&mut outbox, &batch, &invalid_ack),
            Err(SyncError::AckOutOfRange)
        ));
        assert_eq!(outbox.stats().unwrap().pending_events, 2);
    }

    #[test]
    fn acknowledgement_for_another_batch_or_device_is_rejected() {
        let identity = DeviceIdentity::generate();
        let mut outbox = Outbox::open_in_memory().unwrap();
        outbox
            .enqueue(&observation(&identity, "session-1"))
            .unwrap();
        let batch = prepare_sync_batch(&outbox, &identity, 10).unwrap().unwrap();

        let wrong_batch = SyncAck {
            protocol_version: SYNC_PROTOCOL_VERSION,
            batch_id: Uuid::new_v4(),
            device_id: batch.device_id.clone(),
            accepted_through: batch.last_sequence,
        };
        assert!(matches!(
            apply_sync_ack(&mut outbox, &batch, &wrong_batch),
            Err(SyncError::AckMismatch)
        ));

        let wrong_device = SyncAck {
            protocol_version: SYNC_PROTOCOL_VERSION,
            batch_id: batch.batch_id,
            device_id: "dev_other".into(),
            accepted_through: batch.last_sequence,
        };
        assert!(matches!(
            apply_sync_ack(&mut outbox, &batch, &wrong_device),
            Err(SyncError::AckMismatch)
        ));
        assert_eq!(outbox.stats().unwrap().pending_events, 1);
    }

    #[test]
    fn valid_ack_removes_only_confirmed_events() {
        let identity = DeviceIdentity::generate();
        let mut outbox = Outbox::open_in_memory().unwrap();
        outbox
            .enqueue(&observation(&identity, "session-1"))
            .unwrap();
        outbox
            .enqueue(&observation(&identity, "session-2"))
            .unwrap();
        let batch = prepare_sync_batch(&outbox, &identity, 1).unwrap().unwrap();
        let acknowledgement = SyncAck {
            protocol_version: SYNC_PROTOCOL_VERSION,
            batch_id: batch.batch_id,
            device_id: batch.device_id.clone(),
            accepted_through: batch.last_sequence,
        };

        assert_eq!(
            apply_sync_ack(&mut outbox, &batch, &acknowledgement).unwrap(),
            1
        );
        let pending = outbox.pending(identity.device_id(), 10).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].sequence, 2);
        assert_eq!(
            apply_sync_ack(&mut outbox, &batch, &acknowledgement).unwrap(),
            0
        );
    }

    #[test]
    fn batch_never_contains_the_private_key() {
        let directory = tempfile::tempdir().unwrap();
        let identity_path = directory.path().join("identity.json");
        let identity = DeviceIdentity::generate();
        identity.save_new(&identity_path).unwrap();
        let persisted = serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(identity_path).unwrap(),
        )
        .unwrap();
        let private_key = persisted["private_key"].as_str().unwrap();
        let mut outbox = Outbox::open_in_memory().unwrap();
        outbox
            .enqueue(&observation(&identity, "session-1"))
            .unwrap();

        let batch = prepare_sync_batch(&outbox, &identity, 10).unwrap().unwrap();
        assert!(!serde_json::to_string(&batch).unwrap().contains(private_key));
    }

    #[test]
    fn typescript_generated_protocol_vector_verifies_in_rust() {
        let batch: SyncBatch = serde_json::from_str(include_str!(
            "../../../packages/contracts/fixtures/sync-batch-v1.json"
        ))
        .unwrap();

        batch.verify().unwrap();
    }
}
