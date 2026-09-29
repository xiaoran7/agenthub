import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import React, { useEffect, useState } from "react";

export function useWindowControls() {
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

  return {
    isMaximized,
    handleMinimize,
    handleToggleMaximize,
    handleClose,
    handleStartDrag,
  };
}

export function WindowCaptionControls({
  isMaximized,
  onMinimize,
  onToggleMaximize,
  onClose,
}: {
  isMaximized: boolean;
  onMinimize: (e?: React.MouseEvent) => void;
  onToggleMaximize: (e?: React.MouseEvent) => void;
  onClose: (e?: React.MouseEvent) => void;
}) {
  return (
    <div className="window-caption-controls">
      <button
        type="button"
        className="window-control-btn minimize"
        onMouseDown={(e) => e.stopPropagation()}
        onClick={onMinimize}
        title="最小化"
        aria-label="最小化"
      >
        <Minus size={13} />
      </button>
      <button
        type="button"
        className="window-control-btn maximize"
        onMouseDown={(e) => e.stopPropagation()}
        onClick={onToggleMaximize}
        title={isMaximized ? "向下还原" : "最大化"}
        aria-label={isMaximized ? "向下还原" : "最大化"}
      >
        {isMaximized ? <Copy size={11} /> : <Square size={11} />}
      </button>
      <button
        type="button"
        className="window-control-btn close"
        onMouseDown={(e) => e.stopPropagation()}
        onClick={onClose}
        title="关闭 (Alt+F4)"
        aria-label="关闭"
      >
        <X size={14} />
      </button>
    </div>
  );
}

export function SimpleTitleBar({
  title,
  showControls = true,
}: {
  title?: string;
  showControls?: boolean;
}) {
  const {
    isMaximized,
    handleMinimize,
    handleToggleMaximize,
    handleClose,
    handleStartDrag,
  } = useWindowControls();

  return (
    <header
      className="simple-window-titlebar"
      data-tauri-drag-region
      onMouseDown={handleStartDrag}
      onDoubleClick={handleToggleMaximize}
    >
      <div className="simple-titlebar-drag" data-tauri-drag-region>
        {title && <span className="simple-titlebar-title">{title}</span>}
      </div>
      {showControls && (
        <WindowCaptionControls
          isMaximized={isMaximized}
          onMinimize={handleMinimize}
          onToggleMaximize={handleToggleMaximize}
          onClose={handleClose}
        />
      )}
    </header>
  );
}
