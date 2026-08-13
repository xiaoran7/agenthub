CREATE TABLE agent_pulse_remote_tasks (
  task_id uuid PRIMARY KEY,
  owner_id uuid NOT NULL
    REFERENCES agent_pulse_users(user_id) ON DELETE CASCADE,
  target_device_id text NOT NULL
    REFERENCES agent_platform_devices(device_id),
  agent_type text NOT NULL
    CHECK (agent_type IN ('claude_code', 'codex')),
  prompt text NOT NULL
    CHECK (length(prompt) BETWEEN 1 AND 32000),
  project_fingerprint text
    CHECK (project_fingerprint ~ '^[0-9a-f]{64}$'),
  idempotency_key text NOT NULL
    CHECK (length(idempotency_key) BETWEEN 8 AND 128),
  execution_policy text NOT NULL
    CHECK (execution_policy IN (
      'observe_only', 'local_confirmation', 'trusted_execution'
    )),
  status text NOT NULL DEFAULT 'queued'
    CHECK (status IN (
      'queued', 'delivered', 'accepted', 'running', 'waiting_user',
      'succeeded', 'failed', 'canceled', 'expired'
    )),
  version integer NOT NULL DEFAULT 1 CHECK (version >= 1),
  created_at timestamptz NOT NULL,
  expires_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL,
  delivered_at timestamptz,
  started_at timestamptz,
  completed_at timestamptz,
  agent_session_id text CHECK (length(agent_session_id) <= 512),
  result_summary text CHECK (length(result_summary) <= 2000),
  error_code text CHECK (error_code ~ '^[a-z][a-z0-9_]{0,127}$'),
  UNIQUE (owner_id, idempotency_key),
  CHECK (expires_at > created_at)
);

CREATE INDEX agent_pulse_remote_tasks_owner_idx
  ON agent_pulse_remote_tasks(owner_id, created_at DESC);

CREATE INDEX agent_pulse_remote_tasks_claim_idx
  ON agent_pulse_remote_tasks(target_device_id, created_at)
  WHERE status = 'queued';

CREATE TABLE agent_pulse_remote_task_events (
  event_id bigserial PRIMARY KEY,
  task_id uuid NOT NULL
    REFERENCES agent_pulse_remote_tasks(task_id) ON DELETE CASCADE,
  from_status text,
  to_status text NOT NULL,
  version integer NOT NULL,
  actor text NOT NULL CHECK (actor IN ('user', 'device', 'system')),
  occurred_at timestamptz NOT NULL,
  UNIQUE (task_id, version)
);
