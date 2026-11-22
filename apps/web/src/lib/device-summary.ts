import type { AgentType } from "@agent-pulse/contracts";

import { matchesTab } from "./classify";
import type { DeviceRecord, SessionRecord } from "./types";

const CORE_AGENT_ORDER: readonly AgentType[] = [
  "claude_code",
  "claude_desktop",
  "codex",
  "antigravity",
  "deepseek_harness",
];

export interface DeviceSummary {
  device: DeviceRecord;
  agents: AgentType[];
  totalSessions: number;
  liveSessions: number;
  waitingSessions: number;
}

export function summarizeDevices(
  devices: DeviceRecord[],
  sessions: SessionRecord[],
): DeviceSummary[] {
  return devices.map((device) => {
    const deviceSessions = sessions.filter(
      (session) => session.device_id === device.device_id,
    );
    const observedAgents = new Set(
      deviceSessions.map((session) => session.agent_type),
    );
    const core = CORE_AGENT_ORDER.filter((agent) => observedAgents.has(agent));
    const extras = [...observedAgents]
      .filter((agent) => !CORE_AGENT_ORDER.includes(agent))
      .sort();
    return {
      device,
      agents: [...core, ...extras],
      totalSessions: deviceSessions.length,
      liveSessions: deviceSessions.filter((session) =>
        matchesTab(session, "live"),
      ).length,
      waitingSessions: deviceSessions.filter((session) =>
        matchesTab(session, "waiting"),
      ).length,
    };
  });
}
