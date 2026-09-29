import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { SimpleTitleBar, WindowCaptionControls } from "./WindowControls";

describe("WindowControls", () => {
  it("renders caption controls with minimize, maximize and close buttons", () => {
    const html = renderToStaticMarkup(
      <WindowCaptionControls
        isMaximized={false}
        onMinimize={vi.fn()}
        onToggleMaximize={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="最小化"');
    expect(html).toContain('aria-label="最大化"');
    expect(html).toContain('aria-label="关闭"');
    expect(html).toContain("window-control-btn close");
  });

  it("reflects restore label when maximized", () => {
    const html = renderToStaticMarkup(
      <WindowCaptionControls
        isMaximized={true}
        onMinimize={vi.fn()}
        onToggleMaximize={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(html).toContain('aria-label="向下还原"');
  });

  it("renders SimpleTitleBar with data-tauri-drag-region and title", () => {
    const html = renderToStaticMarkup(<SimpleTitleBar title="登录 AgentHub" />);

    expect(html).toContain("data-tauri-drag-region");
    expect(html).toContain("simple-window-titlebar");
    expect(html).toContain("simple-titlebar-title");
    expect(html).toContain("登录 AgentHub");
    expect(html).toContain('aria-label="最小化"');
    expect(html).toContain('aria-label="关闭"');
  });
});
