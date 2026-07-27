CREATE INDEX agent_platform_events_session_timeline_idx
  ON agent_platform_events (
    device_id,
    (event_data->>'agent_type'),
    (event_data->>'agent_session_id'),
    sequence DESC
  );
