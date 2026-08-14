ALTER TABLE agent_platform_binding_codes
  ADD COLUMN issued_by text
    CHECK (issued_by IS NULL OR length(issued_by) BETWEEN 1 AND 128);

ALTER TABLE agent_pulse_audit_events
  DROP CONSTRAINT agent_pulse_audit_events_target_type_check;

ALTER TABLE agent_pulse_audit_events
  ALTER COLUMN target_id TYPE text USING target_id::text;

ALTER TABLE agent_pulse_audit_events
  ADD CONSTRAINT agent_pulse_audit_events_target_type_check
  CHECK (
    target_type IS NULL OR
    target_type IN (
      'account', 'web_session', 'workspace', 'team_invitation',
      'team_member', 'subscription', 'device', 'remote_task'
    )
  );

ALTER TABLE agent_pulse_audit_events
  ADD CONSTRAINT agent_pulse_audit_events_target_id_public_check
  CHECK (
    target_id IS NULL OR
    (
      length(target_id) BETWEEN 1 AND 128 AND
      target_id !~ '[[:cntrl:]]'
    )
  );
