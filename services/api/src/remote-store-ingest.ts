import type { DatabaseSync, StatementSync } from "node:sqlite";
import { isDeepStrictEqual } from "node:util";
import {
  parseRemoteBinding,
  parseRemoteReady,
  type AgentPlugin,
  type RemoteBinding,
  type RemoteCommand,
  type RemoteReady,
} from "@agent-pulse/contracts";
import {
  normalizePluginList,
  updateSingleRunnerPluginStatus,
  upsertRunnerPluginsRow,
} from "./remote-plugins.js";
import {
  RemoteStoreError,
  dehydrateRemoteCommandBody,
  parseRemoteReceipt,
  projectRunnerHello,
  requireOnlyKeys,
  storableFrame,
  validateRemoteApprovalEvent,
  validateRemoteEventFields,
  validateRemoteEventRaw,
  validateRemoteMetaFrame,
  validateRemoteQuestionEvent,
} from "./remote-store-validation.js";

export type CachedRunnerHello = {
  owner: string;
  full: Record<string, unknown> | null;
  thin: Record<string, unknown> | null;
};

export interface RemoteIngestContext {
  readonly retention: number;
  readonly runnerEventCounts: Map<string, number>;
  readonly runnerHelloCache: Map<string, CachedRunnerHello>;
  stmt(sql: string): StatementSync;
  register(owner: string, runner: string): void;
  watermarks(runner: string): {
    received: number;
    command_seq: number;
    command_ack: number;
  };
  transaction<T>(work: () => T): T;
  runnerBindings(owner: string, runner: string): Map<string, RemoteBinding>;
  assertNativeIdentity(
    existing: ReadonlyMap<string, RemoteBinding>,
    incoming: RemoteBinding,
  ): void;
  writeBindings(
    owner: string,
    runner: string,
    bindings: readonly RemoteBinding[],
  ): void;
  saveBinding(owner: string, runner: string, binding: RemoteBinding): void;
  cacheRunnerHello(runner: string, entry: CachedRunnerHello): void;
  notify(owner: string): void;
  notifyPlugins(owner: string, runner: string): void;
}

export function receiveRemoteFrame(
  ctx: RemoteIngestContext,
  owner: string,
  runner: string,
  seq: number,
  payload: Record<string, unknown>,
  activeGeneration?: string,
): number {
  ctx.register(owner, runner);
  let committedCount: number | undefined;
  let helloMutated = false;
  let pluginsMutated = false;
  try {
    const receivedWatermark = ctx.transaction(() => {
      const watermarks = ctx.watermarks(runner);
      if (!Number.isSafeInteger(seq) || seq < 1)
        throw new RemoteStoreError("invalid_sequence");
      if (seq <= watermarks.received) {
        const old = ctx
          .stmt(
            "SELECT payload FROM remote_events WHERE runner=? AND runner_seq=?",
          )
          .get(runner, seq);
        if (!old) {
          throw new RemoteStoreError("sequence_pruned");
        }
        if (
          !isDeepStrictEqual(
            JSON.parse(String(old.payload)),
            storableFrame(payload),
          )
        ) {
          throw new RemoteStoreError("sequence_conflict");
        }
        return watermarks.received;
      }
      if (seq !== watermarks.received + 1)
        throw new RemoteStoreError("sequence_gap");
      let binding: RemoteBinding | undefined;
      let hub: string | null = null;
      if (payload.type === "event") {
        requireOnlyKeys(payload, ["type", "event"], "invalid_event");
        const event = payload.event as Record<string, unknown> | undefined;
        if (!event || typeof event !== "object" || Array.isArray(event))
          throw new RemoteStoreError("invalid_event");
        requireOnlyKeys(
          event,
          [
            "event_id",
            "host_generation",
            "binding",
            "hub_session_id",
            "provider_session_id",
            "kind",
            "raw",
            "approval_id",
            "approval",
            "question_id",
            "question",
          ],
          "invalid_event",
        );
        validateRemoteEventFields(event);
        const isApprovalEvent =
          event.kind === "approval_requested" ||
          event.kind === "approval_resolved" ||
          event.kind === "unsupported_provider_request";
        const isQuestionEvent =
          event.kind === "question_requested" ||
          event.kind === "question_resolved";
        if (isApprovalEvent) validateRemoteApprovalEvent(event);
        if (isQuestionEvent) validateRemoteQuestionEvent(event);
        validateRemoteEventRaw(event, isApprovalEvent || isQuestionEvent);
        if (event.binding) binding = parseRemoteBinding(event.binding);
        hub =
          typeof event.hub_session_id === "string"
            ? event.hub_session_id
            : null;
        if (binding && hub !== binding.hub_session_id)
          throw new RemoteStoreError("binding_conflict");
        if (
          binding &&
          activeGeneration !== undefined &&
          binding.host_generation !== activeGeneration
        )
          binding = undefined;
      } else if (payload.type === "receipt") {
        requireOnlyKeys(payload, ["type", "receipt"], "invalid_receipt");
        const receipt = parseRemoteReceipt(payload.receipt);
        const queued = ctx
          .stmt("SELECT body FROM remote_commands WHERE runner=? AND cmd_id=?")
          .get(runner, receipt.client_cmd_id);
        if (!queued) throw new RemoteStoreError("unknown_command_receipt");
        const rawBody = String(queued.body);
        const command = JSON.parse(rawBody) as RemoteCommand;
        if (receipt.hub_session_id !== command.hub_session_id)
          throw new RemoteStoreError("receipt_session_conflict");
        hub = command.hub_session_id;
        if (receipt.result?.binding) binding = receipt.result.binding;
        if (binding && binding.hub_session_id !== hub)
          throw new RemoteStoreError("binding_conflict");
        if (
          binding &&
          activeGeneration !== undefined &&
          binding.host_generation !== activeGeneration
        )
          binding = undefined;
        const dehydrated = dehydrateRemoteCommandBody(rawBody);
        ctx
          .stmt(
            "UPDATE remote_commands SET receipt=?, body=? WHERE runner=? AND cmd_id=? AND receipt IS NULL",
          )
          .run(
            JSON.stringify(receipt),
            dehydrated,
            runner,
            receipt.client_cmd_id,
          );
      } else if (payload.type === "ready") {
        let ready: RemoteReady;
        try {
          ready = parseRemoteReady(payload);
        } catch {
          throw new RemoteStoreError("invalid_ready");
        }
        const sessions = ready.sessions;
        if (
          activeGeneration === undefined ||
          ready.host_generation === activeGeneration
        ) {
          const existing = ctx.runnerBindings(owner, runner);
          const currentHubs = new Set(
            sessions.map((session) => session.hub_session_id),
          );
          const updates: RemoteBinding[] = [];
          for (const binding of existing.values()) {
            if (!currentHubs.has(binding.hub_session_id))
              updates.push({
                ...binding,
                control_connected: false,
                execution_state: "unknown",
                current_turn_id: null,
              });
          }
          for (const session of sessions) {
            ctx.assertNativeIdentity(existing, session);
            updates.push(session);
          }
          ctx.writeBindings(owner, runner, updates);
          ctx
            .stmt(
              "UPDATE remote_runners SET hello=?, device_name=COALESCE(?, device_name) WHERE runner=?",
            )
            .run(JSON.stringify(payload), ready.device_name ?? null, runner);
          if ("plugins" in payload) {
            upsertRunnerPluginsRow(
              (sql) => ctx.stmt(sql),
              owner,
              runner,
              undefined,
              normalizePluginList(payload.plugins, runner),
            );
            pluginsMutated = true;
          }
          helloMutated = true;
        }
      } else if (payload.type === "plugin_status") {
        validateRemoteMetaFrame(payload);
        const pluginId = String(payload.plugin_id);
        const runtimeStatus = String(
          payload.runtime_status ?? "idle",
        ) as AgentPlugin["runtimeStatus"];
        const err =
          typeof payload.error === "string" ? payload.error : undefined;
        if (
          updateSingleRunnerPluginStatus(
            (sql) => ctx.stmt(sql),
            owner,
            runner,
            pluginId,
            runtimeStatus,
            err,
          )
        ) {
          pluginsMutated = true;
        }
      } else {
        validateRemoteMetaFrame(payload);
      }
      if (binding) ctx.saveBinding(owner, runner, binding);
      if (
        payload.type === "host_lost" &&
        (activeGeneration === undefined ||
          payload.host_generation === activeGeneration)
      ) {
        const existing = ctx.runnerBindings(owner, runner);
        ctx.writeBindings(
          owner,
          runner,
          [...existing.values()].map((binding) => ({
            ...binding,
            control_connected: false,
            execution_state: "unknown",
          })),
        );
      }
      ctx
        .stmt(
          "INSERT INTO remote_events(owner,runner,runner_seq,hub,payload) VALUES (?,?,?,?,?)",
        )
        .run(owner, runner, seq, hub, JSON.stringify(storableFrame(payload)));
      let count = ctx.runnerEventCounts.get(runner);
      if (count === undefined) {
        const row = ctx
          .stmt("SELECT COUNT(*) AS cnt FROM remote_events WHERE runner=?")
          .get(runner) as { cnt: number } | undefined;
        count = Number(row?.cnt ?? 0);
      } else {
        count += 1;
      }
      if (count > ctx.retention) {
        const excess = count - ctx.retention;
        const deleted = ctx
          .stmt(
            "DELETE FROM remote_events WHERE id IN (SELECT id FROM remote_events WHERE runner=? ORDER BY id ASC LIMIT ?)",
          )
          .run(runner, excess);
        count -= Number(deleted.changes);
      }
      committedCount = count;
      ctx
        .stmt("UPDATE remote_runners SET received=? WHERE runner=?")
        .run(seq, runner);
      return seq;
    });
    if (committedCount !== undefined) {
      ctx.runnerEventCounts.set(runner, committedCount);
    }
    if (helloMutated) {
      ctx.cacheRunnerHello(runner, {
        owner,
        full: payload,
        thin: projectRunnerHello(payload),
      });
    }
    if (pluginsMutated) {
      ctx.notifyPlugins(owner, runner);
    }
    ctx.notify(owner);
    return receivedWatermark;
  } catch (error) {
    ctx.runnerEventCounts.delete(runner);
    if (helloMutated) ctx.runnerHelloCache.delete(runner);
    throw error;
  }
}

export function migrateRemoteStoreSchema(db: DatabaseSync): void {
  const columns = db
    .prepare("PRAGMA table_info(remote_commands)")
    .all() as Array<{ name: string }>;
  if (!columns.some((column) => column.name === "hub")) {
    db.exec("ALTER TABLE remote_commands ADD COLUMN hub TEXT");
    const rows = db.prepare("SELECT body FROM remote_commands").all() as Array<{
      body: string;
    }>;
    const backfill = db.prepare(
      "UPDATE remote_commands SET hub=? WHERE body=?",
    );
    for (const row of rows) {
      let hub: string | null = null;
      try {
        hub = (JSON.parse(row.body) as RemoteCommand).hub_session_id ?? null;
      } catch {
        // leave null on unparseable rows
      }
      backfill.run(hub, row.body);
    }
  }
  if (!columns.some((column) => column.name === "cmd_digest")) {
    db.exec("ALTER TABLE remote_commands ADD COLUMN cmd_digest TEXT");
  }
  const runnerColumns = db
    .prepare("PRAGMA table_info(remote_runners)")
    .all() as Array<{ name: string }>;
  if (!runnerColumns.some((column) => column.name === "device_name")) {
    db.exec("ALTER TABLE remote_runners ADD COLUMN device_name TEXT");
  }
  db.exec(
    "CREATE INDEX IF NOT EXISTS remote_command_session ON remote_commands(runner,hub,seq)",
  );
}

export function findRemoteResumePoint(
  stmt: (sql: string) => StatementSync,
  owner: string,
  runner: string,
  hub: string,
  lookback: number,
): { transcript: number | null; approvals: number | null } {
  let transcript: number | null = null;
  const hubIter = stmt(
    `SELECT id,payload FROM remote_events
       WHERE owner=? AND runner=? AND hub=?
       ORDER BY id DESC LIMIT ${lookback}`,
  ).iterate(owner, runner, hub) as Iterable<{ id: number; payload: string }>;
  for (const row of hubIter) {
    if (!row.payload.includes("history/snapshot")) continue;
    try {
      const parsed = JSON.parse(row.payload) as {
        type?: unknown;
        event?: { kind?: unknown };
      };
      if (
        parsed?.type === "event" &&
        parsed.event?.kind === "history/snapshot"
      ) {
        transcript = Number(row.id);
        break;
      }
    } catch {
      // ignore unparseable row
    }
  }
  let approvals: number | null = null;
  const runnerIter = stmt(
    `SELECT id,payload FROM remote_events
       WHERE owner=? AND runner=? AND hub IS NULL
       ORDER BY id DESC LIMIT ${lookback}`,
  ).iterate(owner, runner) as Iterable<{ id: number; payload: string }>;
  for (const row of runnerIter) {
    if (!row.payload.includes("pending_approvals")) continue;
    try {
      const parsed = JSON.parse(row.payload) as {
        type?: unknown;
        pending_approvals?: unknown;
      };
      if (parsed?.type === "ready" && Array.isArray(parsed.pending_approvals)) {
        approvals = Number(row.id);
        break;
      }
    } catch {
      // ignore unparseable row
    }
  }
  return { transcript, approvals };
}
