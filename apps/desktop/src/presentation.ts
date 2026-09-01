import type { ActivityStatus, Session } from "./types";

export function activityTime(session: Session): string {
  return session.payload.last_activity_at ?? session.observed_at;
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
  if (type === "claude_code") return "Claude Code";
  if (type === "codex") return "Codex";
  if (type === "antigravity") return "Google Antigravity";
  return "Grok Build";
}

export function agentShortLabel(type: Session["agent_type"]): string {
  if (type === "claude_code") return "Claude";
  if (type === "codex") return "Codex";
  if (type === "antigravity") return "Antigravity";
  return "Grok";
}

export function permissionFieldLabel(type: Session["agent_type"]): string {
  if (type === "claude_code") return "权限模式";
  if (type === "antigravity") return "安全级别";
  if (type === "grok_build") return "Agent 配置";
  return "审批策略";
}

export const PROVIDER_MODELS: Record<
  Session["agent_type"],
  Array<{ value: string; label: string; desc: string }>
> = {
  claude_code: [
    { value: "claude-3-7-sonnet", label: "Claude 3.7 Sonnet", desc: "最强混合推理与自主开发" },
    { value: "claude-3-5-sonnet", label: "Claude 3.5 Sonnet", desc: "行业标杆通用编程" },
    { value: "claude-3-5-haiku", label: "Claude 3.5 Haiku", desc: "超低延迟快速响应" },
    { value: "claude-opus-4-7", label: "Claude Opus 4.7", desc: "复杂多文件架构与规划" },
  ],
  codex: [
    { value: "gpt-5.5-codex", label: "GPT-5.5 Codex", desc: "最新代码推理专家" },
    { value: "gpt-5-codex", label: "GPT-5 Codex", desc: "全栈自主代理" },
    { value: "gpt-4.1-codex", label: "GPT-4.1 Codex", desc: "高速高上下文工程" },
    { value: "o3-mini", label: "OpenAI o3-mini", desc: "极速高精度代码推理" },
  ],
  antigravity: [
    { value: "gemini-3.7-flash", label: "Gemini 3.7 Flash", desc: "高阶混合推理与视觉" },
    { value: "gemini-2.5-pro", label: "Gemini 2.5 Pro", desc: "超长上下文复杂逻辑架构" },
    { value: "gemini-2.5-flash", label: "Gemini 2.5 Flash", desc: "极速轻量智能体协作" },
  ],
  grok_build: [
    { value: "grok-4.6", label: "Grok 4.6", desc: "xAI 全栈实时开发" },
    { value: "grok-4.0", label: "Grok 4.0", desc: "标准工程构建助手" },
    { value: "grok-code-fast", label: "Grok Code Fast", desc: "低延迟快速单步任务" },
  ],
};

export function getProviderModels(type: Session["agent_type"]) {
  return PROVIDER_MODELS[type] ?? [];
}

export function getDefaultModel(type: Session["agent_type"], customModel?: string) {
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
}> = [
  {
    type: "claude_code",
    label: "Claude Code",
    shortLabel: "Claude",
    defaultModel: "claude-3-7-sonnet",
    desc: "Anthropic 原生 CLI",
  },
  {
    type: "codex",
    label: "OpenAI Codex",
    shortLabel: "Codex",
    defaultModel: "gpt-5.5-codex",
    desc: "OpenAI 代码助手",
  },
  {
    type: "antigravity",
    label: "Google Antigravity",
    shortLabel: "Antigravity",
    defaultModel: "gemini-3.7-flash",
    desc: "DeepMind Coding Agent",
  },
  {
    type: "grok_build",
    label: "Grok Build",
    shortLabel: "Grok",
    defaultModel: "grok-4.6",
    desc: "xAI 全栈开发",
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
    sessions: workspaceSessions,
  }));
}

export function groupSessionsByProvider(sessions: Session[]): ProviderGroup[] {
  const groups = new Map<Session["agent_type"], Session[]>();
  for (const p of PROVIDER_LIST) {
    groups.set(p.type, []);
  }

  for (const session of sessions) {
    const list = groups.get(session.agent_type);
    if (list) {
      list.push(session);
    } else {
      groups.set(session.agent_type, [session]);
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

export function sessionDisplayTitle(session: Session): string {
  const explicit = session.payload.runtime?.title?.trim();
  if (explicit && explicit !== "未命名会话" && explicit !== "Untitled") {
    return explicit;
  }
  const project = session.payload.project.display_name?.trim();
  const agent = agentShortLabel(session.agent_type);
  if (project && project !== "未分类项目" && project !== "unknown") {
    return `${agent} · ${project}`;
  }
  const shortId = session.agent_session_id.slice(0, 8);
  return `${agent} 会话 (${shortId})`;
}
