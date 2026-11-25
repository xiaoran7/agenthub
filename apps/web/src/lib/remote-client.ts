import type { DiscoveryCandidate, RemoteEvent } from "./remote-types";

const REDACTED_PATH = "[local_path_redacted]";

/**
 * One prompt this device is showing but the transcript does not carry yet.
 *
 * `neededUserRows` is an absolute threshold fixed when the prompt is sent: the
 * transcript must carry at least this many user rows before this prompt counts
 * as absorbed.
 */
export interface OptimisticPrompt {
  id: string;
  text: string;
  createdAt: number;
  neededUserRows: number;
}

/** Past this an unconfirmed prompt is a ghost, not a pending send. */
export const OPTIMISTIC_PROMPT_TTL_MS = 90_000;

export function optimisticPromptLanded(
  neededUserRows: number,
  userRowCount: number,
): boolean {
  return userRowCount >= neededUserRows;
}

export function nextOptimisticPromptNeeded(
  pending: number[],
  userRowCount: number,
): number {
  const head = userRowCount + 1;
  const behindQueue =
    (pending.length > 0 ? pending[pending.length - 1]! : 0) + 1;
  return Math.max(head, behindQueue);
}

export function pendingOptimisticPrompts(
  prompts: OptimisticPrompt[],
  userItems: Array<{ kind: string }>,
): OptimisticPrompt[] {
  const userRowCount = userItems.filter((item) => item.kind === "user").length;
  return prompts.filter(
    (prompt) => !optimisticPromptLanded(prompt.neededUserRows, userRowCount),
  );
}

export function pruneOptimisticPrompts(
  prompts: OptimisticPrompt[],
  userItems: Array<{ kind: string }>,
  now: number,
): { visible: OptimisticPrompt[]; kept: OptimisticPrompt[] } {
  const kept = pendingOptimisticPrompts(prompts, userItems).filter(
    (prompt) => now - prompt.createdAt <= OPTIMISTIC_PROMPT_TTL_MS,
  );
  return { visible: kept, kept };
}

export function readableRemoteText(value: string): string {
  return value.replaceAll(REDACTED_PATH, "（本机路径已隐藏）");
}

/**
 * Cleans plain-text error banner copy so Markdown inline backticks or control
 * characters forwarded from CLI stderr read cleanly in alert banners.
 */
export function cleanBannerText(value: string): string {
  return readableRemoteText(value)
    .replace(/`([^`\n]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

export function isRedactedPathOnly(value: string): boolean {
  return value.trim() === REDACTED_PATH;
}

export interface RemoteContextUsage {
  used: number;
  window: number;
  source: "reported" | "usage_only";
}

export function inferModelContextWindow(
  model?: string | null,
  provider?: string | null,
): number {
  const m = (model ?? "").toLowerCase();
  if (m.includes("2m")) return 2_000_000;
  if (
    m.includes("1m") ||
    m.startsWith("claude-opus-5") ||
    m.startsWith("claude-sonnet-5") ||
    m.startsWith("claude-fable-5") ||
    m.startsWith("claude-mythos-5") ||
    m.includes("anthropic.claude-sonnet-5") ||
    m.includes("gemini")
  ) {
    return 1_000_000;
  }
  const p = (provider ?? "").toLowerCase();
  if (
    p.includes("antigravity") &&
    !m.includes("claude") &&
    !m.includes("gpt")
  ) {
    return 1_000_000;
  }
  if (m.length > 0 || p.length > 0) {
    return 200_000;
  }
  return 0;
}

/** Only show a percentage when the Host reports actual token use. */
export function latestContextUsage(
  events: RemoteEvent[],
  candidate?: DiscoveryCandidate | null,
): RemoteContextUsage | null {
  const positive = (value: unknown) =>
    typeof value === "number" && Number.isFinite(value) && value > 0
      ? value
      : undefined;
  const fallbackWindow =
    positive(candidate?.context_window_tokens) ??
    (candidate
      ? inferModelContextWindow(candidate.model, candidate.provider) ||
        undefined
      : undefined);
  for (let index = events.length - 1; index >= 0; index--) {
    const params = events[index]?.payload.event?.raw?.params;
    if (!params) continue;
    const usage = (params.tokenUsage ?? params.token_usage ?? params.usage) as
      Record<string, unknown> | undefined;
    const info = (params.info ?? params.turn) as
      Record<string, unknown> | undefined;
    const last = (usage?.last ??
      usage?.last_token_usage ??
      info?.last_token_usage ??
      info?.usage) as Record<string, unknown> | undefined;
    const values = last ?? usage;
    const used =
      positive(values?.used) ??
      positive(usage?.used) ??
      positive(values?.context_used_tokens) ??
      positive(values?.contextUsedTokens) ??
      positive(values?.input_tokens) ??
      positive(values?.inputTokens) ??
      positive(values?.total_tokens) ??
      positive(values?.totalTokens);
    if (!used) continue;
    const window =
      positive(usage?.window) ??
      positive(values?.window) ??
      positive(usage?.model_context_window) ??
      positive(usage?.modelContextWindow) ??
      positive(usage?.context_window_tokens) ??
      positive(usage?.contextWindowTokens) ??
      positive(info?.model_context_window) ??
      positive(info?.context_window_tokens) ??
      positive(params.model_context_window) ??
      positive(params.context_window_tokens) ??
      fallbackWindow;
    return {
      used,
      window: window ?? 0,
      source: window ? "reported" : "usage_only",
    };
  }
  if (candidate) {
    const window = fallbackWindow ?? 0;
    const explicitUsed = positive(candidate.context_used_tokens);
    if (explicitUsed) {
      return {
        used: explicitUsed,
        window,
        source: window ? "reported" : "usage_only",
      };
    }
    const cumulativePrompt =
      positive(candidate.prompt_tokens) ?? positive(candidate.total_tokens);
    if (cumulativePrompt) {
      const calls = Math.max(
        1,
        positive(candidate.request_count) ?? positive(candidate.calls) ?? 1,
      );
      const estimatedUsed = Math.round(cumulativePrompt / calls);
      const boundedUsed =
        window > 0 ? Math.min(window, estimatedUsed) : estimatedUsed;
      return {
        used: boundedUsed,
        window,
        source: window ? "reported" : "usage_only",
      };
    }
  }
  return null;
}

export async function remoteRequest<T>(
  path: string,
  body?: unknown,
  timeoutMs = 25000,
  userId?: string,
  externalSignal?: AbortSignal,
): Promise<T> {
  const controller = new AbortController();
  let timedOut = false;
  const onExternalAbort = () => controller.abort(externalSignal?.reason);
  if (externalSignal !== undefined) {
    if (externalSignal.aborted) {
      throw new Error("request_aborted");
    }
    externalSignal.addEventListener("abort", onExternalAbort, { once: true });
  }
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort(new Error("request_timeout"));
  }, timeoutMs);
  let response: Response;
  let rawText: string;
  try {
    response = await fetch(`/v3/remote${path}`, {
      method: body === undefined ? "GET" : "POST",
      cache: "no-store",
      credentials: "same-origin",
      headers: {
        "content-type": "application/json",
        ...(userId === undefined || userId === ""
          ? {}
          : { "x-user-id": userId }),
      },
      signal: controller.signal,
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    rawText = await response.text();
  } catch (err) {
    if (timedOut) {
      throw new Error("request_timeout");
    }
    if (
      externalSignal?.aborted ||
      (err instanceof Error && err.name === "AbortError")
    ) {
      throw new Error("request_aborted");
    }
    if (err instanceof TypeError && /fetch|network|failed/i.test(err.message)) {
      throw new Error("gateway_unreachable");
    }
    throw err;
  } finally {
    clearTimeout(timer);
    externalSignal?.removeEventListener("abort", onExternalAbort);
  }
  let value: (T & { error?: string }) | undefined;
  if (rawText.trim().length > 0) {
    try {
      value = JSON.parse(rawText) as T & { error?: string };
    } catch {
      value = undefined;
    }
  }
  if (!response.ok) {
    const code =
      response.status === 401
        ? "authentication_required"
        : response.status === 413
          ? "payload_too_large"
          : response.status === 404 && path === "/sessions"
            ? "remote_not_enabled"
            : (value?.error ?? "remote_unavailable");
    const error = new Error(code);
    if (response.status >= 400 && response.status < 500) {
      error.name = "RemoteRequestRejected";
    }
    throw error;
  }
  if (value === undefined) {
    throw new Error("remote_unavailable");
  }
  return value;
}

export function remoteError(error: unknown): string {
  const messages: Record<string, string> = {
    gateway_unreachable: "网关服务暂时无法连接，正在自动重连。",
    request_timeout: "网络请求响应超时，正在自动重试。",
    request_aborted: "请求已取消。",
    payload_too_large: "发送内容或附件体积超过服务器限制，请精简后重试。",
    remote_not_enabled:
      "远程连接尚未启用。请先配置 Gateway 内容存储并连接 Runner。",
    runner_upgrade_required: "请先更新这台电脑上的 AgentHub Runner。",
    runner_offline: "这台电脑已离线，连接恢复后可继续。",
    native_writer_busy:
      "Codex 桌面端或其他应用仍在使用这条会话，请在那里关闭后重试。",
    explicit_control_required: "请主动接管会话后继续。",
    session_control_releasing: "正在归还会话控制，请稍后重试。",
    session_not_started: "请先发送一条消息，保存会话后再归还控制。",
    session_control_lost: "会话连接已失效，请先恢复会话。",
    session_busy_or_unknown: "当前会话仍在执行，或状态尚未确认。",
    approval_not_pending: "这项审批已处理或已过期。",
    host_generation_mismatch: "会话已重新连接，请刷新后重试。",
    command_id_conflict: "请求编号已使用，请刷新后重试。",
    attach_not_supported: "这个来源暂不支持连接正在运行的会话。",
    external_session_not_resumable: "原生会话正在使用中，暂不能恢复。",
    provider_rejected: "原生 Agent 拒绝了请求，请检查本机登录、版本和配置。",
    workspace_not_authorized: "这个工作目录尚未在本机授权。",
    previous_execution_unconfirmed: "上次请求结果尚未确认，已阻止重复执行。",
    slash_not_supported: "暂不支持这个快捷命令，请使用对应按钮。",
    invalid_command_identity: "请求编号或会话编号无效，请刷新后重试。",
    host_lost: "这台电脑上的会话进程已退出，请重新连接。",
    workspace_unavailable: "这个工作目录已经不可用。",
    workspace_required: "请先选择一个工作目录。",
    workspace_limit_exceeded: "工作目录数量已达上限。",
    thread_list_malformed: "本机 Agent 返回的会话列表无法读取。",
    session_already_bound: "该会话已在当前设备上处于连接状态。",
    session_not_bound: "这个会话还没有绑定，请先恢复它。",
    invalid_prompt: "发送的内容为空或过长。",
    approval_session_mismatch: "这项审批属于另一个会话。",
    approval_type_not_supported: "暂不支持这种审批请求。",
    approval_decision_not_available: "这项审批已经处理过了。",
    question_not_pending: "这项提问已处理或已过期。",
    question_session_mismatch: "这项提问属于另一个会话。",
    native_id_required: "缺少本机会话编号。",
    native_id_mismatch: "本机会话编号与会话不匹配。",
    native_session_already_bound: "这个本机会话已经绑定过了。",
    resume_workspace_mismatch: "会话的工作目录与所选目录不一致。",
    create_cannot_set_native_id: "新建会话时不能指定本机会话编号。",
    invalid_model: "所选模型不可用。",
    invalid_permission_mode: "所选权限模式不受支持。",
    provider_not_available: "本机没有可用的 Agent 客户端。",
    plugin_disabled: "该智能体插件当前已停用或未启用。",
    plugin_not_found: "本机 Runner 上未找到该智能体插件。",
    plugin_action_timeout: "插件操作等待 Runner 响应超时。",
    plugin_sha256_mismatch: "插件文件 SHA-256 校验失败，已中止安装。",
    deepseek_harness_runtime_not_implemented:
      "DeepSeek Harness 预留接口尚未接入本地运行时。",
    claude_session_not_found: "本机找不到这个 Claude 会话。",
    acp_session_not_found: "本机找不到这个 ACP 会话。",
    antigravity_not_available: "本机没有可用的 Antigravity 客户端或后台服务。",
    antigravity_session_not_found: "本机找不到这个 Antigravity 会话。",
  };
  return (
    messages[error instanceof Error ? error.message : ""] ??
    "暂时无法完成操作，请检查电脑连接后重试。"
  );
}

export function isTransientRemoteMessage(message: string): boolean {
  return (
    message === "网络请求响应超时，正在自动重试。" ||
    message === "网关服务暂时无法连接，正在自动重连。"
  );
}

export function isTransientRemoteError(error: unknown): boolean {
  if (error instanceof Error) {
    return (
      error.message === "request_timeout" ||
      error.message === "gateway_unreachable"
    );
  }
  return false;
}
