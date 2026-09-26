import { describe, expect, it } from "vitest";
import {
  buildDesktopSessionTree,
  getDeviceDisplayName,
  isAutomatedTestWorkspace,
} from "./session-tree";
import type { Session } from "../types";

function createMockSession(overrides: Partial<Session> = {}): Session {
  return {
    event_id: "evt-1",
    device_id: "dev-local-1",
    agent_type: "codex",
    agent_session_id: "sess-abc-123",
    observed_at: new Date().toISOString(),
    payload: {
      project: { display_name: "agenthub-repo", fingerprint: "fp-1" },
      state: { activity_status: "IDLE", presence: "ACTIVE" },
      last_activity_at: new Date().toISOString(),
    },
    ...overrides,
  };
}

describe("Desktop Session Tree", () => {
  it("formats device names correctly", () => {
    expect(getDeviceDisplayName("dev-1", "dev-1", "XIAORAN")).toBe(
      "本机电脑 (XIAORAN)",
    );
    expect(getDeviceDisplayName("dev-1", "dev-1")).toBe("本机电脑");
    expect(getDeviceDisplayName("dev-remote-123456", "dev-1")).toBe(
      "远程电脑 · 123456",
    );
    expect(
      getDeviceDisplayName("dev-remote", "dev-1", undefined, "办公室台式机"),
    ).toBe("办公室台式机");
  });

  it("builds a 4-level folder tree: Device -> Agent -> Workspace -> Session", () => {
    const s1 = createMockSession({
      agent_type: "codex",
      agent_session_id: "codex-session-1",
      payload: {
        project: { display_name: "Project Alpha", fingerprint: "fp-a" },
        state: { activity_status: "WORKING", presence: "ACTIVE" },
      },
    });
    const s2 = createMockSession({
      agent_type: "claude_code",
      agent_session_id: "claude-session-2",
      payload: {
        project: { display_name: "Project Beta", fingerprint: "fp-b" },
        state: { activity_status: "IDLE", presence: "ACTIVE" },
      },
    });

    const tree = buildDesktopSessionTree({
      sessions: [s1, s2],
      localDeviceId: "dev-local-1",
      localDeviceName: "XIAORAN",
    });

    expect(tree.devices.length).toBe(1);
    const localDev = tree.devices[0];
    expect(localDev.label).toBe("本机电脑 (XIAORAN)");
    expect(localDev.totalCount).toBe(2);

    // Level 2: Agents
    const codexAgent = localDev.agents.find((a) => a.provider === "codex");
    const claudeAgent = localDev.agents.find(
      (a) => a.provider === "claude_code",
    );
    expect(codexAgent).toBeDefined();
    expect(claudeAgent).toBeDefined();

    // Level 3: Workspaces
    expect(codexAgent!.workspaces.length).toBe(1);
    expect(codexAgent!.workspaces[0].name).toBe("Project Alpha");

    // Level 4: Sessions
    expect(codexAgent!.workspaces[0].sessions.length).toBe(1);
    expect(codexAgent!.workspaces[0].sessions[0].session.agent_session_id).toBe(
      "codex-session-1",
    );
  });

  it("consolidates historical/legacy device sessions under 本机电脑 and ignores empty devices", () => {
    const sLocal = createMockSession({
      device_id: "XIAORAN",
      agent_type: "codex",
      agent_session_id: "s-1",
    });
    const sLegacy1 = createMockSession({
      device_id: "dev_642c69",
      agent_type: "codex",
      agent_session_id: "s-2",
    });
    const sLegacy2 = createMockSession({
      device_id: "dev_cca111",
      agent_type: "claude_code",
      agent_session_id: "s-3",
    });

    const tree = buildDesktopSessionTree({
      sessions: [sLocal, sLegacy1, sLegacy2],
      localDeviceId: "dev_e4e408",
      localDeviceName: "XIAORAN",
      registeredDevices: [
        { device_id: "dev_780f12", online: false, name: "已离线设备" },
        { device_id: "dev_000000", online: true, name: "空会话设备" },
      ],
    });

    // Only 1 device node: 本机电脑 (XIAORAN), consolidating all historical sessions, ignoring empty/offline
    expect(tree.devices.length).toBe(1);
    expect(tree.devices[0].label).toBe("本机电脑 (XIAORAN)");
    expect(tree.devices[0].totalCount).toBe(3);
    expect(tree.totalSessions).toBe(3);
  });

  it("hides non-enabled plugin sessions by default (such as zcode when plugin is off)", () => {
    const zcodeSession = createMockSession({
      agent_type: "zcode",
      agent_session_id: "zcode-session-99",
      payload: {
        project: { display_name: "ZCode Project", fingerprint: "fp-z" },
        state: { activity_status: "IDLE", presence: "ACTIVE" },
      },
    });

    const tree = buildDesktopSessionTree({
      sessions: [zcodeSession],
      localDeviceId: "dev-local-1",
    });

    // Since zcode is in plugin marketplace and not enabled in localStorage, it shouldn't show
    const localDev = tree.devices[0];
    const zcodeAgent = localDev.agents.find((a) => a.provider === "zcode");
    expect(zcodeAgent).toBeUndefined();
    expect(tree.totalSessions).toBe(0);
  });

  it("filters out automated test directories (agenthub-native-*, etc.) to keep workspace tree clean", () => {
    expect(isAutomatedTestWorkspace("agenthub-native-7El6zF")).toBe(true);
    expect(isAutomatedTestWorkspace("agenthub-remote-123")).toBe(true);
    expect(isAutomatedTestWorkspace("Claudespace")).toBe(false);
    expect(isAutomatedTestWorkspace("agenthub")).toBe(false);

    const sTest = createMockSession({
      agent_type: "codex",
      agent_session_id: "test-sess",
      observed_at: "2026-09-26T12:00:00Z",
      payload: {
        project: {
          display_name: "agenthub-native-7El6zF",
          fingerprint: "fp-test",
        },
        state: { activity_status: "IDLE", presence: "ACTIVE" },
        last_activity_at: "2026-09-26T12:00:00Z",
      },
    });
    const sReal = createMockSession({
      agent_type: "codex",
      agent_session_id: "real-sess",
      observed_at: "2026-09-20T12:00:00Z",
      payload: {
        project: { display_name: "Claudespace", fingerprint: "fp-real" },
        state: { activity_status: "IDLE", presence: "ACTIVE" },
        last_activity_at: "2026-09-20T12:00:00Z",
      },
    });

    const tree = buildDesktopSessionTree({
      sessions: [sTest, sReal],
      localDeviceId: "dev-1",
      localDeviceName: "XIAORAN",
    });

    const codex = tree.devices[0].agents.find((a) => a.provider === "codex")!;
    // Only real workspace is kept, automated test workspace is completely filtered out
    expect(codex.workspaces.length).toBe(1);
    expect(codex.workspaces[0].name).toBe("Claudespace");
    expect(codex.totalCount).toBe(1);
  });
});
