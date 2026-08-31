import { useState } from "react";
import type { PanelConfig } from "../lib/config";
import { useModalDialog } from "../lib/dialog-focus";
import { launchV2Session, describeApiError } from "../lib/api";
import { Icon } from "./Icon";

interface LaunchSessionDialogProps {
  config: PanelConfig;
  onClose: () => void;
  onLaunched?: (hub_session_id: string) => void;
}

export function LaunchSessionDialog({
  config,
  onClose,
  onLaunched,
}: LaunchSessionDialogProps) {
  const dialogRef = useModalDialog<HTMLDivElement>(onClose);
  const [workspacePath, setWorkspacePath] = useState(
    "d:/ClaudeSpace/Project/solum",
  );
  const [workspaceName, setWorkspaceName] = useState("Solum");
  const [targetRunnerId, setTargetRunnerId] = useState("run_local_01");
  const [provider, setProvider] = useState<
    "claude_code" | "codex_app_server" | "antigravity"
  >("claude_code");
  const [modelId, setModelId] = useState("claude-3-7-sonnet-20250219");
  const [thinkingBudget, setThinkingBudget] = useState(8000);
  const [permissionMode, setPermissionMode] = useState<
    "plan_only" | "ask_all_edits" | "accept_edits" | "bypass_permissions"
  >("ask_all_edits");
  const [initialPrompt, setInitialPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | undefined>();

  const providerModels: Record<string, Array<{ id: string; label: string }>> = {
    claude_code: [
      {
        id: "claude-3-7-sonnet-20250219",
        label: "Claude 3.7 Sonnet (Thinking)",
      },
      { id: "claude-3-5-haiku-20241022", label: "Claude 3.5 Haiku" },
    ],
    codex_app_server: [
      { id: "o3-mini", label: "o3-mini (Reasoning)" },
      { id: "gpt-4.5-preview", label: "GPT-4.5 Preview" },
    ],
    antigravity: [
      { id: "gemini-2.5-pro", label: "Gemini 2.5 Pro (Thinking)" },
      { id: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
    ],
  };

  function handleProviderChange(
    newProvider: "claude_code" | "codex_app_server" | "antigravity",
  ) {
    setProvider(newProvider);
    const models = providerModels[newProvider];
    if (models && models.length > 0 && models[0]) {
      setModelId(models[0].id);
    }
  }

  async function handleLaunch(e: React.FormEvent) {
    e.preventDefault();
    if (!workspacePath.trim()) {
      setError("请输入工作区路径");
      return;
    }
    setBusy(true);
    setError(undefined);

    const profile = {
      workspace_path: workspacePath.trim(),
      workspace_display_name: workspaceName.trim() || undefined,
      target_runner_id: targetRunnerId.trim(),
      provider,
      model: {
        model_id: modelId,
        reasoning:
          thinkingBudget > 0
            ? { enabled: true, budget_tokens: thinkingBudget }
            : undefined,
      },
      permissions: {
        approval_mode: permissionMode,
        allow_tool_execution: true,
      },
      initial_prompt: initialPrompt.trim() || undefined,
    };

    try {
      const res = await launchV2Session(config, profile);
      onLaunched?.(res.hub_session_id);
      onClose();
    } catch (err: unknown) {
      setError(describeApiError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog-scrim" role="presentation">
      <div
        ref={dialogRef}
        className="dialog-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="launch-dialog-title"
        style={{ maxWidth: "560px", width: "95%" }}
      >
        <header className="dialog-header">
          <div>
            <h2 id="launch-dialog-title">发起新 Agent 会话 (Launch Session)</h2>
            <p className="dialog-subhead">
              强类型 Launch Profile 调度多 Agent 远程控制
            </p>
          </div>
          <button
            type="button"
            className="dialog-close"
            aria-label="关闭"
            onClick={onClose}
          >
            <Icon name="close" size={16} />
          </button>
        </header>

        <form
          onSubmit={handleLaunch}
          className="dialog-body"
          style={{ display: "flex", flexDirection: "column", gap: "16px" }}
        >
          {error && (
            <div
              className="dialog-alert"
              role="alert"
              style={{
                color: "var(--red)",
                background: "var(--red-subtle)",
                padding: "8px 12px",
                borderRadius: "6px",
              }}
            >
              {error}
            </div>
          )}

          <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
            <label
              style={{
                fontSize: "12px",
                fontWeight: 600,
                color: "var(--text-muted)",
              }}
            >
              工作区路径 (Workspace Path)
            </label>
            <input
              type="text"
              value={workspacePath}
              onChange={(e) => setWorkspacePath(e.target.value)}
              placeholder="e.g. d:/ClaudeSpace/Project/solum"
              required
              style={{
                padding: "8px 12px",
                borderRadius: "6px",
                border: "1px solid var(--line)",
                background: "var(--paper)",
              }}
            />
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1fr 1fr",
              gap: "12px",
            }}
          >
            <div
              style={{ display: "flex", flexDirection: "column", gap: "6px" }}
            >
              <label
                style={{
                  fontSize: "12px",
                  fontWeight: 600,
                  color: "var(--text-muted)",
                }}
              >
                Agent 平台 (Provider)
              </label>
              <select
                value={provider}
                onChange={(e) => handleProviderChange(e.target.value as any)}
                style={{
                  padding: "8px 12px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  background: "var(--paper)",
                }}
              >
                <option value="claude_code">Claude Code</option>
                <option value="codex_app_server">OpenAI Codex</option>
                <option value="antigravity">Google Antigravity</option>
              </select>
            </div>

            <div
              style={{ display: "flex", flexDirection: "column", gap: "6px" }}
            >
              <label
                style={{
                  fontSize: "12px",
                  fontWeight: 600,
                  color: "var(--text-muted)",
                }}
              >
                模型选择 (Model)
              </label>
              <select
                value={modelId}
                onChange={(e) => setModelId(e.target.value)}
                style={{
                  padding: "8px 12px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  background: "var(--paper)",
                }}
              >
                {(providerModels[provider] ?? []).map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1fr 1fr",
              gap: "12px",
            }}
          >
            <div
              style={{ display: "flex", flexDirection: "column", gap: "6px" }}
            >
              <label
                style={{
                  fontSize: "12px",
                  fontWeight: 600,
                  color: "var(--text-muted)",
                }}
              >
                会话权限模式 (Permission Mode)
              </label>
              <select
                value={permissionMode}
                onChange={(e) => setPermissionMode(e.target.value as any)}
                style={{
                  padding: "8px 12px",
                  borderRadius: "6px",
                  border: "1px solid var(--line)",
                  background: "var(--paper)",
                }}
              >
                <option value="ask_all_edits">
                  ask_all_edits (每次编辑与命令审批 - 推荐)
                </option>
                <option value="accept_edits">
                  accept_edits (自动允许编辑，审批Shell)
                </option>
                <option value="plan_only">plan_only (只读架构审查)</option>
                <option value="bypass_permissions">
                  bypass_permissions (全自动放行)
                </option>
              </select>
            </div>

            <div
              style={{ display: "flex", flexDirection: "column", gap: "6px" }}
            >
              <label
                style={{
                  fontSize: "12px",
                  fontWeight: 600,
                  color: "var(--text-muted)",
                }}
              >
                思考预算 (Thinking Budget: {thinkingBudget} Tokens)
              </label>
              <input
                type="range"
                min="0"
                max="64000"
                step="4000"
                value={thinkingBudget}
                onChange={(e) => setThinkingBudget(Number(e.target.value))}
                style={{ marginTop: "8px" }}
              />
            </div>
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
            <label
              style={{
                fontSize: "12px",
                fontWeight: 600,
                color: "var(--text-muted)",
              }}
            >
              初始提示词 (Initial Prompt - 可选)
            </label>
            <textarea
              value={initialPrompt}
              onChange={(e) => setInitialPrompt(e.target.value)}
              placeholder="输入任务描述或指令，如：检查协议并给出重构方案..."
              rows={3}
              style={{
                padding: "8px 12px",
                borderRadius: "6px",
                border: "1px solid var(--line)",
                background: "var(--paper)",
                resize: "vertical",
              }}
            />
          </div>

          <footer
            className="dialog-footer"
            style={{
              display: "flex",
              justifyContent: "flex-end",
              gap: "8px",
              marginTop: "8px",
            }}
          >
            <button
              type="button"
              className="btn btn-secondary"
              onClick={onClose}
              disabled={busy}
            >
              取消
            </button>
            <button type="submit" className="btn btn-primary" disabled={busy}>
              {busy ? "正在启动..." : "立即创建并接入 (Launch)"}
            </button>
          </footer>
        </form>
      </div>
    </div>
  );
}
