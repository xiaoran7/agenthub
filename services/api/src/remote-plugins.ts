import { randomUUID } from "node:crypto";
import { isIP } from "node:net";
import type { StatementSync } from "node:sqlite";
import {
  isValidAgentType,
  type AgentPlugin,
  type PluginActionRequest,
  type PluginCapabilities,
  type PluginCommandAction,
  type PluginRuntimeStatus,
  type PluginStatePayload,
  type RemotePluginsResponse,
} from "@agent-pulse/contracts";
import type { FastifyInstance, FastifyRequest } from "fastify";
import type { WebSocket } from "ws";
import { RemoteStoreError } from "./remote-store-validation.js";

const PLUGIN_ACTIONS: ReadonlySet<PluginCommandAction> = new Set([
  "list",
  "catalog",
  "refresh_catalog",
  "install",
  "uninstall",
  "toggle",
  "enable",
  "disable",
  "set_registry",
]);

const RUNTIME_STATUSES: ReadonlySet<PluginRuntimeStatus> = new Set([
  "not_installed",
  "idle",
  "ready",
  "running",
  "downloading",
  "error",
  "disabled",
]);

function isForbiddenIpv4(a: number, b: number): boolean {
  return (
    a === 0 ||
    a === 10 ||
    a === 127 ||
    a >= 224 ||
    (a === 169 && b === 254) ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 100 && b >= 64 && b <= 127)
  );
}

function parseIpv6Groups(ip: string): number[] | null {
  const noZone = ip.split("%")[0]?.toLowerCase() ?? "";
  if (!noZone) return null;
  const dbl = noZone.split("::");
  if (dbl.length > 2) return null;
  const parseSide = (part: string): number[] | null => {
    if (!part) return [];
    const segs = part.split(":");
    const out: number[] = [];
    for (let i = 0; i < segs.length; i += 1) {
      const seg = segs[i]!;
      if (seg.includes(".")) {
        if (i !== segs.length - 1 || isIP(seg) !== 4) return null;
        const [a, b, c, d] = seg.split(".").map(Number);
        out.push(((a! << 8) | b!) >>> 0, ((c! << 8) | d!) >>> 0);
      } else if (/^[0-9a-f]{1,4}$/.test(seg)) {
        out.push(parseInt(seg, 16));
      } else {
        return null;
      }
    }
    return out;
  };
  const left = parseSide(dbl[0] ?? "");
  const right = dbl.length === 2 ? parseSide(dbl[1] ?? "") : [];
  if (!left || !right) return null;
  const missing = 8 - (left.length + right.length);
  if (
    (dbl.length === 1 && missing !== 0) ||
    (dbl.length === 2 && missing < 1)
  ) {
    return null;
  }
  return [...left, ...new Array<number>(missing).fill(0), ...right];
}

function isForbiddenHost(hostname: string): boolean {
  const h = hostname
    .trim()
    .replace(/^\[|\]$/g, "")
    .split("%")[0]!
    .toLowerCase();
  if (
    !h ||
    h === "localhost" ||
    h.endsWith(".localhost") ||
    h.endsWith(".local") ||
    h.endsWith(".internal")
  ) {
    return true;
  }
  const ipKind = isIP(h);
  if (ipKind === 4) {
    const [a, b] = h.split(".").map(Number);
    return isForbiddenIpv4(a!, b!);
  }
  if (ipKind === 6 || h.includes(":")) {
    const g = parseIpv6Groups(h);
    if (!g) return true;
    const zeroHead = g.slice(0, 5).every((v) => v === 0);
    if (zeroHead && g[5] === 0 && g[6] === 0 && g[7]! <= 1) return true;
    if (zeroHead && (g[5] === 0xffff || g[5] === 0)) {
      return isForbiddenIpv4(g[6]! >> 8, g[6]! & 0xff);
    }
    const first = g[0]!;
    return (
      (first & 0xfe00) === 0xfc00 ||
      (first & 0xffc0) === 0xfe80 ||
      (first & 0xff00) === 0xff00
    );
  }
  return false;
}

function isSafeHttpsRegistryUrl(raw: unknown): raw is string {
  if (typeof raw !== "string") return false;
  const trimmed = raw.trim();
  if (!trimmed || trimmed.length > 1024) return false;
  try {
    const u = new URL(trimmed);
    if (u.username || u.password) return false;
    if (process.env.AGENTHUB_ALLOW_INSECURE_PLUGIN_REGISTRY === "1") {
      return u.protocol === "https:" || u.protocol === "http:";
    }
    return u.protocol === "https:" && !isForbiddenHost(u.hostname);
  } catch {
    return false;
  }
}

export function parsePluginActionRequest(body: unknown): PluginActionRequest {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    throw new RemoteStoreError("invalid_body");
  }
  const obj = body as Record<string, unknown>;
  const allowed = new Set([
    "action",
    "plugin_id",
    "enabled",
    "source_url",
    "registry_url",
  ]);
  if (Object.keys(obj).some((k) => !allowed.has(k))) {
    throw new RemoteStoreError("invalid_body");
  }
  if (
    typeof obj.action !== "string" ||
    !PLUGIN_ACTIONS.has(obj.action as PluginCommandAction)
  ) {
    throw new RemoteStoreError("invalid_plugin_action");
  }
  const action = obj.action as PluginCommandAction;
  const requiresPluginId = [
    "install",
    "uninstall",
    "toggle",
    "enable",
    "disable",
  ].includes(action);

  let plugin_id: string | undefined;
  if (obj.plugin_id !== undefined) {
    if (!isValidAgentType(obj.plugin_id)) {
      throw new RemoteStoreError("invalid_plugin_id");
    }
    plugin_id = obj.plugin_id;
  } else if (requiresPluginId) {
    throw new RemoteStoreError("invalid_plugin_id");
  }

  if (obj.enabled !== undefined && typeof obj.enabled !== "boolean") {
    throw new RemoteStoreError("invalid_body");
  }
  if (obj.source_url !== undefined && obj.source_url !== null) {
    throw new RemoteStoreError("remote_source_url_unsupported");
  }
  let registry_url: string | undefined;
  if (obj.registry_url !== undefined && obj.registry_url !== null) {
    if (typeof obj.registry_url !== "string") {
      throw new RemoteStoreError("invalid_plugin_registry_url");
    }
    const trimmed = obj.registry_url.trim();
    if (trimmed !== "") {
      if (!isSafeHttpsRegistryUrl(trimmed)) {
        throw new RemoteStoreError("invalid_plugin_registry_url");
      }
      registry_url = new URL(trimmed).toString();
    } else {
      registry_url = "";
    }
  }

  return {
    action,
    ...(plugin_id !== undefined ? { plugin_id } : {}),
    ...(obj.enabled !== undefined ? { enabled: obj.enabled } : {}),
    ...(registry_url !== undefined ? { registry_url } : {}),
  };
}

function normalizeCapabilities(raw: unknown): PluginCapabilities {
  const obj =
    raw && typeof raw === "object" && !Array.isArray(raw)
      ? (raw as Record<string, unknown>)
      : {};
  return {
    discover: obj.discover !== false,
    launch: obj.launch !== false,
    resume: obj.resume !== false,
    models: obj.models !== false,
    history: obj.history !== false,
    approvals: obj.approvals === true,
  };
}

export function normalizePluginItem(
  raw: unknown,
  runner?: string,
): AgentPlugin | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const obj = raw as Record<string, unknown>;
  if (!isValidAgentType(obj.id)) return null;
  const statusRaw = obj.runtimeStatus ?? obj.runtime_status;
  const runtimeStatus: PluginRuntimeStatus =
    typeof statusRaw === "string" &&
    RUNTIME_STATUSES.has(statusRaw as PluginRuntimeStatus)
      ? (statusRaw as PluginRuntimeStatus)
      : obj.enabled === true
        ? "ready"
        : "idle";
  const name =
    typeof obj.name === "string" && obj.name.trim() ? obj.name.trim() : obj.id;
  const displayName =
    typeof obj.displayName === "string" && obj.displayName.trim()
      ? obj.displayName.trim()
      : typeof obj.display_name === "string" && obj.display_name.trim()
        ? obj.display_name.trim()
        : name;
  const installedVersion =
    typeof obj.installedVersion === "string"
      ? obj.installedVersion
      : typeof obj.installed_version === "string"
        ? obj.installed_version
        : undefined;
  const sessionPrefix =
    typeof obj.sessionPrefix === "string"
      ? obj.sessionPrefix
      : typeof obj.session_prefix === "string"
        ? obj.session_prefix
        : undefined;
  const rawPrefixes = obj.sessionPrefixes ?? obj.session_prefixes;
  const sessionPrefixes = Array.isArray(rawPrefixes)
    ? Array.from(
        new Set(
          rawPrefixes
            .filter((p): p is string => typeof p === "string")
            .map((p) => p.trim())
            .filter(Boolean),
        ),
      )
    : undefined;
  const binaryPath =
    typeof obj.binaryPath === "string"
      ? obj.binaryPath
      : typeof obj.binary_path === "string"
        ? obj.binary_path
        : undefined;
  const sourceUrl =
    typeof obj.sourceUrl === "string"
      ? obj.sourceUrl
      : typeof obj.source_url === "string"
        ? obj.source_url
        : undefined;
  const updatedAt =
    typeof obj.updatedAt === "string" && obj.updatedAt.trim()
      ? obj.updatedAt
      : typeof obj.updated_at === "string" && obj.updated_at.trim()
        ? obj.updated_at
        : new Date().toISOString();

  return {
    id: obj.id,
    name,
    displayName,
    category:
      typeof obj.category === "string" && obj.category.trim()
        ? obj.category
        : "agent",
    description: typeof obj.description === "string" ? obj.description : "",
    version:
      typeof obj.version === "string" && obj.version.trim()
        ? obj.version
        : "1.0.0",
    ...(installedVersion !== undefined ? { installedVersion } : {}),
    installed: Boolean(obj.installed),
    enabled: Boolean(obj.enabled),
    runtimeStatus,
    ...(typeof obj.error === "string" ? { error: obj.error } : {}),
    ...(sessionPrefix !== undefined ? { sessionPrefix } : {}),
    ...(sessionPrefixes && sessionPrefixes.length > 0
      ? { sessionPrefixes }
      : {}),
    ...(Array.isArray(obj.aliases)
      ? {
          aliases: obj.aliases.filter(
            (a): a is string => typeof a === "string" && a.trim().length > 0,
          ),
        }
      : {}),
    ...(typeof obj.icon === "string" ? { icon: obj.icon } : {}),
    capabilities: normalizeCapabilities(obj.capabilities),
    ...(typeof obj.available === "boolean" ? { available: obj.available } : {}),
    ...(binaryPath !== undefined ? { binaryPath } : {}),
    ...(sourceUrl !== undefined ? { sourceUrl } : {}),
    updatedAt,
    ...(runner ? { runner } : {}),
  };
}

export function normalizePluginList(
  raw: unknown,
  runner?: string,
): AgentPlugin[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const out: AgentPlugin[] = [];
  for (const item of raw) {
    const parsed = normalizePluginItem(item, runner);
    if (!parsed || seen.has(parsed.id)) continue;
    seen.add(parsed.id);
    out.push(parsed);
  }
  return out;
}

export function parsePluginStateFrame(
  frame: Record<string, unknown>,
  runner?: string,
): PluginStatePayload {
  if (frame.type !== "plugin_state" || !Array.isArray(frame.plugins)) {
    throw new RemoteStoreError("invalid_plugin_state");
  }
  if (
    frame.request_id !== undefined &&
    frame.request_id !== null &&
    (typeof frame.request_id !== "string" || frame.request_id.length > 128)
  ) {
    throw new RemoteStoreError("invalid_plugin_state");
  }
  if (
    frame.registry_url !== undefined &&
    frame.registry_url !== null &&
    (typeof frame.registry_url !== "string" || frame.registry_url.length > 1024)
  ) {
    throw new RemoteStoreError("invalid_plugin_state");
  }
  if (frame.ok !== undefined && typeof frame.ok !== "boolean") {
    throw new RemoteStoreError("invalid_plugin_state");
  }
  if (
    frame.error !== undefined &&
    frame.error !== null &&
    (typeof frame.error !== "string" || frame.error.length > 4096)
  ) {
    throw new RemoteStoreError("invalid_plugin_state");
  }
  return {
    type: "plugin_state",
    ...(typeof frame.request_id === "string"
      ? { request_id: frame.request_id }
      : {}),
    ...(runner ? { runner } : {}),
    ...(typeof frame.registry_url === "string"
      ? { registry_url: frame.registry_url }
      : {}),
    plugins: normalizePluginList(frame.plugins, runner),
    ...(typeof frame.ok === "boolean" ? { ok: frame.ok } : {}),
    ...(typeof frame.error === "string" ? { error: frame.error } : {}),
  };
}

interface PendingCommand {
  runner: string;
  resolve: (state: PluginStatePayload) => void;
  reject: (err: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class PendingPluginCommandBroker {
  private readonly pending = new Map<string, PendingCommand>();

  register(
    requestId: string,
    runner: string,
    timeoutMs: number = 60_000,
  ): Promise<PluginStatePayload> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(new RemoteStoreError("plugin_action_timeout"));
      }, timeoutMs);
      timer.unref?.();
      this.pending.set(requestId, { runner, resolve, reject, timer });
    });
  }

  resolve(runner: string, state: PluginStatePayload): boolean {
    if (!state.request_id || typeof state.ok !== "boolean") {
      return false;
    }
    const entry = this.pending.get(state.request_id);
    if (!entry || entry.runner !== runner) {
      return false;
    }
    clearTimeout(entry.timer);
    this.pending.delete(state.request_id);
    entry.resolve(state);
    return true;
  }

  reject(requestId: string, code: string): void {
    const entry = this.pending.get(requestId);
    if (!entry) return;
    clearTimeout(entry.timer);
    this.pending.delete(requestId);
    entry.reject(new RemoteStoreError(code));
  }

  rejectForRunner(runner: string, code: string): void {
    for (const [reqId, entry] of [...this.pending.entries()]) {
      if (entry.runner !== runner) continue;
      clearTimeout(entry.timer);
      this.pending.delete(reqId);
      entry.reject(new RemoteStoreError(code));
    }
  }
}

export type StmtFn = (sql: string) => StatementSync;

export function upsertRunnerPluginsRow(
  stmt: StmtFn,
  owner: string,
  runner: string,
  registryUrl: string | undefined,
  plugins: AgentPlugin[],
): void {
  const now = new Date().toISOString();
  const cleaned = plugins.map((p) => {
    const copy = { ...p };
    delete copy.runner;
    return copy;
  });
  stmt(
    `INSERT INTO remote_runner_plugins (owner, runner, registry_url, plugins_json, updated_at)
     VALUES (?, ?, ?, ?, ?)
     ON CONFLICT(runner) DO UPDATE SET
       owner=excluded.owner,
       registry_url=COALESCE(excluded.registry_url, remote_runner_plugins.registry_url),
       plugins_json=excluded.plugins_json,
       updated_at=excluded.updated_at`,
  ).run(owner, runner, registryUrl ?? null, JSON.stringify(cleaned), now);
}

export function queryRunnerPluginsResponse(
  stmt: StmtFn,
  owner: string,
  runnerFilter?: string,
): RemotePluginsResponse {
  const rows = (
    runnerFilter
      ? stmt(
          "SELECT runner, registry_url, plugins_json, updated_at FROM remote_runner_plugins WHERE owner=? AND runner=? ORDER BY runner",
        ).all(owner, runnerFilter)
      : stmt(
          "SELECT runner, registry_url, plugins_json, updated_at FROM remote_runner_plugins WHERE owner=? ORDER BY runner",
        ).all(owner)
  ) as Array<{
    runner: string;
    registry_url: string | null;
    plugins_json: string;
    updated_at: string;
  }>;

  const runners: RemotePluginsResponse["runners"] = [];
  const byId = new Map<string, AgentPlugin>();
  for (const row of rows) {
    let parsed: AgentPlugin[] = [];
    try {
      parsed = normalizePluginList(
        JSON.parse(row.plugins_json) as unknown,
        row.runner,
      );
    } catch {
      parsed = [];
    }
    runners.push({
      runner: row.runner,
      ...(row.registry_url ? { registry_url: row.registry_url } : {}),
      updated_at: row.updated_at,
      plugins: parsed,
    });
    for (const plugin of parsed) {
      const prev = byId.get(plugin.id);
      if (
        !prev ||
        (!prev.installed && plugin.installed) ||
        (!prev.enabled && plugin.enabled)
      ) {
        byId.set(plugin.id, plugin);
      }
    }
  }
  return { plugins: [...byId.values()], runners };
}

export function updateSingleRunnerPluginStatus(
  stmt: StmtFn,
  owner: string,
  runner: string,
  pluginId: string,
  status: AgentPlugin["runtimeStatus"],
  error?: string | null,
): void {
  if (!isValidAgentType(pluginId)) return;
  const row = stmt(
    "SELECT registry_url, plugins_json FROM remote_runner_plugins WHERE owner=? AND runner=?",
  ).get(owner, runner) as
    { registry_url: string | null; plugins_json: string } | undefined;
  if (!row) return;
  const plugins = normalizePluginList(
    JSON.parse(row.plugins_json) as unknown,
    runner,
  );
  const target = plugins.find((p) => p.id === pluginId);
  if (!target) return;
  target.runtimeStatus = status;
  target.updatedAt = new Date().toISOString();
  if (error) target.error = error;
  else delete target.error;
  upsertRunnerPluginsRow(
    stmt,
    owner,
    runner,
    row.registry_url ?? undefined,
    plugins,
  );
}

export interface PluginRouteConn {
  socket: WebSocket;
  owner: string;
  runner: string;
  supportsPluginsV2?: boolean;
}

export interface PluginRoutesDeps<C extends PluginRouteConn> {
  app: FastifyInstance;
  connections: ReadonlyMap<string, C>;
  pluginBroker: PendingPluginCommandBroker;
  listRunnerPlugins: (owner: string, runner?: string) => RemotePluginsResponse;
  listActiveRunners?: (owner: string) => Promise<Set<string>>;
  resolveOwner: (request: FastifyRequest) => Promise<string>;
  resolveActor: (
    request: FastifyRequest,
  ) => Promise<{ owner: string; role: "owner" | "admin" | "member" }>;
  requireDevice: (owner: string, runner: string) => Promise<void>;
  transmit: (conn: C, value: unknown) => void;
}

export function registerRemotePluginRoutes<C extends PluginRouteConn>(
  deps: PluginRoutesDeps<C>,
): void {
  const {
    app,
    connections,
    pluginBroker,
    listRunnerPlugins,
    listActiveRunners,
    resolveOwner,
    resolveActor,
    requireDevice,
    transmit,
  } = deps;

  app.get<{ Querystring: { runner?: string } }>(
    "/v3/remote/plugins",
    async (request) => {
      const owner = await resolveOwner(request);
      const resp = listRunnerPlugins(owner, request.query?.runner);
      if (!listActiveRunners) return resp;
      const active = await listActiveRunners(owner);
      const runners = resp.runners.filter((r) => active.has(r.runner));
      const byId = new Map<string, AgentPlugin>();
      for (const r of runners) {
        for (const plugin of r.plugins) {
          const prev = byId.get(plugin.id);
          if (
            !prev ||
            (!prev.installed && plugin.installed) ||
            (!prev.enabled && plugin.enabled)
          ) {
            byId.set(plugin.id, plugin);
          }
        }
      }
      return { plugins: [...byId.values()], runners };
    },
  );

  app.post<{ Params: { runner: string }; Body: unknown }>(
    "/v3/remote/runners/:runner/plugins/action",
    async (request, reply) => {
      const actor = await resolveActor(request);
      if (actor.role === "member") {
        throw new RemoteStoreError("plugin_permission_denied");
      }
      const { runner } = request.params;
      await requireDevice(actor.owner, runner);
      const actionReq = parsePluginActionRequest(request.body);
      const conn = connections.get(runner);
      if (
        !conn ||
        conn.owner !== actor.owner ||
        conn.socket.readyState !== 1 ||
        !conn.supportsPluginsV2
      ) {
        throw new RemoteStoreError("runner_offline");
      }
      const requestId = `plg-${randomUUID()}`;
      const waitState = pluginBroker.register(requestId, runner);
      try {
        transmit(conn, {
          type: "plugin_command",
          request_id: requestId,
          ...actionReq,
        });
      } catch (err) {
        const code =
          err instanceof RemoteStoreError ? err.code : "runner_offline";
        pluginBroker.reject(requestId, code);
        waitState.catch(() => {});
        throw err;
      }
      const state = await waitState;
      const status = state.ok === false ? 400 : 200;
      return reply.status(status).send({
        ok: state.ok !== false,
        runner,
        registry_url: state.registry_url,
        plugins: state.plugins,
        ...(state.error ? { error: state.error } : {}),
      });
    },
  );

  app.post<{ Params: { id: string }; Body: unknown }>(
    "/v3/remote/plugins/:id/toggle",
    async (request, reply) => {
      const actor = await resolveActor(request);
      if (actor.role === "member") {
        throw new RemoteStoreError("plugin_permission_denied");
      }
      const { id } = request.params;
      if (!isValidAgentType(id)) {
        throw new RemoteStoreError("invalid_plugin_id");
      }
      let enabled: boolean | undefined;
      let runner: string | undefined;
      if (request.body !== undefined && request.body !== null) {
        if (typeof request.body !== "object" || Array.isArray(request.body)) {
          throw new RemoteStoreError("invalid_body");
        }
        const obj = request.body as Record<string, unknown>;
        if (
          Object.keys(obj).some((key) => key !== "enabled" && key !== "runner")
        ) {
          throw new RemoteStoreError("invalid_body");
        }
        if (obj.enabled !== undefined && typeof obj.enabled !== "boolean") {
          throw new RemoteStoreError("invalid_body");
        }
        if (obj.runner !== undefined && typeof obj.runner !== "string") {
          throw new RemoteStoreError("invalid_body");
        }
        enabled = obj.enabled;
        runner = obj.runner;
      }
      if (runner) {
        await requireDevice(actor.owner, runner);
      }
      const summary = listRunnerPlugins(actor.owner);
      const targetConn = (() => {
        if (runner) {
          const c = connections.get(runner);
          return c &&
            c.owner === actor.owner &&
            c.socket.readyState === 1 &&
            c.supportsPluginsV2
            ? c
            : undefined;
        }
        for (const r of summary.runners) {
          if (r.plugins.some((p) => p.id === id)) {
            const c = connections.get(r.runner);
            if (
              c &&
              c.owner === actor.owner &&
              c.socket.readyState === 1 &&
              c.supportsPluginsV2
            ) {
              return c;
            }
          }
        }
        for (const c of connections.values()) {
          if (
            c.owner === actor.owner &&
            c.socket.readyState === 1 &&
            c.supportsPluginsV2
          ) {
            return c;
          }
        }
        return undefined;
      })();
      if (!targetConn) {
        throw new RemoteStoreError("runner_offline");
      }
      const requestId = `plg-${randomUUID()}`;
      const waitState = pluginBroker.register(requestId, targetConn.runner);
      try {
        transmit(targetConn, {
          type: "plugin_command",
          request_id: requestId,
          action: "toggle",
          plugin_id: id,
          ...(enabled !== undefined ? { enabled } : {}),
        });
      } catch (err) {
        const code =
          err instanceof RemoteStoreError ? err.code : "runner_offline";
        pluginBroker.reject(requestId, code);
        waitState.catch(() => {});
        throw err;
      }
      const state = await waitState;
      const plugin = state.plugins.find((p) => p.id === id);
      if (state.ok === false) {
        const code = state.error?.includes("not_found") ? 404 : 400;
        return reply.status(code).send({
          error: state.error ?? "plugin_toggle_failed",
          plugins: state.plugins,
        });
      }
      if (!plugin) throw new RemoteStoreError("plugin_not_found");
      return { plugin, plugins: state.plugins, runner: targetConn.runner };
    },
  );
}
