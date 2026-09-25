import { Channel, invoke } from "@tauri-apps/api/core";
import {
  ArrowLeft,
  ArrowUp,
  ChevronLeft,
  ChevronRight,
  Eye,
  Paperclip,
  RefreshCw,
  Square,
  X,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { useAgentModels } from "../../agentModels";
import { cachedConversation, loadConversation } from "../../conversationCache";
import {
  hasActiveTurn,
  rememberTitleOverride,
  sessionDisplayTitle,
  sessionKey,
  statusView,
} from "../../presentation";
import {
  cancelSessionTurn,
  resolveSessionApproval,
  sendSessionMessage,
} from "../../sessionClient";
import {
  appendAssistantDelta,
  dropTrailingEmptyAssistant,
  replaceAssistantMessage,
  type ConversationMessage,
} from "../../turn-stream";
import {
  normalizeAgentType,
  type Session,
  type SessionTurnEvent,
} from "../../types";
import { SelectDropdown } from "../SelectDropdown";
import { errorMessage } from "../auth/AuthScreen";
import {
  ConversationItem,
  groupConversationMessages,
  ToolCallGroupBlock,
} from "../ConversationRenderer";
import { ComposerPlusMenu } from "./ComposerPlusMenu";
import { ContextCapacityRing } from "./ContextCapacityPopover";
import { ConversationLoadState } from "./ConversationLoadState";
import { isManagedProvider, PERMISSION_OPTIONS } from "./LaunchSessionModal";

export interface TurnOptionsState {
  model?: string;
  permission?: string;
}

export function sameConversation(
  a: ConversationMessage[],
  b: ConversationMessage[],
): boolean {
  return (
    a.length === b.length &&
    a.every(
      (message, index) =>
        message.role === b[index].role &&
        message.kind === b[index].kind &&
        message.status === b[index].status &&
        message.content === b[index].content,
    )
  );
}

export function SessionControlPanel({
  session,
  previousSession,
  nextSession,
  onNavigate,
  onClose,
}: {
  session: Session;
  previousSession?: Session;
  nextSession?: Session;
  onNavigate: (session: Session) => void;
  onClose: () => void;
}) {
  const [prompt, setPrompt] = useState("");
  const cacheKey = sessionKey(session);
  // Render from the preloaded cache immediately, then verify against the
  // agent transcript and refresh only when the content actually differs.
  const [messages, setMessages] = useState<ConversationMessage[]>(
    () =>
      cachedConversation(
        `${normalizeAgentType(session.agent_type)}:${session.agent_session_id}`,
      ) ?? [],
  );
  const [busy, setBusy] = useState(() =>
    hasActiveTurn(session.payload.state.activity_status),
  );
  const [cancelling, setCancelling] = useState(false);
  const [pendingApproval, setPendingApproval] = useState(
    session.payload.pending_approval ?? undefined,
  );
  const [approvalBusy, setApprovalBusy] = useState(false);
  const [agentActivity, setAgentActivity] = useState<string>();
  const [error, setError] = useState<string>();
  const [plusMenuOpen, setPlusMenuOpen] = useState(false);
  const [refreshingMessages, setRefreshingMessages] = useState(
    () =>
      "__TAURI_INTERNALS__" in window && isManagedProvider(session.agent_type),
  );
  const [conversationError, setConversationError] = useState<string>();
  const conversationRequest = useRef(0);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  const insertToken = (token: string) => {
    const el = textareaRef.current;
    if (!el) {
      setPrompt((prev) => prev + token);
      return;
    }
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    const val = el.value;
    const next = val.substring(0, start) + token + val.substring(end);
    setPrompt(next);
    setTimeout(() => {
      el.focus();
      const nextPos = start + token.length;
      el.setSelectionRange(nextPos, nextPos);
    }, 10);
  };

  const [attachments, setAttachments] = useState<
    Array<{ name: string; path: string }>
  >([]);
  const [turnOptions, setTurnOptions] = useState<
    Record<string, TurnOptionsState>
  >(() => {
    try {
      const stored = window.localStorage.getItem("agenthub:turn-options");
      return stored
        ? (JSON.parse(stored) as Record<string, TurnOptionsState>)
        : {};
    } catch {
      return {};
    }
  });
  const models = useAgentModels(session.agent_type);
  const permissionOptions = PERMISSION_OPTIONS[session.agent_type];
  const pendingDelta = useRef("");
  const deltaFrame = useRef<number | undefined>(undefined);
  const paneRef = useRef<HTMLElement | null>(null);
  const status = statusView(
    session.payload.state.activity_status,
    session.payload.state.presence,
  );
  const runtime = session.payload.runtime;
  const title = sessionDisplayTitle(session);
  const permission =
    session.agent_type === "codex"
      ? (runtime?.permission_mode ?? runtime?.approval_policy)
      : runtime?.permission_mode;
  const sessionOptions = turnOptions[cacheKey] ?? {};
  const currentModel =
    sessionOptions.model &&
    models.some((option) => option.value === sessionOptions.model)
      ? sessionOptions.model
      : (models[0]?.value ?? "");
  const currentPermission =
    sessionOptions.permission &&
    permissionOptions.some(
      (option: { value: string; label: string }) =>
        option.value === sessionOptions.permission,
    )
      ? sessionOptions.permission
      : "";

  useEffect(() => {
    setPendingApproval(session.payload.pending_approval ?? undefined);
  }, [session.payload.pending_approval]);

  useEffect(() => {
    setBusy(hasActiveTurn(session.payload.state.activity_status));
    if (!hasActiveTurn(session.payload.state.activity_status)) {
      setCancelling(false);
    }
  }, [session.payload.state.activity_status]);

  const updateTurnOptions = (patch: TurnOptionsState) => {
    setTurnOptions((current) => {
      const next = {
        ...current,
        [cacheKey]: { ...current[cacheKey], ...patch },
      };
      try {
        window.localStorage.setItem(
          "agenthub:turn-options",
          JSON.stringify(next),
        );
      } catch {
        // ignore
      }
      return next;
    });
  };

  const reloadMessages = useCallback(async () => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (!isManagedProvider(session.agent_type)) return;
    const request = ++conversationRequest.current;
    setRefreshingMessages(true);
    setConversationError(undefined);
    try {
      const next = await loadConversation(
        session.agent_type,
        session.agent_session_id,
        true,
      );
      if (request !== conversationRequest.current) return;
      setMessages((current) =>
        current.length > 0 && sameConversation(current, next) ? current : next,
      );
      const firstUser = next.find(
        (message) =>
          message.role === "user" &&
          message.kind === undefined &&
          message.content.trim().length >= 4,
      );
      if (firstUser && !session.payload.runtime?.title) {
        rememberTitleOverride(cacheKey, firstUser.content);
      }
    } catch (err) {
      if (request === conversationRequest.current)
        setConversationError(errorMessage(err));
    } finally {
      if (request === conversationRequest.current) setRefreshingMessages(false);
    }
  }, [
    cacheKey,
    session.agent_session_id,
    session.agent_type,
    session.payload.runtime?.title,
  ]);

  useEffect(() => {
    // Demo mode (no Tauri bridge) keeps the seeded sample transcript.
    if (!("__TAURI_INTERNALS__" in window)) return;
    // Discovery-only sessions expose bounded metadata, never full transcripts:
    // reading history is reserved for sessions that can be continued remotely.
    if (!isManagedProvider(session.agent_type)) {
      setMessages((current) => (current.length > 0 ? [] : current));
      return;
    }
    void reloadMessages();
    return () => {
      conversationRequest.current += 1;
    };
  }, [reloadMessages, session.agent_type]);

  // Active polling: auto-refresh conversation when busy or on last_activity_at change (with document visibility check)
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    if (!isManagedProvider(session.agent_type)) return;
    const intervalMs = busy ? 3_000 : 12_000;
    const timer = window.setInterval(() => {
      if (typeof document !== "undefined" && document.hidden) {
        return;
      }
      void reloadMessages();
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [
    busy,
    reloadMessages,
    session.agent_type,
    session.payload.last_activity_at,
  ]);

  useEffect(() => {
    const pane = paneRef.current;
    if (pane) {
      pane.scrollTop = pane.scrollHeight;
    }
  }, [messages, busy, agentActivity]);

  const flushDelta = useCallback(() => {
    deltaFrame.current = undefined;
    if (pendingDelta.current === "") return;
    const delta = pendingDelta.current;
    pendingDelta.current = "";
    setMessages((current) => appendAssistantDelta(current, delta));
  }, []);

  const handleAssistantDelta = useCallback(
    (delta: string) => {
      pendingDelta.current += delta;
      if (deltaFrame.current === undefined) {
        deltaFrame.current = window.requestAnimationFrame(flushDelta);
      }
    },
    [flushDelta],
  );

  const attachFiles = async () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      setError("演示模式不支持附件");
      return;
    }
    try {
      const picked = await invoke<string[]>("pick_files", {
        startDir: null,
      });
      setAttachments((current) => {
        const merged = [...current];
        for (const path of picked) {
          if (merged.some((item) => item.path === path)) continue;
          if (merged.length >= 8) break;
          const name = path.split(/[\\/]/).pop() || path;
          merged.push({ name, path });
        }
        return merged;
      });
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const sendMessage = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || prompt.trim() === "") return;

    // Safety pre-flight check: ensure the underlying session isn't running another turn
    if (hasActiveTurn(session.payload.state.activity_status)) {
      setBusy(true);
      setError("该会话正在执行上一轮任务，请等待执行完成或取消后再提交。");
      return;
    }

    const nextPrompt = prompt.trim();
    setPrompt("");
    setBusy(true);
    setError(undefined);
    setPendingApproval(undefined);
    setAgentActivity(undefined);
    const sentAttachments = attachments;
    setAttachments([]);
    setMessages((current) => [
      ...current,
      { role: "user", content: nextPrompt },
      { role: "assistant", content: "" },
    ]);
    try {
      const onEvent = new Channel<SessionTurnEvent>();
      onEvent.onmessage = (event) => {
        if (event.type === "assistant_delta") {
          handleAssistantDelta(event.text);
        } else if (event.type === "assistant_message") {
          if (deltaFrame.current !== undefined) {
            window.cancelAnimationFrame(deltaFrame.current);
            deltaFrame.current = undefined;
          }
          pendingDelta.current = "";
          setMessages((current) =>
            replaceAssistantMessage(current, event.text),
          );
        } else if (event.type === "activity") {
          setAgentActivity(event.label);
        } else if (event.type === "approval_requested") {
          setPendingApproval(event.approval);
          setAgentActivity("等待审批确认...");
        }
      };
      const result = await sendSessionMessage(
        {
          agent_type: session.agent_type,
          agent_session_id: session.agent_session_id,
          prompt: nextPrompt,
          model: currentModel || undefined,
          permission_mode: currentPermission || undefined,
          attachments: sentAttachments,
        },
        onEvent,
      );
      if (deltaFrame.current !== undefined) {
        window.cancelAnimationFrame(deltaFrame.current);
        deltaFrame.current = undefined;
      }
      pendingDelta.current = "";
      setMessages((current) =>
        replaceAssistantMessage(current, result.response),
      );
    } catch (nextError) {
      if (deltaFrame.current !== undefined) {
        window.cancelAnimationFrame(deltaFrame.current);
        deltaFrame.current = undefined;
      }
      pendingDelta.current = "";
      setMessages((current) => dropTrailingEmptyAssistant(current));
      setError(errorMessage(nextError));
      // Give the user a chance to retry with the same attachments.
      setAttachments((current) =>
        current.length === 0 && sentAttachments.length > 0
          ? sentAttachments
          : current,
      );
    } finally {
      setBusy(false);
      setCancelling(false);
      setAgentActivity(undefined);
      void loadConversation(session.agent_type, session.agent_session_id).catch(
        () => undefined,
      );
    }
  };

  const cancelTurn = async () => {
    if (!busy || cancelling) return;
    setCancelling(true);
    try {
      await cancelSessionTurn({
        agent_type: session.agent_type,
        agent_session_id: session.agent_session_id,
      });
    } catch (nextError) {
      setError(errorMessage(nextError));
      setCancelling(false);
    }
  };

  const resolveApproval = async (approved: boolean) => {
    if (!pendingApproval || approvalBusy) return;
    setApprovalBusy(true);
    setError(undefined);
    try {
      await resolveSessionApproval({
        agent_type: session.agent_type,
        agent_session_id: session.agent_session_id,
        approval_id: pendingApproval.approval_id,
        approved,
      });
      setPendingApproval(undefined);
      setAgentActivity(
        approved ? "审批已允许，继续执行..." : "审批已拒绝，等待收敛...",
      );
    } catch (nextError) {
      setError(errorMessage(nextError));
    } finally {
      setApprovalBusy(false);
    }
  };

  return (
    <div
      className="session-control-panel-scrim"
      role="region"
      aria-label={title}
    >
      <aside className="session-control-panel">
        <header>
          <div className="panel-nav">
            <button
              className="icon-button"
              aria-label="返回"
              onClick={onClose}
              title="返回工作区首页"
            >
              <ArrowLeft />
            </button>
            <span className="session-breadcrumb">
              <strong>{title}</strong>
              <small>
                {session.payload.project.display_name || "未分类项目"}
              </small>
            </span>
          </div>
          <div className="panel-heading-actions">
            <span className={"status-pill " + status.className}>
              <i />
              {status.label}
            </span>
            <button
              className={`icon-button ${refreshingMessages ? "rotating" : ""}`}
              aria-label="刷新消息"
              title="刷新消息流"
              disabled={refreshingMessages}
              onClick={() => void reloadMessages()}
            >
              <RefreshCw size={14} />
            </button>
            <button
              className="icon-button"
              aria-label="上一个会话"
              title="上一个会话"
              disabled={previousSession === undefined}
              onClick={() => previousSession && onNavigate(previousSession)}
            >
              <ChevronLeft />
            </button>
            <button
              className="icon-button"
              aria-label="下一个会话"
              title="下一个会话"
              disabled={nextSession === undefined}
              onClick={() => nextSession && onNavigate(nextSession)}
            >
              <ChevronRight />
            </button>
            <button
              className="icon-button"
              aria-label="关闭面板"
              onClick={onClose}
              title="关闭会话"
            >
              <X />
            </button>
          </div>
        </header>

        <section className="conversation-pane" ref={paneRef} aria-live="polite">
          {messages.length === 0 ? (
            <ConversationLoadState
              loading={refreshingMessages}
              error={conversationError}
              managed={isManagedProvider(session.agent_type)}
              onRetry={() => void reloadMessages()}
            />
          ) : (
            groupConversationMessages(messages).map((group, groupIdx) => {
              if (group.type === "tool_group") {
                return (
                  <ToolCallGroupBlock
                    key={`tool-group-${group.startIndex}-${groupIdx}`}
                    messages={group.messages}
                  />
                );
              }
              const message = group.message;
              const index = group.index;
              return (
                <ConversationItem
                  message={message}
                  index={index}
                  key={
                    message.role +
                    "-" +
                    (message.kind ?? "message") +
                    "-" +
                    index
                  }
                />
              );
            })
          )}
          {busy && agentActivity !== undefined && (
            <div className="agent-thinking">
              <RefreshCw className="spin" /> {agentActivity}
            </div>
          )}
        </section>

        {pendingApproval !== undefined && (
          <div
            className="approval-actions"
            role="group"
            aria-label="Codex 审批操作"
          >
            <span>Codex 正在等待权限确认</span>
            <button
              type="button"
              disabled={approvalBusy || !pendingApproval.can_decline}
              onClick={() => void resolveApproval(false)}
            >
              拒绝
            </button>
            <button
              type="button"
              className="primary"
              disabled={approvalBusy || !pendingApproval.can_approve}
              onClick={() => void resolveApproval(true)}
            >
              允许
            </button>
          </div>
        )}
        {messages.length > 0 && conversationError && (
          <p className="panel-error" role="alert">
            历史消息刷新失败，当前保留上次内容：{conversationError}
          </p>
        )}
        {error !== undefined && (
          <p className="panel-error" role="alert">
            {error}
          </p>
        )}

        <div className="composer-container">
          {!isManagedProvider(session.agent_type) ? (
            <div className="provider-readonly-notice" role="status">
              <Eye size={14} />
              <span>该 Provider 尚未接入远程控制，暂不能发送消息</span>
            </div>
          ) : (
            <>
              {attachments.length > 0 && (
                <div className="attachment-row">
                  {attachments.map((file) => (
                    <span
                      className="attachment-chip"
                      key={file.path}
                      title={file.path}
                    >
                      <Paperclip size={11} />
                      <span className="attachment-name">{file.name}</span>
                      <button
                        type="button"
                        aria-label={"移除附件 " + file.name}
                        onClick={() =>
                          setAttachments((current) =>
                            current.filter((item) => item.path !== file.path),
                          )
                        }
                      >
                        <X size={11} />
                      </button>
                    </span>
                  ))}
                </div>
              )}
              <form
                className="session-composer modern-composer"
                onSubmit={sendMessage}
              >
                <textarea
                  ref={textareaRef}
                  aria-label="发送给 Agent"
                  maxLength={16_000}
                  placeholder="继续这个会话… (Enter 发送，Shift+Enter 换行)"
                  value={prompt}
                  onChange={(event) => setPrompt(event.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.shiftKey) {
                      e.preventDefault();
                      void sendMessage(e);
                    }
                  }}
                  disabled={busy}
                />
                <div className="composer-toolbar">
                  <div className="composer-plus-wrapper">
                    <button
                      type="button"
                      className="composer-plus-btn"
                      onClick={() => setPlusMenuOpen((prev) => !prev)}
                      aria-label="添加附件与常用词"
                    >
                      +
                    </button>
                    <ComposerPlusMenu
                      isOpen={plusMenuOpen}
                      onClose={() => setPlusMenuOpen(false)}
                      onAttachFiles={() => void attachFiles()}
                      onInsertToken={insertToken}
                    />
                  </div>
                  <SelectDropdown
                    className="ghost"
                    value={currentModel}
                    options={models.map((option) => ({
                      value: option.value,
                      label: option.label,
                      desc: option.desc,
                    }))}
                    onChange={(val) => updateTurnOptions({ model: val })}
                    size="sm"
                    ariaLabel="会话模型"
                  />
                  {permissionOptions.length > 1 && (
                    <SelectDropdown
                      className="ghost"
                      value={currentPermission}
                      options={permissionOptions}
                      onChange={(val) => updateTurnOptions({ permission: val })}
                      size="sm"
                      ariaLabel="权限模式"
                    />
                  )}
                  <span className="composer-spacer" />
                  <ContextCapacityRing
                    runtime={runtime}
                    messages={messages}
                    modelName={currentModel}
                  />
                  {busy ? (
                    <button
                      className="composer-send turn-cancel-btn"
                      type="button"
                      disabled={cancelling}
                      onClick={() => void cancelTurn()}
                      title="停止当前生成"
                      aria-label="停止"
                    >
                      <Square size={14} />
                    </button>
                  ) : (
                    <button
                      className="composer-send"
                      type="submit"
                      aria-label="发送"
                      disabled={busy || prompt.trim() === ""}
                    >
                      <ArrowUp size={15} />
                    </button>
                  )}
                </div>
              </form>
            </>
          )}
        </div>
      </aside>
    </div>
  );
}
