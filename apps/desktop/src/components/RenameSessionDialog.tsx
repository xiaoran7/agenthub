import React, { useState, useEffect, useRef } from "react";
import { Pencil, X, Check } from "lucide-react";
import type { Session } from "../types";
import { sessionDisplayTitle } from "../presentation";

interface RenameSessionDialogProps {
  session: Session | null;
  onClose: () => void;
  onRename: (session: Session, newTitle: string) => void;
}

export function RenameSessionDialog({
  session,
  onClose,
  onRename,
}: RenameSessionDialogProps) {
  const [title, setTitle] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (session) {
      setTitle(sessionDisplayTitle(session));
      setTimeout(() => {
        inputRef.current?.focus();
        inputRef.current?.select();
      }, 50);
    }
  }, [session]);

  if (!session) return null;

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const clean = title.trim();
    if (!clean) return;
    onRename(session, clean);
    onClose();
  };

  return (
    <div className="panel-scrim launch-session-scrim" onMouseDown={onClose}>
      <div
        className="launch-modal-card rename-modal-card"
        onMouseDown={(e) => e.stopPropagation()}
        style={{ maxWidth: "460px" }}
      >
        <header className="launch-modal-header">
          <div>
            <span className="launch-modal-mark">
              <Pencil size={18} />
            </span>
            <span>
              <small>RENAME SESSION</small>
              <h3>自定义会话名称</h3>
            </span>
          </div>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            aria-label="关闭重命名"
          >
            <X size={18} />
          </button>
        </header>

        <form onSubmit={handleSubmit} className="launch-modal-form">
          <label className="launch-form-field">
            <span>会话名称</span>
            <input
              ref={inputRef}
              type="text"
              className="launch-cwd-picker"
              style={{
                width: "100%",
                padding: "8px 12px",
                borderRadius: "6px",
                border:
                  "1px solid var(--border-color, rgba(255, 255, 255, 0.15))",
                background: "var(--input-bg, rgba(0, 0, 0, 0.2))",
                color: "inherit",
                fontSize: "14px",
                outline: "none",
              }}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="输入易于辨识的会话名称..."
              maxLength={64}
              required
            />
          </label>

          <div
            style={{
              fontSize: "12px",
              opacity: 0.7,
              display: "flex",
              flexDirection: "column",
              gap: "4px",
              margin: "4px 0 12px",
            }}
          >
            <span>
              会话 ID：<code>{session.agent_session_id}</code>
            </span>
            {session.payload.project.display_name && (
              <span>工作区：{session.payload.project.display_name}</span>
            )}
          </div>

          <footer
            style={{
              display: "flex",
              justifyContent: "flex-end",
              gap: "8px",
              marginTop: "8px",
            }}
          >
            <button
              type="button"
              className="outline-button"
              onClick={onClose}
              style={{ padding: "6px 14px" }}
            >
              取消
            </button>
            <button
              type="submit"
              className="primary-button"
              disabled={!title.trim()}
              style={{
                padding: "6px 16px",
                display: "inline-flex",
                alignItems: "center",
                gap: "6px",
              }}
            >
              <Check size={14} />
              <span>保存名称</span>
            </button>
          </footer>
        </form>
      </div>
    </div>
  );
}
