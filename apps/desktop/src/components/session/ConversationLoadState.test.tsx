import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ConversationLoadState } from "./ConversationLoadState";

describe("conversation loading states", () => {
  it("does not present a failed read as an empty/new conversation", () => {
    const html = renderToStaticMarkup(
      <ConversationLoadState
        loading={false}
        error="本机会话服务启动失败"
        managed
        onRetry={() => {}}
      />,
    );
    expect(html).toContain("会话内容加载失败");
    expect(html).toContain("本机会话服务启动失败");
    expect(html).toContain("重新加载");
    expect(html).not.toContain("从这里继续会话");
    expect(html).not.toContain("暂无可显示");
  });
  it("distinguishes loading from a successful empty read", () => {
    expect(
      renderToStaticMarkup(
        <ConversationLoadState loading managed onRetry={() => {}} />,
      ),
    ).toContain("正在读取会话内容");
    const empty = renderToStaticMarkup(
      <ConversationLoadState loading={false} managed onRetry={() => {}} />,
    );
    expect(empty).toContain("暂无可显示的历史消息");
    expect(empty).not.toContain('role="alert"');
  });
  it("does not offer transcript retries for unsupported providers", () => {
    const html = renderToStaticMarkup(
      <ConversationLoadState
        loading={false}
        managed={false}
        onRetry={() => {}}
      />,
    );
    expect(html).toContain("暂不支持继续此会话");
    expect(html).not.toContain("重新加载");
  });
});
