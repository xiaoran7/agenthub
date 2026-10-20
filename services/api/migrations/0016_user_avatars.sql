ALTER TABLE agent_pulse_users
  ADD COLUMN IF NOT EXISTS avatar_data_url text;
