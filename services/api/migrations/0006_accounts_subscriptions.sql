CREATE TABLE agent_pulse_users (
  user_id uuid PRIMARY KEY,
  email text NOT NULL,
  password_hash text NOT NULL,
  display_name text NOT NULL
    CHECK (length(display_name) BETWEEN 1 AND 80),
  status text NOT NULL DEFAULT 'active'
    CHECK (status IN ('active', 'suspended', 'deleted')),
  email_verified_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE UNIQUE INDEX agent_pulse_users_email_unique_idx
  ON agent_pulse_users (lower(email));

CREATE TABLE agent_pulse_subscriptions (
  user_id uuid PRIMARY KEY
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  plan text NOT NULL DEFAULT 'free'
    CHECK (plan IN ('free', 'pro', 'team')),
  status text NOT NULL DEFAULT 'active'
    CHECK (status IN ('trialing', 'active', 'past_due', 'canceled', 'expired')),
  provider text,
  provider_customer_id text,
  provider_subscription_id text,
  current_period_end timestamptz,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE UNIQUE INDEX agent_pulse_subscriptions_provider_subscription_idx
  ON agent_pulse_subscriptions(provider, provider_subscription_id)
  WHERE provider IS NOT NULL AND provider_subscription_id IS NOT NULL;

CREATE TABLE agent_pulse_web_sessions (
  session_digest text PRIMARY KEY
    CHECK (session_digest ~ '^[0-9a-f]{64}$'),
  user_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  expires_at timestamptz NOT NULL,
  last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CHECK (expires_at > created_at)
);

CREATE INDEX agent_pulse_web_sessions_user_idx
  ON agent_pulse_web_sessions(user_id, expires_at DESC);
