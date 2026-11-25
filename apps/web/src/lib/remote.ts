import type {
  RemotePendingApproval,
  RemotePendingQuestion,
} from "@agent-pulse/contracts/remote";
import { isRedactedPathOnly } from "./remote-client";
import type {
  ConversationGroup,
  ConversationItem,
  RemoteEvent,
  TurnFailure,
} from "./remote-types";

export * from "./remote-types";
export * from "./remote-client";
export {
  conversationItems,
  settledConversationItems,
} from "./remote-transcript-fold";

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
    const acceptedResponseReceipt =
      payload.type === "receipt" &&
      payload.receipt?.status === "accepted" &&
      payload.receipt?.result?.response_sent === true;
    if (
      REPLACEABLE_PAYLOAD_TYPES.has(String(payload.type)) ||
      kind === "approval_requested" ||
      kind === "approval_resolved" ||
      kind === "question_requested" ||
      kind === "question_resolved" ||
      kind === "turn/completed" ||
      acceptedResponseReceipt
    ) {
      keep.add(index);
    }
  }
  const kept: RemoteEvent[] = [];
  for (const index of keep) kept.push(events[index]!);
  kept.push(...events.slice(lastSnapshot));
  return kept;
}
