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

export type ProviderKind =
  | "codex"
  | "claude"
  | "claude_desktop"
  | "antigravity"
  | "deepseek_harness"
  | (string & {});

export interface ProviderDescriptor {
  type: ProviderKind;
  name: string;
  badgeClass: string;
}

export interface PluginTreeHint {
  id: string;
  name?: string | undefined;
  displayName?: string | undefined;
  sessionPrefix?: string | undefined;
  sessionPrefixes?: ReadonlyArray<string> | undefined;
}

/** Ordering used for agent folders: 5 native core engines first, then plugins alphabetically. */
const NATIVE_PROVIDER_ORDER: Record<string, number> = {
  codex: 0,
  claude: 1,
  claude_desktop: 2,
  antigravity: 3,
  deepseek_harness: 4,
};

function providerSortRank(provider: ProviderKind): number {
  return NATIVE_PROVIDER_ORDER[provider] ?? 5;
}

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

const NATIVE_PROVIDER_BY_NAME: Record<string, ProviderDescriptor> = {
  codex: { type: "codex", name: "Codex", badgeClass: "codex" },
  claude_code: { type: "claude", name: "Claude Code", badgeClass: "claude" },
  claude: { type: "claude", name: "Claude Code", badgeClass: "claude" },
  // Desktop and the CLI share one transcript tree, so the Runner tags each
  // entry with the entry point that wrote it. Without this entry every
  // Desktop conversation fell through to the id prefix and was filed as a
  // Coding Assistant session.
  claude_desktop: {
    type: "claude_desktop",
    name: "Claude Desktop",
    badgeClass: "claude-desktop",
  },
  antigravity: {
    type: "antigravity",
    name: "Antigravity",
    badgeClass: "antigravity",
  },
  deepseek_harness: {
    type: "deepseek_harness",
    name: "DeepSeek Harness",
    badgeClass: "deepseek",
  },
  deepseek: {
    type: "deepseek_harness",
    name: "DeepSeek Harness",
    badgeClass: "deepseek",
  },
};

/**
 * Classify a Provider from its session id, authoritative Host provider string,
 * and optional Runner-installed ACP plugin list.
 */
export function getProviderInfo(
  providerSessionId: string,
  provider?: string,
  plugins?: ReadonlyArray<PluginTreeHint>,
): ProviderDescriptor {
  const norm = provider?.trim().toLowerCase() ?? "";
  if (norm !== "") {
    const native = NATIVE_PROVIDER_BY_NAME[norm];
    if (native !== undefined) return native;
    const matchedPlugin = plugins?.find((p) => p.id.toLowerCase() === norm);
    return {
      type: norm,
      name:
        matchedPlugin?.displayName || matchedPlugin?.name || provider!.trim(),
      badgeClass: "plugin",
    };
  }
  if (plugins) {
    for (const plugin of plugins) {
      const prefixes = [
        ...(plugin.sessionPrefixes ?? []),
        plugin.sessionPrefix || `${plugin.id}_`,
      ];
      if (prefixes.some((p) => p && providerSessionId.startsWith(p))) {
        const id = plugin.id.toLowerCase();
        return {
          type: id,
          name: plugin.displayName || plugin.name || plugin.id,
          badgeClass: "plugin",
        };
      }
    }
  }
  if (providerSessionId.startsWith("claude_")) {
    return { type: "claude", name: "Claude Code", badgeClass: "claude" };
  }
  if (providerSessionId.startsWith("antigravity_")) {
    return {
      type: "antigravity",
      name: "Antigravity",
      badgeClass: "antigravity",
    };
  }
  if (providerSessionId.startsWith("deepseek_")) {
    return {
      type: "deepseek_harness",
      name: "DeepSeek Harness",
      badgeClass: "deepseek",
    };
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
 * slug rather than showing the raw token. Also decodes triple-underscore
 * encoded path slugs (e.g. `C___Projects` -> `Projects`).
 */
export function workspaceLabel(workspaceId: string): string {
  let raw = workspaceId.startsWith("ws-")
    ? workspaceId.slice(3).replace(/-[0-9a-f]{16}$/u, "") || workspaceId
    : basename(workspaceId);
  if (raw.includes("___")) {
    const parts = raw
      .split(/___+/u)
      .flatMap((part) => part.split(/_+/u))
      .filter((part) => part.length > 0 && !/^[A-Za-z]:?$/u.test(part));
    if (parts.length > 0) {
      raw = parts[parts.length - 1]!;
    }
  }
  if (!/[\p{L}\p{N}]/u.test(raw)) {
    const hashMatch = /-([0-9a-f]{4})[0-9a-f]{12}$/iu.exec(workspaceId);
    return hashMatch ? `本地目录 · ${hashMatch[1]!.toLowerCase()}` : "本地目录";
  }
  return raw;
}

/**
 * Cleans raw transcript prompt titles (XML slash-command tags, system wrappers,
 * Markdown links, and inline code backticks) so sidebar session rows read as
 * natural human titles.
 */
export function sanitizeSessionTitle(raw?: string | null): string {
  if (!raw || !raw.trim()) return formatFallbackTitle();
  const cmdMatch = /<command-name>\s*([^<]+?)\s*<\/command-name>/iu.exec(raw);
  let cleaned = raw
    .replace(/<command-message>[\s\S]*?<\/command-message>/giu, " ")
    .replace(/<user_settings_change>[\s\S]*?<\/user_settings_change>/giu, " ")
    .replace(/<environment_context>[\s\S]*?<\/environment_context>/giu, " ")
    .replace(/<turn_aborted>[\s\S]*?<\/turn_aborted>/giu, " ")
    .replace(/<context_summary>[\s\S]*?<\/context_summary>/giu, " ")
    .replace(/<command-args>([\s\S]*?)<\/command-args>/giu, " $1 ")
    .replace(/<command-name>\s*([^<]+?)\s*<\/command-name>/giu, " $1 ")
    .replace(/<\/?[a-zA-Z][a-zA-Z0-9_-]*(?:\s[^>]*)?>/gu, " ")
    .replace(/\[([^\]]+)\]\([^)]+\)/gu, "$1")
    .replace(/`([^`\n]+)`/gu, "$1")
    .replace(/^\s*#{1,6}\s+/u, "")
    .replace(/[\r\n\t]+/gu, " ")
    .replace(/\s+/gu, " ")
    .trim();
  if (!cleaned && cmdMatch?.[1]) {
    cleaned = cmdMatch[1].trim();
  }
  return cleaned || formatFallbackTitle();
}

/**
 * The tree shows the workspace name (the folder), not the full path, because
 * the full path is long and mostly noise in a narrow sidebar. The full path
 * stays available as the row's tooltip.
 */
export function workspaceTooltip(workspaceId: string): string {
  return stripPathPrefix(workspaceId);
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
    provider?: string | undefined;
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
  /** Optional Runner-reported plugins for prefix and display name resolution. */
  plugins?: ReadonlyArray<PluginTreeHint> | undefined;
}

/**
 * Derives effective activity timestamp in milliseconds for session tree sorting.
 * Recently active sessions sort first, with active connected sessions at the top.
 */
export function getItemActivityMs(item: TreeSessionItem): number {
  const time =
    typeof item.lastActivityAtMs === "number" &&
    !Number.isNaN(item.lastActivityAtMs) &&
    item.lastActivityAtMs > 0
      ? item.lastActivityAtMs
      : 0;

  // Active connected sessions get the highest tier (2e14), followed by online
  // active sessions (1e14), then candidates/offline sessions (0).
  // Real timestamps (~1.7e12) preserve recency within each tier.
  if (item.kind === "active" && item.session?.control_connected) {
    return 2e14 + time;
  }
  if (item.kind === "active" && item.session?.online) {
    return 1e14 + time;
  }
  return time;
}

/**
 * Derives the maximum activity timestamp among all items in a workspace.
 * Avoids array allocations and spread syntax to eliminate call stack overflow risks.
 */
export function getWorkspaceActivityMs(workspace: {
  items: TreeSessionItem[];
}): number {
  if (workspace.items.length === 0) return 0;
  let max = 0;
  for (const item of workspace.items) {
    const act = getItemActivityMs(item);
    if (act > max) max = act;
  }
  return max;
}

/**
 * An untitled conversation is just "新会话".
 *
 * It used to append the first four characters of the provider session id, which
 * read as a serial number: the id is an internal key and has no meaning to the
 * person reading the sidebar. Two untitled sessions in the same workspace now
 * share a title, which is honest — neither of them has a name yet.
 */
export function formatFallbackTitle(): string {
  return "新会话";
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
      disc?.provider ?? session.provider,
      input.plugins,
    );
    if (providerFilter !== "all" && providerFilter !== provider.type) continue;
    items.push({
      kind: "active",
      key: providerKey,
      runnerId: session.runner_id,
      providerSessionId: session.provider_session_id,
      workspaceId: session.workspace_path,
      title: sanitizeSessionTitle(session.title),
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
      input.plugins,
    );
    if (providerFilter !== "all" && providerFilter !== provider.type) continue;
    items.push({
      kind: "candidate",
      key: providerKey,
      runnerId: candidate.runner_id,
      providerSessionId: candidate.provider_session_id,
      workspaceId: candidate.workspace_path,
      title: sanitizeSessionTitle(candidate.title),
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
        providerSortRank(a.provider) - providerSortRank(b.provider) ||
        collator.compare(a.label, b.label),
    );
    for (const agent of device.workspaces) {
      for (const workspace of agent.workspaces) {
        workspace.items.sort((a, b) => {
          const timeA = getItemActivityMs(a);
          const timeB = getItemActivityMs(b);
          if (timeA !== timeB) return timeB - timeA;
          return collator.compare(a.title, b.title);
        });
      }
      agent.workspaces.sort((a, b) => {
        const timeA = getWorkspaceActivityMs(a);
        const timeB = getWorkspaceActivityMs(b);
        if (timeA !== timeB) return timeB - timeA;
        return byLabel(a, b);
      });
    }
  }

  return [...devices.values()].sort((a, b) => {
    // Online machines first; an offline machine's sessions cannot be used.
    if (a.online !== b.online) return a.online ? -1 : 1;
    return collator.compare(a.label, b.label);
  });
}

/**
 * Every session across every device that is blocked on the person.
 *
 * The console's sidebar is a drill-down: you pick a computer, then an agent,
 * then a workspace, and only then see that machine's sessions. That is the
 * right shape for browsing, and it is not what this module changes. What it
 * fixes is the one thing a drill-down cannot show: a session on a machine you
 * are *not* looking at, sitting in `awaiting_permission` since this morning.
 *
 * So the sidebar keeps its selectors and gains one strip above them, built
 * from this function. The strip is independent of the selected device by
 * design — filtering it by the device selector would put the waiting session
 * back behind the dropdown it is trying to escape.
 */

/** The two execution states where the agent is blocked on a person. */
export function isWaitingOnPerson(executionState: string): boolean {
  return (
    executionState === "awaiting_permission" ||
    executionState === "awaiting_input"
  );
}

export interface AttentionEntry {
  /** `${runnerId}/${hubSessionId}` — the key the console selects by. */
  sessionKey: string;
  runnerId: string;
  hubSessionId: string;
  /** So a row can say which computer it is on without a second lookup. */
  deviceLabel: string;
  agentLabel: string;
  title: string;
  executionState: string;
  /** How many permission prompts this session is blocked on right now. */
  pendingApprovals: number;
  activityMs: number;
}

/**
 * Group pending approvals by `${runnerId}/${hubSessionId}`.
 *
 * `hello.pending_approvals` is the Host's own snapshot of what it is waiting
 * on, so it stays correct for sessions whose event stream this browser is not
 * attached to — which is every session but the open one. The event-derived
 * `pendingApprovals()` in `lib/remote.ts` answers a narrower question: what
 * the *open* session has asked for, in order, from its event window.
 *
 * Typed structurally rather than against `RemoteRunner` so this file stays
 * free of the transport layer and can be tested on plain objects.
 */
export function groupPendingApprovals<
  T extends { approval_id: string; hub_session_id: string },
>(
  runners: ReadonlyArray<{
    runner_id: string;
    hello: { pending_approvals?: T[] } | null;
  }>,
): Map<string, T[]> {
  const grouped = new Map<string, T[]>();
  for (const runner of runners) {
    for (const approval of runner.hello?.pending_approvals ?? []) {
      const k = `${runner.runner_id}/${approval.hub_session_id}`;
      const bucket = grouped.get(k);
      if (bucket === undefined) grouped.set(k, [approval]);
      else bucket.push(approval);
    }
  }
  return grouped;
}

/**
 * Sessions a person has to act on, newest activity first.
 *
 * A session qualifies on either signal, and they are not the same signal:
 *
 * - `execution_state` is what the Provider runtime said it is blocked on;
 * - `hello.pending_approvals` is what the Host is holding an open prompt for.
 *
 * They usually agree, but they are published on different clocks, so a session
 * can be in the approval list for a beat before its state flips. Requiring
 * both would make the row appear late; taking either makes it appear when
 * either source knows, which is the behaviour that keeps the strip honest.
 *
 * `control_connected` is required, matching `attentionEntries` on Android
 * (`apps/android/.../ui/SessionTree.kt`). An approval on a session this Host
 * is not controlling cannot be resolved from here, so listing it would offer
 * a button that fails. The two clients must answer this question the same way
 * or the phone and the browser disagree about who is waiting.
 */
export function attentionEntries(
  tree: DeviceGroup[],
  pendingBySessionKey?: ReadonlyMap<string, readonly unknown[]>,
): AttentionEntry[] {
  const entries: AttentionEntry[] = [];

  for (const device of tree) {
    for (const agent of device.workspaces) {
      for (const workspace of agent.workspaces) {
        for (const item of workspace.items) {
          if (item.kind !== "active") continue;
          if (!item.session.control_connected) continue;

          const pendingApprovals =
            pendingBySessionKey?.get(item.sessionKey)?.length ?? 0;
          if (
            !isWaitingOnPerson(item.session.execution_state) &&
            pendingApprovals === 0
          ) {
            continue;
          }

          entries.push({
            sessionKey: item.sessionKey,
            runnerId: item.runnerId,
            // `sessionKey` is built as `${runnerId}/${hubSessionId}` by
            // `buildSessionTree`; slicing keeps a `/` inside either half from
            // splitting the wrong boundary.
            hubSessionId: item.sessionKey.slice(item.runnerId.length + 1),
            deviceLabel: device.label,
            agentLabel: agent.label,
            title: item.title,
            executionState: item.session.execution_state,
            pendingApprovals,
            activityMs: getItemActivityMs(item),
          });
        }
      }
    }
  }

  // Most recent first, the same order Android's `attentionEntries` uses. A
  // strip that reorders itself between polls is one people stop trusting.
  entries.sort((a, b) => b.activityMs - a.activityMs);
  return entries;
}
