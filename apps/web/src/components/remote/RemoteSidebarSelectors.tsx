import type { RefObject } from "react";
import type { ProviderKind } from "../../lib/session-tree";
import { ChevronDownIcon } from "../remote-console-constants";

export interface DeviceOptionItem {
  runnerId: string;
  label: string;
  online: boolean;
  workspaces: string[];
}

export interface AgentOptionItem {
  type: ProviderKind;
  name: string;
  count: number;
}

export interface RemoteSidebarSelectorsProps {
  deviceMenuRef: RefObject<HTMLDivElement | null>;
  deviceMenuButtonRef: RefObject<HTMLButtonElement | null>;
  agentMenuRef: RefObject<HTMLDivElement | null>;
  agentMenuButtonRef: RefObject<HTMLButtonElement | null>;
  deviceList: DeviceOptionItem[];
  activeRunnerId: string;
  activeDevice?: DeviceOptionItem | undefined;
  deviceMenuOpen: boolean;
  setDeviceMenuOpen: React.Dispatch<React.SetStateAction<boolean>>;
  agentMenuOpen: boolean;
  setAgentMenuOpen: React.Dispatch<React.SetStateAction<boolean>>;
  isActiveRunnerHelloPending: boolean;
  availableAgentsForCurrentDevice: AgentOptionItem[];
  currentAgentInfo: AgentOptionItem;
  providerFilter: ProviderKind;
  onSelectDevice: (runnerId: string) => void;
  onSelectAgent: (agentType: ProviderKind) => void;
}

export function RemoteSidebarSelectors({
  deviceMenuRef,
  deviceMenuButtonRef,
  agentMenuRef,
  agentMenuButtonRef,
  deviceList,
  activeRunnerId,
  activeDevice,
  deviceMenuOpen,
  setDeviceMenuOpen,
  agentMenuOpen,
  setAgentMenuOpen,
  isActiveRunnerHelloPending,
  availableAgentsForCurrentDevice,
  currentAgentInfo,
  providerFilter,
  onSelectDevice,
  onSelectAgent,
}: RemoteSidebarSelectorsProps) {
  const isAgentSelectorDisabled =
    !activeRunnerId ||
    isActiveRunnerHelloPending ||
    availableAgentsForCurrentDevice.length === 0;

  return (
    <div className="remote-selectors-bar">
      <div className="remote-selector-dropdown" ref={deviceMenuRef}>
        <button
          type="button"
          className={`remote-selector-btn ${deviceMenuOpen ? "active" : ""}`}
          ref={deviceMenuButtonRef}
          onClick={() => {
            setDeviceMenuOpen((prev) => !prev);
            setAgentMenuOpen(false);
          }}
          aria-expanded={deviceMenuOpen}
          aria-haspopup="menu"
          title={`${activeDevice?.label ?? "未连接设备"} · 点击切换电脑`}
        >
          <span className="remote-selector-btn-left">
            <span className="remote-selector-btn-icon" aria-hidden="true">
              <svg
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.9"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                <line x1="8" y1="21" x2="16" y2="21" />
                <line x1="12" y1="17" x2="12" y2="21" />
              </svg>
            </span>
            <span className="remote-selector-btn-text">
              {activeDevice?.label ??
                (activeRunnerId
                  ? `电脑 · ${activeRunnerId.slice(-6)}`
                  : "选择电脑")}
            </span>
          </span>
          <span className="remote-selector-btn-right">
            <span
              className={`remote-status-dot ${
                activeDevice?.online ? "online" : "offline"
              }`}
              title={activeDevice?.online ? "电脑在线" : "电脑离线"}
            />
            <span
              className={`remote-selector-arrow ${
                deviceMenuOpen ? "open" : ""
              }`}
              aria-hidden="true"
            >
              <ChevronDownIcon size={13} />
            </span>
          </span>
        </button>

        {deviceMenuOpen && (
          <div
            className="remote-selector-popover remote-device-popover"
            role="menu"
          >
            <div className="remote-popover-header">选择电脑</div>
            <div className="remote-popover-list">
              {deviceList.length === 0 ? (
                <div className="remote-popover-empty">暂无可用电脑</div>
              ) : (
                deviceList.map((dev) => (
                  <button
                    type="button"
                    role="menuitemradio"
                    aria-checked={dev.runnerId === activeRunnerId}
                    className={`remote-popover-item ${
                      dev.runnerId === activeRunnerId ? "selected" : ""
                    }`}
                    key={dev.runnerId}
                    onClick={() => onSelectDevice(dev.runnerId)}
                  >
                    <span
                      className={`remote-status-dot ${
                        dev.online ? "online" : "offline"
                      }`}
                    />
                    <span className="remote-popover-item-name">
                      {dev.label}
                    </span>
                    <span className="remote-popover-item-badge">
                      {dev.online ? "在线" : "离线"}
                    </span>
                    {dev.runnerId === activeRunnerId && (
                      <svg
                        className="remote-popover-check"
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <polyline points="20 6 9 17 4 12" />
                      </svg>
                    )}
                  </button>
                ))
              )}
            </div>
          </div>
        )}
      </div>

      <div className="remote-selector-dropdown" ref={agentMenuRef}>
        <button
          type="button"
          disabled={isAgentSelectorDisabled}
          className={`remote-selector-btn ${agentMenuOpen ? "active" : ""} ${
            isAgentSelectorDisabled ? "disabled" : ""
          }`}
          ref={agentMenuButtonRef}
          onClick={() => {
            if (isAgentSelectorDisabled) return;
            setAgentMenuOpen((prev) => !prev);
            setDeviceMenuOpen(false);
          }}
          aria-expanded={agentMenuOpen}
          aria-haspopup="menu"
          aria-disabled={isAgentSelectorDisabled}
          title={
            !activeRunnerId
              ? "请先选择电脑"
              : isActiveRunnerHelloPending
                ? "正在同步当前电脑的 Agent 与会话列表…"
                : availableAgentsForCurrentDevice.length === 0
                  ? "当前电脑未检测到可用 Agent"
                  : `${currentAgentInfo.name} · 点击切换 Agent`
          }
        >
          <span className="remote-selector-btn-left">
            <span className="remote-selector-btn-icon" aria-hidden="true">
              <svg
                width="15"
                height="15"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.9"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="m12 3-1.912 5.813a2 2 0 0 1-1.275 1.275L3 12l5.813 1.912a2 2 0 0 1 1.275 1.275L12 21l1.912-5.813a2 2 0 0 1 1.275-1.275L21 12l-5.813-1.912a2 2 0 0 1-1.275-1.275L12 3Z" />
                <path d="M5 3v4M19 17v4M3 5h4M17 19h4" />
              </svg>
            </span>
            <span className="remote-selector-btn-text">
              {currentAgentInfo.name}
            </span>
          </span>
          <span className="remote-selector-btn-right">
            {currentAgentInfo.count > 0 && (
              <span
                className="remote-selector-count-badge"
                title={`${currentAgentInfo.count} 个会话`}
              >
                {currentAgentInfo.count}
              </span>
            )}
            {!isAgentSelectorDisabled && (
              <span
                className={`remote-selector-arrow ${
                  agentMenuOpen ? "open" : ""
                }`}
                aria-hidden="true"
              >
                <ChevronDownIcon size={13} />
              </span>
            )}
          </span>
        </button>

        {agentMenuOpen && activeRunnerId && !isActiveRunnerHelloPending && (
          <div
            className="remote-selector-popover remote-agent-popover"
            role="menu"
          >
            <div className="remote-popover-header">
              <span>选择 Agent</span>
              <span className="remote-popover-sub">
                {activeDevice?.label
                  ? `来自 ${activeDevice.label}`
                  : "当前电脑"}
              </span>
            </div>
            <div className="remote-popover-list">
              {availableAgentsForCurrentDevice.length === 0 ? (
                <div className="remote-popover-empty">
                  当前电脑未检测到 Agent
                </div>
              ) : (
                availableAgentsForCurrentDevice.map((agent) => (
                  <button
                    type="button"
                    role="menuitemradio"
                    aria-checked={providerFilter === agent.type}
                    className={`remote-popover-item ${
                      providerFilter === agent.type ? "selected" : ""
                    }`}
                    key={agent.type}
                    onClick={() => onSelectAgent(agent.type)}
                  >
                    <span
                      className="remote-popover-agent-indicator"
                      data-agent={agent.type}
                    />
                    <span className="remote-popover-item-name">
                      {agent.name}
                    </span>
                    {agent.count > 0 ? (
                      <span className="remote-popover-item-badge active-count">
                        {agent.count}
                      </span>
                    ) : (
                      <span className="remote-popover-item-badge">0</span>
                    )}
                    {providerFilter === agent.type && (
                      <svg
                        className="remote-popover-check"
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.5"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <polyline points="20 6 9 17 4 12" />
                      </svg>
                    )}
                  </button>
                ))
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
