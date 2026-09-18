import type { Session } from "./types";

export interface AgentPlugin {
  id: Session["agent_type"];
  name: string;
  shortLabel: string;
  category: "官方核心" | "实验室插件" | "第三方集成";
  author: string;
  version: string;
  iconName: "google" | "xai" | "anthropic" | "openai" | "zcode";
  summary: string;
  description: string;
  defaultModel: string;
  builtin: boolean; // 内置受控核心不可禁用（Codex, Claude, ZCode）
  status: "available" | "experimental" | "coming_soon";
  tags: string[];
}

export const AGENT_PLUGIN_MARKETPLACE: AgentPlugin[] = [
  {
    id: "antigravity",
    name: "Google Antigravity Remote",
    shortLabel: "Antigravity",
    category: "实验室插件",
    author: "Google DeepMind / Gemini Labs",
    version: "1.0.0-remote",
    iconName: "google",
    summary: "对齐 ChatGPT Remote 架构的 Google 实验性 Agent 接管驱动。",
    description:
      "面向 Google Antigravity / Gemini CLI 打造的后台全双工 Remote 运行时。支持自动发现本地会话、出站接管（thread/resume、turn/start、turn/interrupt）、实时增量打字机流式回传与任务级协同中断。",
    defaultModel: "gemini-3-pro",
    builtin: false,
    status: "available",
    tags: ["Google", "Gemini", "DeepMind", "全双工Remote"],
  },
  {
    id: "grok_build",
    name: "xAI Grok Build Remote",
    shortLabel: "Grok",
    category: "第三方集成",
    author: "xAI Ecosystem",
    version: "1.0.0-remote",
    iconName: "xai",
    summary: "对齐 ChatGPT Remote 架构的 xAI 全栈极速 Agent 接管驱动。",
    description:
      "面向 xAI Grok Build 打造的原生 Remote 运行时。支持自动探测本地 sessions 目录、长连接会话挂载与恢复、流式响应摄取与全双工急停中断，实现离开工位后无感远程操控。",
    defaultModel: "grok-4",
    builtin: false,
    status: "available",
    tags: ["xAI", "Grok", "全栈开发", "全双工Remote"],
  },
];

const STORAGE_KEY = "agenthub:installed-plugins";

export interface PluginState {
  enabledPluginIds: Session["agent_type"][];
}

export function readPluginState(): PluginState {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (!stored) {
      // 默认状态：反重力与 Grok 默认不直接展示，需在插件商城中手动启用
      return { enabledPluginIds: [] };
    }
    const parsed = JSON.parse(stored);
    if (Array.isArray(parsed)) {
      return { enabledPluginIds: parsed };
    }
  } catch {
    // ignore
  }
  return { enabledPluginIds: [] };
}

export function savePluginState(state: PluginState): void {
  try {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(state.enabledPluginIds),
    );
  } catch {
    // ignore
  }
}

export function isPluginEnabled(pluginId: Session["agent_type"]): boolean {
  // 核心受控 Provider 永久启用
  if (
    pluginId === "codex" ||
    pluginId === "claude_code" ||
    pluginId === "zcode"
  ) {
    return true;
  }
  const state = readPluginState();
  return state.enabledPluginIds.includes(pluginId);
}
