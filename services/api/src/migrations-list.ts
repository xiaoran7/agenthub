export const MIGRATIONS = [
  {
    version: 1,
    url: new URL("../migrations/0001_gateway.sql", import.meta.url),
  },
  {
    version: 2,
    url: new URL(
      "../migrations/0002_session_timeline_index.sql",
      import.meta.url,
    ),
  },
  {
    version: 3,
    url: new URL("../migrations/0003_push_subscriptions.sql", import.meta.url),
  },
  {
    version: 4,
    url: new URL("../migrations/0004_device_lifecycle.sql", import.meta.url),
  },
  {
    version: 5,
    url: new URL("../migrations/0005_device_auth.sql", import.meta.url),
  },
  {
    version: 6,
    url: new URL(
      "../migrations/0006_accounts_subscriptions.sql",
      import.meta.url,
    ),
  },
  {
    version: 7,
    url: new URL("../migrations/0007_remote_tasks.sql", import.meta.url),
  },
  {
    version: 8,
    url: new URL("../migrations/0008_account_lifecycle.sql", import.meta.url),
  },
  {
    version: 9,
    url: new URL("../migrations/0009_password_lifecycle.sql", import.meta.url),
  },
  {
    version: 10,
    url: new URL("../migrations/0010_team_workspaces.sql", import.meta.url),
  },
  {
    version: 11,
    url: new URL("../migrations/0011_audit_events.sql", import.meta.url),
  },
  {
    version: 12,
    url: new URL("../migrations/0012_device_heartbeat.sql", import.meta.url),
  },
  {
    version: 13,
    url: new URL("../migrations/0013_device_task_audit.sql", import.meta.url),
  },
  {
    version: 14,
    url: new URL(
      "../migrations/0014_remote_task_receipts.sql",
      import.meta.url,
    ),
  },
  {
    version: 15,
    url: new URL("../migrations/0015_oauth_accounts.sql", import.meta.url),
  },
  {
    version: 16,
    url: new URL("../migrations/0016_user_avatars.sql", import.meta.url),
  },
  {
    version: 17,
    url: new URL("../migrations/0017_expand_agent_types.sql", import.meta.url),
  },
  {
    version: 18,
    url: new URL(
      "../migrations/0018_dynamic_plugin_agent_types.sql",
      import.meta.url,
    ),
  },
];
