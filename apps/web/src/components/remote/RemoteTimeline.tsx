import type { RefObject } from "react";
import type {
  RemotePendingApproval,
  RemotePendingQuestion,
} from "@agent-pulse/contracts/remote";
import {
  cleanBannerText,
  remoteError,
  type ConversationGroup,
  type ConversationItem,
  type DiscoveryCandidate,
  type OptimisticPrompt,
  type RemoteCommandRecord,
  type RemoteSession,
  type TurnFailure,
} from "../../lib/remote";
import {
  formatFallbackTitle,
  getProviderInfo,
  type PluginTreeHint,
} from "../../lib/session-tree";
import { ConversationSkeleton } from "../ConversationSkeleton";
import { ErrorBoundary } from "../ErrorBoundary";
import { RemoteMessageCard } from "../RemoteMessageCard";
import { RemoteToolCard } from "../RemoteToolCard";
import { RemoteApprovalCard } from "./RemoteApprovalCard";
import { RemoteQuestionCard } from "./RemoteQuestionCard";
import { RemoteToolGroup } from "./RemoteToolGroup";

export interface RemoteTimelineProps {
  timelineRef: RefObject<HTMLDivElement | null>;
  session?: RemoteSession | undefined;
  discovered: Map<string, DiscoveryCandidate>;
  allKnownPlugins?: ReadonlyArray<PluginTreeHint> | undefined;
  connected: boolean;
  codexSession: boolean;
  releasingControl: boolean;
  busy: boolean;
  sessionLoading: boolean;
  itemsCount: number;
  effectiveItems: ConversationItem[];
  itemGroups: ConversationGroup[];
  isTurnExecuting: boolean;
  isWaitingFirstAssistant: boolean;
  optimisticPrompts: OptimisticPrompt[];
  hasInFlightPrompt: boolean;
  turnFailure: TurnFailure | null;
  latestFailure?: RemoteCommandRecord | undefined;
  dismissedFailure: string | null;
  approvals: RemotePendingApproval[];
  questions: RemotePendingQuestion[];
  submittingApproval: { id: string; action: "approve" | "decline" } | null;
  submittingQuestionId: string | null;
  followBottom: boolean;
  onScroll: () => void;
  onScrollToBottom: () => void;
  onDismissFailure: (text: string) => void;
  onReconnect: (session: RemoteSession) => void;
  onReleaseControl: () => void;
  onResolveApproval: (approvalId: string, approved: boolean) => void;
  onAnswerQuestion: (
    questionId: string,
    answer: string | string[],
  ) => Promise<boolean> | void;
}

export function RemoteTimeline({
  timelineRef,
  session,
  discovered,
  allKnownPlugins,
  connected,
  codexSession,
  releasingControl,
  busy,
  sessionLoading,
  itemsCount,
  effectiveItems,
  itemGroups,
  isTurnExecuting,
  isWaitingFirstAssistant,
  optimisticPrompts,
  hasInFlightPrompt,
  turnFailure,
  latestFailure,
  dismissedFailure,
  approvals,
  questions,
  submittingApproval,
  submittingQuestionId,
  followBottom,
  onScroll,
  onScrollToBottom,
  onDismissFailure,
  onReconnect,
  onReleaseControl,
  onResolveApproval,
  onAnswerQuestion,
}: RemoteTimelineProps) {
  const sessionProviderName = session
    ? getProviderInfo(
        session.binding.provider_session_id,
        discovered.get(
          `${session.runner_id}/${session.binding.provider_session_id}`,
        )?.provider ?? session.provider,
        allKnownPlugins,
      ).name
    : getProviderInfo("", undefined, allKnownPlugins).name;

  const failText =
    !isTurnExecuting &&
    (turnFailure && !turnFailure.retrying
      ? cleanBannerText(turnFailure.message)
      : latestFailure
        ? remoteError(new Error(latestFailure.receipt?.code ?? ""))
        : session?.binding.execution_state === "error"
          ? "上一轮执行异常中断，请检查当前客户端模型或网关配置后重试。"
          : "");

  const lastEffectiveItem = effectiveItems[effectiveItems.length - 1];

  return (
    <>
      {session && !connected && (
        <div className="remote-notice session-control">
          <span>
            {releasingControl
              ? "正在归还控制…"
              : session.online
                ? "正在只读查看会话"
                : "电脑离线，可查看已同步的历史"}
          </span>
          {session.online && !releasingControl && (
            <button disabled={busy} onClick={() => onReconnect(session)}>
              接管会话
            </button>
          )}
        </div>
      )}
      {codexSession &&
        connected &&
        !isTurnExecuting &&
        approvals.length === 0 &&
        questions.length === 0 && (
          <div className="remote-notice session-control">
            <span>AgentHub 已接管会话</span>
            <button disabled={busy} onClick={onReleaseControl}>
              归还控制
            </button>
          </div>
        )}
      <div
        className="remote-timeline cds-scroll-fade-mask"
        ref={timelineRef}
        onScroll={onScroll}
        aria-label="会话内容"
      >
        {itemGroups.map((group) => {
          if (group.type === "tool_group") {
            const groupKey = group.items[0]?.id;
            return (
              <ErrorBoundary key={groupKey} scope="item" resetKey={groupKey}>
                <RemoteToolGroup items={group.items} />
              </ErrorBoundary>
            );
          }
          const item = group.item;
          if (item.kind === "tool") {
            return (
              <ErrorBoundary key={item.id} scope="item" resetKey={item.id}>
                <RemoteToolCard item={item} />
              </ErrorBoundary>
            );
          }

          const isStreamingThis = Boolean(
            item.kind === "assistant" &&
            session?.binding.current_turn_id &&
            item === lastEffectiveItem,
          );
          const providerName =
            item.kind !== "user" ? sessionProviderName : undefined;

          return (
            <ErrorBoundary key={item.id} scope="item" resetKey={item.id}>
              <RemoteMessageCard
                item={item}
                isStreamingThis={isStreamingThis}
                providerName={providerName}
              />
            </ErrorBoundary>
          );
        })}

        {isTurnExecuting &&
          approvals.length === 0 &&
          questions.length === 0 &&
          (isWaitingFirstAssistant ||
            (lastEffectiveItem?.kind === "tool" &&
              lastEffectiveItem?.status === "running")) && (
            <div className="remote-turn-live" role="status" aria-live="polite">
              <div className="remote-turn-live-main">
                <span className="remote-turn-live-dots" aria-hidden="true">
                  <i />
                  <i />
                  <i />
                </span>
                <span className="remote-turn-live-label">
                  {optimisticPrompts.length > 0 ||
                  (hasInFlightPrompt && !session?.binding.current_turn_id)
                    ? "已发送，正在等待响应…"
                    : lastEffectiveItem?.kind === "tool" &&
                        lastEffectiveItem?.status === "running"
                      ? `正在执行 · ${lastEffectiveItem?.title || "工具调用"}`
                      : "正在思考并生成回复…"}
                </span>
                {session && (
                  <span className="remote-turn-live-provider">
                    {sessionProviderName}
                  </span>
                )}
              </div>
            </div>
          )}

        {failText && failText !== dismissedFailure && (
          <div className="remote-timeline-error" role="alert">
            <div className="remote-timeline-error-body">
              <strong>
                {latestFailure?.receipt?.code === "native_writer_busy"
                  ? "接管失败"
                  : "本轮执行未完成"}
              </strong>
              <span>{failText}</span>
            </div>
            <button
              type="button"
              aria-label="关闭提示"
              onClick={() => onDismissFailure(failText)}
            >
              ×
            </button>
          </div>
        )}

        {!session || ((sessionLoading || busy) && itemsCount === 0) ? (
          <ConversationSkeleton label="正在加载会话内容…" />
        ) : itemsCount === 0 ? (
          <div className="remote-timeline-empty">
            <div className="remote-timeline-empty-glyph" aria-hidden="true">
              <svg
                width="26"
                height="26"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
              </svg>
            </div>
            <h3 className="remote-timeline-empty-title">
              {discovered.get(
                `${session.runner_id}/${session.binding.provider_session_id}`,
              )?.title || formatFallbackTitle()}
            </h3>
            <p className="remote-timeline-empty-desc">
              {session.online ? "暂无已同步的消息" : "电脑离线"}
            </p>
          </div>
        ) : null}

        {approvals.map((approval) => (
          <ErrorBoundary
            key={approval.approval_id}
            scope="item"
            resetKey={approval.approval_id}
          >
            <RemoteApprovalCard
              approval={approval}
              providerName={sessionProviderName}
              connected={connected}
              busy={busy}
              submittingApproval={submittingApproval}
              onResolve={onResolveApproval}
            />
          </ErrorBoundary>
        ))}

        {questions.map((q) => (
          <ErrorBoundary
            key={q.question_id}
            scope="item"
            resetKey={q.question_id}
          >
            <RemoteQuestionCard
              question={q}
              connected={connected}
              busy={busy}
              isSubmitting={submittingQuestionId === q.question_id}
              onAnswer={onAnswerQuestion}
            />
          </ErrorBoundary>
        ))}
      </div>

      {!followBottom && itemsCount > 0 && (
        <button
          type="button"
          className="remote-scroll-bottom-btn"
          onClick={onScrollToBottom}
          aria-label="回到最新消息"
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="6 9 12 15 18 9" />
          </svg>
          <span>回到最新</span>
        </button>
      )}
    </>
  );
}
