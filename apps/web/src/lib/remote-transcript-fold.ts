import type {
  ConversationItem,
  RemoteCommandRecord,
  RemoteEvent,
} from "./remote";
import {
  extractEventTurnId,
  foldEventsInto,
  mutableItem,
  type ConversationFold,
} from "./remote-transcript-events";

function stableConversationItem(
  previousItems: Map<string, ConversationItem> | undefined,
  item: ConversationItem,
): ConversationItem {
  const previous = previousItems?.get(item.id);
  if (
    previous !== undefined &&
    previous.kind === item.kind &&
    previous.text === item.text &&
    previous.title === item.title &&
    previous.command === item.command &&
    previous.output === item.output &&
    previous.description === item.description &&
    previous.status === item.status &&
    previous.turnId === item.turnId
  ) {
    return previous;
  }
  return item;
}

interface TranscriptCache {
  fold: ConversationFold;
  items: Map<string, ConversationItem>;
}

const transcriptCaches = new Map<string, TranscriptCache>();
const MAX_TRANSCRIPT_CACHES = 4;
const INCREMENTAL_EVENT_LIMIT = 512;
const TOOL_RESULT_MARKER = "\n\n〔结果〕\n";

function cacheTranscript(scope: string, cache: TranscriptCache): void {
  transcriptCaches.delete(scope);
  transcriptCaches.set(scope, cache);
  while (transcriptCaches.size > MAX_TRANSCRIPT_CACHES) {
    const oldest = transcriptCaches.keys().next().value;
    if (oldest === undefined) break;
    transcriptCaches.delete(oldest);
  }
}

export function conversationItems(
  events: RemoteEvent[],
  commands: RemoteCommandRecord[],
  scope = "",
  activeTurnId: string | null = null,
): ConversationItem[] {
  const cached = transcriptCaches.get(scope);
  const previous = cached?.fold ?? null;
  const commandsKey = commandKey(commands);
  const fold =
    resumableFold(previous, events, scope, commandsKey) &&
    previous!.activeTurnId === activeTurnId
      ? continueFold(previous!, events, commandsKey, commands)
      : buildConversationItems(
          events,
          commands,
          scope,
          commandsKey,
          activeTurnId,
        );
  const nextCache = new Map<string, ConversationItem>();
  const result = fold.items.map((item) => {
    const stable = stableConversationItem(cached?.items, item);
    fold.frozen.add(stable);
    nextCache.set(stable.id, stable);
    return stable;
  });
  cacheTranscript(scope, { fold, items: nextCache });
  return result;
}

function commandKey(commands: RemoteCommandRecord[]): string {
  let key = "";
  for (const record of commands) {
    key += `${record.command.client_cmd_id}:${record.receipt ? JSON.stringify(record.receipt) : ""}\u0000`;
  }
  return key;
}

function eventsArePrefixOf(prefix: RemoteEvent[], all: RemoteEvent[]): boolean {
  for (let index = 0; index < prefix.length; index++) {
    if (prefix[index] !== all[index]) return false;
  }
  return true;
}

function buildConversationItems(
  events: RemoteEvent[],
  commands: RemoteCommandRecord[],
  scope: string,
  commandsKey: string,
  activeTurnId: string | null,
): ConversationFold {
  const lastHistoryIndex = lastHistoryIndexIn(events);
  const fold: ConversationFold = {
    items: [],
    byId: new Map(),
    prompts: collectPrompts(commands),
    renderedPromptCmdIds: new Set(),
    promptedTurnIds: new Set(),
    currentTurnId: null,
    seenTurnIds: new Set(),
    activeTurnId,
    runningToolsByTurn: new Map(),
    runningToolsByCmd: new Map(),
    events,
    commandsKey,
    scope,
    frozen: new WeakSet(),
    trailingPromptIds: [],
    historyIndex: lastHistoryIndex,
    history:
      lastHistoryIndex >= 0
        ? (events[lastHistoryIndex]!.payload.event!.raw!.params!.items as Array<
            Record<string, unknown>
          >)
        : undefined,
  };
  if (fold.history) foldHistoryInto(fold, fold.history, scope);
  foldEventsInto(fold, events, lastHistoryIndex + 1);
  appendUnansweredPrompts(fold, commands);
  return fold;
}

function lastHistoryIndexIn(events: RemoteEvent[], from = 0): number {
  for (let index = events.length - 1; index >= from; index--) {
    const native = events[index]!.payload.event;
    if (
      native?.kind === "history/snapshot" &&
      Array.isArray(native.raw?.params?.items) &&
      (native.raw.params.items as unknown[]).length > 0
    ) {
      return index;
    }
  }
  return -1;
}

function resumableFold(
  previous: ConversationFold | null,
  events: RemoteEvent[],
  scope: string,
  commandsKey: string,
): boolean {
  return (
    previous !== null &&
    previous.scope === scope &&
    previous.commandsKey === commandsKey &&
    lastHistoryIndexIn(events, previous.historyIndex + 1) === -1 &&
    previous.history !== undefined &&
    previous.history ===
      events[previous.historyIndex]?.payload.event?.raw?.params?.items &&
    previous.events.length <= events.length &&
    events.length - previous.events.length <= INCREMENTAL_EVENT_LIMIT &&
    previous.events.length > 0 &&
    eventsArePrefixOf(previous.events, events)
  );
}

function continueFold(
  previous: ConversationFold,
  events: RemoteEvent[],
  commandsKey: string,
  commands: RemoteCommandRecord[],
): ConversationFold {
  const fold: ConversationFold = {
    items: [...previous.items],
    byId: previous.byId,
    prompts: previous.prompts,
    renderedPromptCmdIds: previous.renderedPromptCmdIds,
    promptedTurnIds: previous.promptedTurnIds,
    currentTurnId: previous.currentTurnId,
    seenTurnIds: previous.seenTurnIds,
    activeTurnId: previous.activeTurnId,
    runningToolsByTurn: previous.runningToolsByTurn,
    runningToolsByCmd: previous.runningToolsByCmd,
    events,
    commandsKey,
    scope: previous.scope,
    frozen: previous.frozen,
    trailingPromptIds: [],
    historyIndex: previous.historyIndex,
    history: previous.history,
  };
  for (const id of previous.trailingPromptIds) {
    const position = fold.items.findIndex((item) => item.id === id);
    if (position >= 0) fold.items.splice(position, 1);
    fold.byId.delete(id);
  }
  void mutableItem;
  foldEventsInto(fold, events, previous.events.length);
  appendUnansweredPrompts(fold, commands);
  return fold;
}

function collectPrompts(
  commands: RemoteCommandRecord[],
): Map<string, RemoteCommandRecord> {
  const prompts = new Map<string, RemoteCommandRecord>();
  for (const command of commands) {
    if (command.receipt?.result?.turn?.id) {
      prompts.set(command.receipt.result.turn.id, command);
    }
  }
  return prompts;
}

function looksLikeStructuredToolInput(value: string): boolean {
  const trimmed = value.trim();
  if (
    trimmed.startsWith("--- ") ||
    trimmed.startsWith("@@") ||
    trimmed.includes("\n@@ ")
  ) {
    return true;
  }
  return (
    trimmed.startsWith("{") &&
    trimmed.endsWith("}") &&
    (trimmed.includes('"old_string"') ||
      trimmed.includes('"new_string"') ||
      trimmed.includes('"questions"'))
  );
}

function splitHistoricalToolValue(value: string): {
  command: string | undefined;
  output: string | undefined;
} {
  const markerIndex = value.indexOf(TOOL_RESULT_MARKER);
  if (markerIndex >= 0) {
    const head = value.slice(0, markerIndex).trim();
    const tail = value.slice(markerIndex + TOOL_RESULT_MARKER.length);
    return {
      command: head || undefined,
      output: tail || undefined,
    };
  }
  if (looksLikeStructuredToolInput(value)) {
    return { command: value, output: undefined };
  }
  return { command: undefined, output: value || undefined };
}

function foldHistoryInto(
  fold: ConversationFold,
  history: Array<Record<string, unknown>>,
  scope: string,
): void {
  let pendingThoughts: string[] = [];
  let pendingThoughtId = "";

  const flushThoughts = () => {
    if (pendingThoughts.length === 0) return;
    fold.items.push({
      id: pendingThoughtId,
      kind: "assistant",
      text: `<thinking>${pendingThoughts.join("\n\n")}</thinking>`,
    });
    pendingThoughts = [];
    pendingThoughtId = "";
  };

  for (const [index, entry] of history.entries()) {
    const role = entry.role;
    const kind = entry.kind;
    const value =
      typeof entry.text === "string"
        ? entry.text
        : typeof entry.content === "string"
          ? entry.content
          : "";
    const entryId =
      typeof entry.id === "string" && entry.id.length > 0
        ? `${scope}${entry.id}`
        : `${scope}history-${index}`;

    if (kind === "thinking") {
      const trimmed = value.trim();
      if (trimmed) {
        if (pendingThoughts.length === 0) pendingThoughtId = entryId;
        pendingThoughts.push(trimmed);
      }
      continue;
    }

    if (kind !== "message" && kind !== "tool" && kind !== "status") continue;
    if (!value && kind !== "tool") continue;

    if (
      kind === "message" &&
      role !== "user" &&
      pendingThoughts.length > 0 &&
      !/^\s*<(?:thinking|thought)>/iu.test(value)
    ) {
      const mergedText = `<thinking>${pendingThoughts.join("\n\n")}</thinking>\n\n${value}`;
      pendingThoughts = [];
      pendingThoughtId = "";
      fold.items.push({
        id: entryId,
        kind: "assistant",
        text: mergedText,
      });
      continue;
    }

    flushThoughts();

    const item: ConversationItem = {
      id: entryId,
      kind: kind === "tool" ? "tool" : role === "user" ? "user" : "assistant",
      text: value,
    };
    if (kind === "tool") {
      item.title = typeof entry.label === "string" ? entry.label : "工具调用";
      const split = splitHistoricalToolValue(value);
      item.command = split.command;
      item.output = split.output;
      item.status =
        entry.status === "running" || entry.status === "failed"
          ? entry.status
          : "completed";
    }
    fold.items.push(item);
  }
  flushThoughts();
}

function isCommandAwaitingTurnStart(
  command: RemoteCommandRecord,
  activeTurnId: string | null,
  seenTurnIds: ReadonlySet<string>,
): boolean {
  if (command.command.command.action !== "send_prompt") return false;
  const receipt = command.receipt;
  if (receipt === null) return true;
  const acceptedTurnId =
    receipt.status === "accepted" ? receipt.result?.turn?.id : undefined;
  return Boolean(
    acceptedTurnId !== undefined &&
    activeTurnId !== null &&
    acceptedTurnId === activeTurnId &&
    !seenTurnIds.has(acceptedTurnId),
  );
}

function appendUnansweredPrompts(
  fold: ConversationFold,
  commands: RemoteCommandRecord[],
): void {
  fold.trailingPromptIds = [];
  for (const command of commands) {
    if (
      command.command.command.action === "send_prompt" &&
      isCommandAwaitingTurnStart(
        command,
        fold.activeTurnId,
        fold.seenTurnIds,
      ) &&
      !fold.renderedPromptCmdIds.has(command.command.client_cmd_id)
    ) {
      const attNames =
        command.command.command.attachments
          ?.map((a) => `[附件: ${a.name}]`)
          .join(" ") ?? "";
      const fullText = [attNames, command.command.command.text]
        .filter((part) => part && part.trim().length > 0)
        .join("\n");
      if (fullText) {
        const id = command.command.client_cmd_id;
        fold.items.push({ id, kind: "user", text: fullText });
        fold.trailingPromptIds.push(id);
      }
    }
  }
}

export function settledConversationItems(
  items: ConversationItem[],
  events: RemoteEvent[],
  commands: RemoteCommandRecord[],
  activeTurnId: string | null = null,
): ConversationItem[] {
  const seenTurnIds = new Set<string>();
  const start = lastHistoryIndexIn(events) + 1;
  for (let i = start; i < events.length; i++) {
    const p = events[i]!.payload.event?.raw?.params ?? {};
    const tid = extractEventTurnId(p);
    if (tid) seenTurnIds.add(tid);
  }
  const pendingIds = new Set<string>();
  for (const command of commands) {
    if (isCommandAwaitingTurnStart(command, activeTurnId, seenTurnIds)) {
      pendingIds.add(command.command.client_cmd_id);
    }
  }
  if (pendingIds.size === 0) return items;
  return items.filter((item) => !pendingIds.has(item.id));
}
