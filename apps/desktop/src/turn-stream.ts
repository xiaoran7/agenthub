export interface ConversationMessage {
  role: "user" | "assistant" | "system";
  kind?: "message" | "tool" | "command" | "status" | "thinking";
  label?: string;
  status?: "running" | "completed" | "failed" | "unknown";
  content: string;
}

export function appendAssistantDelta(
  current: ConversationMessage[],
  delta: string,
): ConversationMessage[] {
  if (delta === "") return current;
  const next = [...current];
  for (let index = next.length - 1; index >= 0; index -= 1) {
    if (next[index].role === "assistant") {
      next[index] = {
        ...next[index],
        role: "assistant",
        content: `${next[index].content}${delta}`,
      };
      return next;
    }
  }
  return [...next, { role: "assistant", content: delta }];
}

export function replaceAssistantMessage(
  current: ConversationMessage[],
  content: string,
): ConversationMessage[] {
  const next = [...current];
  for (let index = next.length - 1; index >= 0; index -= 1) {
    if (next[index].role === "assistant") {
      next[index] = { ...next[index], role: "assistant", content };
      return next;
    }
  }
  return [...next, { role: "assistant", content }];
}

export function dropTrailingEmptyAssistant(
  current: ConversationMessage[],
): ConversationMessage[] {
  const last = current.at(-1);
  return last?.role === "assistant" && last.content === ""
    ? current.slice(0, -1)
    : current;
}

/**
 * Deduplicate consecutive identical user messages (e.g. from queue resubmits or prompt retries)
 */
export function deduplicateMessages(
  messages: ConversationMessage[],
): ConversationMessage[] {
  const deduped: ConversationMessage[] = [];
  for (const msg of messages) {
    if (msg.role === "user") {
      const prev = deduped[deduped.length - 1];
      if (
        prev &&
        prev.role === "user" &&
        prev.content.trim() === msg.content.trim()
      ) {
        // Overwrite earlier turn with latest
        deduped[deduped.length - 1] = msg;
        continue;
      }
    }
    deduped.push(msg);
  }
  return deduped;
}
