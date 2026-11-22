-- Replace hardcoded 5-agent CHECK constraints with slug format validation
-- so native 5 core engines and any Runner-installed ACP plugin agent_type are accepted.

DO $$
DECLARE
  tbl text;
  cname text;
  c record;
BEGIN
  FOREACH tbl IN ARRAY ARRAY[
    'agent_platform_sessions',
    'agent_platform_auth_snapshots',
    'agent_pulse_remote_tasks'
  ]
  LOOP
    cname := tbl || '_agent_type_check';
    FOR c IN
      SELECT conname
      FROM pg_constraint
      WHERE conrelid = tbl::regclass
        AND contype = 'c'
        AND pg_get_constraintdef(oid) LIKE '%agent_type%'
    LOOP
      EXECUTE format('ALTER TABLE %I DROP CONSTRAINT %I', tbl, c.conname);
    END LOOP;
    EXECUTE format(
      'ALTER TABLE %I ADD CONSTRAINT %I CHECK (agent_type ~ ''^[a-z][a-z0-9_-]{1,63}$'')',
      tbl,
      cname
    );
  END LOOP;
END $$;
