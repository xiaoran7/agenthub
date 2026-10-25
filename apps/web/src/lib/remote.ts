import type {
  RemoteAttachment,
  RemoteBinding,
  RemoteCommand,
  RemotePendingApproval,
  RemotePendingQuestion,
  RemotePermissionMode,
} from "@agent-pulse/contracts/remote";
export type { RemoteAttachment, RemotePermissionMode };
export type RemoteRequest = <T>(
  path: string,
  body?: unknown,
  timeoutMs?: number,
  userId?: string,
) => Promise<T>;
export interface DiscoveryCandidate {
  provider_session_id: string;
  workspace_path: string;
  title: string | null;
  provider?: string;
  resumable?: boolean;
  model?: string | null;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  total_tokens?: number | null;
  cache_read_tokens?: number | null;
  context_used_tokens?: number | null;
  context_window_tokens?: number | null;
  calls?: number | null;
  request_count?: number | null;
  last_activity_at_ms?: number | null;
  execution_state?: string | null;
}
export interface RemoteRunner {
  runner_id: string;
  online: boolean;
  revoked: boolean;
  hello: null | {
    host_generation: string;
    /** Human-readable name for the computer, shown instead of its id. */
    device_name?: string;
    pending_approvals?: RemotePendingApproval[];
    workspaces: string[];
    models?: {
      data: Array<{
        id: string;
        model?: string;
        displayName?: string;
        display_name?: string;
        name?: string;
      }>;
      /**
       * The same catalog split by the engine that can run each model. An id
       * read from one engine's config is rejected by another, so the launch
       * dialog offers only the chosen engine's own list.
       */
      agents?: Record<string, Array<{ id: string; display_name?: string }>>;
    };
    discovery?: {
      candidates: DiscoveryCandidate[];
    };
  };
}
export interface RemoteSession {
  runner_id: string;
  binding: RemoteBinding;
  online: boolean;
}
export interface RemoteSnapshot {
  cursor: number;
  runners: RemoteRunner[];
  sessions: RemoteSession[];
}
export interface RemoteEvent {
  cursor: number;
  runner_id: string;
  payload: {
    type: string;
    pending_approvals?: RemotePendingApproval[];
    pending_questions?: RemotePendingQuestion[];
    event?: {
      kind: string;
      raw?: { params?: Record<string, unknown> };
      approval_id?: string;
      approval?: RemotePendingApproval;
      question_id?: string;
      question?: RemotePendingQuestion;
      binding?: RemoteBinding;
    };
    receipt?: {
      client_cmd_id: string;
      status: string;
      code?: string;
      result?: {
        turn?: { id: string };
        approval_id?: string;
        question_id?: string;
        response_sent?: boolean;
      };
    };
  };
}
export interface RemoteCommandRecord {
  command: RemoteCommand;
  receipt: {
    status: string;
    /** The failure reason. Null on an accepted receipt. */
    code?: string | null;
    result?: { turn?: { id: string } };
  } | null;
}
/** Response of `GET /v3/remote/sessions/:runner/:hub`. */
export interface RemoteSessionDetail {
  /** Null once the runner is gone; command history still answers. */
  binding: RemoteBinding | null;
  /** Highest event cursor visible to the owner, for cheap change detection. */
  cursor: number;
  commands: RemoteCommandRecord[];
}

export interface ConversationItem {
  id: string;
  kind: "user" | "assistant" | "tool";
  text: string;
  title?: string | undefined;
  command?: string | undefined;
  output?: string | undefined;
  description?: string | undefined;
  status?: "running" | "completed" | "failed";
  turnId?: string | undefined;
}

/** A turn that failed, with the reason the Runner was allowed to forward. */
export interface TurnFailure {
  turnId: string | null;
  message: string;
  /** True while the provider says it is still retrying the same turn. */
  retrying: boolean;
}

/**
 * Reads the cause of a failed turn out of the event stream.
 *
 * The Runner projects only an allowlisted subset of a provider error, so this
 * returns `null` rather than guessing when the projection carries no usable
 * text. `willRetry` is honoured: a transient error the provider is retrying is
 * not shown as the turn's final outcome.
 */
export function latestTurnFailure(
  events: RemoteEvent[],
  hubSessionId: string,
): TurnFailure | null {
  let failure: TurnFailure | null = null;
  for (const envelope of events) {
    const native = envelope.payload.event;
    if (!native) continue;
    // A new turn supersedes an earlier failure; a completed turn clears it. Both
    // are scoped the same way: the console's stream is already filtered to one
    // session, but an event that names a different one must not clear this
    // session's banner.
    const belongsToSession =
      native.binding?.hub_session_id === undefined ||
      native.binding.hub_session_id === hubSessionId;
    if (!belongsToSession) continue;
    if (native.kind === "turn/started") {
      failure = null;
      continue;
    }
    if (native.kind === "turn/completed") {
      const turn = native.raw?.params?.turn as
        Record<string, unknown> | undefined;
      const status = typeof turn?.status === "string" ? turn.status : undefined;
      const turnError =
        typeof turn?.error === "string" ? turn.error.trim() : "";
      const isFailedCompletion =
        status === "failed" ||
        status === "error" ||
        Boolean(turnError) ||
        native.binding?.execution_state === "error";
      if (isFailedCompletion) {
        const prevFailure = failure as TurnFailure | null;
        const turnId: string | null =
          typeof turn?.id === "string"
            ? turn.id
            : (prevFailure?.turnId ?? null);
        if (turnError) {
          failure = {
            turnId,
            message: turnError,
            retrying: false,
          };
        } else if (prevFailure) {
          failure = {
            turnId,
            message: prevFailure.message,
            retrying: false,
          };
        }
      } else {
        failure = null;
      }
      continue;
    }
    if (native.kind !== "error") continue;
    const params = native.raw?.params;
    const error = params?.error as Record<string, unknown> | undefined;
    const message = error?.message;
    if (typeof message !== "string" || message.trim() === "") continue;
    failure = {
      turnId: typeof params?.turnId === "string" ? params.turnId : null,
      message: message.trim(),
      retrying: params?.willRetry === true,
    };
  }
  return failure;
}

const REDACTED_PATH = "[local_path_redacted]";

export function readableRemoteText(value: string): string {
  return value.replaceAll(REDACTED_PATH, "（本机路径已隐藏）");
}

export function isRedactedPathOnly(value: string): boolean {
  return value.trim() === REDACTED_PATH;
}

export type ConversationGroup =
  | { type: "single"; item: ConversationItem }
  | { type: "tool_group"; items: ConversationItem[] };

/** Match the Desktop conversation's consecutive tool grouping. */
export function groupConversationItems(
  items: ConversationItem[],
): ConversationGroup[] {
  const groups: ConversationGroup[] = [];
  let tools: ConversationItem[] = [];
  const flush = () => {
    if (tools.length === 1) groups.push({ type: "single", item: tools[0]! });
    else if (tools.length > 1)
      groups.push({ type: "tool_group", items: tools });
    tools = [];
  };
  for (const item of items) {
    if (item.kind === "tool") tools.push(item);
    else {
      flush();
      groups.push({ type: "single", item });
    }
  }
  flush();
  return groups;
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
    m.length > 0 ||
    p.includes("claude") ||
    p.includes("codex") ||
    p.includes("zcode")
  ) {
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
      positive(values?.inputTokens);
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

/**
 * Sends a request to the Gateway's remote API.
 *
 * Identity comes from the browser's account session cookie. The development
 * `x-user-id` header is only a fallback for a Gateway running without
 * accounts: it is read from the panel config, never hardcoded, because a
 * fixed value silently resolves a *different* owner than the one the user
 * signed in as — which is why the console used to show an empty session list
 * while the Runner was publishing sessions under the real account.
 */
export async function remoteRequest<T>(
  path: string,
  body?: unknown,
  timeoutMs = 12000,
  userId?: string,
): Promise<T> {
  const controller = new AbortController();
  const timer = setTimeout(() => {
    controller.abort(new Error("request_timeout"));
  }, timeoutMs);
  let response: Response;
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
  } catch (err) {
    if (err instanceof Error && err.name === "AbortError") {
      throw new Error("request_timeout");
    }
    throw err;
  } finally {
    clearTimeout(timer);
  }
  const value = (await response.json()) as T & { error?: string };
  if (!response.ok) {
    const error = new Error(
      response.status === 401
        ? "authentication_required"
        : response.status === 404 && path === "/sessions"
          ? "remote_not_enabled"
          : (value.error ?? "remote_unavailable"),
    );
    if (response.status >= 400 && response.status < 500)
      error.name = "RemoteRequestRejected";
    throw error;
  }
  return value;
}
export function remoteError(error: unknown): string {
  const messages: Record<string, string> = {
    gateway_unreachable: "网关服务暂时无法连接，正在自动重连。",
    request_timeout: "网络请求响应超时，正在自动重试。",
    remote_not_enabled:
      "远程连接尚未启用。请先配置 Gateway 内容存储并连接 Runner。",
    runner_offline: "这台电脑已离线，连接恢复后可继续。",
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
    zcode_not_available: "本机没有安装 ZCode。",
    claude_session_not_found: "本机找不到这个 Claude 会话。",
    acp_session_not_found: "本机找不到这个 ACP 会话。",
  };
  return (
    messages[error instanceof Error ? error.message : ""] ??
    "暂时无法完成操作，请检查电脑连接后重试。"
  );
}
export function conversationItems(
  events: RemoteEvent[],
  commands: RemoteCommandRecord[],
): ConversationItem[] {
  const result: ConversationItem[] = [];
  const items = new Map<string, ConversationItem>();
  const prompts = new Map<string, RemoteCommandRecord>();
  for (const command of commands)
    if (command.receipt?.result?.turn?.id)
      prompts.set(command.receipt.result.turn.id, command);
  const lastHistoryIndex = events.findLastIndex(
    (event) =>
      event.payload.event?.kind === "history/snapshot" &&
      Array.isArray(event.payload.event.raw?.params?.items) &&
      (event.payload.event.raw.params.items as unknown[]).length > 0,
  );
  if (lastHistoryIndex >= 0) {
    const history = events[lastHistoryIndex]!.payload.event!.raw!.params!
      .items as Array<Record<string, unknown>>;
    for (const [index, entry] of history.entries()) {
      const role = entry.role;
      const kind = entry.kind;
      const value =
        typeof entry.text === "string"
          ? entry.text
          : typeof entry.content === "string"
            ? entry.content
            : "";
      if (kind !== "message" && kind !== "tool" && kind !== "status") continue;
      if (!value && kind !== "tool") continue;
      const item: ConversationItem = {
        id: `history-${events[lastHistoryIndex]!.cursor}-${index}`,
        kind: kind === "tool" ? "tool" : role === "user" ? "user" : "assistant",
        text: value,
      };
      if (kind === "tool") {
        item.title = typeof entry.label === "string" ? entry.label : "工具调用";
        item.output = value;
        item.status =
          entry.status === "running" || entry.status === "failed"
            ? entry.status
            : "completed";
      }
      result.push(item);
    }
  }
  const renderedPromptCmdIds = new Set<string>();
  for (const event of events.slice(lastHistoryIndex + 1)) {
    const native = event.payload.event;
    if (!native) continue;
    const p = native.raw?.params ?? {};
    if (native.kind === "turn/started") {
      const turn = p.turn as { id?: string } | undefined;
      const command = prompts.get(turn?.id ?? "");
      if (command?.command.command.action === "send_prompt") {
        const attNames =
          command.command.command.attachments
            ?.map((a) => `[附件: ${a.name}]`)
            .join(" ") ?? "";
        const fullText = [attNames, command.command.command.text]
          .filter((part) => part && part.trim().length > 0)
          .join("\n");
        renderedPromptCmdIds.add(command.command.client_cmd_id);
        result.push({
          id: command.command.client_cmd_id,
          kind: "user",
          text: fullText,
        });
      }
    }
    const item = p.item as
      | {
          id?: string;
          type?: string;
          text?: string;
          command?: string;
          aggregatedOutput?: string;
          output?: string;
          name?: string;
          tool?: string;
          server?: string;
          status?: string;
          input?: { command?: string } | string;
          content?: Array<{ type?: string; text?: string }>;
        }
      | undefined;
    const id = typeof p.itemId === "string" ? p.itemId : item?.id;
    if (!id) continue;
    if (
      (native.kind === "item/completed" || native.kind === "item/started") &&
      item?.type === "userMessage" &&
      !items.has(id)
    ) {
      const userText =
        item.content
          ?.filter((c) => c.type === "text" && c.text)
          .map((c) => c.text)
          .join("\n") ??
        item.text ??
        "";
      if (userText) {
        const target: ConversationItem = {
          id,
          kind: "user",
          text: userText,
        };
        items.set(id, target);
        result.push(target);
      }
    } else if (native.kind === "item/agentMessage/delta") {
      let target = items.get(id);
      const deltaText = typeof p.delta === "string" ? p.delta : "";

      // 过滤裸露在 agentMessage delta 里的工具调用 JSON
      if (
        !target &&
        deltaText.trim().startsWith('{"command":') &&
        deltaText.includes('"description":')
      ) {
        try {
          const parsed = JSON.parse(deltaText.trim());
          if (parsed && typeof parsed.command === "string") {
            const cmd = parsed.command;
            target = {
              id,
              kind: "tool" as const,
              title: "命令执行",
              text: "工具执行中…",
              command: cmd,
              description: parsed.description,
              status: "running",
              turnId: typeof p.turnId === "string" ? p.turnId : undefined,
            };
            items.set(id, target);
            result.push(target);
            continue;
          }
        } catch {
          // ignore json parse error and treat as text
        }
      }

      if (!target) {
        target = { id, kind: "assistant", text: "" };
        items.set(id, target);
        result.push(target);
      }
      target.text += deltaText;
    } else if (
      native.kind === "item/completed" &&
      item?.type === "agentMessage"
    ) {
      let target = items.get(id);
      const fullText = item.text ?? "";

      // 同样检测并过滤 completed 消息中的裸工具 JSON
      if (
        fullText.trim().startsWith('{"command":') &&
        fullText.includes('"description":')
      ) {
        try {
          const parsed = JSON.parse(fullText.trim());
          if (parsed && typeof parsed.command === "string") {
            const cmd = parsed.command;
            if (!target) {
              target = {
                id,
                kind: "tool" as const,
                title: "命令执行",
                text: "工具执行已完成",
                command: cmd,
                description: parsed.description,
                status: "completed",
                turnId: typeof p.turnId === "string" ? p.turnId : undefined,
              };
              items.set(id, target);
              result.push(target);
            } else {
              target.kind = "tool";
              target.title = "命令执行";
              target.command = cmd;
              target.description = parsed.description;
              target.status = "completed";
            }
            continue;
          }
        } catch {
          // ignore
        }
      }

      if (!target) {
        target = { id, kind: "assistant", text: "" };
        items.set(id, target);
        result.push(target);
      }
      target.text = item.text ?? target.text;
    } else if (
      (native.kind === "item/completed" || native.kind === "item/started") &&
      item?.type === "commandExecution"
    ) {
      let target = items.get(id);
      const cmd =
        item.command ||
        (typeof item.input === "object" &&
        typeof item.input?.command === "string"
          ? item.input.command
          : "");
      const out = item.aggregatedOutput || item.output || "";

      // 如果当前 ID 没有直接命中，但前序有一个处于 running 状态的 tool（来自 agentMessage 拦截的 JSON 参数），则将其合并，防止产生“一个执行中、一个已完成”的割裂重复卡片
      if (!target) {
        for (let idx = result.length - 1; idx >= 0; idx--) {
          const prev = result[idx];
          if (
            prev &&
            prev.kind === "tool" &&
            prev.status === "running" &&
            ((typeof p.turnId === "string" && prev.turnId === p.turnId) ||
              (Boolean(prev.command) && prev.command === cmd)) &&
            (!prev.command || !cmd || prev.command === cmd)
          ) {
            target = prev;
            items.set(id, target); // 将 commandExecution ID 也映射到该项
            break;
          }
        }
      }

      if (!target) {
        target = {
          id,
          kind: "tool" as const,
          title: "命令执行",
          text:
            native.kind === "item/completed" ? "工具执行已完成" : "工具执行中…",
          command: cmd || undefined,
          output: out || undefined,
          status:
            item.status === "failed"
              ? "failed"
              : native.kind === "item/completed"
                ? "completed"
                : "running",
          turnId: typeof p.turnId === "string" ? p.turnId : undefined,
        };
        items.set(id, target);
        result.push(target);
      } else {
        if (cmd) {
          target.command = cmd;
          target.title = "命令执行";
        }
        if (out) {
          target.output = out;
        }
        if (native.kind === "item/completed") {
          target.status = item.status === "failed" ? "failed" : "completed";
          target.text = "工具执行已完成";
        }
      }
    } else if (
      (native.kind === "item/completed" || native.kind === "item/started") &&
      ["toolCall", "functionCall", "mcpToolCall", "fileChange"].includes(
        item?.type ?? "",
      )
    ) {
      const label =
        item?.type === "fileChange"
          ? "文件修改"
          : [item?.server, item?.tool ?? item?.name]
              .filter((part) => typeof part === "string" && part.length > 0)
              .join(" · ") || "工具调用";
      let target = items.get(id);
      if (!target) {
        target = { id, kind: "tool", text: "", title: label };
        items.set(id, target);
        result.push(target);
      }
      target.title = label;
      target.command =
        typeof item?.input === "string" ? item.input : target.command;
      target.output = item?.output ?? target.output;
      target.status =
        item?.status === "failed"
          ? "failed"
          : native.kind === "item/completed"
            ? "completed"
            : "running";
    }
  }
  for (const command of commands) {
    if (
      command.command.command.action === "send_prompt" &&
      command.receipt === null &&
      !renderedPromptCmdIds.has(command.command.client_cmd_id)
    ) {
      const attNames =
        command.command.command.attachments
          ?.map((a) => `[附件: ${a.name}]`)
          .join(" ") ?? "";
      const fullText = [attNames, command.command.command.text]
        .filter((part) => part && part.trim().length > 0)
        .join("\n");
      if (fullText) {
        result.push({
          id: command.command.client_cmd_id,
          kind: "user",
          text: fullText,
        });
      }
    }
  }
  return result;
}
export function pendingApprovals(
  events: RemoteEvent[],
  hubSessionId: string,
): RemotePendingApproval[] {
  const pending = new Map<string, RemotePendingApproval>();
  for (const envelope of events) {
    if (
      envelope.payload.type === "ready" &&
      Array.isArray(envelope.payload.pending_approvals)
    ) {
      pending.clear();
      for (const approval of envelope.payload.pending_approvals)
        if (approval.hub_session_id === hubSessionId)
          pending.set(approval.approval_id, approval);
      continue;
    }
    if (envelope.payload.type === "host_lost") {
      pending.clear();
      continue;
    }
    const native = envelope.payload.event;
    const receipt = envelope.payload.receipt;
    if (
      native?.kind === "approval_requested" &&
      native.approval &&
      native.approval.hub_session_id === hubSessionId
    )
      pending.set(native.approval.approval_id, native.approval);
    if (
      native?.kind === "approval_resolved" &&
      typeof native.approval_id === "string"
    )
      pending.delete(native.approval_id);
    if (
      native?.kind === "turn/completed" &&
      native.binding?.hub_session_id === hubSessionId
    )
      pending.clear();
    if (
      receipt?.status === "accepted" &&
      receipt.result?.response_sent === true &&
      typeof receipt.result.approval_id === "string"
    )
      pending.delete(receipt.result.approval_id);
  }
  return [...pending.values()];
}

export function pendingQuestions(
  events: RemoteEvent[],
  hubSessionId: string,
): RemotePendingQuestion[] {
  const pending = new Map<string, RemotePendingQuestion>();
  for (const envelope of events) {
    if (
      envelope.payload.type === "ready" &&
      Array.isArray(envelope.payload.pending_questions)
    ) {
      pending.clear();
      for (const question of envelope.payload.pending_questions) {
        if (question.hub_session_id === hubSessionId) {
          pending.set(question.question_id, question);
        }
      }
      continue;
    }
    if (envelope.payload.type === "host_lost") {
      pending.clear();
      continue;
    }
    const native = envelope.payload.event;
    const receipt = envelope.payload.receipt;
    if (
      native?.kind === "question_requested" &&
      native.question &&
      native.question.hub_session_id === hubSessionId
    ) {
      pending.set(native.question.question_id, native.question);
    }
    if (
      native?.kind === "question_resolved" &&
      typeof native.question_id === "string"
    ) {
      pending.delete(native.question_id);
    }
    if (
      native?.kind === "turn/completed" &&
      native.binding?.hub_session_id === hubSessionId
    ) {
      pending.clear();
    }
    if (
      receipt?.status === "accepted" &&
      receipt.result?.response_sent === true &&
      typeof receipt.result.question_id === "string"
    ) {
      pending.delete(receipt.result.question_id);
    }
  }
  return [...pending.values()];
}
