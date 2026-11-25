import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { splitMarkdownBlocks } from "../src/lib/markdown-blocks";

describe("splitMarkdownBlocks", () => {
  it("splits top-level paragraphs while keeping fenced code blocks with blank lines intact", () => {
    const md = [
      "First paragraph.",
      "",
      "```ts",
      "function a() {",
      "",
      "  return 1;",
      "}",
      "```",
      "",
      "Final streaming line",
    ].join("\n");

    const blocks = splitMarkdownBlocks(md);
    assert.equal(blocks.length, 3);
    assert.equal(blocks[0], "First paragraph.");
    assert.equal(blocks[1], "```ts\nfunction a() {\n\n  return 1;\n}\n```");
    assert.equal(blocks[2], "Final streaming line");
  });

  it("handles unclosed streaming code fences without tearing", () => {
    const md = "Intro\n\n```python\ndef f():\n\n    pass";
    const blocks = splitMarkdownBlocks(md);
    assert.equal(blocks.length, 2);
    assert.equal(blocks[0], "Intro");
    assert.equal(blocks[1], "```python\ndef f():\n\n    pass");
  });

  it("keeps GFM tables together in a single block", () => {
    const md = "| A | B |\n|---|---|\n| 1 | 2 |\n\nDone.";
    const blocks = splitMarkdownBlocks(md);
    assert.equal(blocks.length, 2);
    assert.equal(blocks[0], "| A | B |\n|---|---|\n| 1 | 2 |");
    assert.equal(blocks[1], "Done.");
  });

  it("keeps loose ordered lists in one block so numbering does not reset", () => {
    const md = "Intro\n\n1. First\n\n2. Second\n\n3. Third\n\nOutro";
    const blocks = splitMarkdownBlocks(md);
    assert.equal(blocks.length, 3);
    assert.equal(blocks[0], "Intro");
    assert.equal(blocks[1], "1. First\n\n2. Second\n\n3. Third");
    assert.equal(blocks[2], "Outro");
  });

  it("falls back to single block when reference link definitions are present", () => {
    const md = "See [docs][1].\n\n[1]: https://example.com";
    const blocks = splitMarkdownBlocks(md);
    assert.equal(blocks.length, 1);
    assert.equal(blocks[0], md);
  });
});
