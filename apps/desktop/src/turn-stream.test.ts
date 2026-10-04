import { describe, expect, it } from "vitest";

import {
  appendAssistantDelta,
  dropTrailingEmptyAssistant,
  dropTrailingOptimisticTurn,
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

  it("rolls back both empty assistant and rejected optimistic user turn", () => {
    const optimisticTurn: ConversationMessage[] = [
      { role: "user", content: "turn 1" },
      { role: "assistant", content: "reply 1" },
      { role: "user", content: "turn 2 (rejected)" },
      { role: "assistant", content: "" },
    ];
    expect(
      dropTrailingOptimisticTurn(optimisticTurn, "turn 2 (rejected)"),
    ).toEqual([
      { role: "user", content: "turn 1" },
      { role: "assistant", content: "reply 1" },
    ]);
  });

  it("never appends deltas past user prompt into previous assistant turn", () => {
    // Multi-turn setup: Turn 1 finished, Turn 2 user prompt submitted, but trailing assistant was dropped on error
    const history: ConversationMessage[] = [
      { role: "user", content: "turn 1 prompt" },
      { role: "assistant", content: "turn 1 reply" },
      { role: "user", content: "turn 2 prompt" },
    ];
    // If a late-arriving delta comes in, it must NOT mutate Turn 1's reply!
    const result = appendAssistantDelta(history, "turn 2 reply");
    expect(result[1].content).toBe("turn 1 reply");
    expect(result.at(-1)).toEqual({
      role: "assistant",
      content: "turn 2 reply",
    });
    expect(result).toHaveLength(4);
  });

  it("never replaces past user prompt into previous assistant turn", () => {
    const history: ConversationMessage[] = [
      { role: "user", content: "turn 1 prompt" },
      { role: "assistant", content: "turn 1 reply" },
      { role: "user", content: "turn 2 prompt" },
    ];
    const result = replaceAssistantMessage(history, "turn 2 final");
    expect(result[1].content).toBe("turn 1 reply");
    expect(result.at(-1)).toEqual({
      role: "assistant",
      content: "turn 2 final",
    });
    expect(result).toHaveLength(4);
  });
});
