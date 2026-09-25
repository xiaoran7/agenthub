"use client";

import React, { useState, useMemo } from "react";

interface MarkdownViewProps {
  content: string;
  streaming?: boolean;
  className?: string;
}

/**
 * 复制按钮组件
 */
function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      if (navigator.clipboard) {
        await navigator.clipboard.writeText(text);
      } else {
        const textarea = document.createElement("textarea");
        textarea.value = text;
        textarea.style.position = "fixed";
        textarea.style.opacity = "0";
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand("copy");
        document.body.removeChild(textarea);
      }
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // 忽略复制错误
    }
  };

  return (
    <button
      type="button"
      className={`remote-code-copy-btn ${copied ? "copied" : ""}`}
      onClick={handleCopy}
      title={copied ? "已复制到剪贴板" : "复制代码"}
      aria-label={copied ? "已复制" : "复制代码"}
    >
      {copied ? (
        <>
          <svg
            width="13"
            height="13"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
          >
            <polyline points="20 6 9 17 4 12" />
          </svg>
          <span>已复制</span>
        </>
      ) : (
        <>
          <svg
            width="13"
            height="13"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
          <span>复制</span>
        </>
      )}
    </button>
  );
}

/**
 * 行内 Markdown 解析：处理 **加粗**, `行内代码`, *斜体*, [链接](url)
 */
function renderInlineMarkdown(text: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const pattern = /(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\)|\*[^*]+\*)/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let keyIdx = 0;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > lastIndex) {
      nodes.push(text.substring(lastIndex, match.index));
    }
    const token = match[0];
    if (token.startsWith("**") && token.endsWith("**")) {
      nodes.push(
        <strong key={`b-${keyIdx++}`} className="remote-md-bold">
          {token.slice(2, -2)}
        </strong>,
      );
    } else if (token.startsWith("`") && token.endsWith("`")) {
      nodes.push(
        <code key={`c-${keyIdx++}`} className="remote-md-inline-code">
          {token.slice(1, -1)}
        </code>,
      );
    } else if (
      token.startsWith("[") &&
      token.includes("](") &&
      token.endsWith(")")
    ) {
      const closeBracket = token.indexOf("](");
      const label = token.slice(1, closeBracket);
      const url = token.slice(closeBracket + 2, -1);
      nodes.push(
        <a
          key={`a-${keyIdx++}`}
          href={url}
          target="_blank"
          rel="noopener noreferrer"
          className="remote-md-link"
        >
          {label}
        </a>,
      );
    } else if (token.startsWith("*") && token.endsWith("*")) {
      nodes.push(
        <em key={`i-${keyIdx++}`} className="remote-md-italic">
          {token.slice(1, -1)}
        </em>,
      );
    }
    lastIndex = pattern.lastIndex;
  }

  if (lastIndex < text.length) {
    nodes.push(text.substring(lastIndex));
  }

  return nodes.length > 0 ? nodes : [text];
}

interface BlockSegment {
  type:
    "code" | "paragraph" | "header" | "list" | "blockquote" | "table" | "hr";
  language?: string;
  code?: string;
  level?: number;
  items?: string[];
  lines?: string[];
  raw?: string;
}

/**
 * 分块解析 Markdown 文本
 */
function parseMarkdownBlocks(rawText: string): BlockSegment[] {
  const blocks: BlockSegment[] = [];
  const lines = rawText.split(/\r?\n/);
  let i = 0;

  while (i < lines.length) {
    const currentLine = lines[i];
    if (currentLine === undefined) {
      i++;
      continue;
    }

    // 1. 代码块
    if (currentLine.trim().startsWith("```")) {
      const lang = currentLine.trim().slice(3).trim();
      const codeLines: string[] = [];
      i++;
      while (i < lines.length) {
        const codeLine = lines[i];
        if (codeLine === undefined || codeLine.trim().startsWith("```")) break;
        codeLines.push(codeLine);
        i++;
      }
      i++; // 跳过闭合 ```
      blocks.push({
        type: "code",
        language: lang || "text",
        code: codeLines.join("\n"),
      });
      continue;
    }

    // 2. 水平分割线
    if (/^(\*\*\*|---|___)$/.test(currentLine.trim())) {
      blocks.push({ type: "hr" });
      i++;
      continue;
    }

    // 3. 标题
    const headerMatch = currentLine.match(/^(#{1,6})\s+(.*)$/);
    if (headerMatch && headerMatch[1] && headerMatch[2] !== undefined) {
      blocks.push({
        type: "header",
        level: headerMatch[1].length,
        raw: headerMatch[2],
      });
      i++;
      continue;
    }

    // 4. 引用块
    if (currentLine.startsWith("> ") || currentLine === ">") {
      const quoteLines: string[] = [];
      while (i < lines.length) {
        const qLine = lines[i];
        if (qLine === undefined || (!qLine.startsWith("> ") && qLine !== ">"))
          break;
        quoteLines.push(qLine.replace(/^>\s?/, ""));
        i++;
      }
      blocks.push({
        type: "blockquote",
        lines: quoteLines,
      });
      continue;
    }

    // 5. 表格
    const nextLine = lines[i + 1];
    if (
      currentLine.includes("|") &&
      nextLine !== undefined &&
      /^\|?[\s-:]+\|[\s-:|]*$/.test(nextLine.trim())
    ) {
      const tableLines: string[] = [];
      while (i < lines.length) {
        const tLine = lines[i];
        if (
          tLine === undefined ||
          !tLine.includes("|") ||
          tLine.trim().length === 0
        )
          break;
        tableLines.push(tLine);
        i++;
      }
      blocks.push({
        type: "table",
        lines: tableLines,
      });
      continue;
    }

    // 6. 列表 (有序 / 无序)
    const isListItem = /^\s*([*+-]|\d+\.)\s+(.*)$/.test(currentLine);
    if (isListItem) {
      const listItems: string[] = [];
      while (i < lines.length) {
        const lLine = lines[i];
        if (lLine === undefined || !/^\s*([*+-]|\d+\.)\s+(.*)$/.test(lLine))
          break;
        const itemText = lLine.replace(/^\s*([*+-]|\d+\.)\s+/, "");
        listItems.push(itemText);
        i++;
      }
      blocks.push({
        type: "list",
        items: listItems,
      });
      continue;
    }

    // 7. 空行
    if (currentLine.trim() === "") {
      i++;
      continue;
    }

    // 8. 普通段落 (合并连续文本行)
    const paraLines: string[] = [];
    while (i < lines.length) {
      const pLine = lines[i];
      if (
        pLine === undefined ||
        pLine.trim() === "" ||
        pLine.trim().startsWith("```") ||
        pLine.startsWith("#") ||
        pLine.startsWith("> ") ||
        /^\s*([*+-]|\d+\.)\s+/.test(pLine) ||
        /^(\*\*\*|---|___)$/.test(pLine.trim())
      ) {
        break;
      }
      paraLines.push(pLine);
      i++;
    }
    blocks.push({
      type: "paragraph",
      raw: paraLines.join("\n"),
    });
  }

  return blocks;
}

export function MarkdownView({
  content,
  streaming = false,
  className = "",
}: MarkdownViewProps) {
  const blocks = useMemo(() => parseMarkdownBlocks(content), [content]);

  return (
    <div className={`remote-md-content ${className}`}>
      {blocks.map((block, idx) => {
        const isLastBlock = idx === blocks.length - 1;

        if (block.type === "code") {
          return (
            <div className="remote-code-block" key={idx}>
              <div className="remote-code-header">
                <span className="remote-code-lang">{block.language}</span>
                <CopyButton text={block.code || ""} />
              </div>
              <pre className="remote-code-pre">
                <code>{block.code}</code>
              </pre>
            </div>
          );
        }

        if (block.type === "header") {
          const lvl = Math.min(Math.max(block.level || 1, 1), 4);
          const headingContent = (
            <>
              {renderInlineMarkdown(block.raw || "")}
              {isLastBlock && streaming && (
                <span className="remote-typing-cursor" aria-hidden="true" />
              )}
            </>
          );
          if (lvl === 1)
            return (
              <h1 key={idx} className="remote-md-heading h1">
                {headingContent}
              </h1>
            );
          if (lvl === 2)
            return (
              <h2 key={idx} className="remote-md-heading h2">
                {headingContent}
              </h2>
            );
          if (lvl === 3)
            return (
              <h3 key={idx} className="remote-md-heading h3">
                {headingContent}
              </h3>
            );
          return (
            <h4 key={idx} className="remote-md-heading h4">
              {headingContent}
            </h4>
          );
        }

        if (block.type === "list") {
          return (
            <ul className="remote-md-list" key={idx}>
              {block.items?.map((item, itemIdx) => (
                <li key={itemIdx}>
                  {renderInlineMarkdown(item)}
                  {isLastBlock &&
                    itemIdx === (block.items?.length ?? 1) - 1 &&
                    streaming && (
                      <span
                        className="remote-typing-cursor"
                        aria-hidden="true"
                      />
                    )}
                </li>
              ))}
            </ul>
          );
        }

        if (block.type === "blockquote") {
          return (
            <blockquote className="remote-md-blockquote" key={idx}>
              {block.lines?.map((qLine, qIdx) => (
                <p key={qIdx}>{renderInlineMarkdown(qLine)}</p>
              ))}
              {isLastBlock && streaming && (
                <span className="remote-typing-cursor" aria-hidden="true" />
              )}
            </blockquote>
          );
        }

        if (block.type === "table" && block.lines && block.lines.length >= 2) {
          const firstLine = block.lines[0] || "";
          const headerCells = firstLine
            .split("|")
            .map((c) => c.trim())
            .filter((c, i, arr) =>
              i === 0 && c === ""
                ? false
                : i === arr.length - 1 && c === ""
                  ? false
                  : true,
            );
          const rowLines = block.lines.slice(2);

          return (
            <div className="remote-md-table-wrapper" key={idx}>
              <table className="remote-md-table">
                <thead>
                  <tr>
                    {headerCells.map((h, hIdx) => (
                      <th key={hIdx}>{renderInlineMarkdown(h)}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {rowLines.map((rowStr, rIdx) => {
                    const cells = rowStr
                      .split("|")
                      .map((c) => c.trim())
                      .filter((c, i, arr) =>
                        i === 0 && c === ""
                          ? false
                          : i === arr.length - 1 && c === ""
                            ? false
                            : true,
                      );
                    return (
                      <tr key={rIdx}>
                        {cells.map((cell, cIdx) => (
                          <td key={cIdx}>{renderInlineMarkdown(cell)}</td>
                        ))}
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          );
        }

        if (block.type === "hr") {
          return <hr className="remote-md-hr" key={idx} />;
        }

        // Paragraph
        return (
          <p className="remote-md-paragraph" key={idx}>
            {renderInlineMarkdown(block.raw || "")}
            {isLastBlock && streaming && (
              <span className="remote-typing-cursor" aria-hidden="true" />
            )}
          </p>
        );
      })}
      {blocks.length === 0 && streaming && (
        <span className="remote-typing-cursor" aria-hidden="true" />
      )}
    </div>
  );
}
