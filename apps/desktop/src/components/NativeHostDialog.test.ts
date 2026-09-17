import { describe, expect, it } from "vitest";
import { applyHostStatus, normalizeWorkspacePaths } from "./NativeHostDialog";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { NativeHostDialog } from "./NativeHostDialog";

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

describe("NativeHostDialog workspace normalization", () => {
  it("merges Windows aliases case-insensitively and preserves Unix case", () => {
    expect(
      normalizeWorkspacePaths([
        "\\\\?\\D:\\Project\\Agent-Pulse",
        "d:/project/AGENT-PULSE",
      ]),
    ).toEqual(["D:/Project/Agent-Pulse"]);
    expect(normalizeWorkspacePaths(["/srv/project", "/SRV/project"])).toEqual([
      "/srv/project",
      "/SRV/project",
    ]);
    expect(normalizeWorkspacePaths(["D:\\"])).toEqual(["D:/"]);
  });

  it("keeps dialog-added paths and stored paths in the same canonical form", () => {
    const status = {
      phase: "stopped" as const,
      profile: {
        workspace_paths: ["D:\\Project"],
        codex_binary: "",
        remote_enabled: false,
      },
    };
    const applied = applyHostStatus(empty, status);
    expect(applied.profile.workspace_paths).toEqual(["D:/Project"]);
  });
});

describe("NativeHostDialog status recovery", () => {
  it("does not claim provider readiness or duplicate provider settings", () => {
    const html = renderToStaticMarkup(
      createElement(NativeHostDialog, { onClose() {} }),
    );
    expect(html).not.toContain("已就绪");
    expect(html).not.toContain("ZCode App Server");
    expect(html).not.toContain("Claude 可执行文件");
    expect(html).toContain("以实际检测结果为准");
  });

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
