import type { Session } from "./types";

export interface EffortOption {
  value: string;
  label: string;
  desc?: string;
}

/**
 * Reasoning effort is a per-turn override, and each CLI exposes a different
 * set of levels. These lists are the values verified against each CLI's own
 * help output, so an unsupported level is never offered. Providers absent from
 * this map do not accept a per-turn effort and must not render the control.
 */
const EFFORT_LEVELS: Partial<Record<Session["agent_type"], EffortOption[]>> = {
  // claude --effort <level>: low, medium, high, xhigh, max
  claude_code: [
    { value: "low", label: "低", desc: "最快响应，日常小改动" },
    { value: "medium", label: "中", desc: "质量与速度平衡" },
    { value: "high", label: "高", desc: "复杂任务，日常主力" },
    { value: "xhigh", label: "很高", desc: "深度推理，耗时更长" },
    { value: "max", label: "最高", desc: "最强推理，最慢最贵" },
  ],
  // Codex has no --effort flag: it takes model_reasoning_effort, whose levels
  // are model-dependent. Stick to the widely supported core set.
  codex: [
    { value: "low", label: "低", desc: "最快响应，日常小改动" },
    { value: "medium", label: "中", desc: "质量与速度平衡" },
    { value: "high", label: "高", desc: "复杂任务，日常主力" },
  ],
};

/** Effort levels a provider accepts, or an empty list when it accepts none. */
export function effortOptionsFor(
  agentType: Session["agent_type"],
): EffortOption[] {
  return EFFORT_LEVELS[agentType] ?? [];
}

export function supportsEffort(agentType: Session["agent_type"]): boolean {
  return effortOptionsFor(agentType).length > 0;
}

/**
 * Resolves the effort to display: an explicit choice the session still supports,
 * otherwise the effort the provider last reported, otherwise the default.
 */
export function resolveEffort(
  agentType: Session["agent_type"],
  stored: string | undefined,
  reported: string | undefined,
): string {
  const options = effortOptionsFor(agentType);
  if (options.length === 0) return "";
  if (stored && options.some((option) => option.value === stored))
    return stored;
  if (reported) {
    const match = options.find(
      (option) =>
        option.value === reported.toLowerCase() || option.label === reported,
    );
    if (match) return match.value;
  }
  // Prefer a mid-tier default over the fastest level.
  return (
    options.find((option) => option.value === "medium")?.value ??
    options[0].value
  );
}
