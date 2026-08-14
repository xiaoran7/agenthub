ALTER TABLE agent_pulse_remote_tasks
  ADD COLUMN execution_started_at timestamptz,
  ADD COLUMN execution_finished_at timestamptz,
  ADD COLUMN receipt_digest text CHECK (receipt_digest ~ '^[0-9a-f]{64}$'),
  ADD COLUMN exit_code integer CHECK (exit_code BETWEEN -1 AND 255);

ALTER TABLE agent_pulse_remote_tasks
  ADD CONSTRAINT remote_task_receipt_time_order
  CHECK (
    execution_started_at IS NULL
    OR execution_finished_at IS NULL
    OR execution_finished_at >= execution_started_at
  );
