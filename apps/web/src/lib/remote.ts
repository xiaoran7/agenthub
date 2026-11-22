import type {
  RemoteAttachment,
  RemoteBinding,
  RemoteCommand,
  RemotePendingApproval,
  RemotePendingQuestion,
  RemotePermissionMode,
} from "@agent-pulse/contracts/remote";
import { isRedactedPathOnly } from "./remote-client";

export type { RemoteAttachment, RemotePermissionMode };
export * from "./remote-client";
export {
  conversationItems,
  settledConversationItems,
} from "./remote-transcript-fold";

export type RemoteRequest = <T>(
  path: string,
  body?: unknown,
  timeoutMs?: number,
  userId?: string,
  signal?: AbortSignal,
) => Promise<T>;

export interface DiscoverySubagent {
  id: string;
  agent_type?: string | null | undefined;
  description?: string | null | undefined;
  model?: string | null | undefined;
  total_tokens?: number | null | undefined;
  last_activity_at_ms?: number | null | undefined;
}

export interface DiscoveryCandidate {
  provider_session_id: string;
  workspace_path: string;
  title: string | null;
  provider?: string;
  resumable?: boolean;
  model?: string | null;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  total_tokens?: number | null;
  cache_read_tokens?: number | null;
  context_used_tokens?: number | null;
  context_window_tokens?: number | null;
  calls?: number | null;
  request_count?: number | null;
  last_activity_at_ms?: number | null;
  execution_state?: string | null;
  subagents?: DiscoverySubagent[] | undefined;
}

export interface RemoteRunnerModels {
  data: Array<{
    id: string;
    model?: string;
    displayName?: string;
    display_name?: string;
    name?: string;
  }>;
  agents?: Record<string, Array<{ id: string; display_name?: string }>>;
}

export type RemoteRunnerHello = null | {
  host_generation: string;
  device_name?: string;
  pending_approvals?: RemotePendingApproval[];
  workspaces: string[];
  models?: RemoteRunnerModels;
  discovery?: {
    candidates: DiscoveryCandidate[];
  };
};

export interface RemoteRunnerHelloResponse {
  runner_id: string;
  online: boolean;
  revoked: boolean;
  hello: RemoteRunnerHello;
  codex_session_control?: boolean;
}

export interface RemoteRunner {
  runner_id: string;
  online: boolean;
  revoked: boolean;
  hello: RemoteRunnerHello;
  codex_session_control?: boolean;
}

export interface RemoteSession {
  runner_id: string;
  binding: RemoteBinding;
  online: boolean;
  provider?: string;
}

export interface RemoteSnapshot {
  cursor: number;
  runners: RemoteRunner[];
  sessions: RemoteSession[];
}

export interface RemoteEvent {
  cursor: number;
  runner_id: string;
  payload: {
    type: string;
    pending_approvals?: RemotePendingApproval[];
    pending_questions?: RemotePendingQuestion[];
    event?: {
      kind: string;
      raw?: { params?: Record<string, unknown> };
      approval_id?: string;
      approval?: RemotePendingApproval;
      question_id?: string;
      question?: RemotePendingQuestion;
      binding?: RemoteBinding;
    };
    receipt?: {
      client_cmd_id: string;
      status: string;
      code?: string;
      result?: {
        turn?: { id: string };
        approval_id?: string;
        question_id?: string;
        response_sent?: boolean;
      };
    };
  };
}

export interface RemoteCommandRecord {
  command: RemoteCommand;
  receipt: {
    status: string;
    code?: string | null;
    result?: { turn?: { id: string } };
  } | null;
}

export interface RemoteSessionDetail {
  binding: RemoteBinding | null;
  cursor: number;
  resume?: { transcript: number | null; approvals: number | null } | null;
  commands: RemoteCommandRecord[];
}

export interface ConversationItem {
  id: string;
  kind: "user" | "assistant" | "tool";
  text: string;
  title?: string | undefined;
  command?: string | undefined;
  output?: string | undefined;
  description?: string | undefined;
  status?: "running" | "completed" | "failed" | undefined;
  turnId?: string | undefined;
}

export interface TurnFailure {
  turnId: string | null;
  message: string;
  retrying: boolean;
}

export function latestTurnFailure(
  events: RemoteEvent[],
  hubSessionId: string,
): TurnFailure | null {
  const state = foldWithCache(
    turnFailureFolds,
    `turn-failure\u0000${hubSessionId}`,
    events,
    () => ({ failure: null as TurnFailure | null }),
    reduceTurnFailure(hubSessionId),
  );
  return state.failure;
}

interface TurnFailureState {
  failure: TurnFailure | null;
}

function reduceTurnFailure(
  hubSessionId: string,
): (state: TurnFailureState, envelope: RemoteEvent) => void {
  return (state, envelope) => {
    const native = envelope.payload.event;
    if (!native) return;
    const belongsToSession =
      native.binding?.hub_session_id === undefined ||
      native.binding.hub_session_id === hubSessionId;
    if (!belongsToSession) return;
    if (native.kind === "turn/started") {
      state.failure = null;
      return;
    }
    if (native.kind === "turn/completed") {
      const turn = native.raw?.params?.turn as
        Record<string, unknown> | undefined;
      const status = typeof turn?.status === "string" ? turn.status : undefined;
      const turnError =
        typeof turn?.error === "string" ? turn.error.trim() : "";
      const isFailedCompletion =
        status === "failed" ||
        status === "error" ||
        Boolean(turnError) ||
        native.binding?.execution_state === "error";
      if (isFailedCompletion) {
        const prevFailure = state.failure;
        const turnId: string | null =
          typeof turn?.id === "string"
            ? turn.id
            : (prevFailure?.turnId ?? null);
        if (turnError) {
          state.failure = { turnId, message: turnError, retrying: false };
        } else if (prevFailure) {
          state.failure = {
            turnId,
            message: prevFailure.message,
            retrying: false,
          };
        }
      } else {
        state.failure = null;
      }
      return;
    }
    if (native.kind !== "error") return;
    const params = native.raw?.params;
    const error = params?.error as Record<string, unknown> | undefined;
    const message = error?.message;
    if (typeof message !== "string" || message.trim() === "") return;
    state.failure = {
      turnId: typeof params?.turnId === "string" ? params.turnId : null,
      message: message.trim(),
      retrying: params?.willRetry === true,
    };
  };
}

const INCREMENTAL_EVENT_LIMIT = 512;
const turnFailureFolds = new Map<string, ResumableFold<TurnFailureState>>();

interface ResumableFold<T> {
  state: T;
  events: RemoteEvent[];
  key: string;
}

function eventsArePrefixOf(prefix: RemoteEvent[], all: RemoteEvent[]): boolean {
  for (let index = 0; index < prefix.length; index++) {
    if (prefix[index] !== all[index]) return false;
  }
  return true;
}

function foldWithCache<T>(
  cache: Map<string, ResumableFold<T>>,
  key: string,
  events: RemoteEvent[],
  initial: () => T,
  step: (state: T, envelope: RemoteEvent) => void,
): T {
  const cached = cache.get(key);
  const reusable =
    cached !== undefined &&
    cached.events.length <= events.length &&
    events.length - cached.events.length <= INCREMENTAL_EVENT_LIMIT &&
    eventsArePrefixOf(cached.events, events);
  const state = reusable ? cached.state : initial();
  const start = reusable ? cached.events.length : 0;
  cache.clear();
  cache.set(key, { state, events, key });
  for (let index = start; index < events.length; index++) {
    step(state, events[index]!);
  }
  return state;
}

export type ConversationGroup =
  | { type: "single"; item: ConversationItem }
  | { type: "tool_group"; items: ConversationItem[] };

export function groupConversationItems(
  items: ConversationItem[],
): ConversationGroup[] {
  const groups: ConversationGroup[] = [];
  let tools: ConversationItem[] = [];
  const flush = () => {
    if (tools.length === 1) groups.push({ type: "single", item: tools[0]! });
    else if (tools.length > 1) {
      groups.push({ type: "tool_group", items: tools });
    }
    tools = [];
  };
  for (const item of items) {
    if (item.kind !== "tool" && isRedactedPathOnly(item.text)) {
      continue;
    }
    if (item.kind === "tool") tools.push(item);
    else {
      flush();
      groups.push({ type: "single", item });
    }
  }
  flush();
  return groups;
}

export function pendingApprovals(
  events: RemoteEvent[],
  hubSessionId: string,
): RemotePendingApproval[] {
  const pending = foldWithCache(
    pendingApprovalFolds,
    `approvals\u0000${hubSessionId}`,
    events,
    () => new Map<string, RemotePendingApproval>(),
    reducePendingApprovals(hubSessionId),
  );
  return [...pending.values()];
}

const pendingApprovalFolds = new Map<
  string,
  ResumableFold<Map<string, RemotePendingApproval>>
>();

function reducePendingApprovals(
  hubSessionId: string,
): (state: Map<string, RemotePendingApproval>, envelope: RemoteEvent) => void {
  return (pending, envelope) => {
    if (
      envelope.payload.type === "ready" &&
      Array.isArray(envelope.payload.pending_approvals)
    ) {
      pending.clear();
      for (const approval of envelope.payload.pending_approvals) {
        if (approval.hub_session_id === hubSessionId) {
          pending.set(approval.approval_id, approval);
        }
      }
      return;
    }
    if (envelope.payload.type === "host_lost") {
      pending.clear();
      return;
    }
    const native = envelope.payload.event;
    const receipt = envelope.payload.receipt;
    if (
      native?.kind === "approval_requested" &&
      native.approval &&
      native.approval.hub_session_id === hubSessionId
    ) {
      pending.set(native.approval.approval_id, native.approval);
    }
    if (
      native?.kind === "approval_resolved" &&
      typeof native.approval_id === "string"
    ) {
      pending.delete(native.approval_id);
    }
    if (
      native?.kind === "turn/completed" &&
      native.binding?.hub_session_id === hubSessionId
    ) {
      pending.clear();
    }
    if (
      receipt?.status === "accepted" &&
      receipt.result?.response_sent === true &&
      typeof receipt.result.approval_id === "string"
    ) {
      pending.delete(receipt.result.approval_id);
    }
  };
}

export function pendingQuestions(
  events: RemoteEvent[],
  hubSessionId: string,
): RemotePendingQuestion[] {
  const pending = foldWithCache(
    pendingQuestionFolds,
    `questions\u0000${hubSessionId}`,
    events,
    () => new Map<string, RemotePendingQuestion>(),
    reducePendingQuestions(hubSessionId),
  );
  return [...pending.values()];
}

const pendingQuestionFolds = new Map<
  string,
  ResumableFold<Map<string, RemotePendingQuestion>>
>();

function reducePendingQuestions(
  hubSessionId: string,
): (state: Map<string, RemotePendingQuestion>, envelope: RemoteEvent) => void {
  return (pending, envelope) => {
    if (
      envelope.payload.type === "ready" &&
      Array.isArray(envelope.payload.pending_questions)
    ) {
      pending.clear();
      for (const question of envelope.payload.pending_questions) {
        if (question.hub_session_id === hubSessionId) {
          pending.set(question.question_id, question);
        }
      }
      return;
    }
    if (envelope.payload.type === "host_lost") {
      pending.clear();
      return;
    }
    const native = envelope.payload.event;
    const receipt = envelope.payload.receipt;
    if (
      native?.kind === "question_requested" &&
      native.question &&
      native.question.hub_session_id === hubSessionId
    ) {
      pending.set(native.question.question_id, native.question);
    }
    if (
      native?.kind === "question_resolved" &&
      typeof native.question_id === "string"
    ) {
      pending.delete(native.question_id);
    }
    if (
      native?.kind === "turn/completed" &&
      native.binding?.hub_session_id === hubSessionId
    ) {
      pending.clear();
    }
    if (
      receipt?.status === "accepted" &&
      receipt.result?.response_sent === true &&
      typeof receipt.result.question_id === "string"
    ) {
      pending.delete(receipt.result.question_id);
    }
  };
}

const REPLACEABLE_PAYLOAD_TYPES = new Set(["ready", "host_lost", "models"]);

export function trimSupersededEvents(events: RemoteEvent[]): RemoteEvent[] {
  let lastSnapshot = -1;
  for (let index = events.length - 1; index >= 0; index--) {
    const native = events[index]!.payload.event;
    if (
      native?.kind === "history/snapshot" &&
      Array.isArray(native.raw?.params?.items) &&
      (native.raw.params.items as unknown[]).length > 0
    ) {
      lastSnapshot = index;
      break;
    }
  }
  if (lastSnapshot <= 64) return events;
  const keep = new Set<number>();
  for (let index = 0; index < lastSnapshot; index++) {
    const payload = events[index]!.payload;
    const kind = payload.event?.kind;
    if (
      REPLACEABLE_PAYLOAD_TYPES.has(String(payload.type)) ||
      kind === "approval_requested" ||
      kind === "approval_resolved" ||
      kind === "question_requested" ||
      kind === "question_resolved"
    ) {
      keep.add(index);
    }
  }
  const kept: RemoteEvent[] = [];
  for (const index of keep) kept.push(events[index]!);
  kept.push(...events.slice(lastSnapshot));
  return kept;
}
