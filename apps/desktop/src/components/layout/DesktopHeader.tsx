import { useState, useEffect, useRef } from "react";
import {
  Menu,
  PanelLeft,
  ArrowLeft,
  ArrowRight,
  ChevronRight,
} from "lucide-react";
import { useWindowControls, WindowCaptionControls } from "./WindowControls";

interface DesktopHeaderProps {
  onToggleSidebar?: () => void;
  sidebarCollapsed?: boolean;
  onNavigateBack?: () => void;
  canNavigateBack?: boolean;
  onNavigateForward?: () => void;
  canNavigateForward?: boolean;
  onNewSession?: () => void;
  onOpenSettings?: () => void;
  onOpenLocalHost?: () => void;
  onOpenPlugins?: () => void;
  onReload?: () => void;
  title?: string;
  subtitle?: string;
  isRunning?: boolean;
}

type MenuItem =
  | { type: "separator" }
  | {
      type?: "action";
      label: string;
      shortcut?: string;
      disabled?: boolean;
      action?: () => void;
    };

interface MenuCategory {
  id: string;
  label: string;
  items: MenuItem[];
}

export function DesktopHeader({
  onToggleSidebar,
  sidebarCollapsed = false,
  onNavigateBack,
  canNavigateBack = false,
  onNavigateForward,
  canNavigateForward = false,
  onNewSession,
  onOpenSettings,
  onOpenLocalHost,
  onOpenPlugins,
  onReload,
  title,
  subtitle,
  isRunning = false,
}: DesktopHeaderProps) {
  const {
    isMaximized,
    handleMinimize,
    handleToggleMaximize,
    handleClose,
    handleStartDrag,
  } = useWindowControls();
  const [menuOpen, setMenuOpen] = useState(false);
  const [activeSubmenu, setActiveSubmenu] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Close hamburger menu on outside click or Escape key
  useEffect(() => {
    if (!menuOpen) return;
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
        setActiveSubmenu(null);
      }
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setMenuOpen(false);
        setActiveSubmenu(null);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [menuOpen]);

  const menuSections: MenuCategory[] = [
    {
      id: "file",
      label: "File",
      items: [
        {
          label: "新建会话",
          shortcut: "Ctrl+N",
          action: () => onNewSession?.(),
        },
        {
          label: "系统设置",
          shortcut: "Ctrl+,",
          action: () => onOpenSettings?.(),
        },
        { type: "separator" as const },
        {
          label: "退出",
          shortcut: "Alt+F4",
          action: () => void handleClose(),
        },
      ],
    },
    {
      id: "edit",
      label: "Edit",
      items: [
        {
          label: "撤销",
          shortcut: "Ctrl+Z",
          action: () => document.execCommand("undo"),
        },
        {
          label: "重做",
          shortcut: "Ctrl+Y",
          action: () => document.execCommand("redo"),
        },
        { type: "separator" as const },
        {
          label: "剪切",
          shortcut: "Ctrl+X",
          action: () => document.execCommand("cut"),
        },
        {
          label: "复制",
          shortcut: "Ctrl+C",
          action: () => document.execCommand("copy"),
        },
        {
          label: "粘贴",
          shortcut: "Ctrl+V",
          action: () => document.execCommand("paste"),
        },
        {
          label: "全选",
          shortcut: "Ctrl+A",
          action: () => document.execCommand("selectAll"),
        },
      ],
    },
    {
      id: "view",
      label: "View",
      items: [
        {
          label: sidebarCollapsed ? "展开侧边栏" : "折叠侧边栏",
          shortcut: "Ctrl+B",
          action: () => onToggleSidebar?.(),
        },
        { type: "separator" as const },
        {
          label: "重新加载",
          shortcut: "Ctrl+R",
          action: () => (onReload ? onReload() : window.location.reload()),
        },
        {
          label: isMaximized ? "向下还原" : "切换全屏",
          shortcut: "F11",
          action: () => void handleToggleMaximize(),
        },
      ],
    },
    {
      id: "go",
      label: "Go",
      items: [
        {
          label: "后退",
          shortcut: "Alt+Left",
          disabled: !canNavigateBack,
          action: () => onNavigateBack?.(),
        },
        {
          label: "前进",
          shortcut: "Alt+Right",
          disabled: !canNavigateForward,
          action: () => onNavigateForward?.(),
        },
      ],
    },
    {
      id: "help",
      label: "Help",
      items: [
        {
          label: "管理本机会话守护 (agenthubd)",
          action: () => onOpenLocalHost?.(),
        },
        {
          label: "插件商城与扩展",
          action: () => onOpenPlugins?.(),
        },
        { type: "separator" as const },
        {
          label: "关于 AgentHub",
          action: () => onOpenSettings?.(),
        },
      ],
    },
  ];

  return (
    <header className="desktop-header-zcode">
      {/* Left section: Hamburger Menu, Sidebar Toggle, Navigation Arrows (Claude Style) */}
      <div className="header-left-zone">
        <div className="claude-app-menu-container" ref={menuRef}>
          <button
            type="button"
            className={`claude-menu-trigger-btn ${menuOpen ? "active" : ""}`}
            onClick={() => {
              setMenuOpen((prev) => !prev);
              setActiveSubmenu(null);
            }}
            onMouseDown={(e) => e.stopPropagation()}
            title="应用主菜单"
            aria-label="应用主菜单"
            aria-expanded={menuOpen}
          >
            <Menu size={16} />
          </button>

          {/* Claude-style Dropdown Menu */}
          {menuOpen && (
            <div
              className="claude-app-dropdown-menu"
              onMouseDown={(e) => e.stopPropagation()}
            >
              <div className="claude-menu-categories">
                {menuSections.map((section) => (
                  <div
                    key={section.id}
                    className={`claude-menu-category-item ${activeSubmenu === section.id ? "hovered" : ""}`}
                    onMouseEnter={() => setActiveSubmenu(section.id)}
                    onClick={() =>
                      setActiveSubmenu((prev) =>
                        prev === section.id ? null : section.id,
                      )
                    }
                  >
                    <span className="category-label">{section.label}</span>
                    <ChevronRight size={13} className="category-arrow" />

                    {/* Submenu flyout */}
                    {activeSubmenu === section.id && (
                      <div className="claude-menu-submenu">
                        {section.items.map((item, idx) => {
                          if (item.type === "separator") {
                            return (
                              <div
                                key={`sep-${idx}`}
                                className="claude-menu-separator"
                              />
                            );
                          }
                          return (
                            <button
                              key={item.label}
                              type="button"
                              className="claude-submenu-item"
                              disabled={item.disabled}
                              onClick={() => {
                                item.action?.();
                                setMenuOpen(false);
                                setActiveSubmenu(null);
                              }}
                            >
                              <span className="item-label">{item.label}</span>
                              {item.shortcut && (
                                <span className="item-shortcut">
                                  {item.shortcut}
                                </span>
                              )}
                            </button>
                          );
                        })}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Sidebar Toggle Button (Claude Style) */}
        <button
          type="button"
          className={`nav-arrow-btn sidebar-toggle-btn ${sidebarCollapsed ? "active" : ""}`}
          onClick={onToggleSidebar}
          onMouseDown={(e) => e.stopPropagation()}
          title={
            sidebarCollapsed ? "展开侧边栏 (Ctrl+B)" : "折叠侧边栏 (Ctrl+B)"
          }
          aria-label="切换侧边栏"
        >
          <PanelLeft size={16} />
        </button>

        {/* Back and Forward Arrows (Claude Style) */}
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
            <ArrowLeft size={15} />
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
            <ArrowRight size={15} />
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
            {isRunning && (
              <span
                className="header-running-indicator"
                title="当前会话正在执行中"
              >
                <span className="running-dot-pulse" />
                <span>执行中</span>
              </span>
            )}
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

      {/* Right section: Native window caption controls */}
      <div className="header-right-zone">
        <WindowCaptionControls
          isMaximized={isMaximized}
          onMinimize={handleMinimize}
          onToggleMaximize={handleToggleMaximize}
          onClose={handleClose}
        />
      </div>
    </header>
  );
}
