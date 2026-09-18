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
    name: "Google Antigravity",
    shortLabel: "Antigravity",
    category: "实验室插件",
    author: "Google DeepMind / Gemini Labs",
    version: "0.9.4-beta",
    iconName: "google",
    summary: "Google 实验性开发代理，具备混合推理与多步自主规划能力。",
    description:
      "针对 Google Antigravity (AGY / Gemini CLI) 提供深度集成。支持自动探测本地会话状态、会话树解析以及多模态意图识别。目前处于实验室孵化与接入阶段。",
    defaultModel: "gemini-3.7-flash",
    builtin: false,
    status: "experimental",
    tags: ["Google", "Gemini", "DeepMind", "实验性"],
  },
  {
    id: "grok_build",
    name: "xAI Grok Build",
    shortLabel: "Grok",
    category: "第三方集成",
    author: "xAI Ecosystem",
    version: "0.8.2-preview",
    iconName: "xai",
    summary: "xAI 全栈实时开发代理，支持低延迟极速任务执行与代码分析。",
    description:
      "集成 xAI Grok 系列编程助手。支持本地会话扫描与流式上下文摄取，具备高速代码推理、实时工具调用与长链路架构开发能力。目前开放开发者预览体验。",
    defaultModel: "grok-4.6",
    builtin: false,
    status: "experimental",
    tags: ["xAI", "Grok", "全栈开发", "预览版"],
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
