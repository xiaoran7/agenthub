CREATE TABLE agent_platform_devices (
  device_id text PRIMARY KEY
    CHECK (device_id ~ '^dev_[0-9a-f]{32}$'),
  public_key text NOT NULL
    CHECK (public_key ~ '^[0-9a-f]{64}$'),
  owner_id text NOT NULL
    CHECK (length(owner_id) BETWEEN 1 AND 128),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  last_sequence bigint NOT NULL DEFAULT 0
    CHECK (last_sequence >= 0)
);

CREATE INDEX agent_platform_devices_owner_id_idx
  ON agent_platform_devices(owner_id, device_id);

CREATE TABLE agent_platform_events (
  event_id uuid PRIMARY KEY,
  device_id text NOT NULL
    REFERENCES agent_platform_devices(device_id) ON DELETE CASCADE,
  sequence bigint NOT NULL
    CHECK (sequence > 0),
  observed_at timestamptz NOT NULL,
  received_at timestamptz NOT NULL,
  event_data jsonb NOT NULL,
  UNIQUE (device_id, sequence)
);

CREATE INDEX agent_platform_events_device_observed_idx
  ON agent_platform_events(device_id, observed_at DESC, sequence DESC);

CREATE TABLE agent_platform_sessions (
  device_id text NOT NULL
    REFERENCES agent_platform_devices(device_id) ON DELETE CASCADE,
  agent_type text NOT NULL
    CHECK (agent_type IN ('claude_code', 'codex')),
  agent_session_id text NOT NULL
    CHECK (length(agent_session_id) BETWEEN 1 AND 512),
  observed_at timestamptz NOT NULL,
  received_at timestamptz NOT NULL,
  sequence bigint NOT NULL
    CHECK (sequence > 0),
  confidence text NOT NULL
    CHECK (confidence IN ('unknown', 'inferred', 'observed', 'authoritative')),
  presence text NOT NULL
    CHECK (presence IN ('LIVE', 'PERSISTED', 'UNKNOWN')),
  activity_status text NOT NULL
    CHECK (
      activity_status IN (
        'WORKING',
        'WAITING_INPUT',
        'WAITING_PERMISSION',
        'IDLE',
        'COMPLETED',
        'FAILED',
        'STOPPED',
        'UNKNOWN'
      )
    ),
  event_data jsonb NOT NULL,
  PRIMARY KEY (device_id, agent_type, agent_session_id)
);

CREATE INDEX agent_platform_sessions_owner_filters_idx
  ON agent_platform_sessions(device_id, presence, activity_status, observed_at DESC);
