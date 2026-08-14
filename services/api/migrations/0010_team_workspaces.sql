ALTER TABLE agent_pulse_web_sessions
  ADD COLUMN workspace_owner_id uuid
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE;

UPDATE agent_pulse_web_sessions
SET workspace_owner_id = user_id
WHERE workspace_owner_id IS NULL;

ALTER TABLE agent_pulse_web_sessions
  ALTER COLUMN workspace_owner_id SET NOT NULL;

CREATE TABLE agent_pulse_workspace_members (
  workspace_owner_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  user_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  role text NOT NULL
    CHECK (role IN ('owner', 'admin', 'member')),
  created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
  PRIMARY KEY (workspace_owner_id, user_id),
  CHECK (
    (workspace_owner_id = user_id AND role = 'owner') OR
    (workspace_owner_id <> user_id AND role <> 'owner')
  )
);

INSERT INTO agent_pulse_workspace_members (
  workspace_owner_id, user_id, role
)
SELECT user_id, user_id, 'owner'
FROM agent_pulse_users
ON CONFLICT DO NOTHING;

CREATE INDEX agent_pulse_workspace_members_user_idx
  ON agent_pulse_workspace_members(user_id, workspace_owner_id);

CREATE TABLE agent_pulse_team_invitations (
  invitation_id uuid NOT NULL UNIQUE,
  token_digest text PRIMARY KEY
    CHECK (token_digest ~ '^[0-9a-f]{64}$'),
  workspace_owner_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  invited_user_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  invited_email text NOT NULL,
  role text NOT NULL
    CHECK (role IN ('admin', 'member')),
  invited_by uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  consumed_at timestamptz,
  revoked_at timestamptz,
  CHECK (workspace_owner_id <> invited_user_id),
  CHECK (expires_at > created_at)
);

CREATE UNIQUE INDEX agent_pulse_team_invitations_pending_idx
  ON agent_pulse_team_invitations(workspace_owner_id, invited_user_id)
  WHERE consumed_at IS NULL AND revoked_at IS NULL;

CREATE INDEX agent_pulse_team_invitations_workspace_idx
  ON agent_pulse_team_invitations(workspace_owner_id, created_at DESC);
