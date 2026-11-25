import { useState } from "react";
import type { RemotePendingQuestion } from "@agent-pulse/contracts/remote";
import { CheckIcon } from "../RemoteMessageCard";

export interface RemoteQuestionCardProps {
  question: RemotePendingQuestion;
  connected: boolean;
  busy: boolean;
  isSubmitting: boolean;
  onAnswer: (
    questionId: string,
    answer: string | string[],
  ) => Promise<boolean> | void;
}

export function RemoteQuestionCard({
  question: q,
  connected,
  busy,
  isSubmitting,
  onAnswer,
}: RemoteQuestionCardProps) {
  const [customAnswer, setCustomAnswer] = useState("");
  const [selectedOptions, setSelectedOptions] = useState<string[]>([]);
  const isMulti = Boolean(q.multi_select);

  const submitCustomAnswer = async () => {
    const ans = customAnswer.trim();
    if (!ans || !connected || busy || isSubmitting) return;
    const accepted = await onAnswer(q.question_id, ans);
    if (accepted === true) {
      setCustomAnswer("");
    }
  };

  return (
    <section className="remote-question mcp-tool-decision-card state-pending-question">
      <div className="remote-question-header">
        <div className="remote-question-title-wrap">
          <span className="remote-question-glyph" aria-hidden="true">
            ?
          </span>
          <span>{q.header ?? "Agent 向你提问"}</span>
        </div>
        <div className="remote-question-meta-right">
          {isMulti ? (
            <span className="remote-question-type-badge">可多选</span>
          ) : (
            <span className="remote-question-type-badge single">单项选择</span>
          )}
          <span className="remote-approval-awaiting-pill">
            <span className="awaiting-dot question" />
            等待答复
          </span>
        </div>
      </div>

      <div className="remote-question-prompt">{q.prompt}</div>

      {q.options && q.options.length > 0 && (
        <div className="remote-question-options">
          {q.options.map((opt, idx) => {
            const isOptionSelected = selectedOptions.includes(opt.label);
            return (
              <button
                key={idx}
                type="button"
                className={`remote-option-btn ${
                  isOptionSelected ? "selected" : ""
                }`}
                disabled={!connected || busy || isSubmitting}
                onClick={() => {
                  if (isMulti) {
                    setSelectedOptions((prev) =>
                      prev.includes(opt.label)
                        ? prev.filter((item) => item !== opt.label)
                        : [...prev, opt.label],
                    );
                  } else {
                    onAnswer(q.question_id, opt.label);
                  }
                }}
              >
                <div className="remote-option-top-row">
                  {isMulti && (
                    <span
                      className={`remote-option-check ${
                        isOptionSelected ? "checked" : ""
                      }`}
                      aria-hidden="true"
                    >
                      {isOptionSelected ? <CheckIcon size={12} /> : null}
                    </span>
                  )}
                  <strong>{opt.label}</strong>
                </div>
                {opt.description && (
                  <span className="remote-option-desc">{opt.description}</span>
                )}
                {opt.preview && (
                  <pre className="remote-option-preview">{opt.preview}</pre>
                )}
              </button>
            );
          })}
        </div>
      )}

      {isMulti && (
        <div className="remote-question-multi-actions">
          <button
            type="button"
            className="remote-primary remote-question-submit-btn"
            disabled={
              !connected || busy || isSubmitting || selectedOptions.length === 0
            }
            onClick={() => {
              if (selectedOptions.length > 0) {
                void Promise.resolve(
                  onAnswer(q.question_id, selectedOptions),
                ).then((accepted) => {
                  if (accepted === true) setSelectedOptions([]);
                });
              }
            }}
          >
            {isSubmitting
              ? "提交中…"
              : `确认已选项目 (${selectedOptions.length})`}
          </button>
        </div>
      )}

      <div className="remote-question-input-row">
        <input
          type="text"
          className="remote-question-input"
          placeholder="或者输入自定义回答..."
          disabled={!connected || busy || isSubmitting}
          value={customAnswer}
          onChange={(e) => setCustomAnswer(e.target.value)}
          onKeyDown={(e) => {
            if (
              e.key === "Enter" &&
              !e.nativeEvent.isComposing &&
              e.keyCode !== 229
            ) {
              e.preventDefault();
              void submitCustomAnswer();
            }
          }}
        />
        <button
          type="button"
          className="remote-primary"
          disabled={!connected || busy || isSubmitting || !customAnswer.trim()}
          onClick={() => void submitCustomAnswer()}
        >
          {isSubmitting ? "提交中…" : "提交回答"}
        </button>
      </div>
    </section>
  );
}
