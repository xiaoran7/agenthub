ALTER TABLE agent_pulse_users
  ADD COLUMN IF NOT EXISTS oauth_provider text,
  ADD COLUMN IF NOT EXISTS oauth_sub text;

CREATE UNIQUE INDEX IF NOT EXISTS agent_pulse_users_oauth_idx
  ON agent_pulse_users (oauth_provider, oauth_sub)
  WHERE oauth_provider IS NOT NULL AND oauth_sub IS NOT NULL;
