import { useState, type ReactNode } from "react";

import {
  AGENT_LABELS,
  SESSION_FILTERS,
  type SectionKey,
  type SessionFilter,
  deviceLabel,
  devicePlatform,
  deviceSessionCount,
  deviceVersion,
  authStatus,
  credentialSourceLabel,
  eventDescription,
  eventLabel,
  formatClockCompact,
  formatDateTimeCompact,
  formatDuration,
  formatRelativeCompact,
  quotaLabel,
  sessionStatus,
  sessionTask,
  sessionTool,
  sortPlatformSessions,
  matchesSessionFilter,
} from "../lib/platform-view";
import type { AuthRecord, DeviceRecord, SessionRecord } from "../lib/types";
import { sessionKey, sessionRef } from "../lib/types";
import { Icon, type IconName } from "./Icon";

interface ViewHeaderProps {
  title: string;
  description: string;
  action?: ReactNode;
}

function ViewHeader({ title, description, action }: ViewHeaderProps) {
  return (
    <header className="view-header">
      <div>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {action}
    </header>
  );
}

interface MetricCardProps {
  label: string;
  value: string | number;
  detail: string;
  tone: "blue" | "green" | "gold" | "red";
  icon: IconName;
}

function MetricCard({ label, value, detail, tone, icon }: MetricCardProps) {
  return (
    <article className="metric-card" data-tone={tone}>
      <div className="metric-card-head">
        <span>{label}</span>
        <Icon name={icon} size={20} />
      </div>
      <div className="metric-card-value">{value}</div>
      <div className="metric-card-detail">{detail}</div>
    </article>
  );
}

function SectionPanel({
  title,
  action,
  children,
  className = "",
}: {
  title: string;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`section-panel ${className}`.trim()}>
      <header className="section-panel-header">
        <h2>{title}</h2>
        {action}
      </header>
      {children}
    </section>
  );
}

function StatusMark({ session }: { session: SessionRecord }) {
  const status = sessionStatus(session);
  return (
    <span className="status-mark" data-tone={status.tone}>
      <span className="status-dot" />
      <span>{status.label}</span>
    </span>
  );
}

interface SessionTableProps {
  sessions: SessionRecord[];
  devices: DeviceRecord[];
  nowMs: number;
  selectedKey?: string | undefined;
  onSelect: (session: SessionRecord) => void;
  limit?: number;
  footer?: boolean;
}

export function SessionTable({
  sessions,
  devices,
  nowMs,
  selectedKey,
  onSelect,
  limit,
  footer = true,
}: SessionTableProps) {
  const deviceMap = new Map(
    devices.map((device) => [device.device_id, device]),
  );
  const rows = limit === undefined ? sessions : sessions.slice(0, limit);

  return (
    <div className="session-table-wrap">
      <div className="session-table" role="table" aria-label="Agent 会话列表">
        <div className="session-table-head" role="row">
          <span role="columnheader">状态</span>
          <span role="columnheader">会话</span>
          <span role="columnheader">来源</span>
          <span role="columnheader">设备</span>
          <span role="columnheader">当前任务</span>
          <span role="columnheader">最近活动</span>
          <span role="columnheader">时长</span>
        </div>
        {rows.length === 0 ? (
          <div className="table-empty" role="row">
            <Icon name="sessions" size={20} />
            <strong>此视图暂无会话</strong>
            <span>请尝试更换状态、设备、来源或时间范围。</span>
          </div>
        ) : (
          rows.map((session) => {
            const key = sessionKey(sessionRef(session));
            const device = deviceMap.get(session.device_id);
            return (
              <button
                className="session-row"
                data-selected={key === selectedKey}
                data-session-key={key}
                key={key}
                onClick={() => onSelect(session)}
                role="row"
                type="button"
              >
                <span className="session-status-cell" role="cell">
                  <StatusMark session={session} />
                </span>
                <span className="session-identity" role="cell">
                  <strong>{session.payload.project.display_name}</strong>
                  <span className="mono">{session.agent_session_id}</span>
                </span>
                <span className="session-provider" role="cell">
                  <span
                    className={`provider-mark provider-${session.agent_type}`}
                  >
                    {session.agent_type === "claude_code"
                      ? "C"
                      : session.agent_type === "zcode"
                        ? "Z"
                        : "X"}
                  </span>
                  {AGENT_LABELS[session.agent_type]}
                </span>
                <span className="session-device-cell" role="cell">
                  <strong>{deviceLabel(device, session.device_id)}</strong>
                  <span className="mono">{device?.os ?? "runner"}</span>
                </span>
                <span
                  className="session-task"
                  role="cell"
                  title={sessionTask(session)}
                >
                  {sessionTask(session)}
                </span>
                <span className="session-last-active" role="cell">
                  {formatRelativeCompact(session.observed_at, nowMs)}
                </span>
                <span className="session-duration mono" role="cell">
                  {formatDuration(session.payload.started_at, nowMs)}
                </span>
              </button>
            );
          })
        )}
      </div>
      {footer && rows.length > 0 && (
        <footer className="table-footer">
          <span>
            显示 <strong>{rows.length}</strong> 条，共{" "}
            <strong>{sessions.length}</strong> 条会话
          </span>
          <span className="table-footer-hint">选择一行查看详情</span>
        </footer>
      )}
    </div>
  );
}

export function DeviceHealthPanel({
  devices,
  sessions,
  nowMs,
  onViewAll,
}: {
  devices: DeviceRecord[];
  sessions: SessionRecord[];
  nowMs: number;
  onViewAll: () => void;
}) {
  const activeDevices = devices.filter((device) => !device.revoked).slice(0, 7);
  return (
    <SectionPanel
      title="设备健康"
      action={
        <button className="text-action" onClick={onViewAll} type="button">
          查看全部 <Icon name="arrow-right" size={14} />
        </button>
      }
    >
      <div className="device-health-list">
        {activeDevices.length === 0 ? (
          <div className="panel-empty">暂无已连接设备。</div>
        ) : (
          activeDevices.map((device) => (
            <div className="device-health-row" key={device.device_id}>
              <span className="device-health-name">
                <span
                  className="status-dot"
                  data-tone={device.online ? "live" : "error"}
                />
                <span>
                  <strong>{deviceLabel(device, device.device_id)}</strong>
                  <small>
                    {devicePlatform(device)} · {deviceVersion(device)}
                  </small>
                </span>
              </span>
              <span className="device-health-state">
                <strong className={device.online ? "tone-live" : "tone-error"}>
                  {device.online ? "已连接" : "离线"}
                </strong>
                <small>
                  {formatRelativeCompact(device.last_seen_at, nowMs)}
                </small>
              </span>
            </div>
          ))
        )}
      </div>
    </SectionPanel>
  );
}

export function ActivityPanel({
  sessions,
  devices,
  nowMs,
  onViewAll,
}: {
  sessions: SessionRecord[];
  devices: DeviceRecord[];
  nowMs: number;
  onViewAll: () => void;
}) {
  const deviceMap = new Map(
    devices.map((device) => [device.device_id, device]),
  );
  const recent = sortPlatformSessions(sessions).slice(0, 6);
  return (
    <SectionPanel
      title="最近动态"
      action={
        <button className="text-action" onClick={onViewAll} type="button">
          查看全部 <Icon name="arrow-right" size={14} />
        </button>
      }
    >
      <div className="activity-list">
        {recent.length === 0 ? (
          <div className="panel-empty">暂无最近动态。</div>
        ) : (
          recent.map((session) => (
            <div
              className="activity-row"
              key={`${session.event_id}-${session.sequence}`}
            >
              <span className="activity-time mono">
                {formatRelativeCompact(session.observed_at, nowMs)}
              </span>
              <span
                className="activity-marker"
                data-tone={sessionStatus(session).tone}
              >
                <span className="status-dot" />
              </span>
              <span className="activity-copy">
                <strong>{eventLabel(session)}</strong>
                <span>{eventDescription(session)}</span>
                <small>
                  {session.payload.project.display_name} ·{" "}
                  {deviceLabel(
                    deviceMap.get(session.device_id),
                    session.device_id,
                  )}
                </small>
              </span>
            </div>
          ))
        )}
      </div>
      {recent.length > 0 && (
        <button className="panel-link" onClick={onViewAll} type="button">
          查看全部动态 <Icon name="arrow-right" size={14} />
        </button>
      )}
    </SectionPanel>
  );
}

export function OverviewView({
  sessions,
  devices,
  nowMs,
  selectedKey,
  onSelect,
  onNavigate,
  sessionFilter,
  onSessionFilterChange,
}: {
  sessions: SessionRecord[];
  devices: DeviceRecord[];
  nowMs: number;
  selectedKey?: string | undefined;
  onSelect: (session: SessionRecord) => void;
  onNavigate: (section: SectionKey) => void;
  sessionFilter: SessionFilter;
  onSessionFilterChange: (filter: SessionFilter) => void;
}) {
  const active = sessions.filter(
    (session) => session.payload.state.activity_status === "WORKING",
  ).length;
  const waiting = sessions.filter((session) =>
    matchesSessionFilter(session, "waiting"),
  ).length;
  const errors = sessions.filter((session) =>
    matchesSessionFilter(session, "failed"),
  ).length;
  const online = devices.filter(
    (device) => device.online && !device.revoked,
  ).length;
  const sorted = sortPlatformSessions(
    sessions.filter((session) => matchesSessionFilter(session, sessionFilter)),
  );

  return (
    <>
      <ViewHeader
        title="总览"
        description="查看所有设备上的 Agent 会话与活动状态。"
      />
      <section className="metric-grid" aria-label="设备总览">
        <MetricCard
          label="活动会话"
          value={active}
          detail="当前运行"
          tone="blue"
          icon="sessions"
        />
        <MetricCard
          label="在线设备"
          value={`${online} / ${devices.filter((device) => !device.revoked).length}`}
          detail="已连接"
          tone="green"
          icon="devices"
        />
        <MetricCard
          label="等待中"
          value={waiting}
          detail="需要关注"
          tone="gold"
          icon="clock"
        />
        <MetricCard
          label="错误"
          value={errors}
          detail="需要处理"
          tone="red"
          icon="warning"
        />
      </section>
      <div className="overview-layout">
        <SectionPanel
          title="活动会话"
          className="active-sessions-panel"
          action={
            <button
              className="text-action"
              onClick={() => onNavigate("sessions")}
              type="button"
            >
              查看全部会话 <Icon name="arrow-right" size={14} />
            </button>
          }
        >
          <div className="panel-tabs" aria-label="总览会话视图" role="tablist">
            {SESSION_FILTERS.map((entry) => (
              <button
                aria-selected={entry.key === sessionFilter}
                data-selected={entry.key === sessionFilter}
                key={entry.key}
                onClick={() => onSessionFilterChange(entry.key)}
                role="tab"
                type="button"
              >
                {entry.label}
              </button>
            ))}
          </div>
          <SessionTable
            devices={devices}
            footer
            limit={8}
            nowMs={nowMs}
            onSelect={onSelect}
            selectedKey={selectedKey}
            sessions={sorted}
          />
        </SectionPanel>
        <div className="overview-side-stack">
          <DeviceHealthPanel
            devices={devices}
            onViewAll={() => onNavigate("devices")}
            sessions={sessions}
            nowMs={nowMs}
          />
          <ActivityPanel
            devices={devices}
            onViewAll={() => onNavigate("activity")}
            sessions={sessions}
            nowMs={nowMs}
          />
        </div>
      </div>
    </>
  );
}

export function SessionsView({
  sessions,
  devices,
  nowMs,
  filter,
  onFilterChange,
  selectedKey,
  onSelect,
}: {
  sessions: SessionRecord[];
  devices: DeviceRecord[];
  nowMs: number;
  filter: SessionFilter;
  onFilterChange: (filter: SessionFilter) => void;
  selectedKey?: string | undefined;
  onSelect: (session: SessionRecord) => void;
}) {
  const visible = sortPlatformSessions(
    sessions.filter((session) => matchesSessionFilter(session, filter)),
  );
  return (
    <>
      <ViewHeader
        title="会话"
        description="监控并远程控制所有已连接设备上的 Agent 会话。"
      />
      <section className="session-view-panel section-panel">
        <div className="session-view-toolbar">
          <div
            className="session-filter-tabs"
            role="tablist"
            aria-label="会话状态"
          >
            {SESSION_FILTERS.map((entry) => {
              const count = sessions.filter((session) =>
                matchesSessionFilter(session, entry.key),
              ).length;
              return (
                <button
                  aria-selected={entry.key === filter}
                  data-selected={entry.key === filter}
                  key={entry.key}
                  onClick={() => onFilterChange(entry.key)}
                  role="tab"
                  type="button"
                >
                  {entry.label}
                  <span>{count}</span>
                </button>
              );
            })}
          </div>
          <span className="result-count">{visible.length} 条会话</span>
        </div>
        <SessionTable
          devices={devices}
          nowMs={nowMs}
          onSelect={onSelect}
          selectedKey={selectedKey}
          sessions={visible}
        />
      </section>
    </>
  );
}

function AuthHealthPanel({
  snapshots,
  nowMs,
}: {
  snapshots: AuthRecord[];
  nowMs: number;
}) {
  const byProvider = new Map(
    snapshots.map((snapshot) => [snapshot.provider, snapshot]),
  );
  return (
    <section className="auth-health section-panel" aria-label="Agent 认证状态">
      <header className="section-panel-header">
        <div>
          <h2>Agent 认证与配额</h2>
          <p className="section-panel-subtitle">
            Runner 只同步脱敏状态；原始 token 不会进入网关或浏览器。
          </p>
        </div>
        <span className="result-count">
          {snapshots.length === 0 ? "等待 Runner 上报" : "已同步"}
        </span>
      </header>
      <div className="auth-health-grid">
        {(["claude_code", "codex"] as const).map((provider) => {
          const snapshot = byProvider.get(provider);
          const status =
            snapshot === undefined ? undefined : authStatus(snapshot);
          return (
            <article className="auth-health-card" key={provider}>
              <div className="auth-health-card-head">
                <span className={`provider-mark provider-${provider}`}>
                  {provider === "claude_code" ? "C" : "X"}
                </span>
                <strong>{AGENT_LABELS[provider]}</strong>
                <span
                  className="status-mark"
                  data-tone={status?.tone ?? "unknown"}
                >
                  <span className="status-dot" />
                  {status?.label ?? "未上报"}
                </span>
              </div>
              {snapshot === undefined ? (
                <p className="auth-health-empty">
                  请在启用凭据访问的 Runner 上运行认证状态同步。
                </p>
              ) : (
                <dl className="auth-health-fields">
                  <div>
                    <dt>来源</dt>
                    <dd>{credentialSourceLabel(snapshot.source)}</dd>
                  </div>
                  <div>
                    <dt>账户</dt>
                    <dd>{snapshot.account_hint ?? "未提供"}</dd>
                  </div>
                  <div>
                    <dt>配额</dt>
                    <dd>{quotaLabel(snapshot)}</dd>
                  </div>
                  <div>
                    <dt>检查时间</dt>
                    <dd>{formatRelativeCompact(snapshot.checked_at, nowMs)}</dd>
                  </div>
                </dl>
              )}
            </article>
          );
        })}
      </div>
    </section>
  );
}

export function DevicesView({
  devices,
  sessions,
  nowMs,
  onSelect,
  onAddDevice,
  authSnapshots,
}: {
  devices: DeviceRecord[];
  sessions: SessionRecord[];
  nowMs: number;
  onSelect: (session: SessionRecord) => void;
  onAddDevice: () => void;
  authSnapshots: AuthRecord[];
}) {
  const connected = devices.filter(
    (device) => device.online && !device.revoked,
  ).length;
  const degraded = devices.filter(
    (device) => !device.online && !device.revoked,
  ).length;
  return (
    <>
      <ViewHeader
        title="设备"
        description="查看 Runner 健康状态、最近心跳，以及每台设备上的会话。"
        action={
          <button className="primary" onClick={onAddDevice} type="button">
            <Icon name="devices" size={15} />
            添加设备
          </button>
        }
      />
      <section className="device-overview-strip" aria-label="设备概览">
        <div>
          <span>在线</span>
          <strong>{connected}</strong>
        </div>
        <div>
          <span>异常</span>
          <strong>{degraded}</strong>
        </div>
        <div>
          <span>活动会话</span>
          <strong>
            {
              sessions.filter(
                (session) => session.payload.state.presence === "LIVE",
              ).length
            }
          </strong>
        </div>
        <div>
          <span>Runner 版本</span>
          <strong>
            {
              new Set(devices.map((device) => device.runner_version ?? "未知"))
                .size
            }
          </strong>
        </div>
      </section>
      <AuthHealthPanel nowMs={nowMs} snapshots={authSnapshots} />
      <section className="device-grid" aria-label="已连接设备">
        {devices.length === 0 ? (
          <div className="empty-surface">
            <Icon name="devices" size={22} />
            <strong>暂无已连接设备</strong>
            <span>打开接入向导，几步就能让 Runner 开始上报设备健康状态。</span>
            <button className="primary" onClick={onAddDevice} type="button">
              添加第一台设备 <Icon name="arrow-right" size={15} />
            </button>
          </div>
        ) : (
          devices.map((device) => {
            const count = deviceSessionCount(sessions, device.device_id);
            const latest = sortPlatformSessions(
              sessions.filter(
                (session) => session.device_id === device.device_id,
              ),
            )[0];
            return (
              <article
                className="device-card"
                data-online={device.online}
                key={device.device_id}
              >
                <header>
                  <div className="device-card-title">
                    <Icon name="devices" size={18} />
                    <strong>{deviceLabel(device, device.device_id)}</strong>
                  </div>
                  <span
                    className="status-mark"
                    data-tone={device.online ? "live" : "error"}
                  >
                    <span className="status-dot" />
                    {device.online ? "已连接" : "离线"}
                  </span>
                </header>
                <dl>
                  <div>
                    <dt>操作系统</dt>
                    <dd>{devicePlatform(device)}</dd>
                  </div>
                  <div>
                    <dt>Runner</dt>
                    <dd className="mono">{deviceVersion(device)}</dd>
                  </div>
                  <div>
                    <dt>最近心跳</dt>
                    <dd>{formatRelativeCompact(device.last_seen_at, nowMs)}</dd>
                  </div>
                  <div>
                    <dt>活动会话</dt>
                    <dd>{count}</dd>
                  </div>
                </dl>
                <footer>
                  <span>
                    {latest === undefined
                      ? "暂无最近会话"
                      : `最近：${latest.payload.project.display_name}`}
                  </span>
                  <button
                    className="icon-link"
                    type="button"
                    onClick={() => latest !== undefined && onSelect(latest)}
                    aria-label={`查看 ${deviceLabel(device, device.device_id)}`}
                  >
                    <Icon name="chevron-right" size={16} />
                  </button>
                </footer>
              </article>
            );
          })
        )}
      </section>
    </>
  );
}

function ActivityTimeline({
  sessions,
  devices,
  nowMs,
  expanded = false,
}: {
  sessions: SessionRecord[];
  devices: DeviceRecord[];
  nowMs: number;
  expanded?: boolean;
}) {
  const deviceMap = new Map(
    devices.map((device) => [device.device_id, device]),
  );
  const items = sortPlatformSessions(sessions);
  return (
    <div
      className={`activity-timeline ${expanded ? "activity-timeline-expanded" : ""}`.trim()}
    >
      {items.length === 0 ? (
        <div className="empty-surface">
          <Icon name="activity" size={22} />
          <strong>暂无动态</strong>
          <span>Runner 上报状态变化后会显示在这里。</span>
        </div>
      ) : (
        items.map((session) => (
          <article
            className="timeline-event"
            data-tone={sessionStatus(session).tone}
            key={`${session.event_id}-${session.sequence}`}
          >
            <div className="timeline-event-time mono">
              {formatClockCompact(session.observed_at)}
              <small>{formatRelativeCompact(session.observed_at, nowMs)}</small>
            </div>
            <div className="timeline-event-line">
              <span className="status-dot" />
            </div>
            <div className="timeline-event-content">
              <div className="timeline-event-title">
                <strong>{eventLabel(session)}</strong>
                <span>{AGENT_LABELS[session.agent_type]}</span>
              </div>
              <p>{eventDescription(session)}</p>
              <small>
                {session.payload.project.display_name} ·{" "}
                {deviceLabel(
                  deviceMap.get(session.device_id),
                  session.device_id,
                )}{" "}
                · <span className="mono">{session.agent_session_id}</span>
              </small>
            </div>
          </article>
        ))
      )}
    </div>
  );
}

export function ActivityView({
  sessions,
  devices,
  nowMs,
}: {
  sessions: SessionRecord[];
  devices: DeviceRecord[];
  nowMs: number;
}) {
  const [filter, setFilter] = useState<SessionFilter>("all");
  const visible = sessions.filter((session) =>
    matchesSessionFilter(session, filter),
  );
  return (
    <>
      <ViewHeader
        title="动态"
        description="按时间查看会话状态、工具调用与 Runner 连接变化。"
      />
      <section className="activity-page section-panel">
        <header className="activity-page-toolbar">
          <div className="panel-tabs" role="tablist" aria-label="活动筛选">
            {SESSION_FILTERS.map((entry) => (
              <button
                aria-selected={entry.key === filter}
                data-selected={entry.key === filter}
                key={entry.key}
                onClick={() => setFilter(entry.key)}
                role="tab"
                type="button"
              >
                {entry.label}
              </button>
            ))}
          </div>
          <span className="result-count">{visible.length} 条动态</span>
        </header>
        <ActivityTimeline
          devices={devices}
          expanded
          nowMs={nowMs}
          sessions={visible}
        />
      </section>
    </>
  );
}

export function DiagnosticsView({
  devices,
  authSnapshots,
  connectionLabel,
  connectionTone,
  streamConnected,
  syncError,
}: {
  devices: DeviceRecord[];
  authSnapshots: AuthRecord[];
  connectionLabel: string;
  connectionTone: string;
  streamConnected: boolean;
  syncError?: string | undefined;
}) {
  const checks = [
    {
      label: "网关",
      value: connectionLabel,
      detail: syncError ?? "读取 API 连接状态",
      tone: connectionTone === "live" ? "live" : "error",
      icon: "server" as const,
    },
    {
      label: "Runner 上报",
      value: devices.length > 0 ? "已连接" : "未上报",
      detail: `${devices.length} 条设备记录可用`,
      tone: devices.length > 0 ? "live" : "unknown",
      icon: "devices" as const,
    },
    {
      label: "变更流",
      value: streamConnected ? "已连接" : "轮询",
      detail: streamConnected ? "已启用实时更新" : "使用定时刷新作为备用",
      tone: streamConnected ? "live" : "waiting",
      icon: "activity" as const,
    },
    {
      label: "待发送队列",
      value: "未提供",
      detail: "读取接口未暴露 Runner 队列指标",
      tone: "unknown",
      icon: "sessions" as const,
    },
    {
      label: "签名校验",
      value: "已启用",
      detail: "网关接受带签名的同步批次",
      tone: "live",
      icon: "check" as const,
    },
    {
      label: "API 数据读取",
      value: syncError === undefined ? "读取正常" : "读取失败",
      detail:
        syncError ??
        `已读取 ${devices.length} 台设备和 ${authSnapshots.length} 条认证快照`,
      tone: syncError === undefined ? "live" : "error",
      icon: "server" as const,
    },
    {
      label: "Agent 认证",
      value:
        authSnapshots.length === 0
          ? "未上报"
          : authSnapshots.some(
                (snapshot) => snapshot.state === "reauth_required",
              )
            ? "需重新认证"
            : "状态已同步",
      detail: `${authSnapshots.length} 条脱敏快照；原始 token 未离开 Runner`,
      tone: authSnapshots.some(
        (snapshot) => snapshot.state === "reauth_required",
      )
        ? "waiting"
        : authSnapshots.length === 0
          ? "unknown"
          : "live",
      icon: "check" as const,
    },
  ];
  return (
    <>
      <ViewHeader
        title="诊断"
        description="集中查看网关、Runner、同步链路和数据存储的技术健康信号。"
      />
      <section className="diagnostics-grid">
        {checks.map((check) => (
          <article
            className="diagnostic-row"
            data-tone={check.tone}
            key={check.label}
          >
            <span className="diagnostic-icon">
              <Icon name={check.icon} size={18} />
            </span>
            <span className="diagnostic-copy">
              <strong>{check.label}</strong>
              <small>{check.detail}</small>
            </span>
            <span className="diagnostic-value">
              <span className="status-dot" />
              {check.value}
            </span>
          </article>
        ))}
      </section>
      <section className="diagnostics-note section-panel">
        <Icon name="warning" size={18} />
        <div>
          <strong>诊断信息保持保守。</strong>
          <p>网关未上报的信号会显示为未知，不会被推断为健康。</p>
        </div>
      </section>
    </>
  );
}

export function HelpView({
  onNavigate,
}: {
  onNavigate: (section: SectionKey) => void;
}) {
  return (
    <>
      <ViewHeader
        title="帮助"
        description="用一份简短指南了解 AgentHub，同时保持当前位置。"
      />
      <section className="help-grid">
        <article className="help-card">
          <span className="help-number">01</span>
          <h2>从总览开始</h2>
          <p>
            先看四个核心指标：当前有多少会话在运行、哪些设备已连接，以及哪里需要关注。
          </p>
          <button
            className="text-action"
            type="button"
            onClick={() => onNavigate("overview")}
          >
            前往总览 <Icon name="arrow-right" size={14} />
          </button>
        </article>
        <article className="help-card">
          <span className="help-number">02</span>
          <h2>先查看，再展开</h2>
          <p>
            选择会话行打开右侧检查器；需要时间线和原始元数据时，再打开完整会话。
          </p>
          <button
            className="text-action"
            type="button"
            onClick={() => onNavigate("sessions")}
          >
            前往会话 <Icon name="arrow-right" size={14} />
          </button>
        </article>
        <article className="help-card">
          <span className="help-number">03</span>
          <h2>相信未知状态</h2>
          <p>
            未上报或推断出的信号会保持为未知。AgentHub
            不会把缺失的遥测数据伪装成绿色健康状态。
          </p>
          <button
            className="text-action"
            type="button"
            onClick={() => onNavigate("diagnostics")}
          >
            打开诊断 <Icon name="arrow-right" size={14} />
          </button>
        </article>
      </section>
    </>
  );
}

export function InspectorPreviewFields({
  session,
  devices,
  nowMs,
}: {
  session: SessionRecord;
  devices: DeviceRecord[];
  nowMs: number;
}) {
  const device = devices.find((entry) => entry.device_id === session.device_id);
  const status = sessionStatus(session);
  return (
    <dl className="inspector-fields">
      <div>
        <dt>当前任务</dt>
        <dd>{sessionTask(session)}</dd>
      </div>
      <div>
        <dt>状态</dt>
        <dd>
          <span className="status-mark" data-tone={status.tone}>
            <span className="status-dot" />
            {status.label}
          </span>
        </dd>
      </div>
      <div>
        <dt>来源</dt>
        <dd>{AGENT_LABELS[session.agent_type]}</dd>
      </div>
      <div>
        <dt>设备</dt>
        <dd>
          {deviceLabel(device, session.device_id)}
          <small>{devicePlatform(device)}</small>
        </dd>
      </div>
      <div>
        <dt>开始时间</dt>
        <dd>
          {session.payload.started_at === undefined
            ? "—"
            : formatDateTimeCompact(session.payload.started_at)}
        </dd>
      </div>
      <div>
        <dt>最近活动</dt>
        <dd>{formatRelativeCompact(session.observed_at, nowMs)}</dd>
      </div>
    </dl>
  );
}
