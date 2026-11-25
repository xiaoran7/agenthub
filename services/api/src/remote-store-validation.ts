import { createHash } from "node:crypto";
import {
  isValidAgentType,
  parseRemoteBinding,
  parseRemotePendingApproval,
  parseRemotePendingQuestion,
  type RemoteBinding,
  type RemoteCommand,
} from "@agent-pulse/contracts";

export const REMOTE_STORE_SCHEMA = `PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON;
  CREATE TABLE IF NOT EXISTS remote_runners (runner TEXT PRIMARY KEY, owner TEXT NOT NULL, received INTEGER NOT NULL DEFAULT 0, command_seq INTEGER NOT NULL DEFAULT 0, command_ack INTEGER NOT NULL DEFAULT 0, hello TEXT, device_name TEXT);
  CREATE INDEX IF NOT EXISTS remote_runner_owner ON remote_runners(owner,runner);
  CREATE TABLE IF NOT EXISTS remote_commands (runner TEXT NOT NULL, cmd_id TEXT NOT NULL, seq INTEGER NOT NULL, hub TEXT, cmd_digest TEXT, body TEXT NOT NULL, receipt TEXT, PRIMARY KEY(runner,cmd_id), UNIQUE(runner,seq));
  CREATE TABLE IF NOT EXISTS remote_sessions (owner TEXT NOT NULL, runner TEXT NOT NULL, hub TEXT NOT NULL, native TEXT NOT NULL, binding TEXT NOT NULL, PRIMARY KEY(runner,hub), UNIQUE(runner,native));
  CREATE INDEX IF NOT EXISTS remote_session_owner ON remote_sessions(owner,runner,hub);
  CREATE TABLE IF NOT EXISTS remote_events (id INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, runner TEXT NOT NULL, runner_seq INTEGER NOT NULL, hub TEXT, payload TEXT NOT NULL, UNIQUE(runner,runner_seq));
  CREATE INDEX IF NOT EXISTS remote_event_owner ON remote_events(owner,id);
  CREATE INDEX IF NOT EXISTS remote_event_session ON remote_events(owner,runner,hub,id);
  CREATE INDEX IF NOT EXISTS remote_event_runner_id ON remote_events(runner,id);
  CREATE TABLE IF NOT EXISTS remote_runner_plugins (owner TEXT NOT NULL, runner TEXT NOT NULL, registry_url TEXT, plugins_json TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(runner));
  CREATE INDEX IF NOT EXISTS remote_runner_plugins_owner ON remote_runner_plugins(owner,runner);`;

export function isWorkspaceAuthorizedInHello(
  hello: Record<string, unknown>,
  workspace: string,
): boolean {
  const discoveryCandidates = (
    hello.discovery as { candidates?: Array<{ workspace_path?: string }> }
  )?.candidates;
  const norm = (p: string): string =>
    p
      .replace(/^\\\\\?\\/u, "")
      .replace(/[\\/]+/gu, "/")
      .replace(/\/+$/u, "")
      .toLowerCase();
  const target = norm(workspace);
  const isDiscovered =
    Array.isArray(discoveryCandidates) &&
    discoveryCandidates.some(
      (c) =>
        typeof c === "object" &&
        c !== null &&
        typeof c.workspace_path === "string" &&
        norm(c.workspace_path) === target,
    );
  if (isDiscovered) return true;
  return (
    Array.isArray(hello.workspaces) &&
    hello.workspaces.some((w) => typeof w === "string" && norm(w) === target)
  );
}

export class RemoteStoreError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

/**
 * The parts of a `hello` the session list renders, and nothing else.
 *
 * `models` and `discovery` are dropped here rather than in the caller because
 * they are the two fields that made the list heavy: the model catalog and the
 * whole discovery candidate list are kilobytes per device, and the console
 * re-reads this response on every Gateway push. The console still gets both,
 * from `GET /v3/remote/runners/:id/hello`, which it merges in per device.
 */
export const RUNNER_HELLO_LIST_FIELDS = [
  "host_generation",
  "device_name",
  "workspaces",
  "workspace_labels",
  "pending_approvals",
  "provider",
  "plugins",
] as const;

export function projectRunnerHello(
  hello: Record<string, unknown> | null,
): Record<string, unknown> | null {
  if (!hello) return null;
  const thin: Record<string, unknown> = {};
  for (const key of RUNNER_HELLO_LIST_FIELDS) {
    if (key in hello) thin[key] = hello[key];
  }
  return thin;
}

/**
 * Extracts a `provider_session_id -> provider` lookup map from `hello.discovery.candidates`
 * so the thin `/sessions` response can tag active sessions with their authoritative provider
 * without embedding the full candidate list.
 */
export function extractDiscoveryProviders(
  hello: Record<string, unknown> | null,
): Map<string, string> {
  const providers = new Map<string, string>();
  const candidates = (
    hello?.discovery as
      | {
          candidates?: Array<{
            provider_session_id?: unknown;
            provider?: unknown;
          }>;
        }
      | undefined
  )?.candidates;
  if (!Array.isArray(candidates)) return providers;
  for (const candidate of candidates) {
    if (
      typeof candidate?.provider_session_id === "string" &&
      typeof candidate?.provider === "string"
    ) {
      const trimmed = candidate.provider.trim();
      if (trimmed !== "") {
        providers.set(candidate.provider_session_id, trimmed);
      }
    }
  }
  return providers;
}

/**
 * The frame as the event log stores it.
 *
 * A `ready` frame is a snapshot of current state, not a change: it carries the
 * whole model catalog and the whole discovery list every time the Runner
 * republishes. Neither belongs in an append-only log. They are current state
 * and already live in `remote_runners.hello`, which is the only place any
 * reader has ever taken them from -- a text search over the clients finds no
 * reader of `discovery` or `models` on an event payload, while `hello` is read
 * in five places in the web console and parsed by the Android client.
 *
 * Everything the log's readers do use is kept: `pending_approvals`, which
 * `resumePoint()` finds by scanning the payload text, and the rest of the
 * frame that the duplicate check deep-compares.
 *
 * Other frame types are returned unchanged: an `event` frame's `raw` payload
 * is the transcript itself and must never be trimmed.
 */
export function storableFrame(
  payload: Record<string, unknown>,
): Record<string, unknown> {
  if (payload.type !== "ready") return payload;
  const {
    discovery: _discovery,
    models: _models,
    plugins: _plugins,
    ...rest
  } = payload;
  return rest;
}

export function requireOnlyKeys(
  value: Record<string, unknown>,
  keys: string[],
  code: string,
): void {
  if (Object.keys(value).some((key) => !keys.includes(key)))
    throw new RemoteStoreError(code);
}

export function isSameProviderRebind(
  previous: unknown,
  next: unknown,
): boolean {
  if (typeof previous !== "string" || typeof next !== "string") return false;
  for (const prefix of [
    "claude_",
    "antigravity_",
    "deepseek_",
    "zcode_",
    "gemini_",
    "grok_",
    "sess_",
    "acp_",
  ]) {
    if (previous.startsWith(prefix) && next.startsWith(prefix)) return true;
  }
  const prevMatch = /^([a-z][a-z0-9_-]{1,47}_)[^_]+$/u.exec(previous);
  const nextMatch = /^([a-z][a-z0-9_-]{1,47}_)[^_]+$/u.exec(next);
  if (prevMatch && nextMatch && prevMatch[1] === nextMatch[1]) return true;
  return false;
}

export function validateRemoteEventFields(
  event: Record<string, unknown>,
): void {
  for (const key of ["event_id", "host_generation", "kind"])
    if (
      typeof event[key] !== "string" ||
      !(event[key] as string).trim() ||
      (event[key] as string).length > 256
    )
      throw new RemoteStoreError("invalid_event");
  for (const key of [
    "hub_session_id",
    "provider_session_id",
    "approval_id",
    "question_id",
  ])
    if (
      event[key] !== null &&
      event[key] !== undefined &&
      (typeof event[key] !== "string" ||
        !(event[key] as string).trim() ||
        (event[key] as string).length > 256)
    )
      throw new RemoteStoreError("invalid_event");
  if (event.binding !== null && event.binding !== undefined) {
    try {
      const binding = parseRemoteBinding(event.binding);
      if (
        binding.provider_session_id !== event.provider_session_id &&
        isSameProviderRebind(
          event.provider_session_id,
          binding.provider_session_id,
        )
      ) {
        event.provider_session_id = binding.provider_session_id;
      }
      if (
        binding.host_generation !== event.host_generation ||
        binding.hub_session_id !== event.hub_session_id ||
        binding.provider_session_id !== event.provider_session_id
      )
        throw new RemoteStoreError("binding_conflict");
    } catch (error) {
      if (error instanceof RemoteStoreError) throw error;
      throw new RemoteStoreError("invalid_event");
    }
  }
}

export function validateRemoteApprovalEvent(
  event: Record<string, unknown>,
): void {
  if ("raw" in event) throw new RemoteStoreError("unsafe_approval_event");
  if (
    typeof event.event_id !== "string" ||
    !event.event_id ||
    event.event_id.length > 128 ||
    typeof event.host_generation !== "string" ||
    !event.host_generation ||
    event.host_generation.length > 128 ||
    typeof event.hub_session_id !== "string" ||
    !event.hub_session_id ||
    event.hub_session_id.length > 128 ||
    typeof event.approval_id !== "string" ||
    !/^approval-[0-9a-f]{64}$/u.test(event.approval_id)
  )
    throw new RemoteStoreError("invalid_approval_event");
  if (event.kind === "approval_requested") {
    let approval;
    try {
      approval = parseRemotePendingApproval(event.approval);
    } catch {
      throw new RemoteStoreError("invalid_approval_event");
    }
    if (
      approval.approval_id !== event.approval_id ||
      approval.hub_session_id !== event.hub_session_id ||
      approval.host_generation !== event.host_generation
    )
      throw new RemoteStoreError("invalid_approval_event");
  } else if (event.approval !== undefined) {
    throw new RemoteStoreError("invalid_approval_event");
  }
}

export function validateRemoteQuestionEvent(
  event: Record<string, unknown>,
): void {
  if ("raw" in event) throw new RemoteStoreError("unsafe_question_event");
  if (
    typeof event.event_id !== "string" ||
    !event.event_id ||
    event.event_id.length > 128 ||
    typeof event.host_generation !== "string" ||
    !event.host_generation ||
    event.host_generation.length > 128 ||
    typeof event.hub_session_id !== "string" ||
    !event.hub_session_id ||
    event.hub_session_id.length > 128 ||
    typeof event.question_id !== "string" ||
    !/^[A-Za-z0-9._:-]{1,128}$/u.test(event.question_id)
  ) {
    throw new RemoteStoreError("invalid_question_event");
  }
  if (event.kind === "question_requested") {
    let question;
    try {
      question = parseRemotePendingQuestion(event.question);
    } catch {
      throw new RemoteStoreError("invalid_question_event");
    }
    if (
      question.question_id !== event.question_id ||
      question.hub_session_id !== event.hub_session_id ||
      question.host_generation !== event.host_generation
    ) {
      throw new RemoteStoreError("invalid_question_event");
    }
  } else if (event.question !== undefined) {
    throw new RemoteStoreError("invalid_question_event");
  }
}

export function validateRemoteMetaFrame(
  payload: Record<string, unknown>,
): void {
  if (payload.type === "plugin_status") {
    requireOnlyKeys(
      payload,
      ["type", "plugin_id", "runtime_status", "error"],
      "invalid_content_type",
    );
    if (
      !isValidAgentType(payload.plugin_id) ||
      (payload.runtime_status !== undefined &&
        ![
          "idle",
          "ready",
          "running",
          "downloading",
          "error",
          "disabled",
        ].includes(payload.runtime_status as string)) ||
      (payload.error !== undefined &&
        payload.error !== null &&
        (typeof payload.error !== "string" || payload.error.length > 4096))
    ) {
      throw new RemoteStoreError("invalid_plugin_status");
    }
    return;
  }
  if (payload.type === "host_lost") {
    requireOnlyKeys(
      payload,
      ["type", "code", "host_generation", "reason"],
      "invalid_content_type",
    );
    if (
      (payload.code !== undefined &&
        (typeof payload.code !== "string" || payload.code.length > 128)) ||
      (payload.host_generation !== undefined &&
        (typeof payload.host_generation !== "string" ||
          payload.host_generation.length > 128))
    ) {
      throw new RemoteStoreError("invalid_content_type");
    }
    return;
  }
  if (payload.type === "models") {
    requireOnlyKeys(
      payload,
      ["type", "models", "host_generation"],
      "invalid_content_type",
    );
    return;
  }
  throw new RemoteStoreError("invalid_content_type");
}

function canonicalizeJson(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map((item) =>
      item === undefined ? null : canonicalizeJson(item),
    );
  }
  if (value !== null && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const key of Object.keys(value as Record<string, unknown>).sort()) {
      const entry = (value as Record<string, unknown>)[key];
      if (entry !== undefined) {
        out[key] = canonicalizeJson(entry);
      }
    }
    return out;
  }
  return value;
}

export function commandDigest(command: RemoteCommand): string {
  return createHash("sha256")
    .update(JSON.stringify(canonicalizeJson(command)), "utf8")
    .digest("hex");
}

export function dehydrateRemoteCommandBody(rawBody: string): string {
  try {
    const parsed = JSON.parse(rawBody) as RemoteCommand;
    if (
      parsed.command.action === "send_prompt" &&
      Array.isArray(parsed.command.attachments) &&
      parsed.command.attachments.some((att) => att.data.length > 0)
    ) {
      return JSON.stringify({
        ...parsed,
        command: {
          ...parsed.command,
          attachments: parsed.command.attachments.map((att) => ({
            ...att,
            data: "",
          })),
        },
      });
    }
  } catch {
    // Keep original body if unparseable.
  }
  return rawBody;
}

export function validateRemoteEventRaw(
  event: Record<string, unknown>,
  skipRawValidation: boolean,
): void {
  if (skipRawValidation) return;
  if (event.raw === undefined) return;
  if (!event.raw || typeof event.raw !== "object" || Array.isArray(event.raw))
    throw new RemoteStoreError("unsafe_provider_event");
  const raw = event.raw as Record<string, unknown>;
  if (Object.keys(raw).some((key) => key !== "params"))
    throw new RemoteStoreError("unsafe_provider_event");
  if (
    !raw.params ||
    typeof raw.params !== "object" ||
    Array.isArray(raw.params)
  )
    throw new RemoteStoreError("unsafe_provider_event");
  const params = raw.params as Record<string, unknown>;
  const only = (keys: string[]): boolean =>
    Object.keys(params).every((key) => keys.includes(key));
  const kind = event.kind;
  if (kind === "turn/started" || kind === "turn/completed") {
    const turn = params.turn;
    if (
      !only(["turn"]) ||
      !turn ||
      typeof turn !== "object" ||
      Array.isArray(turn)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typedTurn = turn as Record<string, unknown>;
    const allowedTurnKeys =
      kind === "turn/completed" ? ["id", "status", "error"] : ["id"];
    if (
      Object.keys(typedTurn).some((key) => !allowedTurnKeys.includes(key)) ||
      typeof typedTurn.id !== "string" ||
      (typedTurn.status !== undefined &&
        typeof typedTurn.status !== "string") ||
      (typedTurn.error !== undefined &&
        (typeof typedTurn.error !== "string" || typedTurn.error.length > 4096))
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "error") {
    const error = params.error;
    if (
      !only(["error", "turnId", "willRetry"]) ||
      !error ||
      typeof error !== "object" ||
      Array.isArray(error) ||
      (params.turnId !== undefined && typeof params.turnId !== "string") ||
      (params.willRetry !== undefined && typeof params.willRetry !== "boolean")
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typedError = error as Record<string, unknown>;
    const validCodexErrorInfo = (info: unknown): boolean => {
      if (info === undefined) return true;
      if (typeof info === "string") return info.length <= 2048;
      if (typeof info !== "object" || info === null || Array.isArray(info))
        return false;
      const entries = Object.entries(info as Record<string, unknown>);
      return (
        entries.length <= 32 &&
        entries.every(
          ([k, v]) =>
            k.length <= 128 &&
            (typeof v === "string" ||
              typeof v === "number" ||
              typeof v === "boolean") &&
            String(v).length <= 1024,
        )
      );
    };
    if (
      Object.keys(typedError).some(
        (key) => !["message", "codexErrorInfo", "type"].includes(key),
      ) ||
      (typedError.message !== undefined &&
        (typeof typedError.message !== "string" ||
          typedError.message.length > 4096)) ||
      (typedError.type !== undefined && typeof typedError.type !== "string") ||
      !validCodexErrorInfo(typedError.codexErrorInfo)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "item/agentMessage/delta") {
    if (
      !only(["itemId", "delta"]) ||
      typeof params.itemId !== "string" ||
      typeof params.delta !== "string"
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "thread/tokenUsage/updated") {
    // Context capacity is two integers. Anything richer than that is provider
    // bookkeeping the console has no use for, so it is rejected rather than
    // stored.
    const usage = params.tokenUsage;
    if (
      !only(["tokenUsage"]) ||
      !usage ||
      typeof usage !== "object" ||
      Array.isArray(usage)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typed = usage as Record<string, unknown>;
    const isCount = (value: unknown) =>
      value === undefined ||
      (typeof value === "number" && Number.isSafeInteger(value) && value >= 0);
    if (
      Object.keys(typed).some((key) => !["used", "window"].includes(key)) ||
      typeof typed.used !== "number" ||
      !Number.isSafeInteger(typed.used) ||
      typed.used < 0 ||
      !isCount(typed.window)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "history/snapshot") {
    if (
      !only(["items"]) ||
      !Array.isArray(params.items) ||
      params.items.length > 2000
    )
      throw new RemoteStoreError("unsafe_provider_event");
    for (const item of params.items) {
      if (!item || typeof item !== "object" || Array.isArray(item))
        throw new RemoteStoreError("unsafe_provider_event");
      const entry = item as Record<string, unknown>;
      if (
        Object.keys(entry).some(
          (k) => !["role", "kind", "label", "status", "text"].includes(k),
        ) ||
        (entry.role !== undefined && typeof entry.role !== "string") ||
        (entry.kind !== undefined && typeof entry.kind !== "string") ||
        (entry.text !== undefined && typeof entry.text !== "string") ||
        (entry.label !== undefined && typeof entry.label !== "string") ||
        (entry.status !== undefined && typeof entry.status !== "string")
      )
        throw new RemoteStoreError("unsafe_provider_event");
    }
    return;
  }
  if (kind === "item/started") {
    // An item that announces itself before it has any output. Only the fields
    // a client renders are accepted, so a live "执行中" row is possible without
    // letting an arbitrary provider object ride along.
    const item = params.item;
    if (
      !only(["item"]) ||
      !item ||
      typeof item !== "object" ||
      Array.isArray(item)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typed = item as Record<string, unknown>;
    const itemKeys = Object.keys(typed);
    const optStr = (value: unknown, maxLen: number): boolean =>
      value === undefined ||
      (typeof value === "string" && value.length <= maxLen);
    if (
      typeof typed.id !== "string" ||
      typed.id.length === 0 ||
      typed.id.length > 256 ||
      itemKeys.some(
        (key) =>
          ![
            "id",
            "type",
            "command",
            "commandLine",
            "toolName",
            "name",
            "path",
            "status",
            "text",
          ].includes(key),
      ) ||
      !["commandExecution", "fileChange", "reasoning", "thinking"].includes(
        typed.type as string,
      ) ||
      !optStr(typed.command, 4096) ||
      !optStr(typed.commandLine, 4096) ||
      !optStr(typed.toolName, 256) ||
      !optStr(typed.name, 256) ||
      !optStr(typed.path, 1024) ||
      !optStr(typed.status, 256) ||
      !optStr(typed.text, 65536)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  if (kind === "item/completed") {
    const item = params.item;
    if (
      !only(["item"]) ||
      !item ||
      typeof item !== "object" ||
      Array.isArray(item)
    )
      throw new RemoteStoreError("unsafe_provider_event");
    const typed = item as Record<string, unknown>;
    const itemKeys = Object.keys(typed);
    if (
      typeof typed.id !== "string" ||
      (typed.type === "agentMessage" &&
        (typeof typed.text !== "string" ||
          itemKeys.some((key) => !["id", "type", "text"].includes(key)))) ||
      (typed.type === "commandExecution" &&
        (itemKeys.some(
          (key) =>
            !["id", "type", "command", "output", "status", "exitCode"].includes(
              key,
            ),
        ) ||
          (typed.command !== undefined && typeof typed.command !== "string") ||
          (typed.output !== undefined && typeof typed.output !== "string") ||
          (typed.status !== undefined && typeof typed.status !== "string") ||
          (typed.exitCode !== undefined &&
            typed.exitCode !== null &&
            (typeof typed.exitCode !== "number" ||
              !Number.isSafeInteger(typed.exitCode))))) ||
      (typed.type === "thinking" &&
        ((typed.text !== undefined && typeof typed.text !== "string") ||
          itemKeys.some((key) => !["id", "type", "text"].includes(key)))) ||
      (typed.type === "userMessage" &&
        (itemKeys.some(
          (key) => !["id", "type", "text", "content"].includes(key),
        ) ||
          (typed.text !== undefined && typeof typed.text !== "string") ||
          (typed.content !== undefined &&
            JSON.stringify(typed.content).length > 65536))) ||
      !["agentMessage", "commandExecution", "userMessage", "thinking"].includes(
        typed.type as string,
      )
    )
      throw new RemoteStoreError("unsafe_provider_event");
    return;
  }
  throw new RemoteStoreError("unsafe_provider_event");
}

export interface RemoteReceipt {
  client_cmd_id: string;
  hub_session_id: string;
  status: "accepted" | "rejected" | "failed" | "indeterminate";
  code: string | null;
  result: {
    binding?: RemoteBinding;
    turn?: { id: string };
    approval_id?: string;
    response_sent?: boolean;
  } | null;
}

export function parseRemoteReceipt(value: unknown): RemoteReceipt {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new RemoteStoreError("invalid_receipt");
  const receipt = value as Record<string, unknown>;
  if (
    Object.keys(receipt).some(
      (key) =>
        ![
          "client_cmd_id",
          "hub_session_id",
          "status",
          "code",
          "result",
        ].includes(key),
    ) ||
    typeof receipt.client_cmd_id !== "string" ||
    !receipt.client_cmd_id.trim() ||
    receipt.client_cmd_id.length > 128 ||
    typeof receipt.hub_session_id !== "string" ||
    !receipt.hub_session_id.trim() ||
    receipt.hub_session_id.length > 128 ||
    !["accepted", "rejected", "failed", "indeterminate"].includes(
      receipt.status as string,
    ) ||
    (receipt.code !== null &&
      (typeof receipt.code !== "string" || receipt.code.length > 128))
  )
    throw new RemoteStoreError("invalid_receipt");
  let result: RemoteReceipt["result"] = null;
  if (receipt.result !== null) {
    if (
      !receipt.result ||
      typeof receipt.result !== "object" ||
      Array.isArray(receipt.result)
    )
      throw new RemoteStoreError("invalid_receipt");
    const rawResult = receipt.result as Record<string, unknown>;
    if (
      Object.keys(rawResult).some(
        (key) =>
          !["binding", "turn", "approval_id", "response_sent"].includes(key),
      )
    )
      throw new RemoteStoreError("invalid_receipt");
    result = {};
    if (rawResult.binding !== undefined) {
      try {
        result.binding = parseRemoteBinding(rawResult.binding);
      } catch {
        throw new RemoteStoreError("invalid_receipt");
      }
    }
    if (rawResult.turn !== undefined) {
      const turn = rawResult.turn as Record<string, unknown>;
      if (
        !rawResult.turn ||
        typeof rawResult.turn !== "object" ||
        Array.isArray(rawResult.turn) ||
        Object.keys(turn).some((key) => key !== "id") ||
        typeof turn.id !== "string" ||
        !turn.id ||
        turn.id.length > 256
      )
        throw new RemoteStoreError("invalid_receipt");
      result.turn = { id: turn.id };
    }
    if (rawResult.approval_id !== undefined) {
      if (
        typeof rawResult.approval_id !== "string" ||
        !/^approval-[0-9a-f]{64}$/u.test(rawResult.approval_id)
      )
        throw new RemoteStoreError("invalid_receipt");
      result.approval_id = rawResult.approval_id;
    }
    if (rawResult.response_sent !== undefined) {
      if (typeof rawResult.response_sent !== "boolean")
        throw new RemoteStoreError("invalid_receipt");
      result.response_sent = rawResult.response_sent;
    }
  }
  return {
    client_cmd_id: receipt.client_cmd_id,
    hub_session_id: receipt.hub_session_id,
    status: receipt.status as RemoteReceipt["status"],
    code: receipt.code as string | null,
    result,
  };
}
