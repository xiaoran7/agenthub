CREATE TABLE agent_platform_push_subscriptions (
  endpoint text PRIMARY KEY
    CHECK (length(endpoint) BETWEEN 1 AND 2048),
  owner_id text NOT NULL
    CHECK (length(owner_id) BETWEEN 1 AND 128),
  p256dh text NOT NULL
    CHECK (p256dh ~ '^[A-Za-z0-9_-]{1,200}$'),
  auth text NOT NULL
    CHECK (auth ~ '^[A-Za-z0-9_-]{1,64}$'),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE INDEX agent_platform_push_subscriptions_owner_idx
  ON agent_platform_push_subscriptions(owner_id, endpoint);
