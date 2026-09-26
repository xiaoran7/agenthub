import { normalizeAgentType, type ActivityStatus, type Session } from "./types";

export function activityTime(session: Session): string {
  return session.payload?.last_activity_at ?? session.observed_at ?? "";
}

export function relativeTime(value: string, now = Date.now()): string {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return "时间未知";
  const seconds = Math.max(0, (now - timestamp) / 1_000);
  if (seconds < 60) return "刚刚";
  if (seconds < 3_600) return `${Math.floor(seconds / 60)} 分钟前`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3_600)} 小时前`;
  return `${Math.floor(seconds / 86_400)} 天前`;
}

export function hasActiveTurn(status: ActivityStatus): boolean {
  return (
    status === "WORKING" ||
    status === "WAITING_PERMISSION" ||
    status === "WAITING_INPUT"
  );
}

export function statusView(
  status: ActivityStatus,
  presence: string,
): { label: string; className: string } {
  if (status === "WORKING") return { label: "工作中", className: "working" };
  if (status === "WAITING_PERMISSION")
    return { label: "等待确认", className: "waiting" };
  if (status === "WAITING_INPUT")
    return { label: "等待输入", className: "waiting" };
  if (status === "IDLE") return { label: "空闲", className: "idle" };
  if (status === "FAILED") return { label: "失败", className: "failed" };
  if (presence === "PERSISTED")
    return { label: "历史会话", className: "history" };
  return { label: "未知", className: "history" };
}

export function agentLabel(type: Session["agent_type"]): string {
  if (type === "codex") return "ChatGPT (Codex)";
  if (type === "claude_code") return "Claude Code";
  if (type === "zcode") return "ZCode Agent";
  if (type === "antigravity") return "Google Antigravity";
  return "Grok Build";
}

export function agentShortLabel(type: Session["agent_type"]): string {
  if (type === "codex") return "ChatGPT";
  if (type === "claude_code") return "Claude";
  if (type === "zcode") return "ZCode";
  if (type === "antigravity") return "Antigravity";
  return "Grok";
}

export function permissionFieldLabel(type: Session["agent_type"]): string {
  if (type === "claude_code") return "权限模式";
  if (type === "antigravity") return "安全级别";
  if (type === "grok_build") return "Agent 配置";
  if (type === "zcode") return "执行策略";
  return "审批策略";
}

export const PROVIDER_MODELS: Record<
  Session["agent_type"],
  Array<{ value: string; label: string; desc: string }>
> = {
  claude_code: [
    {
      value: "claude-3-7-sonnet",
      label: "Claude 3.7 Sonnet",
      desc: "最强混合推理与自主开发",
    },
    {
      value: "claude-3-5-sonnet",
      label: "Claude 3.5 Sonnet",
      desc: "行业标杆通用编程",
    },
    {
      value: "claude-3-5-haiku",
      label: "Claude 3.5 Haiku",
      desc: "超低延迟快速响应",
    },
    {
      value: "claude-opus-4-7",
      label: "Claude Opus 4.7",
      desc: "复杂多文件架构与规划",
    },
  ],
  codex: [
    {
      value: "gpt-5.5-codex",
      label: "GPT-5.5 Codex",
      desc: "最新代码推理专家",
    },
    { value: "gpt-5-codex", label: "GPT-5 Codex", desc: "全栈自主代理" },
    {
      value: "gpt-4.1-codex",
      label: "GPT-4.1 Codex",
      desc: "高速高上下文工程",
    },
    { value: "o3-mini", label: "OpenAI o3-mini", desc: "极速高精度代码推理" },
  ],
  antigravity: [
    {
      value: "gemini-3.7-flash",
      label: "Gemini 3.7 Flash",
      desc: "高阶混合推理与视觉",
    },
    {
      value: "gemini-2.5-pro",
      label: "Gemini 2.5 Pro",
      desc: "超长上下文复杂逻辑架构",
    },
    {
      value: "gemini-2.5-flash",
      label: "Gemini 2.5 Flash",
      desc: "极速轻量智能体协作",
    },
  ],
  grok_build: [
    { value: "grok-4.6", label: "Grok 4.6", desc: "xAI 全栈实时开发" },
    { value: "grok-4.0", label: "Grok 4.0", desc: "标准工程构建助手" },
    {
      value: "grok-code-fast",
      label: "Grok Code Fast",
      desc: "低延迟快速单步任务",
    },
  ],
  zcode: [
    {
      value: "gemini-3.8-flash-high",
      label: "Gemini 3.8 Flash High",
      desc: "极速端到端代码生成与工具调用",
    },
    {
      value: "gemini-2.5-pro",
      label: "Gemini 2.5 Pro",
      desc: "超长上下文复杂逻辑架构与多步推理",
    },
    {
      value: "claude-3-7-sonnet",
      label: "Claude 3.7 Sonnet",
      desc: "自研混合思考与复杂重构",
    },
  ],
};

export function getProviderModels(type: Session["agent_type"]) {
  return PROVIDER_MODELS[type] ?? [];
}

export function getDefaultModel(
  type: Session["agent_type"],
  customModel?: string,
) {
  if (customModel && customModel.trim()) return customModel.trim();
  const list = getProviderModels(type);
  return list[0]?.value ?? "default-model";
}

export const PROVIDER_LIST: ReadonlyArray<{
  type: Session["agent_type"];
  label: string;
  shortLabel: string;
  defaultModel: string;
  desc: string;
  managed: boolean;
}> = [
  {
    type: "codex",
    label: "ChatGPT (Codex)",
    shortLabel: "ChatGPT",
    defaultModel: "gpt-5.5-codex",
    desc: "OpenAI 官方代码助手 (Managed)",
    managed: true,
  },
  {
    type: "claude_code",
    label: "Claude Code",
    shortLabel: "Claude",
    defaultModel: "claude-3-7-sonnet",
    desc: "Anthropic 官方开发助手 (Managed)",
    managed: true,
  },
  {
    type: "zcode",
    label: "ZCode Agent",
    shortLabel: "ZCode",
    defaultModel: "gemini-3.8-flash-high",
    desc: "ZCode 官方编程助手 (Plugin)",
    managed: false,
  },
  {
    type: "antigravity",
    label: "Google Antigravity",
    shortLabel: "Antigravity",
    defaultModel: "gemini-3.7-flash",
    desc: "Google DeepMind 开发代理 (Unmanaged)",
    managed: false,
  },
  {
    type: "grok_build",
    label: "Grok Build",
    shortLabel: "Grok",
    defaultModel: "grok-4.6",
    desc: "xAI 全栈实时开发 (Unmanaged)",
    managed: false,
  },
];

export interface ProviderGroup {
  provider: Session["agent_type"];
  label: string;
  shortLabel: string;
  sessions: Session[];
}

export interface WorkspaceGroup {
  name: string;
  sessions: Session[];
}

export function groupSessionsByWorkspace(
  sessions: Session[],
): WorkspaceGroup[] {
  const groups = new Map<string, Session[]>();
  for (const session of sessions) {
    const workspace =
      session.payload.project.display_name?.trim() || "未分类项目";
    const list = groups.get(workspace);
    if (list) list.push(session);
    else groups.set(workspace, [session]);
  }
  return [...groups.entries()].map(([name, workspaceSessions]) => ({
    name,
    sessions: [...workspaceSessions].sort((a, b) =>
      activityTime(b).localeCompare(activityTime(a)),
    ),
  }));
}

export function groupSessionsByProvider(sessions: Session[]): ProviderGroup[] {
  const groups = new Map<Session["agent_type"], Session[]>();
  for (const p of PROVIDER_LIST) {
    groups.set(p.type, []);
  }

  for (const session of sessions) {
    const agentType = normalizeAgentType(session.agent_type);
    const list = groups.get(agentType);
    if (list) {
      list.push(session);
    } else {
      groups.set(agentType, [session]);
    }
  }

  const result: ProviderGroup[] = [];
  for (const p of PROVIDER_LIST) {
    const list = groups.get(p.type) ?? [];
    if (list.length > 0) {
      result.push({
        provider: p.type,
        label: p.label,
        shortLabel: p.shortLabel,
        sessions: list,
      });
    }
  }

  // Include any extra providers if dynamic
  for (const [provider, list] of groups.entries()) {
    if (!PROVIDER_LIST.some((p) => p.type === provider) && list.length > 0) {
      result.push({
        provider,
        label: agentLabel(provider),
        shortLabel: agentShortLabel(provider),
        sessions: list,
      });
    }
  }

  return result;
}

export function isAttentionSession(session: Session): boolean {
  const status = session.payload.state.activity_status;
  return (
    status === "WAITING_PERMISSION" ||
    status === "WAITING_INPUT" ||
    status === "FAILED"
  );
}

export function adjacentItem<T>(
  items: readonly T[],
  index: number,
  offset: -1 | 1,
): T | undefined {
  if (index < 0) return undefined;
  return items[index + offset];
}

const TITLE_OVERRIDES_KEY = "agenthub:title-overrides";

let memoryOverrides: Record<string, string> = {};

function loadTitleOverrides(): Record<string, string> {
  if (typeof window !== "undefined" && window.localStorage) {
    try {
      const stored = window.localStorage.getItem(TITLE_OVERRIDES_KEY);
      if (stored) {
        return {
          ...memoryOverrides,
          ...(JSON.parse(stored) as Record<string, string>),
        };
      }
    } catch {
      // ignore
    }
  }
  return memoryOverrides;
}

/** Clean up title candidates from user messages, stripping system / environment prompt leakage. */
export function cleanUserPromptForTitle(raw: string): string {
  let text = raw
    .replace(/<recommended_plugins>[\s\S]*?<\/recommended_plugins>/gi, "")
    .replace(/<environment_context>[\s\S]*?<\/environment_context>/gi, "")
    .replace(/<system-reminder>[\s\S]*?<\/system-reminder>/gi, "")
    .replace(/<INSTRUCTIONS>[\s\S]*?<\/INSTRUCTIONS>/gi, "")
    .replace(/# AGENTS\.md instructions for[^\n]*\n+/gi, "")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  // If text was entirely composed of system instructions or tags
  if (
    text.startsWith("<") ||
    text.startsWith("The following skills are available") ||
    text.startsWith("Called the ") ||
    text.startsWith("The TodoWrite tool")
  ) {
    return "";
  }
  return text.slice(0, 64);
}

/** Remembers a conversation-derived summary so the sidebar reads like a chat
 * list instead of "agent · working directory". */
export function rememberTitleOverride(sessionKey: string, title: string) {
  const clean = cleanUserPromptForTitle(title);
  if (clean.length < 2) return;
  const overrides = loadTitleOverrides();
  overrides[sessionKey] = clean;
  // Also index by provider:sessionId and sessionId for robust lookup across device ID changes
  const parts = sessionKey.split(":");
  if (parts.length >= 3) {
    const providerAndId = `${parts[1]}:${parts[2]}`;
    overrides[providerAndId] = clean;
    overrides[parts[2]] = clean;
  }
  memoryOverrides = { ...overrides };
  if (typeof window !== "undefined") {
    try {
      window.localStorage?.setItem(
        TITLE_OVERRIDES_KEY,
        JSON.stringify(overrides),
      );
      window.dispatchEvent(
        new CustomEvent("agenthub:title-overrides-changed"),
      );
    } catch {
      // Storage full or unavailable; the in-memory override still applies.
    }
  }
}

export function formatDetailedTime(value?: string): string {
  if (!value) return "时间未知";
  const date = new Date(value);
  if (isNaN(date.getTime())) return "时间未知";
  const pad = (n: number) => n.toString().padStart(2, "0");
  const y = date.getFullYear();
  const m = pad(date.getMonth() + 1);
  const d = pad(date.getDate());
  const h = pad(date.getHours());
  const min = pad(date.getMinutes());
  const s = pad(date.getSeconds());
  return `${y}-${m}-${d} ${h}:${min}:${s}`;
}

export function sessionTooltip(session: Session, displayTitle: string): string {
  const lines: string[] = [displayTitle];
  lines.push(`会话 ID: ${session.agent_session_id}`);
  const time = session.payload.last_activity_at || session.observed_at;
  if (time) {
    lines.push(`活动时间: ${formatDetailedTime(time)} (${relativeTime(time)})`);
  }
  const ws = session.payload.project.display_name?.trim();
  if (ws && ws !== "unknown" && ws !== "未分类项目") {
    lines.push(`工作区: ${ws}`);
  }
  if (session.payload.runtime?.model) {
    lines.push(`模型: ${session.payload.runtime.model}`);
  }
  if (session.payload.runtime?.permission_mode) {
    lines.push(`模式: ${session.payload.runtime.permission_mode}`);
  }
  return lines.join("\n");
}

export function sessionDisplayTitle(session: Session): string {
  const agentType = normalizeAgentType(session.agent_type);
  const key = `${session.device_id}:${agentType}:${session.agent_session_id}`;
  const overrides = loadTitleOverrides();
  const override =
    overrides[key] ||
    overrides[`${agentType}:${session.agent_session_id}`] ||
    overrides[session.agent_session_id];
  if (override) return override;

  const explicit = session.payload.runtime?.title?.trim();
  const project = session.payload.project.display_name?.trim();

  // If explicit title exists, use it as long as it's not a generic placeholder
  // and NOT identical to the project/workspace name
  if (
    explicit &&
    explicit !== "未命名会话" &&
    explicit !== "Untitled" &&
    (!project || explicit.toLowerCase() !== project.toLowerCase())
  ) {
    return explicit;
  }

  // Never fall back to project name, because in the folder tree all sessions are
  // already nested under the workspace folder! Falling back to project name causes
  // dozens of sessions inside the same workspace to have identical titles.
  const shortId = session.agent_session_id.slice(0, 8);
  return `会话 #${shortId}`;
}

export function sessionKey(session: Session): string {
  return (
    session.device_id +
    ":" +
    normalizeAgentType(session.agent_type) +
    ":" +
    session.agent_session_id
  );
}

export function formatTokenCount(tokens: number | undefined): string {
  if (tokens === undefined) return "未提供";
  if (tokens < 1_000) return tokens + " tokens";
  if (tokens < 1_000_000) return (tokens / 1_000).toFixed(1) + "K tokens";
  return (tokens / 1_000_000).toFixed(2) + "M tokens";
}

export function formatContextUsage(
  runtime: Session["payload"]["runtime"],
): string {
  const used = runtime?.context_used_tokens;
  const window = runtime?.context_window_tokens;
  if (used !== undefined && window !== undefined && window > 0) {
    const percentage = Math.min(100, Math.round((used / window) * 100));
    return (
      formatTokenCount(used) +
      " / " +
      formatTokenCount(window) +
      " · " +
      percentage +
      "%"
    );
  }
  if (window !== undefined) return formatTokenCount(window) + " 上限";
  if (used !== undefined) return formatTokenCount(used) + " 已用 · 上限未提供";
  return "未提供";
}

export function initials(value: string): string {
  const parts = value.trim().split(/\s+/u).filter(Boolean);
  if (parts.length === 0) return "AP";
  return parts
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join("");
}
