import { describe, expect, it } from "vitest";
import {
  effortOptionsFor,
  resolveEffort,
  supportsEffort,
} from "./effortOptions";

describe("effortOptionsFor", () => {
  it("offers the levels each CLI actually accepts", () => {
    // claude --effort supports xhigh and max; Codex does not.
    expect(effortOptionsFor("claude_code").map((o) => o.value)).toEqual([
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ]);
    expect(effortOptionsFor("codex").map((o) => o.value)).toEqual([
      "low",
      "medium",
      "high",
    ]);
  });

  it("offers nothing for providers without a per-turn effort", () => {
    expect(effortOptionsFor("zcode")).toEqual([]);
    expect(effortOptionsFor("antigravity")).toEqual([]);
    expect(effortOptionsFor("grok_build")).toEqual([]);
    expect(supportsEffort("zcode")).toBe(false);
    expect(supportsEffort("claude_code")).toBe(true);
  });
});

describe("resolveEffort", () => {
  it("keeps a stored choice the provider still supports", () => {
    expect(resolveEffort("claude_code", "xhigh", "low")).toBe("xhigh");
  });

  it("falls back to the reported level when nothing is stored", () => {
    expect(resolveEffort("claude_code", undefined, "high")).toBe("high");
    expect(resolveEffort("codex", undefined, "HIGH")).toBe("high");
  });

  it("rejects a stored level the provider no longer accepts", () => {
    // max is Claude-only; it must not leak into Codex.
    expect(resolveEffort("codex", "max", undefined)).toBe("medium");
  });

  it("returns nothing for providers that ignore effort", () => {
    expect(resolveEffort("zcode", "high", "high")).toBe("");
  });
});
