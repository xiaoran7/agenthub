CREATE TABLE agent_platform_auth_snapshots (
  device_id text NOT NULL
    REFERENCES agent_platform_devices(device_id) ON DELETE CASCADE,
  agent_type text NOT NULL
    CHECK (agent_type IN ('claude_code', 'codex')),
  checked_at timestamptz NOT NULL,
  received_at timestamptz NOT NULL,
  snapshot_data jsonb NOT NULL,
  PRIMARY KEY (device_id, agent_type)
);

CREATE INDEX agent_platform_auth_snapshots_checked_idx
  ON agent_platform_auth_snapshots(device_id, checked_at DESC);
