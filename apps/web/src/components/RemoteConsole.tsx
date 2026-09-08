"use client";
import { useEffect, useMemo, useRef, useState } from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import {
  conversationItems,
  pendingApprovals,
  remoteError,
  remoteRequest,
  type RemoteRequest,
  type RemoteSnapshot,
  type RemoteSession,
  type RemoteEvent,
  type RemoteCommandRecord,
} from "../lib/remote";
import "./remote.css";

const empty: RemoteSnapshot = { cursor: 0, runners: [], sessions: [] };
const states: Record<string, string> = {
  idle: "就绪",
  running: "正在工作",
  awaiting_permission: "等待审批",
  awaiting_input: "等待输入",
  unknown: "状态待确认",
  error: "执行出错",
  completed: "已完成",
  terminated: "已结束",
};
const key = (session: RemoteSession) =>
  `${session.runner_id}/${session.binding.hub_session_id}`;
const basename = (path: string) =>
  path
    .replace(/[\\/]+$/u, "")
    .split(/[\\/]/u)
    .pop() ?? path;

export function RemoteConsole({
  request = remoteRequest,
  onAuthenticationRequired,
  onManage,
  onLocalHost,
}: {
  request?: RemoteRequest;
  onAuthenticationRequired?: () => void;
  onManage?: () => void;
  onLocalHost?: () => void;
}) {
  const [snapshot, setSnapshot] = useState(empty);
  const [selected, setSelected] = useState("");
  const [events, setEvents] = useState<RemoteEvent[]>([]);
  const [commands, setCommands] = useState<RemoteCommandRecord[]>([]);
  const [error, setError] = useState("");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  // Keep an uncertain POST's identity when the user retries the same operation.
  const attempts = useRef(new Map<string, RemoteCommand>());
  const [loading, setLoading] = useState(true);
  const [runner, setRunner] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [model, setModel] = useState("");
  const [sandbox, setSandbox] = useState<"read-only" | "workspace-write">(
    "read-only",
  );
  const [nativeId, setNativeId] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  const timeline = useRef<HTMLDivElement>(null);
  const refresh = useRef(() => {});
  const session = snapshot.sessions.find((item) => key(item) === selected);
  const runnerInfo = snapshot.runners.find((item) => item.runner_id === runner);
  const connected = !!session?.online && !!session.binding.control_connected;
  const canSend =
    connected &&
    ["idle", "completed"].includes(session?.binding.execution_state ?? "");
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    let inFlight = false;
    const poll = async () => {
      if (stopped || inFlight) return;
      inFlight = true;
      try {
        const value = await request<RemoteSnapshot>("/sessions");
        if (!stopped) {
          setSnapshot(value);
          setLoading(false);
        }
      } catch (err) {
        if (!stopped) {
          setSnapshot((previous) => ({
            ...previous,
            runners: previous.runners.map((item) => ({
              ...item,
              online: false,
            })),
            sessions: previous.sessions.map((item) => ({
              ...item,
              online: false,
            })),
          }));
          setError(remoteError(err));
          setLoading(false);
          if (err instanceof Error && err.message === "authentication_required")
            onAuthenticationRequired?.();
        }
      } finally {
        inFlight = false;
        if (!stopped) timer = setTimeout(() => void poll(), 1500);
      }
    };
    refresh.current = () => {
      clearTimeout(timer);
      void poll();
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [request, onAuthenticationRequired]);
  useEffect(() => {
    setEvents([]);
    setCommands([]);
    setText("");
    if (!selected) return;
    const [runnerId, hub] = selected.split("/");
    if (!runnerId || !hub) return;
    let stopped = false;
    let cursor = 0;
    let accumulated: RemoteEvent[] = [];
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const query = new URLSearchParams({
          runner: runnerId,
          hub,
          after: String(cursor),
        });
        const [batch, detail] = await Promise.all([
          request<{ events: RemoteEvent[] }>(`/events?${query}`),
          request<{ commands: RemoteCommandRecord[] }>(
            `/sessions/${encodeURIComponent(runnerId)}/${encodeURIComponent(hub)}`,
          ),
        ]);
        if (stopped) return;
        if (batch.events.length) {
          cursor = batch.events[batch.events.length - 1]!.cursor;
          accumulated = [...accumulated, ...batch.events];
          setEvents(accumulated);
        }
        setCommands(detail.commands);
      } catch (err) {
        if (!stopped) setError(remoteError(err));
      }
      if (!stopped) timer = setTimeout(() => void poll(), 400);
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [request, selected]);
  const items = useMemo(
    () => conversationItems(events, commands),
    [events, commands],
  );
  const approvals = useMemo(() => pendingApprovals(events), [events]);
  useEffect(() => {
    const confirmed = new Set(
      commands
        .filter((item) => item.receipt)
        .map((item) => item.command.client_cmd_id),
    );
    for (const [key, command] of attempts.current)
      if (confirmed.has(command.client_cmd_id)) attempts.current.delete(key);
  }, [commands]);
  useEffect(() => {
    const pane = timeline.current;
    if (pane && pane.scrollHeight - pane.scrollTop - pane.clientHeight < 300)
      pane.scrollTop = pane.scrollHeight;
  }, [items]);
  const latestFailure = [...commands]
    .reverse()
    .find((item) =>
      ["rejected", "indeterminate"].includes(item.receipt?.status ?? ""),
    );
  const submit = async (action: RemoteCommand["command"], target = session) => {
    if (!target) return;
    setBusy(true);
    setError("");
    const attemptKey = JSON.stringify([
      target.runner_id,
      target.binding.hub_session_id,
      action,
    ]);
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: crypto.randomUUID(),
        hub_session_id: target.binding.hub_session_id,
        host_generation: target.binding.host_generation,
        command: action,
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await request(
        `/runners/${encodeURIComponent(target.runner_id)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      if (action.action === "send_prompt") setText("");
      refresh.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setBusy(false);
    }
  };
  const launch = async () => {
    if (!runner || !workspace) return;
    setBusy(true);
    setError("");
    const attemptKey = JSON.stringify([
      "launch",
      runner,
      workspace,
      nativeId,
      sandbox,
      model,
    ]);
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: crypto.randomUUID(),
        hub_session_id: crypto.randomUUID(),
        host_generation: null,
        command: {
          action: "connect",
          mode: nativeId ? "resume" : "create",
          workspace_path: workspace,
          sandbox,
          approval_policy: "untrusted",
          ...(model ? { model } : {}),
          ...(nativeId ? { provider_session_id: nativeId } : {}),
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await request(`/runners/${encodeURIComponent(runner)}/commands`, command);
      attempts.current.delete(attemptKey);
      setSelected(`${runner}/${command.hub_session_id}`);
      dialog.current?.close();
      refresh.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setBusy(false);
    }
  };
  const groups = new Map<string, RemoteSession[]>();
  for (const item of snapshot.sessions) {
    const group = `${item.runner_id}\n${item.binding.workspace_path}`;
    groups.set(group, [...(groups.get(group) ?? []), item]);
  }
  return (
    <div className="remote-console">
      <aside className="remote-sidebar">
        <div className="remote-brand">
          AgentHub <span>Remote</span>
        </div>
        <button
          className="remote-primary"
          onClick={() => {
            const available = snapshot.runners.find(
              (item) => item.online && item.hello?.workspaces.length,
            );
            setRunner(available?.runner_id ?? "");
            setWorkspace(available?.hello?.workspaces[0] ?? "");
            setNativeId("");
            setModel("");
            dialog.current?.showModal();
          }}
        >
          ＋ 新建会话
        </button>
        <div className="remote-nav-label">工作区</div>
        {onLocalHost && <button onClick={onLocalHost}>连接本机</button>}
        {[...groups].map(([group, sessions]) => (
          <section key={group}>
            <h2 title={sessions[0]!.binding.workspace_path}>
              {basename(sessions[0]!.binding.workspace_path)}
            </h2>
            {sessions.map((item) => (
              <button
                className={`remote-session ${key(item) === selected ? "selected" : ""}`}
                key={key(item)}
                onClick={() => {
                  setSelected(key(item));
                  setError("");
                }}
              >
                <span>
                  Codex · {item.binding.provider_session_id.slice(0, 8)}
                </span>
                <small>
                  {item.online
                    ? states[item.binding.execution_state]
                    : "电脑离线"}
                </small>
              </button>
            ))}
          </section>
        ))}
        {!groups.size && (
          <p className="remote-muted">连接电脑后，在工作区中开始会话。</p>
        )}
        <div className="remote-sidebar-footer">
          {snapshot.runners.filter((item) => item.online).length} 台电脑在线
          <button
            onClick={
              onManage ??
              (() => {
                window.location.href = "/?legacy=1";
              })
            }
          >
            设备与账户
          </button>
        </div>
      </aside>
      <main className="remote-main">
        <header>
          <div>
            <h1>
              {session
                ? basename(session.binding.workspace_path)
                : "随时接着做"}
            </h1>
            <p>
              {session
                ? `Codex · ${session.online ? (connected ? "会话已连接" : "会话未连接") : "电脑离线"}`
                : "电脑执行，随处继续。"}
            </p>
          </div>
          {session?.binding.current_turn_id && (
            <button
              disabled={!connected || busy}
              onClick={() => void submit({ action: "interrupt" })}
            >
              停止任务
            </button>
          )}
        </header>
        {error && (
          <div role="alert" className="remote-notice">
            {error}
            <button aria-label="关闭提示" onClick={() => setError("")}>
              ×
            </button>
          </div>
        )}
        {latestFailure && (
          <div className="remote-notice" role="status">
            {remoteError(new Error(latestFailure.receipt?.code))}
          </div>
        )}
        {!selected ? (
          <div className="remote-welcome">
            <span className="remote-eyebrow">你的工作，仍在原处</span>
            <h2>{loading ? "正在连接…" : "打开一个会话，继续刚才的工作。"}</h2>
            <p>
              消息、工具操作和审批都来自电脑上的原生
              Agent。关闭此页面后，已连接的任务仍可继续执行。
            </p>
          </div>
        ) : (
          <>
            {!session && <p className="remote-muted">正在等待电脑确认会话…</p>}
            {session && !connected && (
              <div className="remote-notice">
                已同步的内容仍可查看。
                {session.online && (
                  <button
                    disabled={busy}
                    onClick={() =>
                      void submit({
                        action: "connect",
                        mode: "resume",
                        workspace_path: session.binding.workspace_path,
                        provider_session_id:
                          session.binding.provider_session_id,
                      })
                    }
                  >
                    恢复会话
                  </button>
                )}
              </div>
            )}
            <div
              className="remote-timeline"
              ref={timeline}
              aria-label="会话内容"
            >
              {items.map((item) =>
                item.kind === "tool" ? (
                  <details className="remote-tool" key={item.id}>
                    <summary>{item.title}</summary>
                    <pre>{item.text}</pre>
                  </details>
                ) : (
                  <article
                    className={`remote-message ${item.kind}`}
                    key={item.id}
                  >
                    <small>{item.kind === "user" ? "你" : "Codex"}</small>
                    <div>{item.text}</div>
                  </article>
                ),
              )}
              {!items.length && session && (
                <p className="remote-muted">会话已建立，可以发送第一条消息。</p>
              )}
              {approvals.map((approval) => (
                <section className="remote-approval" key={approval.id}>
                  <h3>需要你的确认</h3>
                  <strong>{approval.title}</strong>
                  <details>
                    <summary>查看详情</summary>
                    <pre>{approval.details}</pre>
                  </details>
                  <div>
                    <button
                      disabled={!connected || busy}
                      onClick={() =>
                        void submit({
                          action: "resolve_approval",
                          approval_id: approval.id,
                          approved: false,
                        })
                      }
                    >
                      拒绝
                    </button>
                    <button
                      className="remote-primary"
                      disabled={!connected || busy}
                      onClick={() =>
                        void submit({
                          action: "resolve_approval",
                          approval_id: approval.id,
                          approved: true,
                        })
                      }
                    >
                      允许此次操作
                    </button>
                  </div>
                </section>
              ))}
            </div>
            <form
              className="remote-composer"
              onSubmit={(event) => {
                event.preventDefault();
                if (canSend && text.trim())
                  void submit({ action: "send_prompt", text });
              }}
            >
              <label htmlFor="remote-prompt">
                {session
                  ? !session.online
                    ? "连接已断开"
                    : states[session.binding.execution_state]
                  : "正在连接"}
              </label>
              <textarea
                id="remote-prompt"
                value={text}
                onChange={(event) => setText(event.target.value)}
                placeholder={canSend ? "继续这段工作…" : "等待会话就绪后发送"}
                rows={3}
                disabled={!connected}
              />
              <div>
                <small>会话内容会同步到你的 Hub</small>
                <button
                  className="remote-primary"
                  disabled={!canSend || busy || !text.trim()}
                >
                  发送
                </button>
              </div>
            </form>
          </>
        )}
      </main>
      <dialog ref={dialog} className="remote-launch">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void launch();
          }}
        >
          <header>
            <h2>在电脑上开始</h2>
            <button
              type="button"
              aria-label="关闭"
              onClick={() => dialog.current?.close()}
            >
              ×
            </button>
          </header>
          <label>
            电脑
            <select
              value={runner}
              onChange={(event) => {
                setRunner(event.target.value);
                setWorkspace(
                  snapshot.runners.find(
                    (item) => item.runner_id === event.target.value,
                  )?.hello?.workspaces[0] ?? "",
                );
                setNativeId("");
              }}
            >
              <option value="">选择在线电脑</option>
              {snapshot.runners
                .filter((item) => item.online)
                .map((item) => (
                  <option value={item.runner_id} key={item.runner_id}>
                    {item.runner_id}
                  </option>
                ))}
            </select>
          </label>
          <label>
            工作区
            <select
              value={workspace}
              onChange={(event) => {
                setWorkspace(event.target.value);
                setNativeId("");
              }}
            >
              <option value="">选择已授权目录</option>
              {runnerInfo?.hello?.workspaces.map((path) => (
                <option value={path} key={path}>
                  {path}
                </option>
              ))}
            </select>
          </label>
          <label>
            会话
            <select
              value={nativeId}
              onChange={(event) => setNativeId(event.target.value)}
            >
              <option value="">创建新会话</option>
              {runnerInfo?.hello?.discovery?.candidates
                .filter(
                  (item) =>
                    item.workspace_path
                      .replace(/^\\\\\?\\/u, "")
                      .replaceAll("\\", "/")
                      .toLowerCase() ===
                    workspace
                      .replace(/^\\\\\?\\/u, "")
                      .replaceAll("\\", "/")
                      .toLowerCase(),
                )
                .map((item) => (
                  <option
                    key={item.provider_session_id}
                    value={item.provider_session_id}
                  >
                    恢复：{item.title ?? item.provider_session_id}
                  </option>
                ))}
            </select>
          </label>
          <label>
            模型
            <select
              value={model}
              onChange={(event) => setModel(event.target.value)}
            >
              <option value="">沿用原生设置</option>
              {runnerInfo?.hello?.models?.data.map((item) => (
                <option key={item.id} value={item.model ?? item.id}>
                  {item.displayName}
                </option>
              ))}
            </select>
          </label>
          <label>
            文件权限
            <select
              value={sandbox}
              onChange={(event) =>
                setSandbox(event.target.value as typeof sandbox)
              }
            >
              <option value="read-only">只读</option>
              <option value="workspace-write">允许修改工作区</option>
            </select>
          </label>
          <p>
            执行发生在所选电脑。会话内容将同步到你的 Hub，供其他设备查看和继续。
          </p>
          <button
            className="remote-primary"
            disabled={busy || !workspace || !runner}
          >
            {nativeId ? "恢复会话" : "创建会话"}
          </button>
        </form>
      </dialog>
    </div>
  );
}
