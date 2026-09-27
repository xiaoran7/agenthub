import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  ConversationItem,
  classifyTool,
  parseDiffStats,
  extractToolPrimaryValue,
  parseQuestionData,
  ToolCallBlock,
} from "./ConversationRenderer";

const render = (content: string) =>
  renderToStaticMarkup(
    <ConversationItem message={{ role: "assistant", content }} index={0} />,
  );

describe("tool classification", () => {
  it.each([
    ["Bash: cargo test", undefined, "terminal"],
    ["命令执行", "command", "terminal"],
    ["Edit: src/main.rs", undefined, "file_edit"],
    ["Write: config.json", undefined, "file_edit"],
    ["Read: src/lib.rs", undefined, "file_read"],
    ["Grep: retry_timeout", undefined, "file_read"],
    ["Glob: **/*.rs", undefined, "file_read"],
    ["AskUserQuestion: Pick option", undefined, "interactive"],
    ["ExitPlanMode", undefined, "interactive"],
    ["后台任务状态更新", "status", "system"],
    ["通用工具", "tool", "system"],
  ])("classifies '%s' with kind '%s' as '%s'", (label, kind, expected) => {
    expect(classifyTool(label, kind)).toBe(expected);
  });
});

describe("diff stats parsing", () => {
  it("parses additions and deletions from JSON old/new strings", () => {
    const jsonStr = JSON.stringify({
      old: "first line\nsecond line",
      new: "first line\nmodified second line\nthird line",
    });
    const stats = parseDiffStats(jsonStr);
    expect(stats.deletions).toBe(2);
    expect(stats.additions).toBe(3);
    expect(stats.oldText).toBe("first line\nsecond line");
    expect(stats.newText).toBe("first line\nmodified second line\nthird line");
  });

  it("parses unified diff format lines", () => {
    const diffStr = "@@ -1,2 +1,3 @@\n-old line\n+new line 1\n+new line 2";
    const stats = parseDiffStats(diffStr);
    expect(stats.deletions).toBe(1);
    expect(stats.additions).toBe(2);
  });

  it("parses interactive questions from tool arguments", () => {
    const raw = JSON.stringify({
      questions: [
        {
          header: "Auth",
          question: "Which auth method?",
          options: [{ label: "OAuth", description: "Use GitHub" }],
        },
      ],
    });
    const parsed = parseQuestionData(raw);
    expect(parsed).toHaveLength(1);
    expect(parsed?.[0].header).toBe("Auth");
    expect(parsed?.[0].question).toBe("Which auth method?");
    expect(parsed?.[0].options?.[0].label).toBe("OAuth");
  });

  it("extracts tool primary value for quick copy", () => {
    expect(extractToolPrimaryValue("Bash: npm run build")).toBe(
      "npm run build",
    );
    expect(extractToolPrimaryValue("Read: src/types.ts")).toBe("src/types.ts");
    expect(
      extractToolPrimaryValue(undefined, '{"command": "git status"}'),
    ).toBe("git status");
  });
});

describe("tool call block rendering", () => {
  it("renders a terminal tool call with category class and quick copy button", () => {
    const html = renderToStaticMarkup(
      <ToolCallBlock
        message={{
          role: "assistant",
          kind: "tool",
          label: "Bash: cargo test",
          status: "completed",
          content: '{"command": "cargo test"}\n\n〔结果〕\ntest result: ok',
        }}
      />,
    );
    expect(html).toContain("tool-cat-terminal");
    expect(html).toContain("Bash: cargo test");
    expect(html).toContain("tool-quick-copy-btn");
  });

  it("renders an edit tool call with diff stats badge and diff viewer", () => {
    const html = renderToStaticMarkup(
      <ToolCallBlock
        message={{
          role: "assistant",
          kind: "tool",
          label: "Edit: src/lib.rs",
          status: "completed",
          content: `${JSON.stringify({
            old: "let a = 1;",
            new: "let a = 2;",
          })}\n\n〔结果〕\n已修改`,
        }}
      />,
    );
    expect(html).toContain("tool-cat-file_edit");
    expect(html).toContain("diff-stats-badge");
    expect(html).toContain("diff-stat-add");
    expect(html).toContain("+1");
    expect(html).toContain("diff-stat-del");
    expect(html).toContain("-1");
    expect(html).toContain("tool-diff-viewer");
    expect(html).toContain("diff-chunk-del");
    expect(html).toContain("diff-chunk-add");
  });

  it("renders an interactive question tool call with structured choices", () => {
    const html = renderToStaticMarkup(
      <ToolCallBlock
        message={{
          role: "assistant",
          kind: "tool",
          label: "AskUserQuestion",
          status: "completed",
          content: `${JSON.stringify({
            questions: [
              {
                header: "Approach",
                question: "Which approach do you prefer?",
                options: [
                  { label: "Option A", description: "Quick fix" },
                  { label: "Option B", description: "Full refactor" },
                ],
              },
            ],
          })}\n\n〔结果〕\n{"answers": {"Which approach do you prefer?": "Option A"}}`,
        }}
      />,
    );
    expect(html).toContain("tool-cat-interactive");
    expect(html).toContain("tool-question-card");
    expect(html).toContain("Which approach do you prefer?");
    expect(html).toContain("Option A");
    expect(html).toContain("Quick fix");
    expect(html).toContain("已选择");
  });
});

describe("markdown code rendering", () => {
  it.each([
    ["```js\nhello\n```", "js", "hello"],
    ["```c++\nfirst\nsecond\n```", "c++", "first\nsecond"],
    ["```\nhello\n```", "text", "hello"],
    ["```\nfirst\nsecond\n```", "text", "first\nsecond"],
    ["    first\n    second", "text", "first\nsecond"],
  ])(
    "renders one pre and one copy header for %s",
    (source, language, value) => {
      const html = render(source);
      expect(html.match(/<pre\b/g)).toHaveLength(1);
      expect(html.match(/class="code-block-wrapper"/g)).toHaveLength(1);
      expect(html).toContain(`class="code-language">${language}</span>`);
      expect(html).toContain('aria-label="复制代码"');
      expect(html).toContain(`<code>${value}</code>`);
      expect(html).not.toMatch(/<pre[^>]*>\s*<div/);
      expect(html).not.toContain("[object Object]");
      expect(html).not.toContain("node=");
    },
  );

  it("keeps inline code inline, including alongside a fenced block", () => {
    const html = render("Use `hello` here.\n\n```\nworld\n```");
    expect(html).toContain("<p>Use <code>hello</code> here.</p>");
    expect(html.match(/<pre\b/g)).toHaveLength(1);
  });

  it("escapes code text and preserves indentation", () => {
    expect(render("```html\n  <script>bad()</script>\n```")).toContain(
      "<code>  &lt;script&gt;bad()&lt;/script&gt;</code>",
    );
  });

  it("renders table cells and emphasis as semantic markdown", () => {
    const html = render(
      "| Name | Value |\n| --- | --- |\n| **bold** | `inline` |",
    );
    expect(html).toContain("<td><strong>bold</strong></td>");
    expect(html).toContain("<td><code>inline</code></td>");
  });
});
