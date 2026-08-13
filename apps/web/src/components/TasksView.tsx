import { useMemo, useState } from "react";

import { createTask, cancelTask, describeApiError } from "../lib/api";
import type { PanelConfig } from "../lib/config";
import type { AccountView } from "../lib/account";
import type { DeviceRecord, RemoteTask, SessionRecord } from "../lib/types";
import { AGENT_LABELS, formatDateTimeCompact } from "../lib/platform-view";
import { Icon } from "./Icon";

const STATUS_LABELS: Record<RemoteTask["status"], string> = {
  queued: "排队中",
  delivered: "已送达",
  accepted: "已接受",
  running: "执行中",
  waiting_user: "等待本地确认",
  succeeded: "成功",
  failed: "失败",
  canceled: "已取消",
  expired: "已过期",
};

function TaskStatus({ status }: { status: RemoteTask["status"] }) {
  const tone = ["succeeded"].includes(status)
    ? "completed"
    : ["failed", "expired"].includes(status)
      ? "error"
      : ["waiting_user"].includes(status)
        ? "waiting"
        : ["queued", "delivered", "accepted", "running"].includes(status)
          ? "live"
          : "idle";
  return (
    <span className="status-mark" data-tone={tone}>
      <span className="status-dot" />
      {STATUS_LABELS[status]}
    </span>
  );
}

export function TasksView({
  account,
  config,
  devices,
  sessions,
  tasks,
  onRefresh,
}: {
  account: AccountView;
  config: PanelConfig;
  devices: DeviceRecord[];
  sessions: SessionRecord[];
  tasks: RemoteTask[];
  onRefresh: () => Promise<void>;
}) {
  const [deviceId, setDeviceId] = useState(
    devices.find((device) => !device.revoked)?.device_id ?? "",
  );
  const [agentType, setAgentType] = useState<"claude_code" | "codex">("codex");
  const [projectFingerprint, setProjectFingerprint] = useState("");
  const [prompt, setPrompt] = useState("");
  const [expiresIn, setExpiresIn] = useState("3600");
  const [submitting, setSubmitting] = useState(false);
  const [canceling, setCanceling] = useState<string | undefined>();
  const [error, setError] = useState<string | undefined>();
  const projects = useMemo(() => {
    const seen = new Set<string>();
    return sessions.filter((session) => {
      if (session.device_id !== deviceId) return false;
      if (seen.has(session.payload.project.fingerprint)) return false;
      seen.add(session.payload.project.fingerprint);
      return true;
    });
  }, [deviceId, sessions]);
  const canCreate =
    account.entitlements.remote_tasks &&
    devices.some((device) => !device.revoked);

  async function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canCreate) return;
    setSubmitting(true);
    setError(undefined);
    try {
      await createTask(config, {
        target_device_id: deviceId,
        agent_type: agentType,
        prompt: prompt.trim(),
        project_fingerprint: projectFingerprint,
        idempotency_key: crypto.randomUUID(),
        execution_policy: "local_confirmation",
        expires_in_seconds: Number(expiresIn),
      });
      setPrompt("");
      await onRefresh();
    } catch (nextError) {
      setError(describeApiError(nextError));
    } finally {
      setSubmitting(false);
    }
  }

  async function cancel(task: RemoteTask) {
    setCanceling(task.task_id);
    setError(undefined);
    try {
      await cancelTask(config, task.task_id);
      await onRefresh();
    } catch (nextError) {
      setError(describeApiError(nextError));
    } finally {
      setCanceling(undefined);
    }
  }

  return (
    <>
      <header className="view-header">
        <div>
          <h1>远程任务</h1>
          <p>
            通过已绑定 Runner 转发 Claude Code 与 Codex
            任务，执行前必须在本地确认。
          </p>
        </div>
        <span className="plan-chip">
          {account.subscription.plan.toUpperCase()} 方案
        </span>
      </header>
      <div className="tasks-layout">
        <section className="section-panel task-compose">
          <header className="section-panel-header">
            <h2>创建任务</h2>
            <span className="result-count">
              {account.entitlements.max_concurrent_tasks} 个并发额度
            </span>
          </header>
          {!account.entitlements.remote_tasks ? (
            <div className="task-locked" role="status">
              <Icon name="warning" size={20} />
              <strong>当前 FREE 方案不包含远程任务</strong>
              <p>升级到 PRO 或 TEAM 后，才能从网页提交任务到自己的 Runner。</p>
            </div>
          ) : devices.every((device) => device.revoked) ||
            devices.length === 0 ? (
            <div className="task-locked" role="status">
              <Icon name="devices" size={20} />
              <strong>先绑定一台 Runner</strong>
              <p>任务只会发送给你自己已绑定且未撤销的设备。</p>
            </div>
          ) : (
            <form
              className="task-form"
              onSubmit={(event) => void submit(event)}
            >
              <label>
                目标设备
                <select
                  value={deviceId}
                  onChange={(event) => setDeviceId(event.target.value)}
                  required
                >
                  <option value="" disabled>
                    选择设备
                  </option>
                  {devices
                    .filter((device) => !device.revoked)
                    .map((device) => (
                      <option key={device.device_id} value={device.device_id}>
                        {device.display_name ?? device.device_id.slice(0, 14)}
                      </option>
                    ))}
                </select>
              </label>
              <label>
                Agent
                <select
                  value={agentType}
                  onChange={(event) =>
                    setAgentType(event.target.value as typeof agentType)
                  }
                >
                  <option value="claude_code">
                    {AGENT_LABELS.claude_code}
                  </option>
                  <option value="codex">{AGENT_LABELS.codex}</option>
                </select>
              </label>
              <label>
                本地项目
                <select
                  value={projectFingerprint}
                  onChange={(event) =>
                    setProjectFingerprint(event.target.value)
                  }
                  required
                >
                  <option value="" disabled>
                    选择已上报项目
                  </option>
                  {projects.map((session) => (
                    <option
                      key={session.payload.project.fingerprint}
                      value={session.payload.project.fingerprint}
                    >
                      {session.payload.project.display_name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                提示词
                <textarea
                  value={prompt}
                  onChange={(event) => setPrompt(event.target.value)}
                  placeholder="输入要交给本地 Agent 的任务"
                  maxLength={32000}
                  required
                />
              </label>
              <label>
                过期时间
                <select
                  value={expiresIn}
                  onChange={(event) => setExpiresIn(event.target.value)}
                >
                  <option value="1800">30 分钟</option>
                  <option value="3600">1 小时</option>
                  <option value="14400">4 小时</option>
                  <option value="86400">24 小时</option>
                </select>
              </label>
              <div className="task-form-policy">
                <Icon name="check" size={16} />
                <span>
                  执行策略：本地确认。Runner
                  会在执行前显示完整提示词并等待确认。
                </span>
              </div>
              {error !== undefined && (
                <p className="error-text" role="alert">
                  {error}
                </p>
              )}
              <button
                className="primary"
                disabled={
                  submitting ||
                  prompt.trim() === "" ||
                  projectFingerprint === ""
                }
                type="submit"
              >
                {submitting ? "提交中…" : "提交任务"}
              </button>
            </form>
          )}
        </section>
        <section className="section-panel task-list-panel">
          <header className="section-panel-header">
            <h2>任务记录</h2>
            <span className="result-count">{tasks.length} 条</span>
          </header>
          <div className="task-list">
            {tasks.length === 0 ? (
              <div className="panel-empty">还没有远程任务。</div>
            ) : (
              tasks.map((task) => (
                <article className="task-row" key={task.task_id}>
                  <div className="task-row-main">
                    <div className="task-row-title">
                      <TaskStatus status={task.status} />
                      <strong>{AGENT_LABELS[task.agent_type]}</strong>
                      <span className="mono">{task.task_id.slice(0, 8)}</span>
                    </div>
                    <p>{task.prompt}</p>
                    <small>
                      {formatDateTimeCompact(task.created_at)} · 过期于{" "}
                      {formatDateTimeCompact(task.expires_at)}
                    </small>
                  </div>
                  {[
                    "queued",
                    "delivered",
                    "accepted",
                    "running",
                    "waiting_user",
                  ].includes(task.status) && (
                    <button
                      className="secondary"
                      disabled={canceling === task.task_id}
                      onClick={() => void cancel(task)}
                      type="button"
                    >
                      {canceling === task.task_id ? "取消中…" : "取消"}
                    </button>
                  )}
                  {task.error_code !== undefined && (
                    <span className="task-error">{task.error_code}</span>
                  )}
                </article>
              ))
            )}
          </div>
        </section>
      </div>
    </>
  );
}
