import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ContextCapacityRing } from "./ContextCapacityPopover";

describe("ContextCapacityRing & Popover", () => {
  it("renders progress ring with accurate metrics and accessible tooltip", () => {
    const runtime = {
      agent_type: "codex" as const,
      agent_session_id: "test-sess-1",
      context_used_tokens: 227000,
      context_window_tokens: 272000,
      input_tokens: 220000,
      cache_read_tokens: 150000,
      cache_write_tokens: 10000,
      output_tokens: 7000,
    };

    const html = renderToStaticMarkup(
      <ContextCapacityRing runtime={runtime} messages={[]} />,
    );

    expect(html).toContain("composer-context-ring-btn");
    expect(html).toContain("context-ring-svg");
    expect(html).toContain("22.7万");
    expect(html).toContain("27.2万");
    expect(html).toContain("83.5%");
  });
});
