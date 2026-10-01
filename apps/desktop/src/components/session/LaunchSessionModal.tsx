import { Channel, invoke } from "@tauri-apps/api/core";
import { Folder, Play, RefreshCw, Sparkles, X } from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { useAgentModels } from "../../agentModels";
import { PROVIDER_LIST } from "../../presentation";
import { isPluginEnabled } from "../../plugins";
import type { Session, SessionTurnEvent } from "../../types";
import { errorMessage } from "../auth/AuthScreen";
import { SelectDropdown } from "../SelectDropdown";

export const isManagedProvider = (
  agentType: string,
): agentType is Session["agent_type"] =>
  agentType === "codex" ||
  agentType === "claude_code" ||
  agentType === "zcode" ||
  agentType === "antigravity" ||
  agentType === "grok_build";

export const PERMISSION_OPTIONS: Record<
  Session["agent_type"],
  Array<{ value: string; label: string }>
> = {
  claude_code: [
    { value: "", label: "默认权限" },
    { value: "acceptEdits", label: "接受编辑" },
    { value: "plan", label: "计划模式" },
    { value: "bypassPermissions", label: "完全访问" },
  ],
  codex: [
    { value: "", label: "默认沙箱" },
    { value: "read-only", label: "只读" },
    { value: "workspace-write", label: "工作区写入" },
    { value: "danger-full-access", label: "完全访问" },
  ],
  antigravity: [{ value: "", label: "默认权限" }],
  grok_build: [{ value: "", label: "默认权限" }],
  zcode: [
    { value: "", label: "默认执行策略" },
    { value: "auto-approve", label: "全自动执行" },
    { value: "on-request", label: "写操作确认" },
    { value: "read-only", label: "只读审查" },
  ],
};

export function loadRecentCwds(): string[] {
  try {
    const stored = window.localStorage.getItem("agenthub:recent-cwds");
    const parsed = stored ? (JSON.parse(stored) as string[]) : [];
    return Array.isArray(parsed)
      ? parsed.filter((item) => typeof item === "string")
      : [];
  } catch {
    return [];
  }
}

export function rememberRecentCwd(cwd: string) {
  const next = [cwd, ...loadRecentCwds().filter((item) => item !== cwd)].slice(
    0,
    5,
  );
  try {
    window.localStorage.setItem("agenthub:recent-cwds", JSON.stringify(next));
  } catch {
    // ignore
  }
}

export async function pickFolderDialog(
  start?: string,
): Promise<string | undefined> {
  if (!("__TAURI_INTERNALS__" in window)) return undefined;
  const picked = await invoke<string | null>("pick_folder", {
    startDir: start || null,
  });
  return picked ?? undefined;
}

export function LaunchSessionModal({
  open,
  initialAgentType = "claude_code",
  onClose,
  onLaunched,
}: {
  open: boolean;
  initialAgentType?: Session["agent_type"];
  onClose: () => void;
  onLaunched: (
    agentSessionId: string,
    agentType: Session["agent_type"],
  ) => void;
}) {
  const [agentType, setAgentType] =
    useState<Session["agent_type"]>(initialAgentType);
  const models = useAgentModels(agentType);
  const [model, setModel] = useState<string>("");
  const [permission, setPermission] = useState("");
  const [cwd, setCwd] = useState(() => loadRecentCwds()[0] || "");
  const [prompt, setPrompt] = useState("");
  const [launching, setLaunching] = useState(false);
  const [error, setError] = useState<string>();
  const recentCwds = useMemo(() => loadRecentCwds(), [open]);

  useEffect(() => {
    if (open) {
      setAgentType(initialAgentType);
      setPermission("");
      setPrompt("");
      setError(undefined);
      setCwd((current) => current || loadRecentCwds()[0] || "");
    }
  }, [initialAgentType, open]);

  useEffect(() => {
    if (!models.some((option) => option.value === model)) {
      setModel(models[0]?.value ?? "");
    }
  }, [model, models]);

  const chooseCwd = async () => {
    try {
      const picked = await pickFolderDialog(cwd);
      if (picked) setCwd(picked);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (launching || prompt.trim() === "") return;
    if (!isManagedProvider(agentType)) {
      setError("该 Provider 当前仅支持发现和历史读取");
      return;
    }
    if (!cwd.trim()) {
      setError("请选择工作目录，Agent 将在该目录中启动");
      return;
    }
    setLaunching(true);
    setError(undefined);
    try {
      const onEvent = new Channel<SessionTurnEvent>();
      const result = await invoke<{
        agent_session_id: string;
        agent_type: string;
      }>("launch_new_session", {
        input: {
          agent_type: agentType,
          cwd: cwd.trim(),
          prompt: prompt.trim(),
          model: model || undefined,
          permission_mode: permission || undefined,
        },
        onEvent,
      });
      rememberRecentCwd(cwd.trim());
      onLaunched(result.agent_session_id, agentType);
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setLaunching(false);
    }
  };

  if (!open) return null;

  return (
    <div className="panel-scrim launch-session-scrim" onMouseDown={onClose}>
      <div
        className="launch-modal-card"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="launch-modal-header">
          <div>
            <span className="launch-modal-mark">
              <Sparkles size={18} />
            </span>
            <span>
              <small>NEW LOCAL SESSION</small>
              <h3>发起 Agent 会话</h3>
            </span>
          </div>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            aria-label="关闭新建会话"
          >
            <X size={18} />
          </button>
        </header>
        <form onSubmit={submit} className="launch-modal-form">
          <fieldset className="launch-provider-fieldset">
            <legend>选择目标 Agent</legend>
            <div className="launch-provider-grid">
              {PROVIDER_LIST.filter(
                (item) => item.managed || isPluginEnabled(item.type),
              ).map((item) => (
                <button
                  key={item.type}
                  type="button"
                  className={agentType === item.type ? "selected" : ""}
                  data-provider={item.type}
                  disabled={!isManagedProvider(item.type)}
                  title={
                    isManagedProvider(item.type)
                      ? `使用 ${item.label} 创建 managed Session`
                      : `${item.label} 当前仅支持发现和历史读取`
                  }
                  onClick={() => {
                    setAgentType(item.type);
                    setPermission("");
                  }}
                >
                  <span className="agent-badge-dot" />
                  <strong>{item.label}</strong>
                  <small>{item.shortLabel}</small>
                </button>
              ))}
            </div>
          </fieldset>

          <div className="launch-form-row">
            <div className="launch-form-field">
              <span>模型</span>
              <SelectDropdown
                value={model}
                options={models.map((option) => ({
                  value: option.value,
                  label: option.label,
                  desc: option.desc,
                }))}
                onChange={(val) => setModel(val)}
                disabled={models.length <= 1}
                size="md"
                ariaLabel="选择模型"
              />
            </div>
            <div className="launch-form-field">
              <span>运行权限</span>
              <SelectDropdown
                value={permission}
                options={PERMISSION_OPTIONS[agentType].map((option) => ({
                  value: option.value,
                  label: option.label,
                }))}
                onChange={(val) => setPermission(val)}
                size="md"
                ariaLabel="选择运行权限"
              />
            </div>
          </div>

          <div className="launch-form-field">
            <span>工作目录</span>
            <div className="launch-cwd-picker">
              <input
                type="text"
                placeholder="选择或输入本地工作目录绝对路径"
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
                required
              />
              <button
                type="button"
                className="outline-button browse-btn"
                onClick={chooseCwd}
                title="浏览本地目录"
              >
                <Folder size={14} />
                <span>浏览</span>
              </button>
            </div>
            {recentCwds.length > 0 && (
              <div className="launch-recent-cwds">
                <small>最近目录：</small>
                {recentCwds.map((recent) => (
                  <button
                    key={recent}
                    type="button"
                    className="tag-button"
                    onClick={() => setCwd(recent)}
                    title={recent}
                  >
                    {recent.split(/[/\\]/).filter(Boolean).slice(-2).join("/")}
                  </button>
                ))}
              </div>
            )}
          </div>
          <label className="launch-form-field">
            <span>初始任务指令</span>
            <textarea
              rows={3}
              placeholder="输入你想让 Agent 执行的第一条指令..."
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              required
              autoFocus
            />
          </label>
          {error && <p className="form-error">{error}</p>}
          <footer className="launch-modal-actions">
            <button
              type="button"
              className="outline-button"
              onClick={onClose}
              disabled={launching}
            >
              取消
            </button>
            <button
              type="submit"
              className="primary-button"
              disabled={launching || !prompt.trim() || !cwd.trim()}
            >
              {launching ? (
                <>
                  <RefreshCw className="spin" size={16} /> 正在拉起 Agent...
                </>
              ) : (
                <>
                  <Play size={16} /> 启动并进入会话
                </>
              )}
            </button>
          </footer>
        </form>
      </div>
    </div>
  );
}
