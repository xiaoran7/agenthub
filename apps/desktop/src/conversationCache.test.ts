import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { invoke } from "@tauri-apps/api/core";
import { cachedConversation, loadConversation } from "./conversationCache";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe("conversation cache", () => {
  it("shares in-flight manual and polling reads and stores the provider/session key", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    let resolve!: (messages: unknown) => void;
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }) as never,
    );
    const first = loadConversation("codex", "cache-test", true);
    const second = loadConversation("codex", "cache-test", true);
    expect(second).toBe(first);
    expect(invoke).toHaveBeenCalledTimes(1);
    resolve([{ role: "assistant", content: "latest" }]);
    await first;
    expect(cachedConversation("codex:cache-test")?.[0].content).toBe("latest");
  });
  it("preserves existing cached data on errors and permits retry", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    vi.mocked(invoke).mockResolvedValueOnce([
      { role: "user", content: "original" },
    ]);
    await loadConversation("codex", "retry-test");
    vi.mocked(invoke).mockRejectedValueOnce(
      new Error("local host unavailable"),
    );
    await expect(loadConversation("codex", "retry-test", true)).rejects.toThrow(
      "local host",
    );
    expect(cachedConversation("codex:retry-test")?.[0].content).toBe(
      "original",
    );
    vi.mocked(invoke).mockResolvedValueOnce([
      { role: "assistant", content: "recovered" },
    ]);
    await loadConversation("codex", "retry-test", true);
    expect(cachedConversation("codex:retry-test")?.[0].content).toBe(
      "recovered",
    );
  });
});
