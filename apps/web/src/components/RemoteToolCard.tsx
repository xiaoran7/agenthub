import { memo, useEffect, useMemo, useRef, useState } from "react";
import { copyTextToClipboard } from "../lib/clipboard";
import { readableRemoteText, type ConversationItem } from "../lib/remote";
import { matchesSelectedOption } from "../lib/remote-session-state";
import { CheckIcon, CopyIcon } from "./RemoteMessageCard";
import { ToolIcon, toolKind } from "./remote-console-constants";

interface DiffStats {
  additions: number;
  deletions: number;
  oldText?: string | undefined;
  newText?: string | undefined;
}

export function parseDiffStats(content?: string): DiffStats {
  if (!content) return { additions: 0, deletions: 0 };
  let additions = 0;
  let deletions = 0;
  let oldText: string | undefined;
  let newText: string | undefined;

  const trimmed = content.trim();
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    try {
      const parsed = JSON.parse(trimmed);
      const oldStr = parsed.old ?? parsed.old_string;
      const newStr = parsed.new ?? parsed.new_string;
      if (typeof oldStr === "string" || typeof newStr === "string") {
        if (typeof oldStr === "string") {
          oldText = oldStr;
          deletions = oldStr
            .split("\n")
            .filter((l: string) => l.trim() !== "").length;
        }
        if (typeof newStr === "string") {
          newText = newStr;
          additions = newStr
            .split("\n")
            .filter((l: string) => l.trim() !== "").length;
        }
        return { additions, deletions, oldText, newText };
      }
    } catch {
      // not simple json
    }
  }

  const lines = content.split("\n");
  for (const line of lines) {
    if (
      line.startsWith("+++") ||
      line.startsWith("---") ||
      line.startsWith("@@")
    ) {
      continue;
    }
    if (line.startsWith("+")) additions++;
    else if (line.startsWith("-")) deletions++;
  }

  if (!oldText && !newText && (additions > 0 || deletions > 0)) {
    const oldLines: string[] = [];
    const newLines: string[] = [];
    for (const line of lines) {
      if (
        line.startsWith("+++") ||
        line.startsWith("---") ||
        line.startsWith("@@")
      ) {
        continue;
      }
      if (line.startsWith("-")) {
        oldLines.push(line.slice(1));
      } else if (line.startsWith("+")) {
        newLines.push(line.slice(1));
      } else {
        const ctx = line.startsWith(" ") ? line.slice(1) : line;
        oldLines.push(ctx);
        newLines.push(ctx);
      }
    }
    if (oldLines.length > 0) oldText = oldLines.join("\n");
    if (newLines.length > 0) newText = newLines.join("\n");
  }

  return { additions, deletions, oldText, newText };
}

interface QuestionItem {
  question: string;
  header?: string | undefined;
  options?:
    | Array<{
        label: string;
        description?: string | undefined;
        preview?: string | undefined;
      }>
    | undefined;
  multiSelect?: boolean | undefined;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function parseQuestionData(content?: string): QuestionItem[] | null {
  if (!content) return null;
  const trimmed = content.trim();
  if (!trimmed.startsWith("{") || !trimmed.endsWith("}")) return null;
  try {
    const parsed: unknown = JSON.parse(trimmed);
    if (!isPlainObject(parsed)) return null;
    if (!Array.isArray(parsed.questions) || parsed.questions.length === 0) {
      return null;
    }
    const validated: QuestionItem[] = [];
    for (const rawQuestion of parsed.questions) {
      if (!isPlainObject(rawQuestion)) return null;
      if (typeof rawQuestion.question !== "string") return null;
      let validatedOptions: QuestionItem["options"];
      if (rawQuestion.options !== undefined) {
        if (!Array.isArray(rawQuestion.options)) return null;
        validatedOptions = [];
        for (const rawOpt of rawQuestion.options) {
          if (!isPlainObject(rawOpt) || typeof rawOpt.label !== "string") {
            return null;
          }
          validatedOptions.push({
            label: rawOpt.label,
            ...(typeof rawOpt.description === "string"
              ? { description: rawOpt.description }
              : {}),
            ...(typeof rawOpt.preview === "string"
              ? { preview: rawOpt.preview }
              : {}),
          });
        }
      }
      validated.push({
        question: rawQuestion.question,
        ...(typeof rawQuestion.header === "string"
          ? { header: rawQuestion.header }
          : {}),
        ...(validatedOptions !== undefined
          ? { options: validatedOptions }
          : {}),
        ...(typeof rawQuestion.multiSelect === "boolean"
          ? { multiSelect: rawQuestion.multiSelect }
          : {}),
      });
    }
    return validated;
  } catch {
    // not json
  }
  return null;
}

export const RemoteToolCard = memo(function RemoteToolCard({
  item,
}: {
  item: ConversationItem;
}) {
  const status = item.status ?? "completed";
  const kind = toolKind(item);
  const diffStats = useMemo(() => parseDiffStats(item.command), [item.command]);
  const questionItems = useMemo(
    () => parseQuestionData(item.command),
    [item.command],
  );
  const hasDiff = diffStats.additions > 0 || diffStats.deletions > 0;
  const [copiedSection, setCopiedSection] = useState<string | null>(null);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
    };
  }, []);

  const handleCopy = (section: string, text: string, e: React.MouseEvent) => {
    e.stopPropagation();
    void copyTextToClipboard(text).then((ok) => {
      if (!ok || !mountedRef.current) return;
      setCopiedSection(section);
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => {
        if (mountedRef.current) {
          setCopiedSection((cur) => (cur === section ? null : cur));
        }
      }, 1800);
    });
  };

  return (
    <details
      className={`remote-tool mcp-tool-decision-card state-${status}`}
      data-tool={kind}
    >
      <summary className="remote-tool-summary">
        <span className="remote-disclosure" aria-hidden="true">
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="9 6 15 12 9 18" />
          </svg>
        </span>
        <span className="remote-tool-header-left">
          <span className="remote-tool-glyph">
            <ToolIcon kind={kind} />
          </span>
          <span className="remote-tool-title">{item.title ?? "工具调用"}</span>
          {hasDiff && (
            <span
              className="remote-diff-badge"
              aria-label={`+${diffStats.additions} -${diffStats.deletions}`}
            >
              {diffStats.additions > 0 && (
                <span className="diff-add">+{diffStats.additions}</span>
              )}
              {diffStats.deletions > 0 && (
                <span className="diff-del">-{diffStats.deletions}</span>
              )}
            </span>
          )}
        </span>
        <span className={`remote-tool-status ${status}`}>
          {status === "running" && (
            <span className="remote-tool-pulse-dots" aria-hidden="true">
              <span className="pulse-dot" />
              <span className="pulse-dot" />
              <span className="pulse-dot" />
            </span>
          )}
          {status === "running" ? (
            <span className="status-label">执行中…</span>
          ) : status === "failed" ? (
            <span className="status-label">执行失败</span>
          ) : (
            <span className="status-label">已完成</span>
          )}
        </span>
      </summary>
      {(item.description || item.command || item.output) && (
        <div className="remote-tool-content">
          {item.description && (
            <p className="remote-tool-description">
              {readableRemoteText(item.description)}
            </p>
          )}
          {questionItems ? (
            <div className="remote-question-card">
              {questionItems.map((q, qIdx) => (
                <div key={qIdx} className="remote-question-item">
                  <div className="remote-question-item-header">
                    {q.header && (
                      <span className="remote-question-tag">{q.header}</span>
                    )}
                    <span className="remote-question-text">{q.question}</span>
                  </div>
                  {q.options && q.options.length > 0 && (
                    <div className="remote-options-list">
                      {q.options.map((opt, optIdx) => {
                        const isSelected = matchesSelectedOption(
                          item.output,
                          opt.label,
                        );
                        return (
                          <div
                            key={optIdx}
                            className={`remote-option-item ${isSelected ? "selected" : ""}`}
                          >
                            <div className="remote-option-label">
                              <span>{opt.label}</span>
                              {isSelected && (
                                <span className="remote-option-badge">
                                  已选择
                                </span>
                              )}
                            </div>
                            {opt.description && (
                              <div className="remote-option-desc">
                                {opt.description}
                              </div>
                            )}
                            {opt.preview && (
                              <pre className="remote-option-preview">
                                {opt.preview}
                              </pre>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              ))}
            </div>
          ) : diffStats.oldText || diffStats.newText ? (
            <div className="remote-diff-viewer">
              {diffStats.oldText && (
                <div className="remote-diff-chunk del">
                  <div className="remote-diff-bar">
                    <span className="remote-diff-tag">- 变更前</span>
                    <button
                      type="button"
                      className="remote-card-mini-copy"
                      onClick={(e) =>
                        handleCopy("diff-del", diffStats.oldText!, e)
                      }
                    >
                      {copiedSection === "diff-del" ? (
                        <span className="remote-copied-badge">
                          <CheckIcon size={12} />
                          <span>已复制</span>
                        </span>
                      ) : (
                        <span className="remote-copy-badge">
                          <CopyIcon size={12} />
                          <span>复制</span>
                        </span>
                      )}
                    </button>
                  </div>
                  <pre className="remote-diff-pre">{diffStats.oldText}</pre>
                </div>
              )}
              {diffStats.newText && (
                <div className="remote-diff-chunk add">
                  <div className="remote-diff-bar">
                    <span className="remote-diff-tag">+ 变更后</span>
                    <button
                      type="button"
                      className="remote-card-mini-copy"
                      onClick={(e) =>
                        handleCopy("diff-add", diffStats.newText!, e)
                      }
                    >
                      {copiedSection === "diff-add" ? (
                        <span className="remote-copied-badge">
                          <CheckIcon size={12} />
                          <span>已复制</span>
                        </span>
                      ) : (
                        <span className="remote-copy-badge">
                          <CopyIcon size={12} />
                          <span>复制</span>
                        </span>
                      )}
                    </button>
                  </div>
                  <pre className="remote-diff-pre">{diffStats.newText}</pre>
                </div>
              )}
            </div>
          ) : item.command ? (
            <div className="remote-tool-section">
              <div className="remote-tool-subhead-bar">
                <span className="remote-tool-subhead">命令 / 参数输入</span>
                <button
                  type="button"
                  className="remote-card-mini-copy"
                  onClick={(e) => handleCopy("cmd", item.command!, e)}
                >
                  {copiedSection === "cmd" ? (
                    <span className="remote-copied-badge">
                      <CheckIcon size={12} />
                      <span>已复制</span>
                    </span>
                  ) : (
                    <span className="remote-copy-badge">
                      <CopyIcon size={12} />
                      <span>复制</span>
                    </span>
                  )}
                </button>
              </div>
              <pre className="remote-tool-cmd">
                {readableRemoteText(item.command)}
              </pre>
            </div>
          ) : null}
          {item.output && !questionItems && (
            <div className="remote-tool-section">
              <div className="remote-tool-subhead-bar">
                <span className="remote-tool-subhead">执行结果输出</span>
                <button
                  type="button"
                  className="remote-card-mini-copy"
                  onClick={(e) => handleCopy("output", item.output!, e)}
                >
                  {copiedSection === "output" ? (
                    <span className="remote-copied-badge">
                      <CheckIcon size={12} />
                      <span>已复制</span>
                    </span>
                  ) : (
                    <span className="remote-copy-badge">
                      <CopyIcon size={12} />
                      <span>复制</span>
                    </span>
                  )}
                </button>
              </div>
              <pre className="remote-tool-output">
                {readableRemoteText(item.output)}
              </pre>
            </div>
          )}
        </div>
      )}
    </details>
  );
});
