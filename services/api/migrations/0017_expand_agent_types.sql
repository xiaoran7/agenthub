-- Expand legacy agent_type CHECK constraints from 2 engines to all 5 supported engines,
-- and add missing indexes on foreign-key / cleanup columns.

DO $$
DECLARE
  c record;
BEGIN
  FOR c IN
    SELECT conname
    FROM pg_constraint
    WHERE conrelid = 'agent_platform_sessions'::regclass
      AND contype = 'c'
      AND pg_get_constraintdef(oid) LIKE '%agent_type%'
  LOOP
    EXECUTE format('ALTER TABLE agent_platform_sessions DROP CONSTRAINT %I', c.conname);
  END LOOP;
  ALTER TABLE agent_platform_sessions
    ADD CONSTRAINT agent_platform_sessions_agent_type_check
    CHECK (agent_type IN ('claude_code', 'codex', 'antigravity', 'grok_build', 'zcode'));

  FOR c IN
    SELECT conname
    FROM pg_constraint
    WHERE conrelid = 'agent_platform_auth_snapshots'::regclass
      AND contype = 'c'
      AND pg_get_constraintdef(oid) LIKE '%agent_type%'
  LOOP
    EXECUTE format('ALTER TABLE agent_platform_auth_snapshots DROP CONSTRAINT %I', c.conname);
  END LOOP;
  ALTER TABLE agent_platform_auth_snapshots
    ADD CONSTRAINT agent_platform_auth_snapshots_agent_type_check
    CHECK (agent_type IN ('claude_code', 'codex', 'antigravity', 'grok_build', 'zcode'));

  FOR c IN
    SELECT conname
    FROM pg_constraint
    WHERE conrelid = 'agent_pulse_remote_tasks'::regclass
      AND contype = 'c'
      AND pg_get_constraintdef(oid) LIKE '%agent_type%'
  LOOP
    EXECUTE format('ALTER TABLE agent_pulse_remote_tasks DROP CONSTRAINT %I', c.conname);
  END LOOP;
  ALTER TABLE agent_pulse_remote_tasks
    ADD CONSTRAINT agent_pulse_remote_tasks_agent_type_check
    CHECK (agent_type IN ('claude_code', 'codex', 'antigravity', 'grok_build', 'zcode'));
END $$;

CREATE INDEX IF NOT EXISTS agent_pulse_web_sessions_workspace_owner_idx
  ON agent_pulse_web_sessions(workspace_owner_id, expires_at DESC);

CREATE INDEX IF NOT EXISTS agent_pulse_web_sessions_expires_idx
  ON agent_pulse_web_sessions(expires_at);

CREATE INDEX IF NOT EXISTS agent_pulse_audit_events_actor_idx
  ON agent_pulse_audit_events(actor_user_id)
  WHERE actor_user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS agent_pulse_team_invitations_invited_by_idx
  ON agent_pulse_team_invitations(invited_by);
