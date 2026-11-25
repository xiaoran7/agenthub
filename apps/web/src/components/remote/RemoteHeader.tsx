import type { RefObject } from "react";
import type { DiscoveryCandidate, RemoteSession } from "../../lib/remote-types";
import {
  getProviderInfo,
  workspaceLabel,
  type AttentionEntry,
  type PluginTreeHint,
  type ProviderKind,
} from "../../lib/session-tree";
import {
  MobileAttentionBanner,
  MobileHeaderActions,
} from "../MobileHeaderAccessories";
import type { SessionBreadcrumb } from "./useRemoteSidebarState";

export interface RemoteHeaderProps {
  isMobile: boolean;
  mobileDrawerOpen: boolean;
  sidebarCollapsed: boolean;
  mobileMenuBtnRef: RefObject<HTMLButtonElement | null>;
  attention: AttentionEntry[];
  selected: string;
  selectedSessionPath?: SessionBreadcrumb[] | undefined;
  session?: RemoteSession | undefined;
  discovered: Map<string, DiscoveryCandidate>;
  allKnownPlugins?: ReadonlyArray<PluginTreeHint> | undefined;
  releasingControl: boolean;
  sessionLoading: boolean;
  itemsCount: number;
  connected: boolean;
  copiedSessionId: string | null;
  onOpenMobileDrawer: () => void;
  onToggleSidebar: () => void;
  onSelectSession: (sessionKey: string) => void;
  onSelectRunnerAndProvider: (
    runnerId?: string,
    provider?: ProviderKind,
  ) => void;
  onCloseMobileDrawer: () => void;
  onCopySessionId: (id: string, e: React.MouseEvent) => void;
  onNewSession: (workspacePath?: string) => void;
  onOpenAttentionSingle: (entry: AttentionEntry) => void;
}

export function RemoteHeader({
  isMobile,
  mobileDrawerOpen,
  sidebarCollapsed,
  mobileMenuBtnRef,
  attention,
  selected,
  selectedSessionPath,
  session,
  discovered,
  allKnownPlugins,
  releasingControl,
  sessionLoading,
  itemsCount,
  connected,
  copiedSessionId,
  onOpenMobileDrawer,
  onToggleSidebar,
  onSelectSession,
  onSelectRunnerAndProvider,
  onCloseMobileDrawer,
  onCopySessionId,
  onNewSession,
  onOpenAttentionSingle,
}: RemoteHeaderProps) {
  const subtitle = session
    ? (() => {
        const providerName = getProviderInfo(
          session.binding.provider_session_id,
          discovered.get(
            `${session.runner_id}/${session.binding.provider_session_id}`,
          )?.provider ?? session.provider,
          allKnownPlugins,
        ).name;
        if (!session.online) return `${providerName} · 电脑离线`;
        if (releasingControl) return `${providerName} · 正在归还控制`;
        if (sessionLoading && itemsCount === 0)
          return `${providerName} · 正在同步历史`;
        if (!connected) return `${providerName} · 只读`;
        return providerName;
      })()
    : "电脑执行，随处继续。";

  return (
    <>
      <header>
        <div className="remote-header-left">
          {isMobile ? (
            <button
              type="button"
              ref={mobileMenuBtnRef}
              className="remote-icon-btn mobile-menu-btn"
              title={
                attention.length > 0
                  ? `打开菜单（${attention.length} 项待处理）`
                  : "打开菜单"
              }
              aria-label={
                attention.length > 0
                  ? `打开菜单，有 ${attention.length} 项待处理`
                  : "打开菜单"
              }
              aria-expanded={mobileDrawerOpen}
              onClick={onOpenMobileDrawer}
            >
              <svg
                width="20"
                height="20"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
              >
                <line x1="3" y1="12" x2="21" y2="12" />
                <line x1="3" y1="6" x2="21" y2="6" />
                <line x1="3" y1="18" x2="21" y2="18" />
              </svg>
              {attention.length > 0 && (
                <span className="remote-mobile-menu-badge" aria-hidden="true">
                  {attention.length > 9 ? "9+" : attention.length}
                </span>
              )}
            </button>
          ) : (
            sidebarCollapsed && (
              <button
                type="button"
                className="remote-icon-btn expand-btn"
                title="展开侧边栏"
                aria-label="展开侧边栏"
                onClick={onToggleSidebar}
              >
                <svg
                  width="18"
                  height="18"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <line x1="9" y1="3" x2="9" y2="21" />
                  <path d="M13 15l3-3-3-3" />
                </svg>
              </button>
            )
          )}
          <div>
            {selectedSessionPath && selectedSessionPath.length > 0 && (
              <nav className="remote-main-breadcrumb" aria-label="会话层级位置">
                <button
                  type="button"
                  className="remote-main-breadcrumb-root"
                  onClick={() => {
                    onSelectSession("");
                    if (isMobile) onCloseMobileDrawer();
                  }}
                >
                  全部会话
                </button>
                {selectedSessionPath.map((crumb, index) => {
                  const isLast = index === selectedSessionPath.length - 1;
                  return (
                    <span
                      className="remote-main-breadcrumb-item"
                      key={crumb.key}
                    >
                      <span
                        className="remote-main-breadcrumb-sep"
                        aria-hidden="true"
                      >
                        /
                      </span>
                      <button
                        type="button"
                        className="remote-main-breadcrumb-crumb"
                        data-current={isLast}
                        onClick={() => {
                          if (isLast) return;
                          if (isMobile) onCloseMobileDrawer();
                          if (crumb.sessionKey !== undefined) {
                            onSelectSession(crumb.sessionKey);
                          } else {
                            onSelectRunnerAndProvider(
                              crumb.runnerId,
                              crumb.provider,
                            );
                            onSelectSession("");
                          }
                        }}
                      >
                        {crumb.label}
                      </button>
                    </span>
                  );
                })}
              </nav>
            )}
            <h1>
              {session
                ? workspaceLabel(session.binding.workspace_path)
                : "随时接着做"}
            </h1>
            <p>{subtitle}</p>
          </div>
        </div>
        <MobileHeaderActions
          isMobile={isMobile}
          selectedSessionKey={selected}
          copiedSessionId={copiedSessionId}
          onCopySessionId={onCopySessionId}
          onNewSession={() => onNewSession(session?.binding.workspace_path)}
        />
      </header>
      {isMobile && !mobileDrawerOpen && (
        <MobileAttentionBanner
          entries={attention}
          selectedSessionKey={selected}
          onOpenSingle={onOpenAttentionSingle}
          onOpenDrawer={onOpenMobileDrawer}
        />
      )}
    </>
  );
}
