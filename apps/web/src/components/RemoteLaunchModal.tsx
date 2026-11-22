import { useRef } from "react";
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
  useDialogFocus(open, dialogRef);
  if (!open) return null;

  const showCodexSandbox = !nativeId && agentType === "codex";

  return (
    <div
      className="remote-modal-backdrop"
      role="presentation"
      onClick={onClose}
    >
      <div
        ref={dialogRef}
        className="remote-launch"
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-launch-title"
        onClick={(event) => event.stopPropagation()}
      >
        <form
          onSubmit={(event) => {
            event.preventDefault();
            onSubmit();
          }}
        >
          <header>
            <h2 id="remote-launch-title">在电脑上开始</h2>
            <button
              type="button"
              className="remote-modal-close"
              aria-label="关闭"
              onClick={onClose}
            >
              ×
            </button>
          </header>
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
          <div className="remote-launch-field">
            <span className="remote-launch-label">会话</span>
            <Select
              label="会话"
              value={nativeId}
              options={launchSessionOptions}
              onChange={onNativeIdChange}
            />
          </div>
          {!nativeId && (
            <div className="remote-launch-field">
              <span className="remote-launch-label">Agent 引擎</span>
              <Select
                label="Agent 引擎"
                value={agentType}
                options={launchAgentOptions}
                onChange={(next) => onAgentTypeChange(next as LaunchAgentType)}
              />
            </div>
          )}
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
                所选电脑尚未上报该引擎的模型列表，将沿用本机默认配置。
              </span>
            )}
          </div>
          {showCodexSandbox && (
            <div className="remote-launch-field">
              <span className="remote-launch-label">文件沙箱范围</span>
              <Select
                label="文件沙箱范围"
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
                if (mode === "acceptEdits" || mode === "bypassPermissions") {
                  onSandboxChange("workspace-write");
                } else if (mode === "plan") {
                  onSandboxChange("read-only");
                }
              }}
            />
          </div>
          <p className="remote-launch-note">
            执行发生在所选电脑，消息与工具审批会实时同步到当前网页。
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
              {busy ? "正在启动…" : nativeId ? "恢复会话" : "创建会话"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
