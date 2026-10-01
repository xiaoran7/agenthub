import { Channel, invoke } from "@tauri-apps/api/core";
import {
  ArrowUp,
  Folder,
  FolderCheck,
  RefreshCw,
  Sparkles,
  Radio,
} from "lucide-react";
import React, {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { useAgentModels } from "../../agentModels";
import { PROVIDER_LIST, agentLabel } from "../../presentation";
import { isPluginEnabled } from "../../plugins";
import type { ConversationMessage } from "../../turn-stream";
import { normalizeAgentType, type Session, type SessionTurnEvent } from "../../types";
import { seedConversation } from "../../conversationCache";
import { errorMessage } from "../auth/AuthScreen";
import { SelectDropdown } from "../SelectDropdown";
import {
  PERMISSION_OPTIONS,
  isManagedProvider,
  loadRecentCwds,
  pickFolderDialog,
  rememberRecentCwd,
} from "./LaunchSessionModal";

interface NewSessionStageProps {
  initialProvider?: Session["agent_type"];
  onLaunched: (
    agentSessionId: string,
    agentType: Session["agent_type"],
  ) => void;
  onOpenHostControl: () => void;
  hostPhase?:
    "loading" | "starting" | "stopped" | "local" | "connecting" | "connected";
  workspaceCount?: number;
  sessionCount?: number;
  displayName?: string;
}

const phaseLabels = {
  loading: { label: "正在检查连接…", tone: "yellow" },
  starting: { label: "正在启动本机服务…", tone: "yellow" },
  stopped: { label: "本机服务未启动", tone: "gray" },
  local: { label: "本机会话 · 远程连接未开启", tone: "blue" },
  connecting: { label: "正在连接远程服务…", tone: "yellow" },
  connected: { label: "远程已连接", tone: "green" },
} as const;

export function NewSessionStage({
  initialProvider = "claude_code",
  onLaunched,
  onOpenHostControl,
  hostPhase = "local",
  workspaceCount = 0,
  sessionCount = 0,
  displayName,
}: NewSessionStageProps) {
  const [agentType, setAgentType] =
    useState<Session["agent_type"]>(initialProvider);
  const models = useAgentModels(agentType);
  const [model, setModel] = useState<string>("");
  const [permission, setPermission] = useState("");
  const [cwd, setCwd] = useState(() => loadRecentCwds()[0] || "");
  const [prompt, setPrompt] = useState("");
  const [launching, setLaunching] = useState(false);
  const [error, setError] = useState<string>();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const recentCwds = useMemo(() => loadRecentCwds(), []);

  const currentPhase = phaseLabels[hostPhase];

  useEffect(() => {
    setAgentType(initialProvider);
  }, [initialProvider]);

  useEffect(() => {
    if (!models.some((option) => option.value === model)) {
      setModel(models[0]?.value ?? "");
    }
  }, [model, models]);

  useEffect(() => {
    if (!cwd) {
      const stored = loadRecentCwds()[0];
      if (stored) setCwd(stored);
    }
    textareaRef.current?.focus();
  }, [cwd]);

  const chooseCwd = async () => {
    try {
      const picked = await pickFolderDialog(cwd);
      if (picked) setCwd(picked);
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const handleLaunch = async (e?: FormEvent) => {
    if (e) e.preventDefault();
    if (launching || !prompt.trim()) return;

    if (!isManagedProvider(agentType)) {
      setError("该 Provider 当前仅支持发现和历史读取");
      return;
    }
    if (!cwd.trim()) {
      setError("请先指定或选择一个本地工作目录");
      return;
    }

    setLaunching(true);
    setError(undefined);

    try {
      const onEvent = new Channel<SessionTurnEvent>();
      const result = await invoke<{
        agent_session_id: string;
        agent_type: string;
        response?: string;
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
      const normType = normalizeAgentType(agentType);
      const initialMessages: ConversationMessage[] = [
        { role: "user", content: prompt.trim() },
      ];
      if (result.response && result.response.trim()) {
        initialMessages.push({
          role: "assistant",
          content: result.response.trim(),
        });
      }
      seedConversation(`${normType}:${result.agent_session_id}`, initialMessages);
      onLaunched(result.agent_session_id, agentType);
    } catch (err) {
      setError(errorMessage(err));
      setLaunching(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleLaunch();
    }
  };

  // Filter available providers: managed or plugin enabled
  const availableProviders = useMemo(
    () =>
      PROVIDER_LIST.filter(
        (item) => item.managed || isPluginEnabled(item.type),
      ),
    [],
  );

  useEffect(() => {
    if (!availableProviders.some((p) => p.type === agentType)) {
      setAgentType(availableProviders[0]?.type ?? "claude_code");
    }
  }, [agentType, availableProviders]);

  return (
    <div className="claude-new-chat-container">
      <div className="claude-new-chat-content">
        {/* Top greeting area (Claude Style) */}
        <header className="claude-new-chat-header">
          <div className="claude-greeting-badge" role="status">
            <Radio
              size={12}
              className={`pulse-dot tone-${currentPhase.tone}`}
            />
            <span>{currentPhase.label}</span>
          </div>
          <h1 className="claude-greeting-title">
            {displayName ? `你好，${displayName}` : "开始新的 Agent 任务"}
          </h1>
          <p className="claude-greeting-desc">
            选择目标 Agent 与工作空间，随时随地开启或继续会话
          </p>
        </header>

        {/* Central interactive Composer Card (Claude Style) */}
        <div className="claude-new-composer-card">
          {/* Card Top Toolbar: Provider pills + Model + Permission + CWD */}
          <div className="composer-card-top-toolbar">
            <div className="composer-provider-pills">
              {availableProviders.map((item) => (
                <button
                  key={item.type}
                  type="button"
                  className={`provider-pill-btn ${agentType === item.type ? "active" : ""}`}
                  data-provider={item.type}
                  disabled={!isManagedProvider(item.type)}
                  onClick={() => {
                    setAgentType(item.type);
                    setPermission("");
                  }}
                  title={
                    isManagedProvider(item.type)
                      ? `切换至 ${item.label}`
                      : `${item.label} 仅只读`
                  }
                >
                  <span className="pill-dot" />
                  <span>{item.shortLabel}</span>
                </button>
              ))}
            </div>

            <div className="composer-meta-controls">
              {/* Dynamic model selector */}
              {models.length > 0 && (
                <div className="composer-dropdown-wrapper">
                  <SelectDropdown
                    value={model}
                    options={models.map((opt) => ({
                      value: opt.value,
                      label: opt.label,
                      desc: opt.desc,
                    }))}
                    onChange={(val) => setModel(val)}
                    size="sm"
                    className="ghost"
                    ariaLabel="选择模型"
                  />
                </div>
              )}

              {/* Permission selector if applicable */}
              {PERMISSION_OPTIONS[agentType]?.length > 1 && (
                <div className="composer-dropdown-wrapper">
                  <SelectDropdown
                    value={permission}
                    options={PERMISSION_OPTIONS[agentType].map((opt) => ({
                      value: opt.value,
                      label: opt.label,
                    }))}
                    onChange={(val) => setPermission(val)}
                    size="sm"
                    className="ghost"
                    ariaLabel="运行权限"
                  />
                </div>
              )}
            </div>
          </div>

          {/* Directory Picker Row */}
          <div className="composer-cwd-row">
            <div className="cwd-display-box" onClick={chooseCwd} title={cwd}>
              <Folder size={14} className="folder-icon" />
              <span className="cwd-text">
                {cwd
                  ? cwd.split(/[/\\]/).filter(Boolean).slice(-2).join("/") ||
                    cwd
                  : "点击选择本地工作目录..."}
              </span>
              <button
                type="button"
                className="cwd-change-btn"
                onClick={(e) => {
                  e.stopPropagation();
                  void chooseCwd();
                }}
              >
                更改
              </button>
            </div>

            {/* Quick recent CWDs */}
            {recentCwds.length > 0 && (
              <div className="recent-cwds-chips">
                {recentCwds.slice(0, 3).map((recent) => (
                  <button
                    key={recent}
                    type="button"
                    className={`recent-chip ${cwd === recent ? "current" : ""}`}
                    onClick={() => setCwd(recent)}
                    title={recent}
                  >
                    {recent.split(/[/\\]/).filter(Boolean).pop() || recent}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Main Prompt Textarea */}
          <div className="composer-textarea-area">
            <textarea
              ref={textareaRef}
              rows={4}
              placeholder={`输入任务或提示词，按 Enter 立即启动 ${agentLabel(agentType)} 会话...`}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              onKeyDown={handleKeyDown}
              disabled={launching}
            />
          </div>

          {error && <div className="composer-card-error">{error}</div>}

          {/* Card Bottom Toolbar */}
          <div className="composer-card-bottom-toolbar">
            <div className="composer-hint-left">
              <span>Enter 发送 · Shift+Enter 换行</span>
            </div>

            <button
              type="button"
              className="composer-launch-submit-btn"
              disabled={launching || !prompt.trim() || !cwd.trim()}
              onClick={() => void handleLaunch()}
              title={
                !cwd.trim()
                  ? "请选择工作目录"
                  : !prompt.trim()
                    ? "请输入提示词"
                    : "启动并进入会话"
              }
              aria-label="启动会话"
            >
              {launching ? (
                <RefreshCw className="spin" size={16} />
              ) : (
                <ArrowUp size={16} />
              )}
            </button>
          </div>
        </div>

        {/* Footer info & secondary action */}
        <footer className="claude-new-chat-footer">
          <button
            type="button"
            className="host-control-link-btn"
            onClick={onOpenHostControl}
          >
            <FolderCheck size={14} />
            <span>管理工作目录与远程连接 (已授权 {workspaceCount} 个)</span>
          </button>
          {sessionCount > 0 && (
            <span className="session-count-tip">
              左侧历史列表中有 {sessionCount} 个会话可供随时恢复
            </span>
          )}
        </footer>
      </div>
    </div>
  );
}
