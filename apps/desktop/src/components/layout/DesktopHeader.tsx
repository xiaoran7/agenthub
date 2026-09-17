import { useState, useEffect } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Minus,
  Square,
  Copy,
  X,
  PanelLeft,
  PanelLeftClose,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
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

  const handleMinimize = async (e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    try {
      await invoke("desktop_window_minimize");
    } catch {
      try {
        await getCurrentWindow().minimize();
      } catch {
        // ignore outside tauri
      }
    }
  };

  const handleToggleMaximize = async (e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    try {
      await invoke("desktop_window_toggle_maximize");
      setIsMaximized((prev) => !prev);
    } catch {
      try {
        await getCurrentWindow().toggleMaximize();
        const next = await getCurrentWindow().isMaximized();
        setIsMaximized(next);
      } catch {
        // ignore outside tauri
      }
    }
  };

  const handleClose = async (e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    try {
      await invoke("desktop_window_close");
    } catch {
      try {
        await getCurrentWindow().close();
      } catch {
        // ignore outside tauri
      }
    }
  };

  const handleStartDrag = (e: React.MouseEvent) => {
    if (e.button === 0) {
      try {
        getCurrentWindow().startDragging();
      } catch {
        // ignore outside tauri
      }
    }
  };

  return (
    <header className="desktop-header-zcode">
      {/* Left section: Brand logo / mark + Navigation arrows */}
      <div className="header-left-zone">
        <div className="header-brand-mark" title="AgentHub">
          <span className="brand-letter">A</span>
        </div>

        <div className="header-nav-arrows">
          <button
            type="button"
            className="nav-arrow-btn"
            disabled={!canNavigateBack}
            onClick={onNavigateBack}
            onMouseDown={(e) => e.stopPropagation()}
            title="后退 (Alt+Left)"
            aria-label="后退"
          >
            <ChevronLeft size={16} />
          </button>
          <button
            type="button"
            className="nav-arrow-btn"
            disabled={!canNavigateForward}
            onClick={onNavigateForward}
            onMouseDown={(e) => e.stopPropagation()}
            title="前进 (Alt+Right)"
            aria-label="前进"
          >
            <ChevronRight size={16} />
          </button>
        </div>

        {(title || subtitle) && (
          <div
            className="header-session-title"
            data-tauri-drag-region
            onMouseDown={handleStartDrag}
            onDoubleClick={handleToggleMaximize}
          >
            {title && <span className="title-text">{title}</span>}
            {subtitle && <span className="subtitle-text">/ {subtitle}</span>}
          </div>
        )}
      </div>

      {/* Center draggable region */}
      <div
        className="header-center-zone"
        data-tauri-drag-region
        onMouseDown={handleStartDrag}
        onDoubleClick={handleToggleMaximize}
      />

      {/* Right section: Sidebar toggle + Window controls */}
      <div className="header-right-zone">
        <div className="header-layout-controls">
          <button
            type="button"
            className={`layout-action-btn ${sidebarCollapsed ? "active" : ""}`}
            onClick={onToggleSidebar}
            onMouseDown={(e) => e.stopPropagation()}
            title={sidebarCollapsed ? "展开侧边栏" : "折叠侧边栏"}
            aria-label="切换侧边栏"
          >
            {sidebarCollapsed ? (
              <PanelLeft size={15} />
            ) : (
              <PanelLeftClose size={15} />
            )}
          </button>
        </div>

        {/* Native window caption controls */}
        <div className="window-caption-controls">
          <button
            type="button"
            className="window-control-btn minimize"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={handleMinimize}
            title="最小化"
            aria-label="最小化"
          >
            <Minus size={13} />
          </button>
          <button
            type="button"
            className="window-control-btn maximize"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={handleToggleMaximize}
            title={isMaximized ? "向下还原" : "最大化"}
            aria-label={isMaximized ? "向下还原" : "最大化"}
          >
            {isMaximized ? <Copy size={11} /> : <Square size={11} />}
          </button>
          <button
            type="button"
            className="window-control-btn close"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={handleClose}
            title="关闭"
            aria-label="关闭"
          >
            <X size={14} />
          </button>
        </div>
      </div>
    </header>
  );
}
