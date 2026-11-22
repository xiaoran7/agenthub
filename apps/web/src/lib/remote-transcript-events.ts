import type {
  ConversationItem,
  RemoteCommandRecord,
  RemoteEvent,
} from "./remote";

export interface ConversationFold {
  items: ConversationItem[];
  byId: Map<string, ConversationItem>;
  prompts: Map<string, RemoteCommandRecord>;
  renderedPromptCmdIds: Set<string>;
  promptedTurnIds: Set<string>;
  currentTurnId: string | null;
  seenTurnIds: Set<string>;
  activeTurnId: string | null;
  runningToolsByTurn: Map<string, ConversationItem>;
  runningToolsByCmd: Map<string, ConversationItem>;
  events: RemoteEvent[];
  commandsKey: string;
  scope: string;
  historyIndex: number;
  history: Array<Record<string, unknown>> | undefined;
  frozen: WeakSet<ConversationItem>;
  trailingPromptIds: string[];
}

export function mutableItem(
  fold: ConversationFold,
  target: ConversationItem,
): ConversationItem {
  if (!fold.frozen.has(target)) return target;
  const copy = { ...target };
  fold.frozen.delete(target);
  fold.byId.set(copy.id, copy);
  const position = fold.items.indexOf(target);
  if (position >= 0) fold.items[position] = copy;
  return copy;
}

export function extractEventTurnId(
  params: Record<string, unknown>,
): string | undefined {
  const turnObj = params.turn as { id?: unknown } | undefined;
  if (typeof turnObj?.id === "string" && turnObj.id.length > 0) {
    return turnObj.id;
  }
  if (typeof params.turnId === "string" && params.turnId.length > 0) {
    return params.turnId;
  }
  if (typeof params.turn_id === "string" && params.turn_id.length > 0) {
    return params.turn_id;
  }
  return undefined;
}

interface LiveEventItem {
  id?: string;
  type?: string;
  text?: string;
  command?: string;
  aggregatedOutput?: string;
  output?: string;
  name?: string;
  tool?: string;
  toolName?: string;
  server?: string;
  status?: string;
  path?: string;
  diff?: string;
  patch?: string;
  failureReason?: string;
  input?: { command?: string } | string;
  arguments?: string;
  content?: Array<{ type?: string; text?: string }>;
  summary?: Array<{ text?: string }>;
  changes?: Array<{
    path?: string;
    diff?: string;
    patch?: string;
  }>;
}

function extractReasoningText(item: LiveEventItem | undefined): string {
  if (!item) return "";
  if (typeof item.text === "string" && item.text.trim()) {
    return item.text.trim();
  }
  const parts = Array.isArray(item.summary) ? item.summary : item.content;
  if (Array.isArray(parts) && parts.length > 0) {
    return parts
      .map((part) => (typeof part?.text === "string" ? part.text.trim() : ""))
      .filter((part) => part.length > 0)
      .join("\n");
  }
  return "";
}

function basenameOfPath(rawPath: string): string {
  const clean = rawPath.replace(/[\\/]+$/u, "");
  return clean.split(/[\\/]/u).pop() || rawPath;
}

function extractFileChangeDetails(item: LiveEventItem | undefined): {
  title: string;
  diff: string | undefined;
} {
  const directPath = typeof item?.path === "string" ? item.path.trim() : "";
  const firstChangePath =
    Array.isArray(item?.changes) && typeof item.changes[0]?.path === "string"
      ? item.changes[0].path.trim()
      : "";
  const primaryPath = directPath || firstChangePath;
  const title = primaryPath
    ? `文件修改 · ${basenameOfPath(primaryPath)}`
    : "文件修改";

  if (typeof item?.diff === "string" && item.diff.trim()) {
    return { title, diff: item.diff };
  }
  if (typeof item?.patch === "string" && item.patch.trim()) {
    return { title, diff: item.patch };
  }
  if (Array.isArray(item?.changes) && item.changes.length > 0) {
    const chunks: string[] = [];
    for (const change of item.changes) {
      const rawDiff =
        typeof change?.diff === "string" && change.diff.trim()
          ? change.diff
          : typeof change?.patch === "string" && change.patch.trim()
            ? change.patch
            : "";
      if (!rawDiff) continue;
      const changePath =
        typeof change?.path === "string" ? change.path.trim() : primaryPath;
      if (rawDiff.startsWith("--- ") || !changePath) {
        chunks.push(rawDiff);
      } else if (rawDiff.startsWith("@@")) {
        chunks.push(`--- a/${changePath}\n+++ b/${changePath}\n${rawDiff}`);
      } else {
        chunks.push(rawDiff);
      }
    }
    if (chunks.length > 0) return { title, diff: chunks.join("\n") };
  }
  if (typeof item?.input === "string" && item.input.trim()) {
    return { title, diff: item.input };
  }
  return { title, diff: undefined };
}

function closeOpenThinkingAndToolsOnTurnComplete(fold: ConversationFold): void {
  for (let idx = 0; idx < fold.items.length; idx++) {
    const current = fold.items[idx]!;
    if (
      current.kind === "assistant" &&
      current.text.includes("<thinking>") &&
      !current.text.includes("</thinking>")
    ) {
      mutableItem(fold, current).text = `${current.text}</thinking>`;
    } else if (current.kind === "tool" && current.status === "running") {
      const copy = mutableItem(fold, current);
      copy.status = "completed";
      copy.text = "工具执行已完成";
    }
  }
  fold.runningToolsByTurn.clear();
  fold.runningToolsByCmd.clear();
}

function parseBareCommandJson(
  raw: string,
): { command: string; description: string | undefined } | null {
  const trimmed = raw.trim();
  if (
    !trimmed.startsWith('{"command":') ||
    !trimmed.endsWith("}") ||
    !trimmed.includes('"description":')
  ) {
    return null;
  }
  try {
    const parsed = JSON.parse(trimmed) as {
      command?: unknown;
      description?: unknown;
    };
    if (parsed && typeof parsed.command === "string") {
      return {
        command: parsed.command,
        description:
          typeof parsed.description === "string"
            ? parsed.description
            : undefined,
      };
    }
  } catch {
    // ignore malformed json fragment
  }
  return null;
}

export function foldEventsInto(
  fold: ConversationFold,
  events: RemoteEvent[],
  start: number,
): void {
  const {
    items: result,
    byId: items,
    prompts,
    renderedPromptCmdIds,
    runningToolsByTurn,
    runningToolsByCmd,
  } = fold;
  const lookup = (id: string): ConversationItem | undefined => {
    const found = items.get(id);
    return found === undefined ? undefined : mutableItem(fold, found);
  };

  for (let eventIndex = start; eventIndex < events.length; eventIndex++) {
    const native = events[eventIndex]!.payload.event;
    if (!native) continue;
    const p = native.raw?.params ?? {};
    const evtTurnId = extractEventTurnId(p);
    if (evtTurnId) fold.seenTurnIds.add(evtTurnId);

    if (native.kind === "turn/started") {
      const turn = p.turn as { id?: string } | undefined;
      const turnId = turn?.id ?? evtTurnId;
      fold.currentTurnId = turnId ?? null;
      const command = prompts.get(turnId ?? "");
      if (command?.command.command.action === "send_prompt") {
        const attNames =
          command.command.command.attachments
            ?.map((a) => `[附件: ${a.name}]`)
            .join(" ") ?? "";
        const fullText = [attNames, command.command.command.text]
          .filter((part) => part && part.trim().length > 0)
          .join("\n");
        renderedPromptCmdIds.add(command.command.client_cmd_id);
        if (turnId) fold.promptedTurnIds.add(turnId);
        result.push({
          id: command.command.client_cmd_id,
          kind: "user",
          text: fullText,
        });
      }
    } else if (native.kind === "turn/completed") {
      fold.currentTurnId = null;
      closeOpenThinkingAndToolsOnTurnComplete(fold);
    }

    const item = p.item as LiveEventItem | undefined;
    const itemId = typeof p.itemId === "string" ? p.itemId : item?.id;
    if (!itemId) continue;
    // Older Claude Runners reused these IDs for every turn. Projected deltas
    // can omit params.turnId, so use the event binding or the turn boundary.
    const turnId =
      evtTurnId ?? native.binding?.current_turn_id ?? fold.currentTurnId;
    const id =
      turnId && (itemId === "claude_msg" || itemId === "claude_thinking")
        ? `${itemId}:${turnId}`
        : itemId;

    if (
      (native.kind === "item/completed" || native.kind === "item/started") &&
      item?.type === "userMessage" &&
      !items.has(id)
    ) {
      const userText =
        item.content
          ?.filter((c) => c.type === "text" && c.text)
          .map((c) => c.text)
          .join("\n") ??
        item.text ??
        "";
      const effectiveTurnId = evtTurnId ?? fold.currentTurnId ?? undefined;
      const lastRendered = result[result.length - 1];
      const alreadyRenderedFromCommand =
        (effectiveTurnId !== undefined &&
          fold.promptedTurnIds.has(effectiveTurnId)) ||
        Boolean(
          effectiveTurnId === undefined &&
          lastRendered?.kind === "user" &&
          renderedPromptCmdIds.has(lastRendered.id) &&
          lastRendered.text.trim() === userText.trim(),
        );
      if (userText && !alreadyRenderedFromCommand) {
        const target: ConversationItem = { id, kind: "user", text: userText };
        items.set(id, target);
        result.push(target);
      }
    } else if (
      native.kind === "item/reasoning/delta" ||
      native.kind === "item/reasoning/textDelta" ||
      native.kind === "item/reasoning/summaryTextDelta"
    ) {
      const deltaText =
        typeof p.delta === "string"
          ? p.delta
          : typeof p.text === "string"
            ? p.text
            : "";
      if (!deltaText) continue;
      const target = lookup(id);
      if (!target) {
        const created: ConversationItem = {
          id,
          kind: "assistant",
          text: `<thinking>${deltaText}`,
        };
        items.set(id, created);
        result.push(created);
      } else if (target.kind === "assistant") {
        if (target.text.endsWith("</thinking>")) {
          target.text = `${target.text.slice(0, -"</thinking>".length)}${deltaText}`;
        } else if (!target.text.includes("<thinking>")) {
          target.text = `<thinking>${deltaText}</thinking>\n\n${target.text}`;
        } else {
          target.text += deltaText;
        }
      }
    } else if (
      (native.kind === "item/started" || native.kind === "item/completed") &&
      (item?.type === "reasoning" || item?.type === "thinking")
    ) {
      const completed = native.kind === "item/completed";
      const failureReason =
        typeof item?.failureReason === "string"
          ? item.failureReason.trim()
          : "";
      const extracted = failureReason || extractReasoningText(item);
      const target = lookup(id);
      if (!target) {
        if (!extracted) continue;
        const created: ConversationItem = {
          id,
          kind: "assistant",
          text: failureReason
            ? failureReason
            : completed
              ? `<thinking>${extracted}</thinking>`
              : `<thinking>${extracted}`,
        };
        items.set(id, created);
        result.push(created);
      } else if (target.kind === "assistant") {
        const prevInner = target.text
          .replace(/^<thinking>/iu, "")
          .replace(/<\/thinking>$/iu, "");
        const body = extracted || prevInner;
        if (body) {
          target.text = failureReason
            ? failureReason
            : completed
              ? `<thinking>${body}</thinking>`
              : `<thinking>${body}`;
        }
      }
    } else if (native.kind === "item/agentMessage/delta") {
      let target = lookup(id);
      const deltaText = typeof p.delta === "string" ? p.delta : "";
      const bare = !target ? parseBareCommandJson(deltaText) : null;
      if (bare) {
        target = {
          id,
          kind: "tool",
          title: "命令执行",
          text: "工具执行中…",
          command: bare.command,
          description: bare.description,
          status: "running",
          turnId: typeof p.turnId === "string" ? p.turnId : undefined,
        };
        items.set(id, target);
        if (target.turnId) runningToolsByTurn.set(target.turnId, target);
        if (bare.command) runningToolsByCmd.set(bare.command, target);
        result.push(target);
        continue;
      }
      if (!target) {
        target = { id, kind: "assistant", text: "" };
        items.set(id, target);
        result.push(target);
      }
      target.text += deltaText;
    } else if (
      native.kind === "item/completed" &&
      item?.type === "agentMessage"
    ) {
      let target = lookup(id);
      const bare = parseBareCommandJson(item.text ?? "");
      if (bare) {
        if (!target) {
          target = {
            id,
            kind: "tool",
            title: "命令执行",
            text: "工具执行已完成",
            command: bare.command,
            description: bare.description,
            status: "completed",
            turnId: typeof p.turnId === "string" ? p.turnId : undefined,
          };
          items.set(id, target);
          result.push(target);
        } else {
          target.kind = "tool";
          target.title = "命令执行";
          target.command = bare.command;
          target.description = bare.description;
          target.status = "completed";
          if (target.turnId) runningToolsByTurn.delete(target.turnId);
          if (target.command) runningToolsByCmd.delete(target.command);
        }
        continue;
      }
      if (!target) {
        target = { id, kind: "assistant", text: "" };
        items.set(id, target);
        result.push(target);
      }
      target.text = item.text ?? target.text;
    } else if (
      native.kind === "item/commandExecution/outputDelta" ||
      native.kind === "item/fileChange/outputDelta"
    ) {
      const deltaOut = typeof p.delta === "string" ? p.delta : "";
      if (!deltaOut) continue;
      const isFile = native.kind === "item/fileChange/outputDelta";
      const target = lookup(id);
      if (!target) {
        const created: ConversationItem = {
          id,
          kind: "tool",
          title: isFile ? "文件修改" : "命令执行",
          text: "工具执行中…",
          output: deltaOut,
          status: "running",
          turnId: evtTurnId,
        };
        items.set(id, created);
        if (!isFile && evtTurnId) runningToolsByTurn.set(evtTurnId, created);
        result.push(created);
      } else if (target.kind === "tool") {
        target.output = (target.output ?? "") + deltaOut;
      }
    } else if (
      (native.kind === "item/completed" || native.kind === "item/started") &&
      item?.type === "commandExecution"
    ) {
      let target = lookup(id);
      const cmd =
        item.command ||
        (typeof item.input === "object" &&
        typeof item.input?.command === "string"
          ? item.input.command
          : "");
      const out = item.aggregatedOutput || item.output || "";
      const label =
        !cmd && typeof item.toolName === "string" && item.toolName.trim()
          ? item.toolName.trim()
          : "命令执行";

      if (!target) {
        const turnId = typeof p.turnId === "string" ? p.turnId : undefined;
        if (turnId && runningToolsByTurn.has(turnId)) {
          const candidate = runningToolsByTurn.get(turnId)!;
          if (!candidate.command || !cmd || candidate.command === cmd) {
            target = candidate;
            items.set(id, target);
          }
        } else if (cmd && runningToolsByCmd.has(cmd)) {
          const candidate = runningToolsByCmd.get(cmd)!;
          if (!turnId || !candidate.turnId || candidate.turnId === turnId) {
            target = candidate;
            items.set(id, target);
          }
        }
      }

      if (!target) {
        target = {
          id,
          kind: "tool",
          title: label,
          text:
            native.kind === "item/completed" ? "工具执行已完成" : "工具执行中…",
          command: cmd || undefined,
          output: out || undefined,
          status:
            item.status === "failed"
              ? "failed"
              : native.kind === "item/completed"
                ? "completed"
                : "running",
          turnId: typeof p.turnId === "string" ? p.turnId : undefined,
        };
        items.set(id, target);
        if (target.status === "running") {
          if (target.turnId) runningToolsByTurn.set(target.turnId, target);
          if (cmd) runningToolsByCmd.set(cmd, target);
        }
        result.push(target);
      } else {
        if (cmd) {
          target.command = cmd;
          target.title = "命令执行";
        }
        if (out) target.output = out;
        if (native.kind === "item/completed" || item.status === "failed") {
          target.status = item.status === "failed" ? "failed" : "completed";
          target.text = "工具执行已完成";
          if (target.turnId) runningToolsByTurn.delete(target.turnId);
          if (target.command) runningToolsByCmd.delete(target.command);
          if (cmd) runningToolsByCmd.delete(cmd);
        }
      }
    } else if (
      (native.kind === "item/completed" || native.kind === "item/started") &&
      (item?.type === "fileChange" || item?.type === "fileChangeStart")
    ) {
      const details = extractFileChangeDetails(item);
      let target = lookup(id);
      if (!target) {
        target = {
          id,
          kind: "tool",
          text:
            native.kind === "item/completed" ? "工具执行已完成" : "工具执行中…",
          title: details.title,
        };
        items.set(id, target);
        result.push(target);
      } else {
        target.title = details.title;
        if (native.kind === "item/completed") target.text = "工具执行已完成";
      }
      target.command = details.diff ?? target.command;
      target.output = item?.output ?? target.output;
      target.status =
        item?.status === "failed"
          ? "failed"
          : native.kind === "item/completed"
            ? "completed"
            : "running";
    } else if (
      (native.kind === "item/completed" || native.kind === "item/started") &&
      ["toolCall", "functionCall", "mcpToolCall", "customToolCall"].includes(
        item?.type ?? "",
      )
    ) {
      const label =
        [item?.server, item?.tool ?? item?.name]
          .filter((part) => typeof part === "string" && part.length > 0)
          .join(" · ") || "工具调用";
      let target = lookup(id);
      if (!target) {
        target = { id, kind: "tool", text: "", title: label };
        items.set(id, target);
        result.push(target);
      }
      target.title = label;
      const rawInput =
        typeof item?.input === "string" && item.input
          ? item.input
          : typeof item?.arguments === "string" && item.arguments
            ? item.arguments
            : undefined;
      target.command = rawInput ?? target.command;
      target.output = item?.output ?? target.output;
      target.status =
        item?.status === "failed"
          ? "failed"
          : native.kind === "item/completed"
            ? "completed"
            : "running";
    }
  }
}
