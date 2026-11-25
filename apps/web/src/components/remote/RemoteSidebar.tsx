import type { RefObject } from "react";
import type { RemotePendingApproval } from "@agent-pulse/contracts/remote";
import type { AccountView } from "../../lib/account";
import type { RemoteRunner, RemoteSession } from "../../lib/remote-types";
import type { AttentionEntry, ProviderKind } from "../../lib/session-tree";
import { AttentionQueue } from "../AttentionQueue";
import { SettingsIcon } from "../remote-console-constants";
import {
  RemoteSessionTree,
  type WorkspaceSessionGroup,
} from "./RemoteSessionTree";
import {
  RemoteSidebarSelectors,
  type AgentOptionItem,
  type DeviceOptionItem,
} from "./RemoteSidebarSelectors";

export interface RemoteSidebarProps {
  isMobile: boolean;
  mobileDrawerOpen: boolean;
  drawerAnimated: boolean;
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  mobileDrawerRef: RefObject<HTMLElement | null>;
  searchInputRef: RefObject<HTMLInputElement | null>;
  deviceMenuRef: RefObject<HTMLDivElement | null>;
  deviceMenuButtonRef: RefObject<HTMLButtonElement | null>;
  agentMenuRef: RefObject<HTMLDivElement | null>;
  agentMenuButtonRef: RefObject<HTMLButtonElement | null>;
  onSidebarTouchStart: React.TouchEventHandler<HTMLElement>;
  onSidebarTouchMove: React.TouchEventHandler<HTMLElement>;
  onSidebarTouchEnd: React.TouchEventHandler<HTMLElement>;
  onCloseMobileDrawer: (restoreFocus?: boolean) => void;
  onToggleSidebar: () => void;
  onResizerMouseDown: (e: React.MouseEvent) => void;
  searchOpen: boolean;
  setSearchOpen: React.Dispatch<React.SetStateAction<boolean>>;
  sessionSearchQuery: string;
  setSessionSearchQuery: (q: string) => void;
  attention: AttentionEntry[];
  pendingApprovalsBySessionKey: Map<string, RemotePendingApproval[]>;
  submittingApproval: { id: string; action: "approve" | "decline" } | null;
  onOpenAttention: (entry: AttentionEntry) => void;
  onResolveAttentionApproval: (
    entry: AttentionEntry,
    approval: RemotePendingApproval,
    approved: boolean,
  ) => void;
  deviceList: DeviceOptionItem[];
  activeRunnerId: string;
  activeDevice?: DeviceOptionItem | undefined;
  deviceMenuOpen: boolean;
  setDeviceMenuOpen: React.Dispatch<React.SetStateAction<boolean>>;
  agentMenuOpen: boolean;
  setAgentMenuOpen: React.Dispatch<React.SetStateAction<boolean>>;
  isActiveRunnerHelloPending: boolean;
  availableAgentsForCurrentDevice: AgentOptionItem[];
  currentAgentInfo: AgentOptionItem;
  providerFilter: ProviderKind;
  onSelectDevice: (runnerId: string) => void;
  onSelectAgent: (agentType: ProviderKind) => void;
  allWorkspacesForDevice: WorkspaceSessionGroup[];
  sessions: RemoteSession[];
  runners: RemoteRunner[];
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
  account?: AccountView | null | undefined;
  onOpenSettings: () => void;
  onLogout?: (() => void) | undefined;
}

export function RemoteSidebar({
  isMobile,
  mobileDrawerOpen,
  drawerAnimated,
  sidebarCollapsed,
  sidebarWidth,
  mobileDrawerRef,
  searchInputRef,
  deviceMenuRef,
  deviceMenuButtonRef,
  agentMenuRef,
  agentMenuButtonRef,
  onSidebarTouchStart,
  onSidebarTouchMove,
  onSidebarTouchEnd,
  onCloseMobileDrawer,
  onToggleSidebar,
  onResizerMouseDown,
  searchOpen,
  setSearchOpen,
  sessionSearchQuery,
  setSessionSearchQuery,
  attention,
  pendingApprovalsBySessionKey,
  submittingApproval,
  onOpenAttention,
  onResolveAttentionApproval,
  deviceList,
  activeRunnerId,
  activeDevice,
  deviceMenuOpen,
  setDeviceMenuOpen,
  agentMenuOpen,
  setAgentMenuOpen,
  isActiveRunnerHelloPending,
  availableAgentsForCurrentDevice,
  currentAgentInfo,
  providerFilter,
  onSelectDevice,
  onSelectAgent,
  allWorkspacesForDevice,
  sessions,
  runners,
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
  account,
  onOpenSettings,
  onLogout,
}: RemoteSidebarProps) {
  return (
    <>
      {isMobile && (
        <div
          className={`remote-drawer-backdrop ${mobileDrawerOpen ? "open" : ""}`}
          onClick={() => onCloseMobileDrawer(true)}
          aria-hidden="true"
        />
      )}
      <aside
        className={`remote-sidebar ${
          isMobile
            ? mobileDrawerOpen
              ? "drawer-open"
              : ""
            : sidebarCollapsed
              ? "collapsed"
              : ""
        }`}
        data-drawer-animated={drawerAnimated ? "true" : undefined}
        ref={mobileDrawerRef}
        role={isMobile ? "dialog" : undefined}
        aria-modal={isMobile && mobileDrawerOpen ? "true" : undefined}
        aria-label={isMobile ? "会话导航抽屉" : "会话侧边栏"}
        aria-hidden={isMobile && !mobileDrawerOpen ? true : undefined}
        inert={isMobile && !mobileDrawerOpen ? true : undefined}
        onTouchStart={onSidebarTouchStart}
        onTouchMove={onSidebarTouchMove}
        onTouchEnd={onSidebarTouchEnd}
        style={
          isMobile
            ? undefined
            : {
                width: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
                minWidth: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
                maxWidth: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
              }
        }
      >
        <div className="remote-sidebar-header">
          <div className="remote-brand">
            AgentHub <span>Remote</span>
          </div>
          <div className="remote-header-actions">
            <button
              type="button"
              className={`remote-header-icon-btn ${
                searchOpen || sessionSearchQuery ? "active" : ""
              }`}
              title={searchOpen ? "收起搜索 (Ctrl+K)" : "搜索会话 (Ctrl+K)"}
              aria-label="搜索会话"
              onClick={() => setSearchOpen((prev) => !prev)}
            >
              <svg
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <circle cx="11" cy="11" r="8" />
                <line x1="21" y1="21" x2="16.65" y2="16.65" />
              </svg>
            </button>

            <button
              type="button"
              className="remote-header-icon-btn"
              title="新建会话"
              aria-label="新建会话"
              onClick={() => onOpenLaunchModal()}
            >
              <svg
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.2"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <line x1="12" y1="5" x2="12" y2="19" />
                <line x1="5" y1="12" x2="19" y2="12" />
              </svg>
            </button>

            {isMobile ? (
              <button
                type="button"
                className="remote-header-icon-btn"
                title="关闭菜单"
                aria-label="关闭菜单"
                onClick={() => onCloseMobileDrawer(true)}
              >
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            ) : (
              <button
                type="button"
                className="remote-header-icon-btn"
                title="收起侧边栏 (Ctrl+B)"
                aria-label="收起侧边栏"
                onClick={onToggleSidebar}
              >
                <svg
                  width="15"
                  height="15"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <line x1="9" y1="3" x2="9" y2="21" />
                  <path d="M15 9l-3 3 3 3" />
                </svg>
              </button>
            )}
          </div>
        </div>

        {(searchOpen || sessionSearchQuery) && (
          <div className="remote-sidebar-search-compact">
            <svg
              className="remote-search-icon"
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              aria-hidden="true"
            >
              <circle cx="11" cy="11" r="8" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <input
              ref={searchInputRef}
              type="text"
              className="remote-sidebar-search-input"
              placeholder="搜索会话或工作区…"
              value={sessionSearchQuery}
              autoFocus
              onChange={(e) => setSessionSearchQuery(e.target.value)}
            />
            {sessionSearchQuery && (
              <button
                type="button"
                className="remote-search-clear"
                title="清空搜索"
                aria-label="清空搜索"
                onClick={() => setSessionSearchQuery("")}
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
                >
                  <line x1="18" y1="6" x2="6" y2="18" />
                  <line x1="6" y1="6" x2="18" y2="18" />
                </svg>
              </button>
            )}
          </div>
        )}

        <AttentionQueue
          entries={attention}
          approvalsBySession={pendingApprovalsBySessionKey}
          submitting={submittingApproval}
          onOpen={onOpenAttention}
          onResolve={onResolveAttentionApproval}
        />

        <RemoteSidebarSelectors
          deviceMenuRef={deviceMenuRef}
          deviceMenuButtonRef={deviceMenuButtonRef}
          agentMenuRef={agentMenuRef}
          agentMenuButtonRef={agentMenuButtonRef}
          deviceList={deviceList}
          activeRunnerId={activeRunnerId}
          activeDevice={activeDevice}
          deviceMenuOpen={deviceMenuOpen}
          setDeviceMenuOpen={setDeviceMenuOpen}
          agentMenuOpen={agentMenuOpen}
          setAgentMenuOpen={setAgentMenuOpen}
          isActiveRunnerHelloPending={isActiveRunnerHelloPending}
          availableAgentsForCurrentDevice={availableAgentsForCurrentDevice}
          currentAgentInfo={currentAgentInfo}
          providerFilter={providerFilter}
          onSelectDevice={onSelectDevice}
          onSelectAgent={onSelectAgent}
        />

        <RemoteSessionTree
          activeRunnerId={activeRunnerId}
          isActiveRunnerHelloPending={isActiveRunnerHelloPending}
          hasAvailableAgents={availableAgentsForCurrentDevice.length > 0}
          activeDeviceOnline={Boolean(activeDevice?.online)}
          currentAgentName={currentAgentInfo.name}
          providerFilter={providerFilter}
          sessionSearchQuery={sessionSearchQuery}
          allWorkspacesForDevice={allWorkspacesForDevice}
          sessions={sessions}
          selected={selected}
          busy={busy}
          resumingCandidateKey={resumingCandidateKey}
          copiedSessionId={copiedSessionId}
          collapsedWorkspaces={collapsedWorkspaces}
          workspaceVisibleCounts={workspaceVisibleCounts}
          pendingApprovalCounts={pendingApprovalCounts}
          supportsCodexControl={supportsCodexControl}
          onToggleWorkspace={onToggleWorkspace}
          onShowMore={onShowMore}
          onOpenLaunchModal={onOpenLaunchModal}
          onSelectActiveSession={onSelectActiveSession}
          onResumeCandidate={onResumeCandidate}
          onCopySessionId={onCopySessionId}
        />

        <div className="remote-sidebar-footer">
          <div className="remote-runner-status">
            <span
              className={`remote-status-dot ${runners.some((item) => !item.revoked && item.online) ? "online" : "offline"}`}
            />
            <span>
              {runners.filter((item) => !item.revoked && item.online).length}{" "}
              台电脑在线
            </span>
          </div>
          {account?.user ? (
            <div className="remote-account-card">
              <div className="remote-account-avatar" aria-hidden="true">
                {account.user.avatar_data_url &&
                account.user.avatar_data_url.trim().length > 0 ? (
                  <img
                    src={account.user.avatar_data_url}
                    alt=""
                    referrerPolicy="no-referrer"
                  />
                ) : (
                  (
                    account.user.display_name?.[0] ||
                    account.user.email?.[0] ||
                    "U"
                  ).toUpperCase()
                )}
              </div>
              <span className="remote-account-name" title={account.user.email}>
                {account.user.display_name || account.user.email}
              </span>
              <div className="remote-account-actions">
                <button
                  className="remote-account-btn"
                  onClick={onOpenSettings}
                  type="button"
                  title="设置"
                >
                  <SettingsIcon size={14} />
                  <span>设置</span>
                </button>
                {onLogout && (
                  <button
                    className="remote-account-btn logout"
                    onClick={onLogout}
                    type="button"
                    title="退出登录"
                  >
                    退出
                  </button>
                )}
              </div>
            </div>
          ) : (
            <div
              className="remote-account-actions"
              style={{ marginTop: "4px" }}
            >
              <button
                className="remote-account-btn"
                onClick={onOpenSettings}
                type="button"
                style={{ width: "100%", justifyContent: "center" }}
                title="设置"
              >
                <SettingsIcon size={14} />
                <span>设置</span>
              </button>
            </div>
          )}
        </div>
      </aside>
      {!sidebarCollapsed && (
        <div
          className="remote-sidebar-resizer"
          onMouseDown={onResizerMouseDown}
          title="拖拽调整侧边栏宽度"
        />
      )}
    </>
  );
}
