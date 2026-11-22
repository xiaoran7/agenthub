import type { ConversationItem } from "../lib/remote";

/** The launch dialog's fixed choices. Everything else is derived from the Host. */
export const EFFORT_OPTIONS = [
  { value: "default", label: "默认思考" },
  { value: "low", label: "低推理 (快速)" },
  { value: "medium", label: "中推理 (标准)" },
  { value: "high", label: "高推理 (深度)" },
  { value: "max", label: "超高推理 (最大)" },
] as const;

export const SANDBOX_OPTIONS = [
  { value: "read-only", label: "只读" },
  { value: "workspace-write", label: "允许修改工作区" },
] as const;

export const PERMISSION_MODE_OPTIONS = [
  { value: "default", label: "默认权限" },
  { value: "acceptEdits", label: "自动允许编辑" },
  { value: "plan", label: "只读规划" },
  { value: "bypassPermissions", label: "完全自动执行" },
] as const;

export const QUICK_PROMPT_SNIPPETS = [
  {
    label: "继续并验证当前改动",
    text: "请继续完成当前改动，并运行对应的测试和检查确认无误。",
  },
  {
    label: "定位并修复报错",
    text: "请定位最近一次错误或测试失败的根因，给出最小修复并验证通过。",
  },
  {
    label: "代码审查 (Code Review)",
    text: "请审查当前未提交的代码改动，重点排查逻辑缺陷、边界条件与安全隐患。",
  },
] as const;

export const CORE_LAUNCH_AGENTS = [
  { value: "codex", label: "Codex", catalog: ["codex"] },
  {
    value: "claude_code",
    label: "Coding Assistant",
    catalog: ["claude_code"],
  },
  { value: "antigravity", label: "Antigravity", catalog: ["antigravity"] },
] as const;

export interface LaunchAgentSpec {
  value: string;
  label: string;
  catalog: readonly string[];
}

export const LAUNCH_AGENTS: readonly LaunchAgentSpec[] = [
  ...CORE_LAUNCH_AGENTS,
];

export const LAUNCH_AGENT_OPTIONS = LAUNCH_AGENTS.map((item) => ({
  value: item.value,
  label: item.label,
}));

export function formatSessionTokens(tokens?: number | null): string | null {
  if (tokens === null || tokens === undefined || tokens <= 0) return null;
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M tok`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k tok`;
  return `${tokens} tok`;
}

export type ToolKind = "command" | "read" | "search" | "edit" | "web" | "tool";

export function toolKind(item: ConversationItem): ToolKind {
  const title = item.title ?? "";
  const name = (title.split("·").pop() ?? title).toLowerCase().trim();
  const has = (...needles: string[]) => needles.some((n) => name.includes(n));
  if (
    has(
      "bash",
      "shell",
      "zsh",
      "powershell",
      "command",
      "exec",
      "terminal",
      "命令",
      "执行",
    )
  )
    return "command";
  if (has("web", "browse", "fetch", "http", "url", "联网", "网络"))
    return "web";
  if (
    has(
      "write",
      "edit",
      "create",
      "patch",
      "delete",
      "remove",
      "rename",
      "move",
      "写入",
      "编辑",
      "修改",
    )
  )
    return "edit";
  if (has("grep", "search", "glob", "find", "list", "ls", "搜索", "查找"))
    return "search";
  if (has("read", "open", "view", "notebook", "cat", "读取", "查看"))
    return "read";
  if (item.command && !/[/\\]/u.test(item.command)) return "command";
  return "tool";
}

const TOOL_PATHS: Record<ToolKind, React.ReactNode> = {
  command: (
    <>
      <polyline points="4 7 4 4 8 4" />
      <polyline points="16 4 20 4 20 7" />
      <polyline points="20 17 20 20 16 20" />
      <polyline points="8 20 4 20 4 17" />
      <line x1="7" y1="8" x2="17" y2="16" />
      <line x1="17" y1="8" x2="7" y2="16" />
    </>
  ),
  read: (
    <>
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
      <polyline points="14 3 14 8 19 8" />
      <line x1="9" y1="13" x2="15" y2="13" />
      <line x1="9" y1="17" x2="13" y2="17" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <line x1="20" y1="20" x2="16.4" y2="16.4" />
    </>
  ),
  edit: (
    <>
      <path d="M12 20h8" />
      <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L8 18l-4 1 1-4z" />
    </>
  ),
  web: (
    <>
      <circle cx="12" cy="12" r="9" />
      <line x1="3" y1="12" x2="21" y2="12" />
      <path d="M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z" />
    </>
  ),
  tool: (
    <path d="m14.7 6.3 3 3M4 20l5.3-1.3L20 8a2.1 2.1 0 0 0-3-3L6.3 15.7 5 21" />
  ),
};

export function ToolIcon({ kind }: { kind: ToolKind }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {TOOL_PATHS[kind]}
    </svg>
  );
}

export function ChevronDownIcon({
  size = 12,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <polyline points="6 9 12 15 18 9" />
    </svg>
  );
}

export function SettingsIcon({
  size = 14,
  className = "",
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}
