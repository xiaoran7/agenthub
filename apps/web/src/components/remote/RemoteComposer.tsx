import type { RefObject } from "react";
import type {
  DiscoveryCandidate,
  RemoteAttachment,
  RemoteEvent,
  RemotePermissionMode,
  RemoteSession,
} from "../../lib/remote-types";
import { QueuedPromptBar, type QueuedPrompt } from "../QueuedPromptBar";
import {
  ChevronDownIcon,
  EFFORT_OPTIONS,
  PERMISSION_MODE_OPTIONS,
  QUICK_PROMPT_SNIPPETS,
} from "../remote-console-constants";
import { RemoteContextCapacity } from "../RemoteContextCapacity";
import { Select } from "../Select";
import type { EffortLevel } from "./useRemoteComposer";

export interface RemoteComposerProps {
  session?: RemoteSession | undefined;
  events: RemoteEvent[];
  candidate?: DiscoveryCandidate | undefined;
  itemsCount: number;
  isMobile: boolean;
  connected: boolean;
  canTakeControl: boolean;
  canSend: boolean;
  busy: boolean;
  isTurnExecuting: boolean;
  submittingInterrupt: boolean;
  text: string;
  setText: React.Dispatch<React.SetStateAction<string>>;
  attachments: RemoteAttachment[];
  onRemoveAttachment: (index: number) => void;
  activeQueuedPrompt?: QueuedPrompt | undefined;
  onSendQueuedNow: () => void;
  onEditQueued: () => void;
  onCancelQueued: () => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  fileInputRef: RefObject<HTMLInputElement | null>;
  attachMenuRef: RefObject<HTMLDivElement | null>;
  attachMenuOpen: boolean;
  setAttachMenuOpen: React.Dispatch<React.SetStateAction<boolean>>;
  attachQuickPromptsOpen: boolean;
  setAttachQuickPromptsOpen: React.Dispatch<React.SetStateAction<boolean>>;
  permissionMode: RemotePermissionMode;
  onPermissionModeChange: (mode: RemotePermissionMode) => void;
  sessionModel: string;
  sessionModelOptions: Array<{ value: string; label: string }>;
  onSessionModelChange: (model: string) => void;
  effort: EffortLevel;
  onEffortChange: (effort: EffortLevel) => void;
  adjustTextareaHeight: () => void;
  onFilesSelected: (files: FileList | File[]) => void;
  onPasteFromClipboard: () => void;
  onInsertCodeSnippet: () => void;
  onSendCurrentPrompt: () => void;
  onEnqueueCurrentPrompt: () => void;
  onInterrupt: () => void;
}

export function RemoteComposer({
  session,
  events,
  candidate,
  itemsCount,
  isMobile,
  connected,
  canTakeControl,
  canSend,
  busy,
  isTurnExecuting,
  submittingInterrupt,
  text,
  setText,
  attachments,
  onRemoveAttachment,
  activeQueuedPrompt,
  onSendQueuedNow,
  onEditQueued,
  onCancelQueued,
  textareaRef,
  fileInputRef,
  attachMenuRef,
  attachMenuOpen,
  setAttachMenuOpen,
  attachQuickPromptsOpen,
  setAttachQuickPromptsOpen,
  permissionMode,
  onPermissionModeChange,
  sessionModel,
  sessionModelOptions,
  onSessionModelChange,
  effort,
  onEffortChange,
  adjustTextareaHeight,
  onFilesSelected,
  onPasteFromClipboard,
  onInsertCodeSnippet,
  onSendCurrentPrompt,
  onEnqueueCurrentPrompt,
  onInterrupt,
}: RemoteComposerProps) {
  return (
    <form
      className="remote-composer-card"
      onSubmit={(event) => {
        event.preventDefault();
        onSendCurrentPrompt();
      }}
    >
      {activeQueuedPrompt && (
        <QueuedPromptBar
          queued={activeQueuedPrompt}
          canSendNow={Boolean(connected && canSend && !isTurnExecuting)}
          onSendNow={onSendQueuedNow}
          onEdit={onEditQueued}
          onCancel={onCancelQueued}
        />
      )}

      {attachments.length > 0 && (
        <div className="remote-composer-attachments" aria-label="已选附件">
          {attachments.map((att, idx) => (
            <div className="remote-attachment-chip" key={`${att.name}-${idx}`}>
              {att.encoding === "base64" &&
              att.data.length > 0 &&
              att.media_type.startsWith("image/") ? (
                <img
                  className="remote-attachment-thumb"
                  src={`data:${att.media_type};base64,${att.data}`}
                  alt={att.name}
                />
              ) : (
                <span className="remote-attachment-icon" aria-hidden="true">
                  <svg
                    width="13"
                    height="13"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                    <polyline points="14 2 14 8 20 8" />
                  </svg>
                </span>
              )}
              <span className="remote-attachment-name" title={att.name}>
                {att.name}
              </span>
              <button
                type="button"
                className="remote-attachment-remove"
                aria-label={`移除附件 ${att.name}`}
                onClick={() => onRemoveAttachment(idx)}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="remote-composer-input-wrapper">
        <textarea
          ref={textareaRef}
          id="remote-prompt"
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            adjustTextareaHeight();
          }}
          onPaste={(event) => {
            const pastedFiles = event.clipboardData?.files;
            if (pastedFiles && pastedFiles.length > 0) {
              event.preventDefault();
              onFilesSelected(pastedFiles);
            }
          }}
          onKeyDown={(event) => {
            if (
              (event.ctrlKey || event.metaKey) &&
              event.key.toLowerCase() === "u"
            ) {
              event.preventDefault();
              if ((connected || canTakeControl) && !busy) {
                fileInputRef.current?.click();
              }
              return;
            }
            const isComposing =
              (event.nativeEvent as KeyboardEvent).isComposing ||
              event.keyCode === 229;
            // 移动端不通过回车触发发送；桌面端双重互锁防输入法选词误发
            if (
              !isMobile &&
              event.key === "Enter" &&
              !event.shiftKey &&
              !isComposing
            ) {
              event.preventDefault();
              onSendCurrentPrompt();
            }
          }}
          placeholder={
            !session
              ? "等待电脑确认会话…"
              : !session.online
                ? "电脑离线"
                : !connected
                  ? canTakeControl
                    ? "输入消息，发送时接管会话"
                    : "等待会话可用"
                  : isTurnExecuting
                    ? isMobile
                      ? '输入下一条消息，点"排队"加入待发…'
                      : "输入下一条消息，按 Enter 加入待发队列…"
                    : itemsCount === 0
                      ? "输入指令开始新对话…"
                      : "继续这段工作…"
          }
          rows={1}
          disabled={!connected && !canTakeControl}
        />
      </div>

      <div className="remote-composer-toolbar">
        <div className="remote-composer-left">
          <input
            ref={fileInputRef}
            type="file"
            multiple
            hidden
            onChange={(event) => {
              if (event.target.files) {
                onFilesSelected(event.target.files);
                event.target.value = "";
              }
            }}
          />
          <div className="remote-attach-menu-wrap" ref={attachMenuRef}>
            <button
              type="button"
              className={`remote-attach-btn ${attachMenuOpen ? "open" : ""}`}
              title="添加内容或附件"
              aria-label="添加内容或附件"
              aria-haspopup="menu"
              aria-expanded={attachMenuOpen}
              disabled={!connected || busy}
              onClick={() => {
                setAttachMenuOpen((prev) => {
                  if (prev) setAttachQuickPromptsOpen(false);
                  return !prev;
                });
              }}
            >
              <svg
                width="17"
                height="17"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <line x1="12" y1="5" x2="12" y2="19" />
                <line x1="5" y1="12" x2="19" y2="12" />
              </svg>
            </button>
            {attachMenuOpen && (
              <div
                className="remote-attach-popover"
                role="menu"
                aria-label="添加附件或快捷内容"
              >
                <button
                  type="button"
                  role="menuitem"
                  className="remote-attach-menu-item"
                  onClick={() => {
                    setAttachMenuOpen(false);
                    setAttachQuickPromptsOpen(false);
                    fileInputRef.current?.click();
                  }}
                >
                  <span className="remote-attach-menu-icon" aria-hidden="true">
                    <svg
                      width="15"
                      height="15"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.9"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" />
                    </svg>
                  </span>
                  <span className="remote-attach-menu-label">
                    添加文件或图片
                  </span>
                  <kbd className="remote-attach-menu-kbd">Ctrl+U</kbd>
                </button>
                <button
                  type="button"
                  role="menuitem"
                  className="remote-attach-menu-item"
                  onClick={onPasteFromClipboard}
                >
                  <span className="remote-attach-menu-icon" aria-hidden="true">
                    <svg
                      width="15"
                      height="15"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.9"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <rect x="9" y="2" width="6" height="4" rx="1" ry="1" />
                      <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
                    </svg>
                  </span>
                  <span className="remote-attach-menu-label">从剪贴板粘贴</span>
                </button>
                <button
                  type="button"
                  role="menuitem"
                  className="remote-attach-menu-item"
                  onClick={onInsertCodeSnippet}
                >
                  <span className="remote-attach-menu-icon" aria-hidden="true">
                    <svg
                      width="15"
                      height="15"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.9"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <polyline points="16 18 22 12 16 6" />
                      <polyline points="8 6 2 12 8 18" />
                    </svg>
                  </span>
                  <span className="remote-attach-menu-label">插入代码片段</span>
                </button>
                <div className="remote-attach-menu-divider" />
                <button
                  type="button"
                  role="menuitem"
                  aria-expanded={attachQuickPromptsOpen}
                  className={`remote-attach-menu-item ${attachQuickPromptsOpen ? "active" : ""}`}
                  onClick={() => setAttachQuickPromptsOpen((prev) => !prev)}
                >
                  <span className="remote-attach-menu-icon" aria-hidden="true">
                    <svg
                      width="15"
                      height="15"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.9"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" />
                    </svg>
                  </span>
                  <span className="remote-attach-menu-label">快捷指令</span>
                  <ChevronDownIcon
                    size={12}
                    className={`remote-attach-chevron ${attachQuickPromptsOpen ? "open" : ""}`}
                  />
                </button>
                {attachQuickPromptsOpen && (
                  <div className="remote-attach-sublist">
                    {QUICK_PROMPT_SNIPPETS.map((snippet) => (
                      <button
                        key={snippet.label}
                        type="button"
                        role="menuitem"
                        className="remote-attach-subitem"
                        onClick={() => {
                          setAttachMenuOpen(false);
                          setAttachQuickPromptsOpen(false);
                          setText((prev) =>
                            prev ? `${prev}\n${snippet.text}` : snippet.text,
                          );
                          requestAnimationFrame(() => {
                            adjustTextareaHeight();
                            textareaRef.current?.focus();
                          });
                        }}
                      >
                        {snippet.label}
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
          <div
            className="remote-permission-selector"
            title="设置当前会话执行权限模式"
          >
            <Select
              variant="compact"
              placement="top"
              label="权限模式"
              value={permissionMode}
              disabled={!connected || busy}
              options={PERMISSION_MODE_OPTIONS}
              onChange={(next) =>
                onPermissionModeChange(next as RemotePermissionMode)
              }
            />
          </div>
        </div>

        <div className="remote-composer-actions">
          <div className="remote-model-selector" title="选择本轮对话模型">
            <Select
              variant="compact"
              placement="top"
              label="会话模型"
              value={sessionModel}
              disabled={!connected || busy}
              options={sessionModelOptions}
              onChange={onSessionModelChange}
            />
          </div>
          <div className="remote-effort-selector" title="设置本轮思考推理深度">
            <Select
              variant="compact"
              placement="top"
              label="推理思考深度"
              value={effort}
              disabled={!connected || busy}
              options={EFFORT_OPTIONS}
              onChange={(next) => onEffortChange(next as EffortLevel)}
            />
          </div>
          <RemoteContextCapacity events={events} candidate={candidate} />
          {/* One control for the send slot, not two buttons side by
              side: while a turn is running the slot *becomes* stop. */}
          {isTurnExecuting ? (
            <div className="remote-live-action-pair">
              {(text.trim() || attachments.length > 0) && (
                <button
                  type="button"
                  className="remote-queue-btn"
                  title="加入待发队列，当前轮次完成后自动发送"
                  disabled={!connected || busy}
                  onClick={onEnqueueCurrentPrompt}
                >
                  排队
                </button>
              )}
              {session?.binding.current_turn_id ? (
                <button
                  type="button"
                  className="remote-interrupt-btn"
                  title="停止这一轮"
                  disabled={!connected || busy || submittingInterrupt}
                  onClick={onInterrupt}
                >
                  <svg
                    width="12"
                    height="12"
                    viewBox="0 0 24 24"
                    fill="currentColor"
                  >
                    <rect x="4" y="4" width="16" height="16" rx="2" />
                  </svg>
                  <span>{submittingInterrupt ? "停止中…" : "停止"}</span>
                </button>
              ) : null}
            </div>
          ) : (
            <button
              type="submit"
              className={
                canTakeControl
                  ? "remote-send-btn remote-send-takeover"
                  : "remote-send-btn"
              }
              title={
                canTakeControl
                  ? "接管并发送"
                  : !connected
                    ? "等待会话连接"
                    : text.trim() || attachments.length > 0
                      ? "发送消息 (Enter)"
                      : "请输入消息内容"
              }
              aria-label={
                canTakeControl
                  ? "接管并发送"
                  : text.trim() || attachments.length > 0
                    ? "发送消息"
                    : "请输入消息内容"
              }
              disabled={
                !canSend || busy || (!text.trim() && attachments.length === 0)
              }
            >
              {canTakeControl && <span>接管并发送</span>}
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <line x1="12" y1="19" x2="12" y2="5" />
                <polyline points="5 12 12 5 19 12" />
              </svg>
            </button>
          )}
        </div>
      </div>
    </form>
  );
}
