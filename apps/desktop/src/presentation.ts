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
    defaultModel: "gpt-5-codex",
    desc: "OpenAI 代码助手",
  },
  {
    type: "antigravity",
    label: "Google Antigravity",
    shortLabel: "Antigravity",
    defaultModel: "gemini-2.5-pro",
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
