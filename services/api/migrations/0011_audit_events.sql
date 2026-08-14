CREATE TABLE agent_pulse_audit_events (
  event_id uuid PRIMARY KEY,
  scope text NOT NULL
    CHECK (scope IN ('account', 'workspace')),
  account_user_id uuid
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  workspace_owner_id uuid
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  actor_user_id uuid
    REFERENCES agent_pulse_users(user_id) ON DELETE SET NULL,
  action text NOT NULL
    CHECK (action ~ '^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$'),
  target_type text
    CHECK (
      target_type IS NULL OR
      target_type IN (
        'account', 'web_session', 'workspace', 'team_invitation',
        'team_member', 'subscription'
      )
    ),
  target_id uuid,
  metadata jsonb NOT NULL DEFAULT '{}'::jsonb
    CHECK (jsonb_typeof(metadata) = 'object'),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  CHECK (
    (scope = 'account' AND account_user_id IS NOT NULL AND workspace_owner_id IS NULL) OR
    (scope = 'workspace' AND account_user_id IS NULL AND workspace_owner_id IS NOT NULL)
  ),
  CHECK ((target_type IS NULL) = (target_id IS NULL))
);

CREATE INDEX agent_pulse_audit_account_idx
  ON agent_pulse_audit_events (account_user_id, created_at DESC, event_id DESC)
  WHERE scope = 'account';

CREATE INDEX agent_pulse_audit_workspace_idx
  ON agent_pulse_audit_events (workspace_owner_id, created_at DESC, event_id DESC)
  WHERE scope = 'workspace';
