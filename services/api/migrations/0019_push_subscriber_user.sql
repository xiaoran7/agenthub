ALTER TABLE agent_platform_push_subscriptions
  ADD COLUMN IF NOT EXISTS subscriber_user_id text
    CHECK (subscriber_user_id IS NULL OR length(subscriber_user_id) BETWEEN 1 AND 128);

CREATE INDEX IF NOT EXISTS agent_platform_push_subscriptions_owner_subscriber_idx
  ON agent_platform_push_subscriptions(owner_id, subscriber_user_id);
