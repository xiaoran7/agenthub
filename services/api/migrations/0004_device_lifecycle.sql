ALTER TABLE agent_platform_devices
  ADD COLUMN revoked_at timestamptz;

ALTER TABLE agent_platform_devices
  ADD CONSTRAINT agent_platform_devices_revoked_after_created
  CHECK (revoked_at IS NULL OR revoked_at >= created_at);

CREATE TABLE agent_platform_binding_codes (
  code_digest text PRIMARY KEY
    CHECK (code_digest ~ '^[0-9a-f]{64}$'),
  owner_id text NOT NULL
    CHECK (length(owner_id) BETWEEN 1 AND 128),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  expires_at timestamptz NOT NULL,
  claimed_at timestamptz,
  claimed_device_id text,
  CHECK (expires_at > created_at),
  CHECK (
    (claimed_at IS NULL AND claimed_device_id IS NULL)
    OR (claimed_at IS NOT NULL AND claimed_device_id IS NOT NULL)
  )
);

CREATE INDEX agent_platform_binding_codes_owner_idx
  ON agent_platform_binding_codes(owner_id, expires_at DESC);
