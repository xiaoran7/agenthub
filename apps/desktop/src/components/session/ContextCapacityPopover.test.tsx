import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ConversationMessage } from "../../turn-stream";
import type { SessionRuntimeContext } from "../../types";
import {
  ContextCapacityRing,
  useContextMetrics,
} from "./ContextCapacityPopover";

function MetricsProbe(props: {
  runtime?: SessionRuntimeContext | null;
  messages?: ConversationMessage[];
  modelName?: string;
}) {
  const metrics = useContextMetrics(
    props.runtime,
    props.messages,
    props.modelName,
  );
  return (
    <div>
      <span data-testid="pct">{metrics.percentage}</span>
      <span data-testid="free">{metrics.freeTokens}</span>
      <span data-testid="measured">{String(metrics.measured)}</span>
      {metrics.categories.map((category) => (
        <span key={category.id} data-testid={category.id}>
          {category.label}|{category.tokens}|{category.windowPct}|
          {category.usedPct}
          {category.details?.map((detail) => (
            <span key={detail.label} data-testid={`${category.id}-detail`}>
              {detail.label}|{detail.tokens}|{detail.count}
            </span>
          ))}
        </span>
      ))}
    </div>
  );
}

const claudeRuntime: SessionRuntimeContext = {
  agent_type: "claude_code",
  agent_session_id: "s-1",
  context_window_tokens: 200_000,
  context_used_tokens: 20_000,
  input_tokens: 20_000,
  cache_read_tokens: 15_000,
  output_tokens: 5_000,
};

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

  it("reports only measured categories when the provider sends a breakdown", () => {
    const html = renderToStaticMarkup(<MetricsProbe runtime={claudeRuntime} />);

    expect(html).toContain('data-testid="measured">true');
    expect(html).toContain('data-testid="pct">10');
    expect(html).toContain('data-testid="free">180000');
    // Only reported counters become rows: no invented system-prompt row.
    expect(html).toContain("缓存读取|15000|7.5|75");
    expect(html).toContain("新输入|5000|2.5|25");
    expect(html).toContain("生成输出|5000|2.5|25");
    expect(html).not.toContain("系统提示");
  });

  it("falls back to transcript weighting without inventing provider counters", () => {
    const runtime: SessionRuntimeContext = {
      agent_type: "claude_code",
      agent_session_id: "s-2",
      context_window_tokens: 200_000,
      context_used_tokens: 10_000,
    };
    const messages: ConversationMessage[] = [
      { role: "user", content: "x".repeat(100) },
      { role: "assistant", content: "y".repeat(100) },
      { role: "assistant", kind: "tool", content: "z".repeat(200) },
    ];

    const html = renderToStaticMarkup(
      <MetricsProbe runtime={runtime} messages={messages} />,
    );

    expect(html).toContain('data-testid="measured">false');
    // Character weights 100/100/200 put half the total in tool output.
    expect(html).toContain("工具调用与结果|5000|2.5|50");
    expect(html).toContain("对话消息|5000|2.5|50");
    // Entry counts are real, so the row is drillable.
    expect(html).toContain("工具条目|5000|1");
    expect(html).toContain("消息条目|5000|2");
  });

  it("infers the window from the model when none is reported", () => {
    const html = renderToStaticMarkup(
      <MetricsProbe
        runtime={{
          agent_type: "zcode",
          agent_session_id: "s-3",
          context_used_tokens: 250_000,
        }}
        modelName="gemini-2.5-pro"
      />,
    );

    expect(html).toContain('data-testid="pct">25');
  });
});
