/**
 * Session tree grouping for the Remote Console sidebar.
 *
 * The tree is a four-level file-folder hierarchy:
 *   device -> agent -> workspace -> session
 *
 * A device is one physical computer (a Runner). An agent is the official
 * Runtime hosting the session (Codex, Claude Code, ZCode, ...). A workspace is
 * an authorized directory the Host may operate in, and a session is one
 * conversation. Every session belongs to exactly one of each, so the tree is a
 * strict partition and no session can appear twice.
 */

export type ProviderKind = "codex" | "zcode" | "claude" | "other";

export interface ProviderDescriptor {
  type: ProviderKind;
  name: string;
  badgeClass: string;
}

/** Ordering used for agent folders: managed engines first, then discovery. */
const PROVIDER_ORDER: Record<ProviderKind, number> = {
  codex: 0,
  claude: 1,
  zcode: 2,
  other: 3,
};

/**
 * A single explicit collator for every sort in this module. `localeCompare`
 * without a fixed locale is host-ICU dependent, which would make the sidebar
 * order differ between the user's browser and CI.
 */
const collator = new Intl.Collator("zh-Hans-CN", {
  numeric: true,
  sensitivity: "base",
});

const byLabel = (a: { label: string }, b: { label: string }): number =>
  collator.compare(a.label, b.label);

const PROVIDER_BY_NAME: Record<string, ProviderDescriptor> = {
  codex: { type: "codex", name: "Codex", badgeClass: "codex" },
  zcode: { type: "zcode", name: "ZCode", badgeClass: "zcode" },
  gemini: { type: "zcode", name: "Gemini CLI", badgeClass: "zcode" },
  gemini_cli: { type: "zcode", name: "Gemini CLI", badgeClass: "zcode" },
  claude_code: { type: "claude", name: "Claude Code", badgeClass: "claude" },
  claude: { type: "claude", name: "Claude Code", badgeClass: "claude" },
  antigravity: {
    type: "other",
    name: "Antigravity",
    badgeClass: "other",
  },
  grok_build: { type: "other", name: "Grok", badgeClass: "other" },
  grok: { type: "other", name: "Grok", badgeClass: "other" },
  cursor: { type: "other", name: "Cursor", badgeClass: "other" },
  cline: { type: "other", name: "Cline", badgeClass: "other" },
};

/**
 * Classify a Provider from its session id and optional discovery metadata.
 *
 * A provider name reported by the Host is authoritative. The id prefix is only
 * a fallback for an active session that discovery has not described yet — a
 * session that *is* described must be filed by what the Host said, not by what
 * its id happens to start with.
 */
export function getProviderInfo(
  providerSessionId: string,
  provider?: string,
): ProviderDescriptor {
  if (provider !== undefined && provider !== "") {
    const named = PROVIDER_BY_NAME[provider];
    if (named !== undefined) return named;
  }
  if (providerSessionId.startsWith("sess_")) {
    return { type: "zcode", name: "ZCode", badgeClass: "zcode" };
  }
  if (providerSessionId.startsWith("gemini_")) {
    return { type: "zcode", name: "Gemini CLI", badgeClass: "zcode" };
  }
  if (providerSessionId.startsWith("claude_")) {
    return { type: "claude", name: "Claude Code", badgeClass: "claude" };
  }
  if (providerSessionId.startsWith("antigravity_")) {
    return {
      type: "other",
      name: "Antigravity",
      badgeClass: "other",
    };
  }
  if (providerSessionId.startsWith("grok_")) {
    return { type: "other", name: "Grok", badgeClass: "other" };
  }
  return { type: "codex", name: "Codex", badgeClass: "codex" };
}

/** Drop the Windows extended-length prefix and any trailing separator. */
function stripPathPrefix(path: string): string {
  return path.replace(/^\\\\\?\\/u, "");
}

function basename(path: string): string {
  return (
    stripPathPrefix(path)
      .replace(/[\\/]+$/u, "")
      .split(/[\\/]/u)
      .pop() ?? path
  );
}

/**
 * A `ws-` workspace id has no filesystem path, so fall back to a readable
 * slug rather than showing the raw token.
 */
export function workspaceLabel(workspaceId: string): string {
  if (workspaceId.startsWith("ws-")) {
    return workspaceId.slice(3).replace(/-[0-9a-f]{16}$/u, "") || workspaceId;
  }
  return basename(workspaceId);
}

/**
 * The tree shows the workspace name (the folder), not the full path, because
 * the full path is long and mostly noise in a narrow sidebar. The full path
 * stays available as the row's tooltip.
 */
export function workspaceTooltip(workspaceId: string): string {
  return stripPathPrefix(workspaceId);
}

export function cleanPath(path: string): string {
  return stripPathPrefix(path);
}

/**
 * Present a device with a friendly name.
 *
 * The device id is a key, not a label: it stays in the backend and out of
 * the tree. The name the Runner advertises is what the console shows. Only a
 * Runner that has not advertised a name yet falls back to the id, and then
 * only its last few characters, so two such machines stay tellable apart.
 */
export function getDeviceLabel(
  runnerId: string,
  hello?: {
    device_name?: unknown;
  } | null,
): string {
  if (
    hello !== null &&
    hello !== undefined &&
    typeof hello.device_name === "string" &&
    hello.device_name.trim() !== ""
  ) {
    return hello.device_name.trim();
  }
  return `电脑 · ${runnerId.slice(-6)}`;
}

export interface TreeSessionBase {
  key: string;
  runnerId: string;
  /**
   * The Provider's own session id. Carried explicitly rather than parsed back
   * out of `key`, because a provider id may itself contain a `/`.
   */
  providerSessionId: string;
  workspaceId: string;
  title: string;
  model?: string | null | undefined;
  totalTokens?: number | null | undefined;
  lastActivityAtMs?: number | null | undefined;
}

export type TreeSessionItem =
  | (TreeSessionBase & {
      kind: "active";
      provider: ProviderDescriptor;
      /**
       * The stable cross-client id (`${runnerId}/${hubSessionId}`) the Web
       * console selects by. It differs from `key`, which is keyed by the
       * Provider's own session id so a discovery candidate can share it.
       */
      sessionKey: string;
      /** A session under live Host control. */
      session: {
        online: boolean;
        control_connected: boolean;
        execution_state: string;
      };
    })
  | (TreeSessionBase & {
      kind: "candidate";
      provider: ProviderDescriptor;
      /** Read-only discovery metadata; resumable unless the Host says not. */
      candidate: { resumable?: boolean };
    });

export interface WorkspaceGroup {
  id: string;
  label: string;
  items: TreeSessionItem[];
}

export interface AgentGroup {
  provider: ProviderKind;
  label: string;
  badgeClass: string;
  workspaces: WorkspaceGroup[];
  totalCount: number;
}

export interface DeviceGroup {
  runnerId: string;
  label: string;
  online: boolean;
  workspaces: AgentGroup[];
  totalCount: number;
}

export interface TreeInput {
  /** Session id -> provider metadata, keyed `${runnerId}/${providerSessionId}`. */
  discovered: Map<
    string,
    {
      provider?: string | undefined;
      model?: string | null | undefined;
      total_tokens?: number | null | undefined;
      last_activity_at_ms?: number | null | undefined;
    }
  >;
  /** Devices seen by the Gateway, including those with no sessions. */
  devices: Array<{
    runner_id: string;
    online: boolean;
    hello?: { hostname?: unknown; device_name?: unknown } | null;
  }>;
  /** Live sessions the Web client can control. */
  activeSessions: Array<{
    runner_id: string;
    hub_session_id: string;
    provider_session_id: string;
    workspace_path: string;
    title?: string | null | undefined;
    online: boolean;
    control_connected: boolean;
    execution_state: string;
  }>;
  /** Read-only history discovered on a Host. */
  candidates: Array<{
    runner_id: string;
    provider_session_id: string;
    workspace_path: string;
    title?: string | null | undefined;
    provider?: string | undefined;
    resumable?: boolean | undefined;
    model?: string | null | undefined;
    total_tokens?: number | null | undefined;
    last_activity_at_ms?: number | null | undefined;
  }>;
  /** Restrict to one agent folder; "all" shows every agent. */
  providerFilter: ProviderKind | "all";
}

/**
 * Build the device -> agent -> workspace -> session tree.
 *
 * A discovery candidate that already has a live session is dropped, so the
 * same conversation never renders twice (once controllable, once as a
 * read-only stub).
 */
export function buildSessionTree(input: TreeInput): DeviceGroup[] {
  const { discovered, providerFilter } = input;

  const activeProviderIds = new Set<string>();
  const items: TreeSessionItem[] = [];

  for (const session of input.activeSessions) {
    const providerKey = `${session.runner_id}/${session.provider_session_id}`;
    activeProviderIds.add(providerKey);
    const disc = discovered.get(providerKey);
    const provider = getProviderInfo(
      session.provider_session_id,
      disc?.provider,
    );
    if (providerFilter !== "all" && providerFilter !== provider.type) continue;
    const shortId = session.provider_session_id.replace(
      /^(sess_|claude_|gemini_)/u,
      "",
    );
    items.push({
      kind: "active",
      key: providerKey,
      runnerId: session.runner_id,
      providerSessionId: session.provider_session_id,
      workspaceId: session.workspace_path,
      // The agent folder above already names the Provider, so an untitled
      // session falls back to its short id rather than repeating it.
      title: session.title ?? shortId.slice(0, 8),
      model: disc?.model,
      totalTokens: disc?.total_tokens,
      lastActivityAtMs: disc?.last_activity_at_ms,
      provider,
      sessionKey: `${session.runner_id}/${session.hub_session_id}`,
      session: {
        online: session.online,
        control_connected: session.control_connected,
        execution_state: session.execution_state,
      },
    });
  }

  for (const candidate of input.candidates) {
    const providerKey = `${candidate.runner_id}/${candidate.provider_session_id}`;
    if (activeProviderIds.has(providerKey)) continue;
    const disc = discovered.get(providerKey);
    const provider = getProviderInfo(
      candidate.provider_session_id,
      candidate.provider ?? disc?.provider,
    );
    if (providerFilter !== "all" && providerFilter !== provider.type) continue;
    const shortId = candidate.provider_session_id.replace(
      /^(sess_|claude_|gemini_)/u,
      "",
    );
    items.push({
      kind: "candidate",
      key: providerKey,
      runnerId: candidate.runner_id,
      providerSessionId: candidate.provider_session_id,
      workspaceId: candidate.workspace_path,
      title: candidate.title ?? shortId.slice(0, 8),
      model: candidate.model ?? disc?.model,
      totalTokens: candidate.total_tokens ?? disc?.total_tokens,
      lastActivityAtMs:
        candidate.last_activity_at_ms ?? disc?.last_activity_at_ms,
      provider,
      candidate: { resumable: candidate.resumable ?? true },
    });
  }

  const devices = new Map<string, DeviceGroup>();
  for (const device of input.devices) {
    devices.set(device.runner_id, {
      runnerId: device.runner_id,
      label: getDeviceLabel(device.runner_id, device.hello),
      online: device.online,
      workspaces: [],
      totalCount: 0,
    });
  }

  for (const item of items) {
    let device = devices.get(item.runnerId);
    if (device === undefined) {
      device = {
        runnerId: item.runnerId,
        label: getDeviceLabel(item.runnerId),
        online: false,
        workspaces: [],
        totalCount: 0,
      };
      devices.set(item.runnerId, device);
    }
    device.totalCount += 1;

    const provider = item.provider;
    let agent = device.workspaces.find((a) => a.provider === provider.type);
    if (agent === undefined) {
      agent = {
        provider: provider.type,
        label: provider.name,
        badgeClass: provider.badgeClass,
        workspaces: [],
        totalCount: 0,
      };
      device.workspaces.push(agent);
    }
    agent.totalCount += 1;

    let workspace = agent.workspaces.find((w) => w.id === item.workspaceId);
    if (workspace === undefined) {
      workspace = {
        id: item.workspaceId,
        label: workspaceLabel(item.workspaceId),
        items: [],
      };
      agent.workspaces.push(workspace);
    }
    workspace.items.push(item);
  }

  for (const device of devices.values()) {
    // Managed engines first, then alphabetical so the order never jitters
    // between polls as sessions stream in.
    device.workspaces.sort(
      (a, b) =>
        PROVIDER_ORDER[a.provider] - PROVIDER_ORDER[b.provider] ||
        collator.compare(a.label, b.label),
    );
    for (const agent of device.workspaces) {
      agent.workspaces.sort(byLabel);
      for (const workspace of agent.workspaces) {
        workspace.items.sort((a, b) => collator.compare(a.title, b.title));
      }
    }
  }

  return [...devices.values()].sort((a, b) => {
    // Online machines first; an offline machine's sessions cannot be used.
    if (a.online !== b.online) return a.online ? -1 : 1;
    return collator.compare(a.label, b.label);
  });
}
