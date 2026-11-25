import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { RemotePermissionMode } from "../lib/remote";
import { useDialogFocus } from "../lib/dialog-focus";
import { Select, type SelectOption } from "./Select";

export type LaunchAgentType = string;

interface RemoteLaunchModalProps {
  open: boolean;
  busy: boolean;
  runner: string;
  workspace: string;
  nativeId: string;
  agentType: LaunchAgentType;
  model: string;
  sandbox: "read-only" | "workspace-write";
  launchPermissionMode: RemotePermissionMode;
  onlineDeviceOptions: readonly SelectOption[];
  launchWorkspaceOptions: readonly SelectOption[];
  launchSessionOptions: readonly SelectOption[];
  launchAgentOptions: readonly SelectOption[];
  launchModelOptions: readonly SelectOption[];
  sandboxOptions: readonly SelectOption[];
  permissionModeOptions: readonly SelectOption[];
  onClose: () => void;
  onSubmit: () => void;
  onRunnerChange: (runnerId: string) => void;
  onWorkspaceChange: (workspacePath: string) => void;
  onNativeIdChange: (nativeId: string) => void;
  onAgentTypeChange: (agentType: LaunchAgentType) => void;
  onModelChange: (model: string) => void;
  onSandboxChange: (sandbox: "read-only" | "workspace-write") => void;
  onPermissionModeChange: (mode: RemotePermissionMode) => void;
}

function AgentOptionGlyph({ id }: { id: string }) {
  if (id === "codex") {
    return (
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <polyline points="4 17 10 11 4 5" />
        <line x1="12" y1="19" x2="20" y2="19" />
      </svg>
    );
  }
  if (id === "claude_code" || id === "claude_desktop") {
    return (
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M12 3v18M4.2 7.5l15.6 9M19.8 7.5l-15.6 9" />
      </svg>
    );
  }
  if (id === "antigravity") {
    return (
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="4" />
        <ellipse
          cx="12"
          cy="12"
          rx="10"
          ry="4.5"
          transform="rotate(-24 12 12)"
        />
      </svg>
    );
  }
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3l2.6 6.4L21 12l-6.4 2.6L12 21l-2.6-6.4L3 12l6.4-2.6z" />
    </svg>
  );
}

export function RemoteLaunchModal({
  open,
  busy,
  runner,
  workspace,
  nativeId,
  agentType,
  model,
  sandbox,
  launchPermissionMode,
  onlineDeviceOptions,
  launchWorkspaceOptions,
  launchSessionOptions,
  launchAgentOptions,
  launchModelOptions,
  sandboxOptions,
  permissionModeOptions,
  onClose,
  onSubmit,
  onRunnerChange,
  onWorkspaceChange,
  onNativeIdChange,
  onAgentTypeChange,
  onModelChange,
  onSandboxChange,
  onPermissionModeChange,
}: RemoteLaunchModalProps) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  useDialogFocus(open, dialogRef);
  useEffect(() => {
    if (!open) setAdvancedOpen(false);
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [open, busy, onClose]);
  if (!open) return null;

  const showCodexSandbox = !nativeId && agentType === "codex";
  const showAdvanced = advancedOpen || Boolean(nativeId);

  return createPortal(
    <div
      className="remote-modal-backdrop"
      role="presentation"
      onClick={(event) => {
        if (!busy && event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className="remote-launch"
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-launch-title"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="remote-launch-handle" aria-hidden="true" />
        <form
          onSubmit={(event) => {
            event.preventDefault();
            onSubmit();
          }}
        >
          <header>
            <h2 id="remote-launch-title">
              {nativeId ? "恢复电脑会话" : "新建任务"}
            </h2>
            <button
              type="button"
              className="remote-modal-close"
              aria-label="关闭"
              disabled={busy}
              onClick={() => {
                if (!busy) onClose();
              }}
            >
              ×
            </button>
          </header>

          {!nativeId && launchAgentOptions.length > 0 && (
            <div className="remote-launch-field">
              <span className="remote-launch-label">选择执行 Agent</span>
              <div
                className="remote-launch-agent-grid"
                role="radiogroup"
                aria-label="选择执行 Agent"
              >
                {launchAgentOptions.map((opt) => {
                  const active = opt.value === agentType;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      role="radio"
                      aria-checked={active}
                      className={`remote-launch-agent-card ${active ? "active" : ""}`}
                      onClick={() =>
                        onAgentTypeChange(opt.value as LaunchAgentType)
                      }
                    >
                      <span className="remote-launch-agent-icon">
                        <AgentOptionGlyph id={opt.value} />
                      </span>
                      <span className="remote-launch-agent-name">
                        {opt.label}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          <div className="remote-launch-row-2">
            <div className="remote-launch-field">
              <span className="remote-launch-label">电脑</span>
              <Select
                label="电脑"
                value={runner}
                placeholder="选择在线电脑"
                options={onlineDeviceOptions}
                onChange={onRunnerChange}
              />
            </div>
            <div className="remote-launch-field">
              <span className="remote-launch-label">工作区</span>
              <Select
                label="工作区"
                value={workspace}
                placeholder="选择已授权目录"
                options={launchWorkspaceOptions}
                onChange={onWorkspaceChange}
              />
            </div>
          </div>

          <button
            type="button"
            className="remote-launch-advanced-toggle"
            aria-expanded={showAdvanced}
            onClick={() => setAdvancedOpen((prev) => !prev)}
          >
            <span>
              {showAdvanced
                ? "收起更多选项"
                : "更多选项（模型 · 权限 · 恢复已有会话）"}
            </span>
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              style={{
                transform: showAdvanced ? "rotate(180deg)" : "none",
                transition: "transform 0.16s ease",
              }}
              aria-hidden="true"
            >
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </button>

          {showAdvanced && (
            <div className="remote-launch-advanced-panel">
              <div className="remote-launch-field">
                <span className="remote-launch-label">会话</span>
                <Select
                  label="会话"
                  value={nativeId}
                  options={launchSessionOptions}
                  onChange={onNativeIdChange}
                />
              </div>
              <div className="remote-launch-field">
                <span className="remote-launch-label">模型</span>
                <Select
                  label="模型"
                  value={model}
                  options={launchModelOptions}
                  onChange={onModelChange}
                />
                {launchModelOptions.length <= 1 && (
                  <span className="remote-launch-hint">
                    所选电脑尚未上报该引擎的模型列表，将使用引擎默认配置。
                  </span>
                )}
              </div>
              {showCodexSandbox && (
                <div className="remote-launch-field">
                  <span className="remote-launch-label">文件沙箱（Codex）</span>
                  <Select
                    label="文件沙箱（Codex）"
                    value={sandbox}
                    options={sandboxOptions}
                    onChange={(next) =>
                      onSandboxChange(next as "read-only" | "workspace-write")
                    }
                  />
                </div>
              )}
              <div className="remote-launch-field">
                <span className="remote-launch-label">执行权限模式</span>
                <Select
                  label="执行权限模式"
                  value={launchPermissionMode}
                  options={permissionModeOptions}
                  onChange={(next) => {
                    const mode = next as RemotePermissionMode;
                    onPermissionModeChange(mode);
                    if (
                      mode === "acceptEdits" ||
                      mode === "bypassPermissions"
                    ) {
                      onSandboxChange("workspace-write");
                    } else if (mode === "plan") {
                      onSandboxChange("read-only");
                    }
                  }}
                />
              </div>
            </div>
          )}

          <p className="remote-launch-note">
            执行发生在所选电脑，消息、工具与审批会实时同步到当前端。
          </p>
          <div className="remote-modal-actions">
            <button
              type="button"
              className="remote-modal-cancel"
              disabled={busy}
              onClick={onClose}
            >
              取消
            </button>
            <button
              type="submit"
              className="remote-primary remote-modal-submit"
              disabled={busy || !workspace || !runner}
            >
              {busy ? "正在启动…" : nativeId ? "恢复会话" : "开始任务"}
            </button>
          </div>
        </form>
      </div>
    </div>,
    document.body,
  );
}
