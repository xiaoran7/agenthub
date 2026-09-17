import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { SessionEmptyState } from "./SessionEmptyState";

const render = (props: Partial<Parameters<typeof SessionEmptyState>[0]> = {}) =>
  renderToStaticMarkup(
    <SessionEmptyState
      onNewSession={() => {}}
      onOpenHostControl={() => {}}
      {...props}
    />,
  );

describe("SessionEmptyState", () => {
  it("offers the two user actions without nested cards or technical jargon", () => {
    const html = render({ sessionCount: 3, workspaceCount: 3 });
    expect(html).toContain("新建 OpenAI Codex 会话");
    expect(html).toContain("管理工作目录与远程连接");
    expect(html).not.toContain("empty-feature-card");
    expect(html).not.toContain("agenthubd");
    expect(html).not.toContain("Runtime");
    expect(html).not.toContain("Gateway");
    expect(html).toContain("左侧有 3 个会话可供选择");
    expect(html).toContain("已授权 3 个工作目录");
  });

  it("reports connection phase in user language", () => {
    expect(render({ hostPhase: "connected" })).toContain("远程已连接");
    expect(render({ hostPhase: "local" })).toContain(
      "本机会话 · 远程连接未开启",
    );
    expect(render({ sessionCount: 0 })).toContain("还没有会话");
  });
});
