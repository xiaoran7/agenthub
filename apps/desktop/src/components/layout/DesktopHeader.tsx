import { useState, useEffect } from "react";
import {
  ChevronLeft,
  ChevronRight,
  Minus,
  Square,
  X,
  PanelLeft,
  PanelLeftClose,
  Columns,
  ChevronDown,
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
    try {
      const appWindow = getCurrentWindow();
      appWindow.isMaximized().then(setIsMaximized).catch(() => {});
      appWindow
        .onResized(() => {
          appWindow.isMaximized().then(setIsMaximized).catch(() => {});
        })
        .then((fn) => {
          unlisten = fn;
        })
        .catch(() => {});
    } catch {
      // Running outside Tauri (web/test mode)
    }
    return () => {
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

  return (
    <header className="desktop-header-zcode" data-tauri-drag-region>
      {/* Left section: Brand logo / mark + Navigation arrows (like [Z] [←] [→]) */}
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
            title="前进 (Alt+Right)"
            aria-label="前进"
          >
            <ChevronRight size={16} />
          </button>
        </div>

        {(title || subtitle) && (
          <div className="header-session-title">
            {title && <span className="title-text">{title}</span>}
            {subtitle && <span className="subtitle-text">/ {subtitle}</span>}
          </div>
        )}
      </div>

      {/* Center draggable region */}
      <div className="header-center-zone" data-tauri-drag-region />

      {/* Right section: Layout toggles + Window controls (like [◨] [▯] [∨] [-] [□] [✕]) */}
      <div className="header-right-zone">
        <div className="header-layout-controls">
          <button
            type="button"
            className="layout-action-btn"
            title="分栏布局视图"
            aria-label="分栏布局视图"
          >
            <Columns size={15} />
          </button>

          <button
            type="button"
            className={`layout-action-btn ${sidebarCollapsed ? "active" : ""}`}
            onClick={onToggleSidebar}
            title={sidebarCollapsed ? "展开侧边栏" : "折叠侧边栏"}
            aria-label="切换侧边栏"
          >
            {sidebarCollapsed ? (
              <PanelLeft size={15} />
            ) : (
              <PanelLeftClose size={15} />
            )}
          </button>

          <button
            type="button"
            className="layout-action-btn"
            title="更多视图选项"
            aria-label="更多视图选项"
          >
            <ChevronDown size={15} />
          </button>
        </div>

        {/* Native window caption controls */}
        <div className="window-caption-controls">
          <button
            type="button"
            className="window-control-btn minimize"
            onClick={handleMinimize}
            title="最小化"
            aria-label="最小化"
          >
            <Minus size={13} />
          </button>
          <button
            type="button"
            className="window-control-btn maximize"
            onClick={handleToggleMaximize}
            title={isMaximized ? "还原" : "最大化"}
            aria-label={isMaximized ? "还原" : "最大化"}
          >
            <Square size={11} />
          </button>
          <button
            type="button"
            className="window-control-btn close"
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
