CREATE TABLE agent_pulse_password_resets (
  token_digest text PRIMARY KEY
    CHECK (token_digest ~ '^[0-9a-f]{64}$'),
  user_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  consumed_at timestamptz,
  CHECK (expires_at > created_at)
);

CREATE INDEX agent_pulse_password_resets_user_idx
  ON agent_pulse_password_resets(user_id, created_at DESC);
