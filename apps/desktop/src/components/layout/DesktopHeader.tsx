import { useState, useEffect } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Minus,
  Square,
  X,
  PanelLeft,
  PanelLeftClose,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface DesktopHeaderProps {
  onToggleSidebar?: () => void;
  sidebarCollapsed?: boolean;
  onNavigateBack?: () => void;
  canNavigateBack?: boolean;
  onNavigateForward?: () => void;
  canNavigateForward?: boolean;
  title?: string;
  subtitle?: string;
}

export function DesktopHeader({
  onToggleSidebar,
  sidebarCollapsed = false,
  onNavigateBack,
  canNavigateBack = false,
  onNavigateForward,
  canNavigateForward = false,
  title,
  subtitle,
}: DesktopHeaderProps) {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let resizeTimer: number | undefined;
    try {
      const appWindow = getCurrentWindow();
      appWindow
        .isMaximized()
        .then(setIsMaximized)
        .catch(() => {});
      appWindow
        .onResized(() => {
          if (resizeTimer !== undefined) return;
          resizeTimer = window.setTimeout(() => {
            resizeTimer = undefined;
            appWindow
              .isMaximized()
              .then(setIsMaximized)
              .catch(() => {});
          }, 150);
        })
        .then((fn) => {
          unlisten = fn;
        })
        .catch(() => {});
    } catch {
      // Running outside Tauri (web/test mode)
    }
    return () => {
      if (resizeTimer !== undefined) window.clearTimeout(resizeTimer);
      if (unlisten) unlisten();
    };
  }, []);

  const handleMinimize = () => {
    try {
      getCurrentWindow().minimize();
    } catch {
      // ignore
    }
  };

  const handleToggleMaximize = () => {
    try {
      getCurrentWindow().toggleMaximize();
    } catch {
      // ignore
    }
  };

  const handleClose = () => {
    try {
      getCurrentWindow().close();
    } catch {
      // ignore
    }
  };

  const handleMouseDown = (e: React.MouseEvent) => {
    // Only primary mouse button initiates drag when not clicking on interactive controls
    if (
      e.button === 0 &&
      !(e.target as HTMLElement).closest("button, a, input, select, textarea, [data-no-drag]")
    ) {
      try {
        getCurrentWindow().startDragging();
      } catch {
        // ignore outside tauri
      }
    }
  };

  const handleDoubleClick = (e: React.MouseEvent) => {
    if (
      e.button === 0 &&
      !(e.target as HTMLElement).closest("button, a, input, select, textarea, [data-no-drag]")
    ) {
      handleToggleMaximize();
    }
  };

  return (
    <header
      className="desktop-header-zcode"
      data-tauri-drag-region
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
    >
      {/* Left section: Brand logo / mark + Navigation arrows (like [Z] [←] [→]) */}
      <div className="header-left-zone" data-tauri-drag-region>
        <div className="header-brand-mark" title="AgentHub" data-no-drag>
          <span className="brand-letter">A</span>
        </div>

        <div className="header-nav-arrows" data-no-drag>
          <button
            type="button"
            className="nav-arrow-btn"
            disabled={!canNavigateBack}
            onClick={onNavigateBack}
            title="后退 (Alt+Left)"
            aria-label="后退"
            data-tauri-drag-region="false"
          >
            <ChevronLeft size={16} />
          </button>
          <button
            type="button"
            className="nav-arrow-btn"
            disabled={!canNavigateForward}
            onClick={onNavigateForward}
            title="前进 (Alt+Right)"
            aria-label="前进"
            data-tauri-drag-region="false"
          >
            <ChevronRight size={16} />
          </button>
        </div>

        {(title || subtitle) && (
          <div className="header-session-title" data-tauri-drag-region>
            {title && <span className="title-text">{title}</span>}
            {subtitle && <span className="subtitle-text">/ {subtitle}</span>}
          </div>
        )}
      </div>

      {/* Center draggable region */}
      <div className="header-center-zone" data-tauri-drag-region />

      {/* Right section: Sidebar toggle + Window controls */}
      <div className="header-right-zone" data-no-drag>
        <div className="header-layout-controls" data-no-drag>
          <button
            type="button"
            className={`layout-action-btn ${sidebarCollapsed ? "active" : ""}`}
            onClick={onToggleSidebar}
            title={sidebarCollapsed ? "展开侧边栏" : "折叠侧边栏"}
            aria-label="切换侧边栏"
            data-tauri-drag-region="false"
          >
            {sidebarCollapsed ? (
              <PanelLeft size={15} />
            ) : (
              <PanelLeftClose size={15} />
            )}
          </button>
        </div>

        {/* Native window caption controls */}
        <div className="window-caption-controls" data-no-drag>
          <button
            type="button"
            className="window-control-btn minimize"
            onClick={handleMinimize}
            title="最小化"
            aria-label="最小化"
            data-tauri-drag-region="false"
          >
            <Minus size={13} />
          </button>
          <button
            type="button"
            className="window-control-btn maximize"
            onClick={handleToggleMaximize}
            title={isMaximized ? "还原" : "最大化"}
            aria-label={isMaximized ? "还原" : "最大化"}
            data-tauri-drag-region="false"
          >
            <Square size={11} />
          </button>
          <button
            type="button"
            className="window-control-btn close"
            onClick={handleClose}
            title="关闭"
            aria-label="关闭"
            data-tauri-drag-region="false"
          >
            <X size={14} />
          </button>
        </div>
      </div>
    </header>
  );
}
