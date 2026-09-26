import React, { useState, useEffect, useCallback } from "react";
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  Monitor,
  Plus,
  Radio,
} from "lucide-react";
import type { Session, AgentType } from "../../types";
import {
  relativeTime,
  statusView,
  isAttentionSession,
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
const COLLAPSED_WORKSPACES_KEY = "agenthub:tree-collapsed-workspaces";

function loadCollapsedSet(storageKey: string): Set<string> {
  try {
    const raw = window.localStorage.getItem(storageKey);
    if (!raw) return new Set();
    const arr = JSON.parse(raw);
    return new Set(Array.isArray(arr) ? arr : []);
  } catch {
    return new Set();
  }
}

function saveCollapsedSet(storageKey: string, set: Set<string>): void {
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
    loadCollapsedSet(COLLAPSED_DEVICES_KEY),
  );
  const [collapsedAgents, setCollapsedAgents] = useState<Set<string>>(() =>
    loadCollapsedSet(COLLAPSED_AGENTS_KEY),
  );
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<Set<string>>(
    () => loadCollapsedSet(COLLAPSED_WORKSPACES_KEY),
  );

  const toggleDevice = useCallback((id: string) => {
    setCollapsedDevices((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveCollapsedSet(COLLAPSED_DEVICES_KEY, next);
      return next;
    });
  }, []);

  const toggleAgent = useCallback((id: string) => {
    setCollapsedAgents((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveCollapsedSet(COLLAPSED_AGENTS_KEY, next);
      return next;
    });
  }, []);

  const toggleWorkspace = useCallback((id: string) => {
    setCollapsedWorkspaces((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      saveCollapsedSet(COLLAPSED_WORKSPACES_KEY, next);
      return next;
    });
  }, []);

  if (tree.devices.length === 0 || tree.totalSessions === 0 && query) {
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
                            agent.workspaces.map((ws) => {
                              const isWsCollapsed = collapsedWorkspaces.has(ws.id);

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
                                      if (e.key === "Enter" || e.key === " ") {
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
                                      {ws.sessions.map((item: TreeNodeSession) => {
                                        const isSelected =
                                          selectedSessionKey === item.key;
                                        const session = item.session;
                                        const statusInfo = statusView(
                                          session.payload.state.activity_status,
                                          session.payload.state.presence,
                                        );
                                        const isWorking =
                                          session.payload.state.activity_status ===
                                          "WORKING";
                                        const attention =
                                          isAttentionSession(session);

                                        return (
                                          <div
                                            key={item.key}
                                            className={`tree-session-item ${isSelected ? "selected" : ""} ${attention ? "needs-attention" : ""}`}
                                            data-provider={session.agent_type}
                                            onContextMenu={(e) => {
                                              e.preventDefault();
                                              onContextMenu(e, session);
                                            }}
                                            onClick={() => onSelectSession(item.key)}
                                            role="button"
                                            tabIndex={0}
                                            onKeyDown={(e) => {
                                              if (e.key === "Enter" || e.key === " ") {
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
                                              title={item.title}
                                            >
                                              {item.title}
                                            </span>
                                            <span className="tree-session-time">
                                              {relativeTime(item.activityTime)}
                                            </span>
                                          </div>
                                        );
                                      })}
                                    </div>
                                  )}
                                </div>
                              );
                            })
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
