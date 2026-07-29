import { useMemo, useState } from "react";

import {
  SESSION_TABS,
  TAB_LABELS,
  tabCounts,
  type SessionTab,
} from "../lib/classify";
import { useModalDialog } from "../lib/dialog-focus";
import { summarizeDevices } from "../lib/device-summary";
import { AGENT_LABELS, deviceShortId, formatRelative } from "../lib/format";
import type { DeviceRecord, SessionRecord } from "../lib/types";

interface MobileFiltersProps {
  devices: DeviceRecord[];
  sessions: SessionRecord[];
  selectedTab: SessionTab;
  selectedDevice: string | undefined;
  nowMs: number;
  onApply: (tab: SessionTab, deviceId: string | undefined) => void;
  onClose: () => void;
}

export function MobileFilters({
  devices,
  sessions,
  selectedTab,
  selectedDevice,
  nowMs,
  onApply,
  onClose,
}: MobileFiltersProps) {
  const [draftTab, setDraftTab] = useState(selectedTab);
  const [draftDevice, setDraftDevice] = useState(selectedDevice);
  const summaries = useMemo(
    () => summarizeDevices(devices, sessions),
    [devices, sessions],
  );
  const draftCounts = useMemo(
    () => tabCounts(sessions, draftDevice),
    [sessions, draftDevice],
  );
  const dialogRef = useModalDialog<HTMLElement>(onClose);

  return (
    <div
      className="mobile-sheet-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <section
        ref={dialogRef}
        className="mobile-sheet mobile-filter-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby="mobile-filter-title"
        tabIndex={-1}
      >
        <header className="mobile-sheet-header">
          <div>
            <p className="eyebrow">FILTER & DEVICES</p>
            <h2 id="mobile-filter-title">筛选与设备</h2>
            <p>选择会话视图和要查看的设备。</p>
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="关闭筛选"
            onClick={onClose}
          >
            ✕
          </button>
        </header>

        <div className="mobile-sheet-scroll">
          <section
            className="mobile-filter-section"
            aria-labelledby="mobile-view-title"
          >
            <div className="mobile-filter-section-heading">
              <h3 id="mobile-view-title">会话视图</h3>
              <span>{TAB_LABELS[draftTab]}</span>
            </div>
            <div className="mobile-filter-views">
              {SESSION_TABS.map((entry) => (
                <button
                  type="button"
                  key={entry}
                  data-selected={entry === draftTab}
                  onClick={() => setDraftTab(entry)}
                >
                  <span>{TAB_LABELS[entry]}</span>
                  <strong>{draftCounts[entry]}</strong>
                </button>
              ))}
            </div>
          </section>

          <section
            className="mobile-filter-section"
            aria-labelledby="mobile-device-title"
          >
            <div className="mobile-filter-section-heading">
              <h3 id="mobile-device-title">设备</h3>
              <span>{devices.length} 台</span>
            </div>
            <div className="mobile-device-list">
              <button
                type="button"
                className="mobile-device-card mobile-device-all"
                data-selected={draftDevice === undefined}
                onClick={() => setDraftDevice(undefined)}
              >
                <span className="mobile-device-icon" aria-hidden="true">
                  ◫
                </span>
                <span>
                  <strong>全部设备</strong>
                  <small>{sessions.length} 条会话</small>
                </span>
                <span className="mobile-device-check" aria-hidden="true">
                  ✓
                </span>
              </button>
              {summaries.map((summary) => (
                <button
                  type="button"
                  className="mobile-device-card"
                  key={summary.device.device_id}
                  data-selected={draftDevice === summary.device.device_id}
                  onClick={() => setDraftDevice(summary.device.device_id)}
                >
                  <span
                    className="device-dot"
                    data-online={
                      summary.device.online && !summary.device.revoked
                    }
                  />
                  <span className="mobile-device-main">
                    <span className="mobile-device-title">
                      <strong className="mono">
                        {deviceShortId(summary.device.device_id)}
                      </strong>
                      <small>
                        {summary.device.revoked
                          ? "已撤销"
                          : summary.device.online
                            ? "在线"
                            : formatRelative(
                                summary.device.last_seen_at,
                                nowMs,
                              )}
                      </small>
                    </span>
                    <span className="mobile-device-agents">
                      {summary.agents.length === 0
                        ? "尚无 Agent 会话"
                        : summary.agents
                            .map((agent) => AGENT_LABELS[agent])
                            .join(" · ")}
                    </span>
                    <span className="mobile-device-metrics">
                      <span>{summary.totalSessions} 会话</span>
                      <span>{summary.liveSessions} 活动</span>
                      <span data-alert={summary.waitingSessions > 0}>
                        {summary.waitingSessions} 等待
                      </span>
                    </span>
                  </span>
                  <span className="mobile-device-check" aria-hidden="true">
                    ✓
                  </span>
                </button>
              ))}
            </div>
          </section>
        </div>

        <footer className="mobile-sheet-actions">
          <button
            type="button"
            className="secondary"
            onClick={() => {
              setDraftTab("all");
              setDraftDevice(undefined);
            }}
          >
            重置
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={() => onApply(draftTab, draftDevice)}
          >
            查看会话
          </button>
        </footer>
      </section>
    </div>
  );
}
