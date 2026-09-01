import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import type { Session } from "./types";

export interface AgentModelOption {
  value: string;
  label: string;
  desc?: string;
  source?: "agent" | "builtin";
}

export type AgentModelMap = Record<string, AgentModelOption[]>;

/**
 * Builtin catalog is only a safety net for fresh installs without local
 * transcripts. The live list comes from `list_agent_models`, which aggregates
 * the models each installed CLI actually reported in local sessions.
 */
export const BUILTIN_MODELS: AgentModelMap = {
  claude_code: [
    {
      value: "claude-opus-4-1",
      label: "Claude Opus 4.1",
      desc: "深度复杂工程与规划",
      source: "builtin",
    },
    {
      value: "claude-sonnet-4-5",
      label: "Claude Sonnet 4.5",
      desc: "日常开发主力",
      source: "builtin",
    },
    {
      value: "claude-haiku-4-5",
      label: "Claude Haiku 4.5",
      desc: "快速轻量响应",
      source: "builtin",
    },
  ],
  codex: [
    {
      value: "gpt-5.1-codex-max",
      label: "GPT-5.1 Codex Max",
      desc: "最强代码推理",
      source: "builtin",
    },
    {
      value: "gpt-5.1-codex",
      label: "GPT-5.1 Codex",
      desc: "全栈自主开发",
      source: "builtin",
    },
    {
      value: "gpt-5-codex",
      label: "GPT-5 Codex",
      desc: "稳定代码代理",
      source: "builtin",
    },
  ],
  antigravity: [
    {
      value: "gemini-3-pro",
      label: "Gemini 3 Pro",
      desc: "DeepMind 最新推理",
      source: "builtin",
    },
    {
      value: "gemini-2.5-pro",
      label: "Gemini 2.5 Pro",
      desc: "超长上下文架构",
      source: "builtin",
    },
  ],
  grok_build: [
    {
      value: "grok-4",
      label: "Grok 4",
      desc: "xAI 旗舰全栈开发",
      source: "builtin",
    },
    {
      value: "grok-code-fast-1",
      label: "Grok Code Fast",
      desc: "低延迟快速任务",
      source: "builtin",
    },
  ],
};

const MODELS_TTL_MS = 60_000;
const hasTauri = () => "__TAURI_INTERNALS__" in window;

let modelsCache: { at: number; data: AgentModelMap } | null = null;
let modelsInflight: Promise<AgentModelMap> | null = null;
const subscribers = new Set<(data: AgentModelMap) => void>();

function publish(data: AgentModelMap) {
  subscribers.forEach((listener) => listener(data));
}

export async function fetchAgentModels(force = false): Promise<AgentModelMap> {
  if (!hasTauri()) return BUILTIN_MODELS;
  if (!force && modelsCache && Date.now() - modelsCache.at < MODELS_TTL_MS) {
    return modelsCache.data;
  }
  if (modelsInflight) return modelsInflight;
  modelsInflight = invoke<
    Array<{ agent_type: string; models: AgentModelOption[] }>
  >("list_agent_models")
    .then((groups) => {
      const next: AgentModelMap = { ...BUILTIN_MODELS };
      for (const group of groups) {
        if (
          group?.agent_type &&
          Array.isArray(group.models) &&
          group.models.length > 0
        ) {
          next[group.agent_type as keyof AgentModelMap] = group.models;
        }
      }
      modelsCache = { at: Date.now(), data: next };
      publish(next);
      return next;
    })
    .catch(() => BUILTIN_MODELS)
    .finally(() => {
      modelsInflight = null;
    });
  return modelsInflight;
}

export function modelsFor(
  map: AgentModelMap,
  agentType: Session["agent_type"],
) {
  const list = map[agentType];
  return list && list.length > 0 ? list : (BUILTIN_MODELS[agentType] ?? []);
}

export function defaultModelFor(
  map: AgentModelMap,
  agentType: Session["agent_type"],
  stored?: string,
) {
  if (
    stored &&
    modelsFor(map, agentType).some((model) => model.value === stored)
  ) {
    return stored;
  }
  if (stored && stored.trim()) return stored;
  return modelsFor(map, agentType)[0]?.value ?? "";
}

/** Subscribes the component to the shared, TTL-cached agent model catalog. */
export function useAgentModels(agentType: Session["agent_type"]) {
  const [models, setModels] = useState<AgentModelOption[]>(() =>
    modelsFor(modelsCache?.data ?? BUILTIN_MODELS, agentType),
  );

  useEffect(() => {
    setModels(modelsFor(modelsCache?.data ?? BUILTIN_MODELS, agentType));
    const listener = (data: AgentModelMap) =>
      setModels(modelsFor(data, agentType));
    subscribers.add(listener);
    void fetchAgentModels();
    return () => {
      subscribers.delete(listener);
    };
  }, [agentType]);

  return models;
}

export function invalidateAgentModels() {
  modelsCache = null;
}
