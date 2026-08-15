export interface ConversationMessage {
  role: "user" | "assistant";
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
      next[index] = { role: "assistant", content };
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
