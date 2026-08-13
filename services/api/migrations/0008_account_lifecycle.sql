ALTER TABLE agent_pulse_web_sessions
  ADD COLUMN session_id uuid;

UPDATE agent_pulse_web_sessions
SET session_id = gen_random_uuid()
WHERE session_id IS NULL;

ALTER TABLE agent_pulse_web_sessions
  ALTER COLUMN session_id SET NOT NULL;

CREATE UNIQUE INDEX agent_pulse_web_sessions_session_id_idx
  ON agent_pulse_web_sessions(session_id);

CREATE TABLE agent_pulse_email_verifications (
  token_digest text PRIMARY KEY
    CHECK (token_digest ~ '^[0-9a-f]{64}$'),
  user_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  consumed_at timestamptz,
  CHECK (expires_at > created_at)
);

CREATE INDEX agent_pulse_email_verifications_user_idx
  ON agent_pulse_email_verifications(user_id, created_at DESC);

CREATE TABLE agent_pulse_login_limits (
  scope_digest text PRIMARY KEY
    CHECK (scope_digest ~ '^[0-9a-f]{64}$'),
  attempt_count integer NOT NULL
    CHECK (attempt_count BETWEEN 1 AND 1000000),
  window_started_at timestamptz NOT NULL,
  blocked_until timestamptz
);

CREATE TABLE agent_pulse_subscription_events (
  event_id uuid PRIMARY KEY,
  user_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  provider text NOT NULL
    CHECK (length(provider) BETWEEN 1 AND 40),
  provider_customer_id text NOT NULL
    CHECK (length(provider_customer_id) BETWEEN 1 AND 255),
  provider_subscription_id text NOT NULL
    CHECK (length(provider_subscription_id) BETWEEN 1 AND 255),
  plan text NOT NULL
    CHECK (plan IN ('free', 'pro', 'team')),
  status text NOT NULL
    CHECK (status IN ('trialing', 'active', 'past_due', 'canceled', 'expired')),
  current_period_end timestamptz,
  occurred_at timestamptz NOT NULL,
  received_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  applied boolean NOT NULL
);

CREATE INDEX agent_pulse_subscription_events_user_idx
  ON agent_pulse_subscription_events(user_id, occurred_at DESC);

ALTER TABLE agent_pulse_subscriptions
  ADD COLUMN last_event_at timestamptz,
  ADD COLUMN last_event_id uuid;
