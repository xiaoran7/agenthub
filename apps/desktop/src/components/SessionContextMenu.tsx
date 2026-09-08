import React, { useEffect, useRef } from "react";
import {
  Pin,
  Copy,
  FolderOpen,
  PlusCircle,
  FileText,
  CheckCircle2,
} from "lucide-react";
import type { Session } from "../types";
import { sessionDisplayTitle } from "../presentation";

export interface ContextMenuPosition {
  x: number;
  y: number;
  session: Session;
}

interface SessionContextMenuProps {
  position: ContextMenuPosition | null;
  onClose: () => void;
  isPinned: boolean;
  onTogglePin: (sessionKey: string) => void;
  onNewSessionForAgent: (agentType: Session["agent_type"]) => void;
  onShowToast: (message: string) => void;
}

export function SessionContextMenu({
  position,
  onClose,
  isPinned,
  onTogglePin,
  onNewSessionForAgent,
  onShowToast,
}: SessionContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        onClose();
      }
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }
    if (position) {
      document.addEventListener("mousedown", handleClickOutside);
      window.addEventListener("keydown", handleKeyDown);
      return () => {
        document.removeEventListener("mousedown", handleClickOutside);
        window.removeEventListener("keydown", handleKeyDown);
      };
    }
  }, [position, onClose]);

  if (!position) return null;

  const { session } = position;
  const key = `${session.device_id}:${session.agent_type}:${session.agent_session_id}`;
  const title = sessionDisplayTitle(session);
  const projectPath = session.payload.project.display_name || "";

  const copyToClipboard = async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      onShowToast(`已复制${label}到剪贴板`);
    } catch {
      onShowToast(`复制失败，请重试`);
    }
    onClose();
  };

  // Adjust coordinates so menu stays inside viewport
  const menuWidth = 200;
  const menuHeight = 220;
  const x = Math.min(position.x, window.innerWidth - menuWidth - 10);
  const y = Math.min(position.y, window.innerHeight - menuHeight - 10);

  return (
    <div
      ref={menuRef}
      className="session-context-menu"
      style={{ left: `${Math.max(10, x)}px`, top: `${Math.max(10, y)}px` }}
      role="menu"
    >
      <div className="context-menu-header">
        <strong className="context-title" title={title}>
          {title}
        </strong>
      </div>
      <div className="context-menu-divider" />

      <button
        type="button"
        className="context-menu-item"
        role="menuitem"
        onClick={() => {
          onTogglePin(key);
          onShowToast(isPinned ? "已取消置顶" : "已置顶该会话");
          onClose();
        }}
      >
        <Pin size={14} className={isPinned ? "pinned-icon" : ""} />
        <span>{isPinned ? "取消置顶会话" : "置顶当前会话"}</span>
      </button>

      <button
        type="button"
        className="context-menu-item"
        role="menuitem"
        onClick={() => copyToClipboard(session.agent_session_id, "会话 ID")}
      >
        <Copy size={14} />
        <span>复制会话 ID</span>
      </button>

      <button
        type="button"
        className="context-menu-item"
        role="menuitem"
        onClick={() => copyToClipboard(title, "会话标题")}
      >
        <FileText size={14} />
        <span>复制会话名称</span>
      </button>

      {projectPath && (
        <button
          type="button"
          className="context-menu-item"
          role="menuitem"
          onClick={() => copyToClipboard(projectPath, "工作区路径")}
        >
          <FolderOpen size={14} />
          <span>复制项目路径</span>
        </button>
      )}

      <div className="context-menu-divider" />

      <button
        type="button"
        className="context-menu-item"
        role="menuitem"
        onClick={() => {
          onNewSessionForAgent(session.agent_type);
          onClose();
        }}
      >
        <PlusCircle size={14} />
        <span>在此 Agent 新建任务</span>
      </button>
    </div>
  );
}
