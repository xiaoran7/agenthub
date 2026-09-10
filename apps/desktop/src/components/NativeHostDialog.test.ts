import { describe, expect, it } from "vitest";
import { applyHostStatus } from "./NativeHostDialog";

const empty = {
  phase: "loading" as const,
  profile: {
    workspace_paths: [] as string[],
    codex_binary: "",
    remote_enabled: false,
  },
  initialized: false,
  error: "ipc_connect_failed",
};

const status = {
  phase: "local" as const,
  profile: {
    workspace_paths: ["D:/workspace"],
    codex_binary: "D:/codex.exe",
    remote_enabled: false,
  },
};

describe("NativeHostDialog status recovery", () => {
  it("hydrates on the first successful poll and clears its poll error", () => {
    expect(applyHostStatus(empty, status)).toEqual({
      ...status,
      initialized: true,
      error: "",
    });
  });

  it("preserves edits during background polls and refreshes after transitions", () => {
    const edited = {
      ...applyHostStatus(empty, status),
      profile: { ...status.profile, codex_binary: "D:/custom-codex.exe" },
    };
    expect(
      applyHostStatus(edited, {
        ...status,
        phase: "connected",
        profile: { ...status.profile, remote_enabled: true },
      }).profile.codex_binary,
    ).toBe("D:/custom-codex.exe");
    expect(
      applyHostStatus(
        edited,
        {
          ...status,
          phase: "connected",
          profile: { ...status.profile, remote_enabled: true },
        },
        true,
      ).profile,
    ).toEqual({ ...status.profile, remote_enabled: true });
  });
});
