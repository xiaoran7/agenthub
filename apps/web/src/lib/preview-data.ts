import type { AgentType, ActivityStatus, Presence } from "./types";
import type { DeviceRecord, SessionRecord } from "./types";

const DEVICE_IDS = {
  desktop: "dev_11111111111111111111111111111111",
  laptop: "dev_22222222222222222222222222222222",
  workstation: "dev_33333333333333333333333333333333",
  server: "dev_44444444444444444444444444444444",
  mini: "dev_55555555555555555555555555555555",
} as const;

function minutesAgo(minutes: number): string {
  return new Date(Date.now() - minutes * 60_000).toISOString();
}

function fingerprint(seed: string): string {
  return seed.repeat(64).slice(0, 64);
}

function eventId(sequence: number): string {
  return `00000000-0000-4000-8000-${String(sequence).padStart(12, "0")}`;
}

function makeSession(input: {
  sequence: number;
  device_id: string;
  agent_type: AgentType;
  project: string;
  session: string;
  status: ActivityStatus;
  presence?: Presence;
  task: string;
  tool?: string;
  waiting_reason?: "user_input" | "permission";
  observedMinutesAgo: number;
  startedMinutesAgo: number;
}): SessionRecord {
  const observedAt = minutesAgo(input.observedMinutesAgo);
  const detail: Record<string, string> = { task: input.task };
  if (input.tool !== undefined) detail.tool = input.tool;
  return {
    sequence: input.sequence,
    event_id: eventId(input.sequence),
    schema_version: 1,
    device_id: input.device_id,
    agent_type: input.agent_type,
    agent_session_id: input.session,
    event_type: "session_observed",
    observed_at: observedAt,
    source:
      input.agent_type === "codex" ? "codex_app_server" : "claude_agents_json",
    confidence: "observed",
    adapter_version: "runner 0.3.2",
    received_at: observedAt,
    payload: {
      project: { display_name: input.project, fingerprint: fingerprint("a") },
      state: {
        presence:
          input.presence ??
          (input.status === "COMPLETED" ? "PERSISTED" : "LIVE"),
        activity_status: input.status,
        raw_status: input.status.toLowerCase(),
        raw_status_detail: detail,
        ...(input.waiting_reason === undefined
          ? {}
          : { waiting_reason: input.waiting_reason }),
      },
      started_at: minutesAgo(input.startedMinutesAgo),
      last_activity_at: observedAt,
    },
  };
}

export const PREVIEW_DEVICES: DeviceRecord[] = [
  {
    device_id: DEVICE_IDS.desktop,
    created_at: minutesAgo(60 * 24 * 10),
    last_seen_at: minutesAgo(2),
    online: true,
    revoked: false,
    display_name: "Desktop-PC",
    os: "Windows 11",
    runner_version: "Runner 0.3.2",
  },
  {
    device_id: DEVICE_IDS.laptop,
    created_at: minutesAgo(60 * 24 * 8),
    last_seen_at: minutesAgo(3),
    online: true,
    revoked: false,
    display_name: "Laptop",
    os: "macOS 14.4",
    runner_version: "Runner 0.3.2",
  },
  {
    device_id: DEVICE_IDS.workstation,
    created_at: minutesAgo(60 * 24 * 5),
    last_seen_at: minutesAgo(5),
    online: true,
    revoked: false,
    display_name: "Workstation-01",
    os: "Windows 11",
    runner_version: "Runner 0.3.1",
  },
  {
    device_id: DEVICE_IDS.server,
    created_at: minutesAgo(60 * 24 * 3),
    last_seen_at: minutesAgo(4),
    online: true,
    revoked: false,
    display_name: "Build-Server",
    os: "Ubuntu 22.04",
    runner_version: "Runner 0.3.2",
  },
  {
    device_id: DEVICE_IDS.mini,
    created_at: minutesAgo(60 * 24 * 2),
    last_seen_at: minutesAgo(18),
    online: false,
    revoked: false,
    display_name: "Mac-mini",
    os: "macOS 13.6",
    runner_version: "Runner 0.2.9",
  },
];

export const PREVIEW_SESSIONS: SessionRecord[] = [
  makeSession({
    sequence: 1,
    device_id: DEVICE_IDS.desktop,
    agent_type: "claude_code",
    project: "runner-heartbeat",
    session: "s_01HZYQ3N8K7R2J3V9W8F",
    status: "WORKING",
    task: "实现 Runner 心跳",
    tool: "tools.write_file",
    observedMinutesAgo: 2,
    startedMinutesAgo: 19,
  }),
  makeSession({
    sequence: 2,
    device_id: DEVICE_IDS.laptop,
    agent_type: "codex",
    project: "session-manager",
    session: "s_01HZYQ5TMA4982C7D6E",
    status: "WORKING",
    task: "重构会话管理器",
    tool: "shell",
    observedMinutesAgo: 3,
    startedMinutesAgo: 27,
  }),
  makeSession({
    sequence: 3,
    device_id: DEVICE_IDS.workstation,
    agent_type: "claude_code",
    project: "agent-pulse",
    session: "s_01HZYQ1X6PB4F6G2H1K",
    status: "WORKING",
    task: "增加重试退避逻辑",
    observedMinutesAgo: 5,
    startedMinutesAgo: 45,
  }),
  makeSession({
    sequence: 4,
    device_id: DEVICE_IDS.server,
    agent_type: "codex",
    project: "ci-runner",
    session: "s_01HZYQ8V2J6K9L3M0N5P",
    status: "WAITING_PERMISSION",
    task: "等待工具响应",
    tool: "shell",
    waiting_reason: "permission",
    observedMinutesAgo: 1,
    startedMinutesAgo: 9,
  }),
  makeSession({
    sequence: 5,
    device_id: DEVICE_IDS.laptop,
    agent_type: "claude_code",
    project: "diagnostics-schema",
    session: "s_01HZYQ6ET73Y2U8I9O4P",
    status: "WORKING",
    task: "更新诊断模式",
    observedMinutesAgo: 8,
    startedMinutesAgo: 72,
  }),
  makeSession({
    sequence: 6,
    device_id: DEVICE_IDS.server,
    agent_type: "codex",
    project: "workspace-build",
    session: "s_01HZYQ9R4B1C7D8E2F6G",
    status: "WAITING_INPUT",
    task: "排队：等待工作区",
    waiting_reason: "user_input",
    observedMinutesAgo: 4,
    startedMinutesAgo: 3,
  }),
  makeSession({
    sequence: 7,
    device_id: DEVICE_IDS.desktop,
    agent_type: "claude_code",
    project: "test-suite",
    session: "s_01HZYQ3A9M7N5B2V6C1D",
    status: "WORKING",
    task: "编写单元测试",
    observedMinutesAgo: 6,
    startedMinutesAgo: 34,
  }),
  makeSession({
    sequence: 8,
    device_id: DEVICE_IDS.mini,
    agent_type: "codex",
    project: "mac-runner",
    session: "s_01HZYQ8X2C4V6B1N7M",
    status: "FAILED",
    task: "工具调用失败",
    tool: "tools.read_file",
    observedMinutesAgo: 10,
    startedMinutesAgo: 11,
  }),
  makeSession({
    sequence: 9,
    device_id: DEVICE_IDS.desktop,
    agent_type: "claude_code",
    project: "log-batching",
    session: "s_01HZYQ7P5L9K2J4H8G3S",
    status: "WORKING",
    task: "优化日志批处理",
    observedMinutesAgo: 12,
    startedMinutesAgo: 52,
  }),
  makeSession({
    sequence: 10,
    device_id: DEVICE_IDS.server,
    agent_type: "codex",
    project: "api-types",
    session: "s_01HZYQ5D61C3H7J2K9L",
    status: "COMPLETED",
    task: "生成 API 类型",
    observedMinutesAgo: 60,
    startedMinutesAgo: 67,
  }),
  makeSession({
    sequence: 11,
    device_id: DEVICE_IDS.laptop,
    agent_type: "claude_code",
    project: "deployment-guide",
    session: "s_01HZYQ2U7T4R8E3W6Q5",
    status: "COMPLETED",
    task: "编写部署指南",
    observedMinutesAgo: 76,
    startedMinutesAgo: 92,
  }),
  makeSession({
    sequence: 12,
    device_id: DEVICE_IDS.desktop,
    agent_type: "codex",
    project: "project-build",
    session: "s_01HZYQ4N2M6B1V9C7D3F",
    status: "FAILED",
    task: "构建项目",
    observedMinutesAgo: 120,
    startedMinutesAgo: 125,
  }),
];

export function previewTimelineFor(session: SessionRecord): SessionRecord[] {
  return [
    {
      ...session,
      sequence: Math.max(1, session.sequence - 2),
      observed_at: minutesAgo(13),
    },
    {
      ...session,
      sequence: Math.max(1, session.sequence - 1),
      observed_at: minutesAgo(7),
    },
    session,
  ];
}
