"use client";

import type { ChangeEvent, ReactNode } from "react";

import {
  AGENT_LABELS,
  MOBILE_NAV_ITEMS,
  NAV_ITEMS,
  SECTION_LABELS,
  type ProviderFilter,
  type RangeFilter,
  type SectionKey,
} from "../lib/platform-view";
import type { DeviceRecord } from "../lib/types";
import type { AccountView } from "../lib/account";
import { Icon } from "./Icon";

interface PlatformShellProps {
  section: SectionKey;
  onNavigate: (section: SectionKey) => void;
  onOpenSettings: () => void;
  devices: DeviceRecord[];
  deviceFilter?: string | undefined;
  onDeviceFilterChange: (deviceId: string | undefined) => void;
  providerFilter: ProviderFilter;
  onProviderFilterChange: (provider: ProviderFilter) => void;
  rangeFilter: RangeFilter;
  onRangeFilterChange: (range: RangeFilter) => void;
  search: string;
  onSearchChange: (value: string) => void;
  connectionLabel: string;
  connectionTone: string;
  inspector?: ReactNode | undefined;
  account?: AccountView | undefined;
  onLogout: () => void;
  children: ReactNode;
}

function SelectControl({
  label,
  value,
  onChange,
  children,
}: {
  label: string;
  value: string;
  onChange: (event: ChangeEvent<HTMLSelectElement>) => void;
  children: ReactNode;
}) {
  return (
    <label className="context-select">
      <span className="sr-only">{label}</span>
      <select aria-label={label} onChange={onChange} value={value}>
        {children}
      </select>
      <Icon name="chevron-down" size={14} />
    </label>
  );
}

function BrandMark({ size = 26 }: { size?: number }) {
  return (
    <span
      className="brand-mark"
      aria-hidden="true"
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: size,
        height: size,
      }}
    >
      <svg
        viewBox="0 0 512 512"
        width={size}
        height={size}
        style={{ filter: "drop-shadow(0 0 6px rgba(0, 229, 255, 0.6))" }}
      >
        <g
          stroke="#00e5ff"
          strokeWidth="20"
          strokeLinecap="round"
          strokeLinejoin="round"
          fill="none"
        >
          <line x1="256" y1="256" x2="256" y2="136" />
          <line x1="256" y1="256" x2="360" y2="196" />
          <line x1="256" y1="256" x2="360" y2="316" />
          <line x1="256" y1="256" x2="256" y2="376" />
          <line x1="256" y1="256" x2="152" y2="316" />
          <line x1="256" y1="256" x2="152" y2="196" />
          <polygon points="256,136 360,196 360,316 256,376 152,316 152,196" />
        </g>
        <g stroke="#00e5ff" strokeWidth="14" fill="#0b0f19">
          <circle cx="256" cy="136" r="22" />
          <circle cx="360" cy="196" r="22" />
          <circle cx="360" cy="316" r="22" />
          <circle cx="256" cy="376" r="22" />
          <circle cx="152" cy="316" r="22" />
          <circle cx="152" cy="196" r="22" />
        </g>
        <circle cx="256" cy="256" r="42" fill="#00e5ff" opacity="0.4" />
        <circle cx="256" cy="256" r="26" fill="#ffffff" />
      </svg>
    </span>
  );
}

function NavigationRail({
  section,
  onNavigate,
  onOpenSettings,
  connectionLabel,
  connectionTone,
  account,
  onLogout,
}: {
  section: SectionKey;
  onNavigate: (section: SectionKey) => void;
  onOpenSettings: () => void;
  connectionLabel: string;
  connectionTone: string;
  account?: AccountView | undefined;
  onLogout: () => void;
}) {
  return (
    <aside className="nav-rail">
      <div className="rail-brand">
        <BrandMark size={28} />
        <div
          style={{ display: "flex", flexDirection: "column", lineHeight: 1.15 }}
        >
          <span
            style={{
              fontWeight: 800,
              fontSize: "15px",
              letterSpacing: "0.02em",
              color: "#f8fafc",
            }}
          >
            Agent{" "}
            <span
              style={{
                background: "linear-gradient(90deg, #00f5d4, #00bbf9, #a855f7)",
                WebkitBackgroundClip: "text",
                WebkitTextFillColor: "transparent",
              }}
            >
              Hub
            </span>
          </span>
          <span
            style={{
              fontSize: "10px",
              color: "#64748b",
              letterSpacing: "0.06em",
            }}
          >
            智能体集成中心
          </span>
        </div>
      </div>
      <div className="rail-group">
        <p className="rail-label">Agent 平台</p>
        <nav aria-label="全局导航">
          {NAV_ITEMS.map((item) => (
            <button
              aria-current={section === item.key ? "page" : undefined}
              className="rail-item"
              data-selected={section === item.key}
              key={item.key}
              onClick={() => onNavigate(item.key)}
              type="button"
            >
              <Icon name={item.icon} size={18} />
              <span>{item.label}</span>
            </button>
          ))}
          <button
            className="rail-item"
            onClick={() => {
              window.location.href = "/?remote=1";
            }}
            type="button"
            title="打开远程接管控制台"
          >
            <Icon name="activity" size={18} />
            <span>远程接管</span>
          </button>
        </nav>
      </div>
      <div className="rail-bottom">
        {account !== undefined && (
          <div className="rail-account">
            <span className="rail-account-avatar">
              {account.user.display_name.slice(0, 1).toUpperCase()}
            </span>
            <span>
              <strong>{account.user.display_name}</strong>
              <small>
                {account.workspace?.name ?? "个人工作区"} ·{" "}
                {(
                  account.workspace_subscription?.plan ??
                  account.subscription.plan
                ).toUpperCase()}
              </small>
            </span>
            <button
              aria-label="退出登录"
              onClick={onLogout}
              title="退出登录"
              type="button"
            >
              <Icon name="chevron-right" size={15} />
            </button>
          </div>
        )}
        <button className="rail-item" onClick={onOpenSettings} type="button">
          <Icon name="settings" size={18} />
          <span>设置</span>
        </button>
        <button
          aria-current={section === "help" ? "page" : undefined}
          className="rail-item"
          data-selected={section === "help"}
          onClick={() => onNavigate("help")}
          type="button"
        >
          <Icon name="help" size={18} />
          <span>帮助</span>
        </button>
        <div className="rail-connection">
          <span className="status-dot" data-tone={connectionTone} />
          <span>
            <strong>{connectionLabel}</strong>
            <small>{connectionTone === "live" ? "实时读取" : "等待重试"}</small>
          </span>
          <Icon name="chevron-right" size={15} />
        </div>
      </div>
    </aside>
  );
}

function ContextBar({
  section,
  devices,
  deviceFilter,
  onDeviceFilterChange,
  providerFilter,
  onProviderFilterChange,
  rangeFilter,
  onRangeFilterChange,
  search,
  onSearchChange,
  connectionLabel,
  connectionTone,
}: Omit<
  PlatformShellProps,
  | "onNavigate"
  | "onOpenSettings"
  | "children"
  | "inspector"
  | "account"
  | "onLogout"
>) {
  const deviceChange = (event: ChangeEvent<HTMLSelectElement>) => {
    onDeviceFilterChange(
      event.target.value === "all" ? undefined : event.target.value,
    );
  };
  const providerChange = (event: ChangeEvent<HTMLSelectElement>) => {
    onProviderFilterChange(event.target.value as ProviderFilter);
  };
  const rangeChange = (event: ChangeEvent<HTMLSelectElement>) => {
    onRangeFilterChange(event.target.value as RangeFilter);
  };
  return (
    <header className="context-bar">
      <div className="mobile-brand-row">
        <span className="mobile-brand">
          <BrandMark />
          <strong>AgentPulse</strong>
        </span>
        <span className="mobile-brand-divider" />
        <strong>{SECTION_LABELS[section]}</strong>
        <span className="mobile-brand-spacer" />
        <span className="mobile-brand-connection" data-tone={connectionTone}>
          <span className="status-dot" />
          {connectionLabel}
        </span>
        <Icon name="search" size={18} />
      </div>
      <div className="context-location">
        <strong>{SECTION_LABELS[section]}</strong>
        <span>/</span>
        <span>
          {section === "sessions"
            ? "当前活动"
            : section === "activity"
              ? "实时动态"
              : section === "diagnostics"
                ? "系统"
                : "设备总览"}
        </span>
      </div>
      <div className="context-controls">
        <SelectControl
          label="设备筛选"
          onChange={deviceChange}
          value={deviceFilter ?? "all"}
        >
          <option value="all">全部设备</option>
          {devices
            .filter((device) => !device.revoked)
            .map((device) => (
              <option key={device.device_id} value={device.device_id}>
                {device.display_name ?? device.device_id.slice(0, 14)}
              </option>
            ))}
        </SelectControl>
        <div
          className="provider-segments"
          aria-label="Agent 来源"
          role="group"
        >
          {(["all", "claude_code", "codex"] as const).map((provider) => (
            <button
              aria-pressed={providerFilter === provider}
              key={provider}
              onClick={() => onProviderFilterChange(provider)}
              type="button"
            >
              {provider === "all" ? "全部" : AGENT_LABELS[provider]}
            </button>
          ))}
        </div>
        <SelectControl
          label="时间范围"
          onChange={rangeChange}
          value={rangeFilter}
        >
          <option value="24h">过去 24 小时</option>
          <option value="7d">过去 7 天</option>
          <option value="all">全部时间</option>
        </SelectControl>
        <label className="search-control">
          <Icon name="search" size={16} />
          <input
            aria-label="搜索会话"
            onChange={(event) => onSearchChange(event.target.value)}
            placeholder="搜索会话或按 ⌘K"
            value={search}
          />
        </label>
      </div>
      <div
        className="context-connection"
        data-tone={connectionTone}
        role="status"
      >
        <span className="status-dot" />
        <span>{connectionLabel}</span>
        <span className="connection-bars">
          <i />
          <i />
          <i />
        </span>
      </div>
    </header>
  );
}

export function PlatformShell(props: PlatformShellProps) {
  const hasInspector = props.inspector !== undefined;
  return (
    <div
      className={`app-shell ${hasInspector ? "has-inspector" : ""}`.trim()}
      data-section={props.section}
    >
      <NavigationRail
        account={props.account}
        connectionLabel={props.connectionLabel}
        connectionTone={props.connectionTone}
        onNavigate={props.onNavigate}
        onOpenSettings={props.onOpenSettings}
        onLogout={props.onLogout}
        section={props.section}
      />
      <div className="app-stage">
        <ContextBar
          connectionLabel={props.connectionLabel}
          connectionTone={props.connectionTone}
          deviceFilter={props.deviceFilter}
          devices={props.devices}
          onDeviceFilterChange={props.onDeviceFilterChange}
          onProviderFilterChange={props.onProviderFilterChange}
          onRangeFilterChange={props.onRangeFilterChange}
          onSearchChange={props.onSearchChange}
          providerFilter={props.providerFilter}
          rangeFilter={props.rangeFilter}
          search={props.search}
          section={props.section}
        />
        <main className="page-body">{props.children}</main>
      </div>
      {props.inspector}
      <nav className="mobile-nav" aria-label="移动端导航">
        {MOBILE_NAV_ITEMS.map((item) => (
          <button
            aria-current={props.section === item.key ? "page" : undefined}
            className="mobile-nav-item"
            data-selected={props.section === item.key}
            key={item.key}
            onClick={() => props.onNavigate(item.key)}
            type="button"
          >
            <Icon name={item.icon} size={20} />
            <span>{item.label}</span>
          </button>
        ))}
      </nav>
    </div>
  );
}
