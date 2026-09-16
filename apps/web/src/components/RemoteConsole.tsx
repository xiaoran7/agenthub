"use client";
import { useEffect, useMemo, useRef, useState } from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import type { AccountView } from "../lib/account";
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
    .replace(/^\\\\\?\\/u, "")
    .replace(/[\\/]+$/u, "")
    .split(/[\\/]/u)
    .pop() ?? path;
const cleanPath = (path: string) => path.replace(/^\\\\\?\\/u, "");
const workspaceLabel = (workspaceId: string) =>
  workspaceId.startsWith("ws-")
    ? workspaceId.slice(3).replace(/-[0-9a-f]{16}$/u, "") || workspaceId
    : basename(workspaceId);

function getProviderInfo(providerSessionId: string): {
  type: "codex" | "zcode" | "claude";
  name: string;
  badgeClass: string;
} {
  if (providerSessionId.startsWith("sess_")) {
    return { type: "zcode", name: "ZCode", badgeClass: "zcode" };
  }
  if (providerSessionId.startsWith("claude_")) {
    return { type: "claude", name: "Claude", badgeClass: "claude" };
  }
  return { type: "codex", name: "Codex", badgeClass: "codex" };
}

export function RemoteConsole({
  request = remoteRequest,
  onAuthenticationRequired,
  onManage,
  onLocalHost,
  account,
  onLogout,
}: {
  request?: RemoteRequest;
  onAuthenticationRequired?: () => void;
  onManage?: () => void;
  onLocalHost?: () => void;
  account?: AccountView | null;
  onLogout?: () => void;
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
  const autoResumedRef = useRef<Set<string>>(new Set());
  const [loading, setLoading] = useState(true);

  // 侧边栏宽度与折叠状态持久化
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    if (typeof window !== "undefined") {
      const saved = localStorage.getItem("agenthub_remote_sidebar_width");
      if (saved) {
        const parsed = parseInt(saved, 10);
        if (!isNaN(parsed) && parsed >= 180 && parsed <= 520) return parsed;
      }
    }
    return 280;
  });
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    if (typeof window !== "undefined") {
      return (
        localStorage.getItem("agenthub_remote_sidebar_collapsed") === "true"
      );
    }
    return false;
  });
  const isResizingRef = useRef(false);

  const toggleSidebar = () => {
    setSidebarCollapsed((prev) => {
      const next = !prev;
      localStorage.setItem("agenthub_remote_sidebar_collapsed", String(next));
      return next;
    });
  };

  const handleResizerMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    isResizingRef.current = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;

    const onMouseMove = (moveEvent: MouseEvent) => {
      if (!isResizingRef.current) return;
      const delta = moveEvent.clientX - startX;
      const nextWidth = Math.min(Math.max(startWidth + delta, 180), 520);
      setSidebarWidth(nextWidth);
    };

    const onMouseUp = () => {
      isResizingRef.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
      localStorage.setItem(
        "agenthub_remote_sidebar_width",
        String(sidebarWidth),
      );
    };

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  };

  const [providerFilter, setProviderFilter] = useState<
    "all" | "codex" | "claude" | "zcode"
  >("all");
  const [runner, setRunner] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [model, setModel] = useState("");
  const [sandbox, setSandbox] = useState<"read-only" | "workspace-write">(
    "read-only",
  );
  const [nativeId, setNativeId] = useState("");
  const dialog = useRef<HTMLDialogElement>(null);
  const timeline = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const refresh = useRef(() => {});
  const session = snapshot.sessions.find((item) => key(item) === selected);
  const runnerInfo = snapshot.runners.find((item) => item.runner_id === runner);
  const connected = !!session?.online && !!session.binding.control_connected;
  const canSend =
    connected &&
    ["idle", "completed"].includes(session?.binding.execution_state ?? "");

  const adjustTextareaHeight = () => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(Math.max(el.scrollHeight, 28), 220)}px`;
  };
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
  const approvals = useMemo(
    () => pendingApprovals(events, session?.binding.hub_session_id ?? ""),
    [events, session?.binding.hub_session_id],
  );
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

  // 核心特性：选定会话时全量自动开始恢复接入，无需手动点击恢复
  useEffect(() => {
    if (
      !session ||
      !session.online ||
      session.binding.control_connected ||
      busy
    )
      return;
    const sessionKey = key(session);
    if (autoResumedRef.current.has(sessionKey)) return;
    autoResumedRef.current.add(sessionKey);
    void submit(
      {
        action: "connect",
        mode: "resume",
        workspace_path: session.binding.workspace_path,
        provider_session_id: session.binding.provider_session_id,
      },
      session,
    );
  }, [selected, session?.online, session?.binding.control_connected, busy]);

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
      if (action.action === "send_prompt") {
        setText("");
        if (textareaRef.current) textareaRef.current.style.height = "auto";
      }
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
    const launchRunner = snapshot.runners.find(
      (item) => item.runner_id === runner,
    );
    const generation = launchRunner?.hello?.host_generation;
    if (!runner || !workspace || !generation) return;
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
        host_generation: generation,
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
  const resumeCandidate = async (
    targetRunnerId: string,
    targetWorkspace: string,
    providerSessionId: string,
  ) => {
    const runnerObj = snapshot.runners.find(
      (item) => item.runner_id === targetRunnerId,
    );
    const generation = runnerObj?.hello?.host_generation;
    if (!runnerObj || !generation || busy) return;
    setBusy(true);
    setError("");
    const attemptKey = JSON.stringify([
      "launch",
      targetRunnerId,
      targetWorkspace,
      providerSessionId,
      "read-only",
      "",
    ]);
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: crypto.randomUUID(),
        hub_session_id: crypto.randomUUID(),
        host_generation: generation,
        command: {
          action: "connect",
          mode: "resume",
          workspace_path: targetWorkspace,
          sandbox: "read-only",
          approval_policy: "untrusted",
          provider_session_id: providerSessionId,
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await request(
        `/runners/${encodeURIComponent(targetRunnerId)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      setSelected(`${targetRunnerId}/${command.hub_session_id}`);
      refresh.current();
    } catch (err) {
      if (err instanceof Error && err.name === "RemoteRequestRejected")
        attempts.current.delete(attemptKey);
      setError(remoteError(err));
    } finally {
      setBusy(false);
    }
  };

  type FlatSessionItem =
    | {
        kind: "active";
        key: string;
        session: RemoteSession;
        title: string;
        subtitle: string;
        badge: string;
      }
    | {
        kind: "candidate";
        key: string;
        runnerId: string;
        candidate: {
          provider_session_id: string;
          workspace_path: string;
          title: string | null;
        };
        title: string;
        subtitle: string;
        badge: string;
      };

  const allItems = useMemo(() => {
    const list: FlatSessionItem[] = [];
    const activeProviderIds = new Set<string>();

    // 1. 已建立受控绑定的会话（排在最前）
    for (const item of snapshot.sessions) {
      activeProviderIds.add(item.binding.provider_session_id);
      const wsLabel = workspaceLabel(item.binding.workspace_path);
      const provider = getProviderInfo(item.binding.provider_session_id);
      if (providerFilter !== "all" && providerFilter !== provider.type)
        continue;

      const rawId = item.binding.provider_session_id.replace(
        /^(sess_|claude_)/,
        "",
      );
      list.push({
        kind: "active",
        key: key(item),
        session: item,
        title: `${provider.name} · ${rawId.slice(0, 8)}`,
        subtitle: `${wsLabel} · ${item.online ? states[item.binding.execution_state] : "电脑离线"}`,
        badge: "已连接",
      });
    }

    // 2. 本机扫描发现的所有历史会话（去重排除已绑定的）
    for (const r of snapshot.runners) {
      if (!r.online || !r.hello?.discovery?.candidates) continue;
      for (const cand of r.hello.discovery.candidates) {
        if (activeProviderIds.has(cand.provider_session_id)) continue;
        const wsLabel = workspaceLabel(cand.workspace_path);
        const provider = getProviderInfo(cand.provider_session_id);
        if (providerFilter !== "all" && providerFilter !== provider.type)
          continue;

        const rawId = cand.provider_session_id.replace(/^(sess_|claude_)/, "");
        const title = cand.title || `${provider.name} · ${rawId.slice(0, 8)}`;
        list.push({
          kind: "candidate",
          key: `${r.runner_id}/${cand.provider_session_id}`,
          runnerId: r.runner_id,
          candidate: cand,
          title,
          subtitle: `${wsLabel} · 本机历史`,
          badge: "点击接管",
        });
      }
    }

    return list;
  }, [snapshot.sessions, snapshot.runners, providerFilter]);

  return (
    <div className="remote-console">
      <aside
        className={`remote-sidebar ${sidebarCollapsed ? "collapsed" : ""}`}
        style={{
          width: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
          minWidth: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
          maxWidth: sidebarCollapsed ? "0px" : `${sidebarWidth}px`,
        }}
      >
        <div className="remote-sidebar-header">
          <div className="remote-brand">
            AgentHub <span>Remote</span>
          </div>
          <button
            type="button"
            className="remote-icon-btn"
            title="收起侧边栏"
            aria-label="收起侧边栏"
            onClick={toggleSidebar}
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
            >
              <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
              <line x1="9" y1="3" x2="9" y2="21" />
              <path d="M15 9l-3 3 3 3" />
            </svg>
          </button>
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
        <div className="remote-filter-pill-row">
          <button
            type="button"
            className={`remote-filter-pill ${providerFilter === "all" ? "active" : ""}`}
            onClick={() => setProviderFilter("all")}
          >
            全部
          </button>
          <button
            type="button"
            className={`remote-filter-pill ${providerFilter === "codex" ? "active" : ""}`}
            onClick={() => setProviderFilter("codex")}
          >
            Codex
          </button>
          <button
            type="button"
            className={`remote-filter-pill ${providerFilter === "claude" ? "active" : ""}`}
            onClick={() => setProviderFilter("claude")}
          >
            Claude
          </button>
          <button
            type="button"
            className={`remote-filter-pill ${providerFilter === "zcode" ? "active" : ""}`}
            onClick={() => setProviderFilter("zcode")}
          >
            ZCode
          </button>
        </div>
        <div className="remote-nav-label">会话记录</div>
        {onLocalHost && <button onClick={onLocalHost}>连接本机</button>}
        <div className="remote-session-list">
          {allItems.map((item) => {
            if (item.kind === "active") {
              const activeSession = item.session;
              const isSelected = key(activeSession) === selected;
              const pInfo = getProviderInfo(
                activeSession.binding.provider_session_id,
              );
              return (
                <button
                  className={`remote-session ${isSelected ? "selected" : ""}`}
                  key={item.key}
                  onClick={() => {
                    setSelected(key(activeSession));
                    setError("");
                    if (
                      activeSession.online &&
                      !activeSession.binding.control_connected
                    ) {
                      void submit(
                        {
                          action: "connect",
                          mode: "resume",
                          workspace_path: activeSession.binding.workspace_path,
                          provider_session_id:
                            activeSession.binding.provider_session_id,
                        },
                        activeSession,
                      );
                    }
                  }}
                >
                  <span className="remote-session-title">{item.title}</span>
                  <div className="remote-session-meta">
                    <small>{item.subtitle}</small>
                    <span
                      className={`remote-session-badge ${pInfo.badgeClass}`}
                    >
                      {item.badge}
                    </span>
                  </div>
                </button>
              );
            }

            const pInfo = getProviderInfo(item.candidate.provider_session_id);
            return (
              <button
                className="remote-session candidate"
                key={item.key}
                title={`点击接管：${item.candidate.title ?? item.candidate.provider_session_id}`}
                disabled={busy}
                onClick={() =>
                  void resumeCandidate(
                    item.runnerId,
                    item.candidate.workspace_path,
                    item.candidate.provider_session_id,
                  )
                }
              >
                <span className="remote-session-title">{item.title}</span>
                <div className="remote-session-meta">
                  <small>{item.subtitle}</small>
                  <span className={`remote-session-badge ${pInfo.badgeClass}`}>
                    {item.badge}
                  </span>
                </div>
              </button>
            );
          })}
        </div>
        {!allItems.length && (
          <p className="remote-muted">连接电脑后，在工作区中开始会话。</p>
        )}
        <div className="remote-sidebar-footer">
          <div className="remote-runner-status">
            <span
              className={`remote-status-dot ${snapshot.runners.some((item) => item.online) ? "online" : "offline"}`}
            />
            <span>
              {snapshot.runners.filter((item) => item.online).length} 台电脑在线
            </span>
          </div>
          {onManage && (
            <button
              className="remote-action-btn"
              onClick={onManage}
              type="button"
            >
              ＋ 绑定新电脑
            </button>
          )}
          {account?.user && (
            <div className="remote-account-bar">
              <span className="remote-account-name" title={account.user.email}>
                {account.user.display_name || account.user.email}
              </span>
              {onLogout && (
                <button
                  className="remote-logout-btn"
                  onClick={onLogout}
                  type="button"
                >
                  退出
                </button>
              )}
            </div>
          )}
        </div>
      </aside>
      {!sidebarCollapsed && (
        <div
          className="remote-sidebar-resizer"
          onMouseDown={handleResizerMouseDown}
          title="拖拽调整侧边栏宽度"
        />
      )}
      <main className="remote-main">
        <header>
          <div className="remote-header-left">
            {sidebarCollapsed && (
              <button
                type="button"
                className="remote-icon-btn expand-btn"
                title="展开侧边栏"
                aria-label="展开侧边栏"
                onClick={toggleSidebar}
              >
                <svg
                  width="18"
                  height="18"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <line x1="9" y1="3" x2="9" y2="21" />
                  <path d="M13 15l3-3-3-3" />
                </svg>
              </button>
            )}
            <div>
              <h1>
                {session
                  ? workspaceLabel(session.binding.workspace_path)
                  : "随时接着做"}
              </h1>
              <p>
                {session
                  ? `${session.binding.provider_session_id.startsWith("sess_") ? "ZCode" : "Codex"} · ${session.online ? (connected ? "会话已连接" : "会话未连接") : "电脑离线"}`
                  : "电脑执行，随处继续。"}
              </p>
            </div>
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
              <div className="remote-notice restoring">
                <span className="remote-notice-spinner" />
                <span>
                  {session.online
                    ? "正在全量恢复连接原生 Agent 会话…"
                    : "电脑处于离线状态，上线后将自动恢复接入。"}
                </span>
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
                    重试连接
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
                    <small>
                      {item.kind === "user"
                        ? "你"
                        : getProviderInfo(
                            session?.binding.provider_session_id ?? "",
                          ).name}
                    </small>
                    <div>{item.text}</div>
                  </article>
                ),
              )}
              {!items.length && session && (
                <p className="remote-muted">会话已建立，可以发送第一条消息。</p>
              )}
              {approvals.map((approval) => (
                <section className="remote-approval" key={approval.approval_id}>
                  <h3>需要你的确认</h3>
                  <strong>
                    {
                      getProviderInfo(
                        session?.binding.provider_session_id ?? "",
                      ).name
                    }{" "}
                    请求执行受保护的操作
                  </strong>
                  <div>
                    <button
                      disabled={!connected || busy || !approval.can_decline}
                      onClick={() =>
                        void submit({
                          action: "resolve_approval",
                          approval_id: approval.approval_id,
                          approved: false,
                        })
                      }
                    >
                      拒绝
                    </button>
                    <button
                      className="remote-primary"
                      disabled={!connected || busy || !approval.can_approve}
                      onClick={() =>
                        void submit({
                          action: "resolve_approval",
                          approval_id: approval.approval_id,
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
              className="remote-composer-card"
              onSubmit={(event) => {
                event.preventDefault();
                if (canSend && text.trim()) {
                  void submit({ action: "send_prompt", text });
                  if (textareaRef.current)
                    textareaRef.current.style.height = "auto";
                }
              }}
            >
              <div className="remote-composer-input-wrapper">
                <textarea
                  ref={textareaRef}
                  id="remote-prompt"
                  value={text}
                  onChange={(event) => {
                    setText(event.target.value);
                    adjustTextareaHeight();
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && !event.shiftKey) {
                      event.preventDefault();
                      if (canSend && text.trim()) {
                        void submit({ action: "send_prompt", text });
                        if (textareaRef.current)
                          textareaRef.current.style.height = "auto";
                      }
                    }
                  }}
                  placeholder={
                    !session
                      ? "等待电脑确认会话…"
                      : !session.online
                        ? "电脑离线，无法发送消息"
                        : !connected
                          ? "会话正在自动恢复接入中…"
                          : canSend
                            ? "继续这段工作… (Enter 发送, Shift+Enter 换行)"
                            : "Agent 正在执行中…"
                  }
                  rows={1}
                  disabled={!connected}
                />
              </div>
              <div className="remote-composer-toolbar">
                <div className="remote-composer-status">
                  <span
                    className={`remote-status-pill ${
                      session?.online
                        ? connected
                          ? session?.binding.execution_state === "running"
                            ? "running"
                            : "ready"
                          : "connecting"
                        : "offline"
                    }`}
                  >
                    <span className="remote-pill-dot" />
                    {session
                      ? !session.online
                        ? "电脑离线"
                        : !connected
                          ? "正在恢复…"
                          : (states[session.binding.execution_state] ?? "就绪")
                      : "正在连接"}
                  </span>
                  <span className="remote-composer-hint">
                    {canSend
                      ? "Enter 发送 · Shift+Enter 换行"
                      : "Hub 双向安全同步"}
                  </span>
                </div>
                <div className="remote-composer-actions">
                  {session?.binding.current_turn_id && (
                    <button
                      type="button"
                      className="remote-interrupt-btn"
                      title="停止任务"
                      disabled={!connected || busy}
                      onClick={() => void submit({ action: "interrupt" })}
                    >
                      <svg
                        width="12"
                        height="12"
                        viewBox="0 0 24 24"
                        fill="currentColor"
                      >
                        <rect x="4" y="4" width="16" height="16" rx="2" />
                      </svg>
                      <span>停止</span>
                    </button>
                  )}
                  <button
                    type="submit"
                    className="remote-send-btn"
                    title={canSend && text.trim() ? "发送" : "等待输入就绪"}
                    disabled={!canSend || busy || !text.trim()}
                  >
                    <svg
                      width="16"
                      height="16"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2.5"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <line x1="12" y1="19" x2="12" y2="5" />
                      <polyline points="5 12 12 5 19 12" />
                    </svg>
                  </button>
                </div>
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
                  {cleanPath(path)}
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
                .map((item) => {
                  const provider = getProviderInfo(item.provider_session_id);
                  return (
                    <option
                      key={item.provider_session_id}
                      value={item.provider_session_id}
                    >
                      [{provider.name}] 接管：
                      {item.title ?? item.provider_session_id}
                    </option>
                  );
                })}
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
