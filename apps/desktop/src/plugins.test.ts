import { describe, expect, it, beforeEach } from "vitest";
import {
  AGENT_PLUGIN_MARKETPLACE,
  isPluginEnabled,
  readPluginState,
  savePluginState,
} from "./plugins";

describe("Agent Plugins System", () => {
  let storage: Record<string, string> = {};

  beforeEach(() => {
    storage = {};
    const mockLocalStorage = {
      getItem: (key: string) => storage[key] ?? null,
      setItem: (key: string, val: string) => {
        storage[key] = val;
      },
      removeItem: (key: string) => {
        delete storage[key];
      },
      clear: () => {
        storage = {};
      },
    };

    (globalThis as unknown as { window: { localStorage: Storage } }).window = {
      localStorage: mockLocalStorage as unknown as Storage,
    };
  });

  it("contains Codex and ZCode in the plugin marketplace", () => {
    const codexPlugin = AGENT_PLUGIN_MARKETPLACE.find((p) => p.id === "codex");
    expect(codexPlugin).toBeDefined();
    expect(codexPlugin?.name).toContain("Codex");
    expect(codexPlugin?.category).toBe("官方核心");
    expect(codexPlugin?.builtin).toBe(false);

    const zcodePlugin = AGENT_PLUGIN_MARKETPLACE.find((p) => p.id === "zcode");
    expect(zcodePlugin).toBeDefined();
    expect(zcodePlugin?.name).toContain("ZCode");
    expect(zcodePlugin?.category).toBe("官方核心");
    expect(zcodePlugin?.builtin).toBe(false);
  });

  it("enables only Claude Code by default (focusing on Claude)", () => {
    expect(isPluginEnabled("claude_code")).toBe(true);
    // Codex, ZCode, Antigravity, Grok require user to enable in marketplace
    expect(isPluginEnabled("codex")).toBe(false);
    expect(isPluginEnabled("zcode")).toBe(false);
    expect(isPluginEnabled("antigravity")).toBe(false);
    expect(isPluginEnabled("grok_build")).toBe(false);
  });

  it("activates Codex and ZCode when enabled in plugin state", () => {
    savePluginState({ enabledPluginIds: ["codex", "zcode"] });
    expect(isPluginEnabled("codex")).toBe(true);
    expect(isPluginEnabled("zcode")).toBe(true);
    expect(readPluginState().enabledPluginIds).toContain("zcode");
  });
});
