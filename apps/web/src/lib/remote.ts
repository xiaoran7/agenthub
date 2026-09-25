import type {
  RemoteBinding,
  RemoteCommand,
  RemotePendingApproval,
  RemotePendingQuestion,
} from "@agent-pulse/contracts/remote";
export type RemoteRequest = <T>(path: string, body?: unknown) => Promise<T>;
export interface DiscoveryCandidate {
  provider_session_id: string;
  workspace_path: string;
  title: string | null;
}
export interface RemoteRunner {
  runner_id: string;
  online: boolean;
  revoked: boolean;
  hello: null | {
    host_generation: string;
    pending_approvals?: RemotePendingApproval[];
    workspaces: string[];
    models?: {
      data: Array<{ id: string; model?: string; displayName: string }>;
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
    code?: string;
    result?: { turn?: { id: string } };
  } | null;
}
export interface ConversationItem {
  id: string;
  kind: "user" | "assistant" | "tool";
  text: string;
  title?: string;
  command?: string;
  output?: string;
  description?: string;
  status?: "running" | "completed" | "failed";
}

export async function remoteRequest<T>(
  path: string,
  body?: unknown,
): Promise<T> {
  const response = await fetch(`/v3/remote${path}`, {
    method: body === undefined ? "GET" : "POST",
    cache: "no-store",
    credentials: "same-origin",
    headers: { "content-type": "application/json", "x-user-id": "local-user" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
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
  for (const event of events) {
    const native = event.payload.event;
    if (!native) continue;
    const p = native.raw?.params ?? {};
    if (native.kind === "turn/started") {
      const turn = p.turn as { id?: string } | undefined;
      const command = prompts.get(turn?.id ?? "");
      if (command?.command.command.action === "send_prompt")
        result.push({
          id: command.command.client_cmd_id,
          kind: "user",
          text: command.command.command.text,
        });
    }
    const item = p.item as
      | {
          id?: string;
          type?: string;
          text?: string;
          command?: string;
          aggregatedOutput?: string;
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
              title: `执行命令: ${cmd.length > 50 ? cmd.slice(0, 50) + "…" : cmd}`,
              text: "工具执行中…",
              command: cmd,
              description: parsed.description,
              status: "running",
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
                title: `执行命令: ${cmd.length > 50 ? cmd.slice(0, 50) + "…" : cmd}`,
                text: "工具执行已完成",
                command: cmd,
                description: parsed.description,
                status: "completed",
              };
              items.set(id, target);
              result.push(target);
            } else {
              target.kind = "tool";
              target.title = `执行命令: ${cmd.length > 50 ? cmd.slice(0, 50) + "…" : cmd}`;
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
        (typeof (item as any).input?.command === "string"
          ? (item as any).input.command
          : "");
      const out = item.aggregatedOutput || (item as any).output || "";

      // 如果当前 ID 没有直接命中，但前序有一个处于 running 状态的 tool（来自 agentMessage 拦截的 JSON 参数），则将其合并，防止产生“一个执行中、一个已完成”的割裂重复卡片
      if (!target) {
        for (let idx = result.length - 1; idx >= 0; idx--) {
          const prev = result[idx];
          if (prev && prev.kind === "tool" && prev.status === "running") {
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
          title: cmd
            ? `执行命令: ${cmd.length > 50 ? cmd.slice(0, 50) + "…" : cmd}`
            : "工具调用",
          text:
            native.kind === "item/completed" ? "工具执行已完成" : "工具执行中…",
          command: cmd || undefined,
          output: out || undefined,
          status: native.kind === "item/completed" ? "completed" : "running",
        };
        items.set(id, target);
        result.push(target);
      } else {
        if (cmd) {
          target.command = cmd;
          target.title = `执行命令: ${cmd.length > 50 ? cmd.slice(0, 50) + "…" : cmd}`;
        }
        if (out) {
          target.output = out;
        }
        if (native.kind === "item/completed") {
          target.status = "completed";
          target.text = "工具执行已完成";
        }
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
