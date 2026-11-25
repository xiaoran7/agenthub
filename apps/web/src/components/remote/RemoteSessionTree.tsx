import type { RemoteSession } from "../../lib/remote-types";
import {
  workspaceTooltip,
  type ProviderKind,
  type TreeSessionItem,
} from "../../lib/session-tree";
import {
  ChevronDownIcon,
  formatSessionTokens,
} from "../remote-console-constants";
import { CheckIcon, CopyIcon } from "../RemoteMessageCard";
import { isCodexThreadId, sessionKey } from "./remote-model-options";

const INITIAL_VISIBLE_COUNT = 5;
const STEP_COUNT = 5;

export interface WorkspaceSessionGroup {
  id: string;
  label: string;
  items: TreeSessionItem[];
}

export interface RemoteSessionTreeProps {
  activeRunnerId: string;
  isActiveRunnerHelloPending: boolean;
  hasAvailableAgents: boolean;
  activeDeviceOnline: boolean;
  currentAgentName: string;
  providerFilter: ProviderKind;
  sessionSearchQuery: string;
  allWorkspacesForDevice: WorkspaceSessionGroup[];
  sessions: RemoteSession[];
  selected: string;
  busy: boolean;
  resumingCandidateKey: string | null;
  copiedSessionId: string | null;
  collapsedWorkspaces: Record<string, boolean>;
  workspaceVisibleCounts: Record<string, number>;
  pendingApprovalCounts: Map<string, number>;
  supportsCodexControl: (runnerId: string) => boolean;
  onToggleWorkspace: (workspaceKey: string) => void;
  onShowMore: (
    workspaceKey: string,
    initialVisibleCount: number,
    stepCount: number,
  ) => void;
  onOpenLaunchModal: (preferredWorkspace?: string) => void;
  onSelectActiveSession: (sessionKey: string) => void;
  onResumeCandidate: (
    runnerId: string,
    workspaceId: string,
    providerSessionId: string,
  ) => void;
  onCopySessionId: (id: string, e: React.MouseEvent) => void;
}

export function RemoteSessionTree({
  activeRunnerId,
  isActiveRunnerHelloPending,
  hasAvailableAgents,
  activeDeviceOnline,
  currentAgentName,
  providerFilter,
  sessionSearchQuery,
  allWorkspacesForDevice,
  sessions,
  selected,
  busy,
  resumingCandidateKey,
  copiedSessionId,
  collapsedWorkspaces,
  workspaceVisibleCounts,
  pendingApprovalCounts,
  supportsCodexControl,
  onToggleWorkspace,
  onShowMore,
  onOpenLaunchModal,
  onSelectActiveSession,
  onResumeCandidate,
  onCopySessionId,
}: RemoteSessionTreeProps) {
  return (
    <div className="remote-session-tree">
      {!activeRunnerId ? (
        <div className="remote-tree-empty-card">
          <p className="remote-tree-empty-text">请先在上方选择电脑</p>
        </div>
      ) : isActiveRunnerHelloPending ? (
        <div className="remote-tree-empty-card">
          <p className="remote-tree-empty-text">正在同步会话列表…</p>
        </div>
      ) : !hasAvailableAgents ? (
        <div className="remote-tree-empty-card">
          <p className="remote-tree-empty-text">
            当前电脑未检测到 Agent 运行环境
          </p>
        </div>
      ) : allWorkspacesForDevice.length === 0 ? (
        <div className="remote-tree-empty-card">
          <p className="remote-tree-empty-text">
            {sessionSearchQuery
              ? `未找到与 "${sessionSearchQuery}" 匹配的会话`
              : activeDeviceOnline
                ? `暂无 ${currentAgentName} 的会话`
                : "电脑已离线"}
          </p>
          {activeDeviceOnline && !sessionSearchQuery && (
            <button
              type="button"
              className="remote-empty-new-btn"
              onClick={() => onOpenLaunchModal()}
            >
              <svg
                width="12"
                height="12"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.5"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
                style={{ marginRight: 6 }}
              >
                <line x1="12" y1="5" x2="12" y2="19" />
                <line x1="5" y1="12" x2="19" y2="12" />
              </svg>
              新建会话
            </button>
          )}
        </div>
      ) : (
        allWorkspacesForDevice.map((workspaceGroup) => {
          const workspaceKey = `${activeRunnerId}/${providerFilter}/${workspaceGroup.id}`;
          const isWorkspaceCollapsed =
            !sessionSearchQuery && collapsedWorkspaces[workspaceKey] === true;
          const visibleLimit = sessionSearchQuery
            ? workspaceGroup.items.length
            : (workspaceVisibleCounts[workspaceKey] ?? INITIAL_VISIBLE_COUNT);
          const visibleItems = workspaceGroup.items.slice(0, visibleLimit);
          const hasMore = workspaceGroup.items.length > visibleLimit;
          const remainingCount = workspaceGroup.items.length - visibleLimit;

          return (
            <div className="remote-workspace-group" key={workspaceKey}>
              <div className="remote-workspace-header">
                <button
                  type="button"
                  className="remote-workspace-title-btn"
                  onClick={() => onToggleWorkspace(workspaceKey)}
                  aria-expanded={!isWorkspaceCollapsed}
                  title={workspaceTooltip(workspaceGroup.id)}
                >
                  <span
                    className={`remote-tree-caret ${
                      isWorkspaceCollapsed ? "collapsed" : ""
                    }`}
                    aria-hidden="true"
                  >
                    <ChevronDownIcon size={12} />
                  </span>
                  <svg
                    className="remote-workspace-icon"
                    width="13"
                    height="13"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    aria-hidden="true"
                  >
                    <path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
                  </svg>
                  <span className="remote-workspace-name">
                    {workspaceGroup.label}
                  </span>
                </button>
                <button
                  type="button"
                  className="remote-workspace-quick-add"
                  title={`在 ${workspaceGroup.label} 中新建会话`}
                  aria-label={`在 ${workspaceGroup.label} 中新建会话`}
                  onClick={(e) => {
                    e.stopPropagation();
                    onOpenLaunchModal(workspaceGroup.id);
                  }}
                >
                  <svg
                    width="11"
                    height="11"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.5"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    aria-hidden="true"
                  >
                    <line x1="12" y1="5" x2="12" y2="19" />
                    <line x1="5" y1="12" x2="19" y2="12" />
                  </svg>
                </button>
              </div>

              {!isWorkspaceCollapsed && (
                <div className="remote-node-sessions">
                  {workspaceGroup.items.length === 0 ? (
                    <div className="remote-workspace-empty-hint">
                      暂无会话，点击新建开始
                    </div>
                  ) : (
                    <>
                      {visibleItems.map((item) => {
                        if (item.kind === "active") {
                          const activeSession = sessions.find(
                            (entry) => sessionKey(entry) === item.sessionKey,
                          );
                          const isSelected = item.sessionKey === selected;
                          const pendingCount =
                            pendingApprovalCounts.get(item.sessionKey) ?? 0;
                          return (
                            <div
                              className={`remote-session ${
                                isSelected ? "selected" : ""
                              }`}
                              key={item.key}
                            >
                              <button
                                type="button"
                                className="remote-session-main"
                                onClick={() => {
                                  if (activeSession === undefined) return;
                                  onSelectActiveSession(item.sessionKey);
                                }}
                              >
                                <span className="remote-session-top-line">
                                  <span
                                    className={`remote-session-bullet ${
                                      !item.session.control_connected &&
                                      activeSession?.online
                                        ? "reconnecting"
                                        : item.session.execution_state ===
                                            "running"
                                          ? "running"
                                          : item.session.execution_state ===
                                                "awaiting_permission" ||
                                              item.session.execution_state ===
                                                "awaiting_input"
                                            ? "waiting"
                                            : "idle"
                                    }`}
                                    aria-hidden="true"
                                  />
                                  <span className="remote-session-title">
                                    {item.title}
                                  </span>
                                </span>
                                <span className="remote-session-meta">
                                  {!item.session.control_connected &&
                                  activeSession?.online ? (
                                    <span className="remote-session-badge reconnecting">
                                      {item.session.execution_state ===
                                      "releasing_control"
                                        ? "正在归还"
                                        : busy && isSelected
                                          ? "处理中"
                                          : "只读"}
                                    </span>
                                  ) : item.session.execution_state ===
                                    "running" ? (
                                    <span className="remote-session-badge running">
                                      正在工作
                                    </span>
                                  ) : item.session.execution_state ===
                                    "awaiting_permission" ? (
                                    <span className="remote-session-badge waiting">
                                      待审批
                                    </span>
                                  ) : item.session.execution_state ===
                                    "awaiting_input" ? (
                                    <span className="remote-session-badge waiting">
                                      待输入
                                    </span>
                                  ) : null}
                                  {item.model && (
                                    <span
                                      className="remote-session-pill-tag"
                                      title={`模型: ${item.model}`}
                                    >
                                      {item.model.replace(/^models\//u, "")}
                                    </span>
                                  )}
                                  {formatSessionTokens(item.totalTokens) && (
                                    <span
                                      className="remote-session-tokens"
                                      title={`Token 用量: ${item.totalTokens?.toLocaleString()}`}
                                    >
                                      {formatSessionTokens(item.totalTokens)}
                                    </span>
                                  )}
                                </span>
                              </button>
                              <div className="remote-session-actions-hover">
                                {pendingCount > 0 && (
                                  <span
                                    className="remote-tree-badge-pending"
                                    title={`有 ${pendingCount} 项待确认审批`}
                                    aria-label={`有 ${pendingCount} 项待确认审批`}
                                  >
                                    !
                                    {pendingCount > 1 ? ` ${pendingCount}` : ""}
                                  </span>
                                )}
                                <button
                                  type="button"
                                  className={`remote-session-action-btn ${
                                    copiedSessionId === item.sessionKey
                                      ? "copied"
                                      : ""
                                  }`}
                                  title={
                                    copiedSessionId === item.sessionKey
                                      ? "已复制会话ID"
                                      : "复制会话ID"
                                  }
                                  aria-label="复制会话ID"
                                  onClick={(e) =>
                                    onCopySessionId(item.sessionKey, e)
                                  }
                                >
                                  {copiedSessionId === item.sessionKey ? (
                                    <>
                                      <CheckIcon size={12} />
                                      <span className="remote-copy-inline-label">
                                        已复制
                                      </span>
                                    </>
                                  ) : (
                                    <CopyIcon size={12} />
                                  )}
                                </button>
                              </div>
                            </div>
                          );
                        }

                        const isResumingThis =
                          resumingCandidateKey === item.key;
                        const canPreview =
                          supportsCodexControl(item.runnerId) &&
                          isCodexThreadId(item.providerSessionId);
                        return (
                          <div
                            className={`remote-session candidate ${isResumingThis ? "resuming" : ""}`}
                            key={item.key}
                          >
                            <button
                              type="button"
                              className="remote-session-main"
                              title={
                                canPreview
                                  ? "查看历史会话"
                                  : item.candidate.resumable === false
                                    ? "电脑端目前仅发现会话元数据，尚不能远程恢复"
                                    : `点击恢复：${item.title}`
                              }
                              disabled={
                                busy ||
                                (!canPreview &&
                                  item.candidate.resumable === false)
                              }
                              onClick={() =>
                                onResumeCandidate(
                                  item.runnerId,
                                  item.workspaceId,
                                  item.providerSessionId,
                                )
                              }
                            >
                              <div className="remote-session-top-line">
                                <span
                                  className={`remote-session-bullet ${isResumingThis ? "reconnecting" : "candidate"}`}
                                  aria-hidden="true"
                                />
                                <span className="remote-session-title">
                                  {item.title}
                                </span>
                              </div>
                              <span className="remote-session-meta">
                                <span
                                  className={`remote-session-badge ${isResumingThis ? "reconnecting" : "candidate"}`}
                                >
                                  {isResumingThis
                                    ? "打开中…"
                                    : canPreview
                                      ? "可查看"
                                      : "可接管"}
                                </span>
                                {item.model && (
                                  <span
                                    className="remote-session-pill-tag"
                                    title={`模型: ${item.model}`}
                                  >
                                    {item.model.replace(/^models\//u, "")}
                                  </span>
                                )}
                                {formatSessionTokens(item.totalTokens) && (
                                  <span
                                    className="remote-session-tokens"
                                    title={`Token 用量: ${item.totalTokens?.toLocaleString()}`}
                                  >
                                    {formatSessionTokens(item.totalTokens)}
                                  </span>
                                )}
                              </span>
                            </button>
                            <div className="remote-session-actions-hover">
                              <button
                                type="button"
                                className={`remote-session-action-btn ${
                                  copiedSessionId === item.providerSessionId
                                    ? "copied"
                                    : ""
                                }`}
                                title={
                                  copiedSessionId === item.providerSessionId
                                    ? "已复制会话ID"
                                    : "复制会话ID"
                                }
                                aria-label="复制会话ID"
                                onClick={(e) =>
                                  onCopySessionId(item.providerSessionId, e)
                                }
                              >
                                {copiedSessionId === item.providerSessionId ? (
                                  <>
                                    <CheckIcon size={12} />
                                    <span className="remote-copy-inline-label">
                                      已复制
                                    </span>
                                  </>
                                ) : (
                                  <CopyIcon size={12} />
                                )}
                              </button>
                            </div>
                          </div>
                        );
                      })}
                      {hasMore && (
                        <div className="remote-workspace-more-row">
                          <button
                            type="button"
                            className="remote-workspace-more-btn"
                            onClick={() =>
                              onShowMore(
                                workspaceKey,
                                INITIAL_VISIBLE_COUNT,
                                STEP_COUNT,
                              )
                            }
                          >
                            <ChevronDownIcon size={12} />
                            <span>展开更多（还有 {remainingCount} 条）</span>
                          </button>
                        </div>
                      )}
                    </>
                  )}
                </div>
              )}
            </div>
          );
        })
      )}
    </div>
  );
}
