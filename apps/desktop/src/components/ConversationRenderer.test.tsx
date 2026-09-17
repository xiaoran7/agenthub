import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ConversationItem } from "./ConversationRenderer";

const render = (content: string) =>
  renderToStaticMarkup(
    <ConversationItem message={{ role: "assistant", content }} index={0} />,
  );

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
