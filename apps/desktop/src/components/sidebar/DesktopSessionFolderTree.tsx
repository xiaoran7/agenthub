import React, { useState, useEffect, useCallback, useMemo } from "react";
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  Monitor,
  Plus,
} from "lucide-react";
import type { Session, AgentType } from "../../types";
import {
  relativeTime,
  statusView,
  isAttentionSession,
  sessionTooltip,
} from "../../presentation";
import type {
  DesktopSessionTree,
  DeviceTreeNode,
  AgentTreeNode,
  WorkspaceTreeNode,
  TreeNodeSession,
} from "../../lib/session-tree";

interface DesktopSessionFolderTreeProps {
  tree: DesktopSessionTree;
  selectedSessionKey?: string;
  onSelectSession: (key: string) => void;
  onLaunchForProvider: (provider: AgentType) => void;
  onContextMenu: (e: React.MouseEvent, session: Session) => void;
  query: string;
}

const COLLAPSED_DEVICES_KEY = "agenthub:tree-collapsed-devices";
const COLLAPSED_AGENTS_KEY = "agenthub:tree-collapsed-agents";
const EXPANDED_WORKSPACES_KEY = "agenthub:tree-expanded-workspaces";

const INITIAL_SESSIONS_COUNT = 8;
const SESSION_EXPAND_STEP = 16;
const INITIAL_WORKSPACES_COUNT = 10;
const WORKSPACE_EXPAND_STEP = 15;

function loadSet(storageKey: string): Set<string> {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return new Set();
    const arr = JSON.parse(raw);
    return new Set(Array.isArray(arr) ? arr : []);
  } catch {
    return new Set();
  }
}

function saveSet(storageKey: string, set: Set<string>): void {
  try {
    window.localStorage.setItem(storageKey, JSON.stringify(Array.from(set)));
  } catch {
    // ignore
  }
}

export function DesktopSessionFolderTree({
  tree,
  selectedSessionKey,
  onSelectSession,
  onLaunchForProvider,
  onContextMenu,
  query,
}: DesktopSessionFolderTreeProps) {
  const [collapsedDevices, setCollapsedDevices] = useState<Set<string>>(() =>
    loadSet(COLLAPSED_DEVICES_KEY),
  );
  const [collapsedAgents, setCollapsedAgents] = useState<Set<string>>(() =>
    loadSet(COLLAPSED_AGENTS_KEY),
  );
  // Default workspaces to COLLAPSED (only explicitly expanded ones are open)
  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(
    () => loadSet(EXPANDED_WORKSPACES_KEY),
  );

  // Pagination states for long session and workspace lists
  const [sessionLimits, setSessionLimits] = useState<Record<string, number>>({});
  const [workspaceLimits, setWorkspaceLimits] = useState<Record<string, number>>(
    {},
  );

  const lastSelectedRef = React.useRef<string | undefined>(selectedSessionKey);

  // If user selects/switches to a session, auto-expand that specific workspace
  useEffect(() => {
    if (selectedSessionKey && selectedSessionKey !== lastSelectedRef.current) {
      lastSelectedRef.current = selectedSessionKey;
      for (const device of tree.devices) {
        for (const agent of device.agents) {
          for (const ws of agent.workspaces) {
            if (ws.sessions.some((s) => s.key === selectedSessionKey)) {
              setExpandedWorkspaces((prev) => {
                if (prev.has(ws.id)) return prev;
                const next = new Set(prev);
                next.add(ws.id);
                saveSet(EXPANDED_WORKSPACES_KEY, next);
                return next;
              });
              return;
            }
          }
        }
      }
    }
  }, [selectedSessionKey, tree]);

  const toggleDevice = useCallback((id: string) => {
    setCollapsedDevices((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveSet(COLLAPSED_DEVICES_KEY, next);
      return next;
    });
  }, []);

  const toggleAgent = useCallback((id: string) => {
    setCollapsedAgents((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveSet(COLLAPSED_AGENTS_KEY, next);
      return next;
    });
  }, []);

  const toggleWorkspace = useCallback((id: string) => {
    setExpandedWorkspaces((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveSet(EXPANDED_WORKSPACES_KEY, next);
      return next;
    });
  }, []);

  const expandSessions = useCallback((wsId: string, total: number) => {
    setSessionLimits((prev) => {
      const cur = prev[wsId] ?? INITIAL_SESSIONS_COUNT;
      return {
        ...prev,
        [wsId]: Math.min(total, cur + SESSION_EXPAND_STEP),
      };
    });
  }, []);

  const expandWorkspaces = useCallback((agentKey: string, total: number) => {
    setWorkspaceLimits((prev) => {
      const cur = prev[agentKey] ?? INITIAL_WORKSPACES_COUNT;
      return {
        ...prev,
        [agentKey]: Math.min(total, cur + WORKSPACE_EXPAND_STEP),
      };
    });
  }, []);

  if (tree.devices.length === 0 || (tree.totalSessions === 0 && query)) {
    return (
      <div className="sidebar-empty">
        <span>{query ? "没有找到匹配的会话" : "正在扫描本机 Agent 会话..."}</span>
      </div>
    );
  }

  return (
    <div className="session-folder-tree">
      {tree.devices.map((device) => {
        const isDeviceCollapsed = collapsedDevices.has(device.deviceId);

        return (
          <div
            key={device.deviceId}
            className={`tree-node tree-device-node ${isDeviceCollapsed ? "collapsed" : "expanded"}`}
          >
            {/* Level 1: 电脑 (Device / Host) */}
            <div
              className="tree-node-row tree-device-row"
              onClick={() => toggleDevice(device.deviceId)}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  toggleDevice(device.deviceId);
                }
              }}
            >
              <span className="tree-chevron">
                {isDeviceCollapsed ? (
                  <ChevronRight size={13} />
                ) : (
                  <ChevronDown size={13} />
                )}
              </span>
              <span className="tree-icon device-icon">
                <Monitor size={14} />
              </span>
              <strong className="tree-label device-label">{device.label}</strong>
              <div className="tree-row-meta">
                <span className="tree-count-badge">{device.totalCount}</span>
              </div>
            </div>

            {/* Level 2: Agent (Provider) */}
            {!isDeviceCollapsed && (
              <div className="tree-children tree-device-children">
                {device.agents.map((agent) => {
                  const agentKey = `${device.deviceId}:${agent.provider}`;
                  const isAgentCollapsed = collapsedAgents.has(agentKey);

                  // Workspace pagination within Agent
                  const wsLimit = query
                    ? agent.workspaces.length
                    : (workspaceLimits[agentKey] ?? INITIAL_WORKSPACES_COUNT);
                  const visibleWorkspaces = agent.workspaces.slice(0, wsLimit);
                  const hasMoreWorkspaces =
                    !query && agent.workspaces.length > wsLimit;
                  const remainingWorkspaces =
                    agent.workspaces.length - wsLimit;

                  return (
                    <div
                      key={agent.provider}
                      className={`tree-node tree-agent-node ${isAgentCollapsed ? "collapsed" : "expanded"}`}
                      data-provider={agent.provider}
                    >
                      <div
                        className="tree-node-row tree-agent-row"
                        onClick={() => toggleAgent(agentKey)}
                        role="button"
                        tabIndex={0}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" || e.key === " ") {
                            e.preventDefault();
                            toggleAgent(agentKey);
                          }
                        }}
                      >
                        <span className="tree-chevron">
                          {isAgentCollapsed ? (
                            <ChevronRight size={12} />
                          ) : (
                            <ChevronDown size={12} />
                          )}
                        </span>
                        <span className="agent-badge-dot" />
                        <span className="tree-label agent-label">
                          {agent.label}
                        </span>
                        <div
                          className="tree-row-meta"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <span className="tree-count-badge">
                            {agent.totalCount}
                          </span>
                          <button
                            type="button"
                            className="tree-action-btn"
                            title={`在 ${agent.label} 新建会话`}
                            aria-label={`在 ${agent.label} 新建会话`}
                            onClick={() => onLaunchForProvider(agent.provider)}
                          >
                            <Plus size={13} />
                          </button>
                        </div>
                      </div>

                      {/* Level 3: 工作区 (Workspace) */}
                      {!isAgentCollapsed && (
                        <div className="tree-children tree-agent-children">
                          {agent.workspaces.length === 0 ? (
                            <div className="tree-empty-hint">
                              <span>暂无会话，点击右侧 + 发起</span>
                            </div>
                          ) : (
                            <>
                              {visibleWorkspaces.map((ws) => {
                                const isWsCollapsed = query
                                  ? false
                                  : !expandedWorkspaces.has(ws.id);

                                // Session pagination within Workspace
                                // If the selected session is in this workspace, ensure it's visible
                                const hasSelected = ws.sessions.some(
                                  (s) => s.key === selectedSessionKey,
                                );
                                const baseLimit =
                                  sessionLimits[ws.id] ?? INITIAL_SESSIONS_COUNT;
                                const sessionLimit = query
                                  ? ws.sessions.length
                                  : hasSelected
                                    ? Math.max(
                                        baseLimit,
                                        ws.sessions.findIndex(
                                          (s) => s.key === selectedSessionKey,
                                        ) + 1,
                                      )
                                    : baseLimit;

                                const visibleSessions = ws.sessions.slice(
                                  0,
                                  sessionLimit,
                                );
                                const hasMoreSessions =
                                  !query && ws.sessions.length > sessionLimit;
                                const remainingSessions =
                                  ws.sessions.length - sessionLimit;

                                return (
                                  <div
                                    key={ws.id}
                                    className={`tree-node tree-workspace-node ${isWsCollapsed ? "collapsed" : "expanded"}`}
                                  >
                                    <div
                                      className="tree-node-row tree-workspace-row"
                                      onClick={() => toggleWorkspace(ws.id)}
                                      role="button"
                                      tabIndex={0}
                                      onKeyDown={(e) => {
                                        if (
                                          e.key === "Enter" ||
                                          e.key === " "
                                        ) {
                                          e.preventDefault();
                                          toggleWorkspace(ws.id);
                                        }
                                      }}
                                    >
                                      <span className="tree-chevron">
                                        {isWsCollapsed ? (
                                          <ChevronRight size={11} />
                                        ) : (
                                          <ChevronDown size={11} />
                                        )}
                                      </span>
                                      <span className="tree-icon ws-icon">
                                        {isWsCollapsed ? (
                                          <Folder size={13} />
                                        ) : (
                                          <FolderOpen size={13} />
                                        )}
                                      </span>
                                      <span
                                        className="tree-label ws-label"
                                        title={ws.name}
                                      >
                                        {ws.name}
                                      </span>
                                      <span className="tree-count-badge ws-count">
                                        {ws.totalCount}
                                      </span>
                                    </div>

                                    {/* Level 4: Session (会话项) */}
                                    {!isWsCollapsed && (
                                      <div className="tree-children tree-workspace-children">
                                        {visibleSessions.map(
                                          (item: TreeNodeSession) => {
                                            const isSelected =
                                              selectedSessionKey === item.key;
                                            const session = item.session;
                                            const statusInfo = statusView(
                                              session.payload.state
                                                .activity_status,
                                              session.payload.state.presence,
                                            );
                                            const isWorking =
                                              session.payload.state
                                                .activity_status === "WORKING";
                                            const attention =
                                              isAttentionSession(session);

                                            return (
                                              <div
                                                key={item.key}
                                                className={`tree-session-item ${isSelected ? "selected" : ""} ${attention ? "needs-attention" : ""}`}
                                                data-provider={
                                                  session.agent_type
                                                }
                                                title={sessionTooltip(
                                                  session,
                                                  item.title,
                                                )}
                                                onContextMenu={(e) => {
                                                  e.preventDefault();
                                                  onContextMenu(e, session);
                                                }}
                                                onClick={() =>
                                                  onSelectSession(item.key)
                                                }
                                                role="button"
                                                tabIndex={0}
                                                onKeyDown={(e) => {
                                                  if (
                                                    e.key === "Enter" ||
                                                    e.key === " "
                                                  ) {
                                                    e.preventDefault();
                                                    onSelectSession(item.key);
                                                  }
                                                }}
                                              >
                                                <span
                                                  className={`session-status-dot ${statusInfo.className} ${isWorking ? "pulse" : ""}`}
                                                  title={statusInfo.label}
                                                />
                                                <span
                                                  className="tree-session-title"
                                                  title={sessionTooltip(
                                                    session,
                                                    item.title,
                                                  )}
                                                >
                                                  {item.title}
                                                </span>
                                                <span className="tree-session-time">
                                                  {relativeTime(
                                                    item.activityTime,
                                                  )}
                                                </span>
                                              </div>
                                            );
                                          },
                                        )}

                                        {/* 展开显示 (会话级分页) */}
                                        {hasMoreSessions && (
                                          <button
                                            type="button"
                                            className="tree-expand-more-btn"
                                            onClick={(e) => {
                                              e.stopPropagation();
                                              expandSessions(
                                                ws.id,
                                                ws.sessions.length,
                                              );
                                            }}
                                            title={`继续展开 ${ws.name} 下的会话`}
                                          >
                                            <span className="expand-text">
                                              展开显示
                                            </span>
                                            <span className="expand-badge">
                                              +{remainingSessions}
                                            </span>
                                            <ChevronDown size={11} />
                                          </button>
                                        )}
                                      </div>
                                    )}
                                  </div>
                                );
                              })}

                              {/* 展开显示更多工作区 (工作区级分页) */}
                              {hasMoreWorkspaces && (
                                <button
                                  type="button"
                                  className="tree-expand-more-btn tree-expand-more-workspaces"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    expandWorkspaces(
                                      agentKey,
                                      agent.workspaces.length,
                                    );
                                  }}
                                  title={`展开更多工作区`}
                                >
                                  <span className="expand-text">
                                    展开显示更多工作区
                                  </span>
                                  <span className="expand-badge">
                                    +{remainingWorkspaces}
                                  </span>
                                  <ChevronDown size={12} />
                                </button>
                              )}
                            </>
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
