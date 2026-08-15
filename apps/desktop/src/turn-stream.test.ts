import { describe, expect, it } from "vitest";

import {
  appendAssistantDelta,
  dropTrailingEmptyAssistant,
  replaceAssistantMessage,
  type ConversationMessage,
} from "./turn-stream";

describe("turn stream state", () => {
  const seeded: ConversationMessage[] = [
    { role: "user", content: "hello" },
    { role: "assistant", content: "" },
  ];

  it("batches deltas into the pending assistant turn", () => {
    const first = appendAssistantDelta(seeded, "Agent");
    const second = appendAssistantDelta(first, "Pulse");

    expect(second.at(-1)).toEqual({
      role: "assistant",
      content: "AgentPulse",
    });
    expect(seeded.at(-1)?.content).toBe("");
  });

  it("replaces streamed text with the authoritative final response", () => {
    const streamed = appendAssistantDelta(seeded, "partial");
    expect(replaceAssistantMessage(streamed, "final").at(-1)?.content).toBe(
      "final",
    );
  });

  it("removes only an empty failed assistant placeholder", () => {
    expect(dropTrailingEmptyAssistant(seeded)).toEqual([seeded[0]]);
    expect(
      dropTrailingEmptyAssistant(appendAssistantDelta(seeded, "partial")),
    ).toHaveLength(2);
  });
});
