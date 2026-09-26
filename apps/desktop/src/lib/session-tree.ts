import type { Session, AgentType } from "../types";
import { normalizeAgentType } from "../types";
import {
  activityTime,
  agentLabel,
  agentShortLabel,
  sessionDisplayTitle,
  sessionKey,
  PROVIDER_LIST,
} from "../presentation";
import { isPluginEnabled } from "../plugins";

export interface TreeNodeSession {
  session: Session;
  key: string;
  title: string;
  activityTime: string;
}

export interface WorkspaceTreeNode {
  id: string;
  name: string;
  sessions: TreeNodeSession[];
  totalCount: number;
}

export interface AgentTreeNode {
  provider: AgentType;
  label: string;
  shortLabel: string;
  workspaces: WorkspaceTreeNode[];
  totalCount: number;
}

export interface DeviceTreeNode {
  deviceId: string;
  label: string;
  online: boolean;
  agents: AgentTreeNode[];
  totalCount: number;
}

export interface DesktopSessionTree {
  devices: DeviceTreeNode[];
  totalSessions: number;
}

const AGENT_ORDER: Record<string, number> = {
  claude_code: 0,
  codex: 1,
  zcode: 2,
  antigravity: 3,
  grok_build: 4,
};

export function getDeviceDisplayName(
  deviceId: string,
  localDeviceId: string,
  localDeviceName?: string,
  customLabel?: string,
): string {
  if (
    deviceId === localDeviceId ||
    (localDeviceName &&
      deviceId.toLowerCase() === localDeviceName.toLowerCase()) ||
    deviceId === "local"
  ) {
    return localDeviceName?.trim()
      ? `本机电脑 (${localDeviceName.trim()})`
      : "本机电脑";
  }
  if (customLabel && customLabel.trim()) {
    return customLabel.trim();
  }
  return `远程电脑 · ${deviceId.slice(-6)}`;
}

export function isAutomatedTestWorkspace(displayName?: string): boolean {
  if (!displayName) return false;
  return (
    displayName.startsWith("agenthub-native-") ||
    displayName.startsWith("agenthub-remote-") ||
    displayName.startsWith("agenthub-test-")
  );
}

export function buildDesktopSessionTree(options: {
  sessions: Session[];
  localDeviceId: string;
  localDeviceName?: string;
  searchQuery?: string;
  registeredDevices?: Array<{
    device_id: string;
    online: boolean;
    name?: string;
    revoked?: boolean;
  }>;
}): DesktopSessionTree {
  const {
    sessions,
    localDeviceId,
    localDeviceName,
    searchQuery = "",
    registeredDevices = [],
  } = options;

  const query = searchQuery.trim().toLowerCase();

  // 1. Filter sessions by plugin enablement, automated test directories, and search query
  const eligibleSessions = sessions.filter((s) => {
    const agent = normalizeAgentType(s.agent_type);
    if (!isPluginEnabled(agent)) return false;

    // Filter out automated test workspaces (agenthub-native-*, agenthub-remote-*, etc.)
    const wsName = s.payload.project.display_name?.trim();
    if (isAutomatedTestWorkspace(wsName)) {
      return false;
    }

    if (!query) return true;
    const title = sessionDisplayTitle(s).toLowerCase();
    const ws = (s.payload.project.display_name || "").toLowerCase();
    const id = s.agent_session_id.toLowerCase();
    const providerLabel = agentLabel(agent).toLowerCase();
    return (
      title.includes(query) ||
      ws.includes(query) ||
      id.includes(query) ||
      providerLabel.includes(query)
    );
  });

  // 2. Discover genuinely distinct active remote devices.
  // In a desktop client, any device that is offline, revoked, or has no sessions is NOT displayed.
  // Historical sessions on this machine (or with local hostname/legacy IDs) belong to 本机电脑.
  const activeRemoteDeviceMap = new Map<
    string,
    { device_id: string; label: string; online: boolean }
  >();

  for (const reg of registeredDevices) {
    if (
      reg.device_id &&
      reg.device_id !== localDeviceId &&
      reg.online === true &&
      !reg.revoked &&
      !(
        localDeviceName &&
        reg.device_id.toLowerCase() === localDeviceName.toLowerCase()
      )
    ) {
      activeRemoteDeviceMap.set(reg.device_id, {
        device_id: reg.device_id,
        label: reg.name?.trim() || `远程电脑 · ${reg.device_id.slice(-6)}`,
        online: true,
      });
    }
  }

  // 3. Partition sessions:
  // All sessions belong to local device UNLESS they belong to a distinct active remote device!
  const localLabel = getDeviceDisplayName(
    localDeviceId,
    localDeviceId,
    localDeviceName,
  );

  const deviceMap = new Map<
    string,
    {
      deviceId: string;
      label: string;
      online: boolean;
      sessions: Session[];
    }
  >();

  deviceMap.set(localDeviceId, {
    deviceId: localDeviceId,
    label: localLabel,
    online: true,
    sessions: [],
  });

  for (const s of eligibleSessions) {
    const sDevId = s.device_id || localDeviceId;
    if (activeRemoteDeviceMap.has(sDevId)) {
      let remoteDev = deviceMap.get(sDevId);
      if (!remoteDev) {
        const info = activeRemoteDeviceMap.get(sDevId)!;
        remoteDev = {
          deviceId: sDevId,
          label: info.label,
          online: info.online,
          sessions: [],
        };
        deviceMap.set(sDevId, remoteDev);
      }
      remoteDev.sessions.push(s);
    } else {
      // Local session (including legacy IDs or hostname matches on this machine)
      const localDev = deviceMap.get(localDeviceId)!;
      localDev.sessions.push(s);
    }
  }

  // 4. For each device, build Agent -> Workspace -> Session hierarchy
  const deviceNodes: DeviceTreeNode[] = [];
  let totalSessions = 0;

  for (const dev of deviceMap.values()) {
    // Only show remote devices if they actually have sessions!
    if (dev.deviceId !== localDeviceId && dev.sessions.length === 0) {
      continue;
    }

    // Determine available providers for this device
    const enabledProviders = PROVIDER_LIST.filter(
      (item) => item.managed || isPluginEnabled(item.type),
    ).map((p) => p.type);

    const providerSessionMap = new Map<AgentType, Session[]>();
    for (const p of enabledProviders) {
      providerSessionMap.set(p, []);
    }

    for (const s of dev.sessions) {
      const p = normalizeAgentType(s.agent_type);
      const list = providerSessionMap.get(p);
      if (list) {
        list.push(s);
      } else {
        providerSessionMap.set(p, [s]);
      }
    }

    const agentNodes: AgentTreeNode[] = [];
    let devTotalCount = 0;

    for (const [provider, pSessions] of providerSessionMap.entries()) {
      // If searching, hide empty agents; otherwise keep enabled agents visible for local device
      if (query && pSessions.length === 0) continue;
      if (dev.deviceId !== localDeviceId && pSessions.length === 0) continue;

      // Group pSessions by workspace
      const wsMap = new Map<string, Session[]>();
      for (const s of pSessions) {
        const ws = s.payload.project.display_name?.trim() || "未命名工作区";
        const wsList = wsMap.get(ws);
        if (wsList) wsList.push(s);
        else wsMap.set(ws, [s]);
      }

      const workspaceNodes: WorkspaceTreeNode[] = [];
      for (const [wsName, wsSessions] of wsMap.entries()) {
        const sortedSessions: TreeNodeSession[] = wsSessions
          .sort((a, b) => activityTime(b).localeCompare(activityTime(a)))
          .map((session) => ({
            session,
            key: sessionKey(session),
            title: sessionDisplayTitle(session),
            activityTime: activityTime(session),
          }));

        workspaceNodes.push({
          id: `${dev.deviceId}:${provider}:${wsName}`,
          name: wsName,
          sessions: sortedSessions,
          totalCount: sortedSessions.length,
        });
      }

      // Sort workspaces by most recent active sessions first
      workspaceNodes.sort((a, b) => {
        const timeA = a.sessions[0]?.activityTime || "";
        const timeB = b.sessions[0]?.activityTime || "";
        return timeB.localeCompare(timeA);
      });

      const agentTotalCount = workspaceNodes.reduce(
        (sum, ws) => sum + ws.totalCount,
        0,
      );

      agentNodes.push({
        provider,
        label: agentLabel(provider),
        shortLabel: agentShortLabel(provider),
        workspaces: workspaceNodes,
        totalCount: agentTotalCount,
      });

      devTotalCount += agentTotalCount;
    }

    // Sort agents by AGENT_ORDER
    agentNodes.sort((a, b) => {
      const ordA = AGENT_ORDER[a.provider] ?? 99;
      const ordB = AGENT_ORDER[b.provider] ?? 99;
      return ordA - ordB;
    });

    deviceNodes.push({
      deviceId: dev.deviceId,
      label: dev.label,
      online: dev.online,
      agents: agentNodes,
      totalCount: devTotalCount,
    });

    totalSessions += devTotalCount;
  }

  // Sort devices: local device always first, then by label
  deviceNodes.sort((a, b) => {
    if (a.deviceId === localDeviceId) return -1;
    if (b.deviceId === localDeviceId) return 1;
    return a.label.localeCompare(b.label);
  });

  return {
    devices: deviceNodes,
    totalSessions,
  };
}
