"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { DeviceStrip } from "../src/components/DeviceStrip";
import { SessionDetail } from "../src/components/SessionDetail";
import { SessionList } from "../src/components/SessionList";
import { SettingsPanel } from "../src/components/SettingsPanel";
import {
  describeApiError,
  fetchDevices,
  fetchSessionEvents,
  fetchSessions,
} from "../src/lib/api";
import type { PanelConfig } from "../src/lib/config";
import { loadConfig, saveConfig } from "../src/lib/config";
import {
  SESSION_TABS,
  TAB_LABELS,
  filterSessions,
  sortSessions,
  tabCounts,
  type SessionTab,
} from "../src/lib/classify";
import { formatRelative } from "../src/lib/format";
import type { DeviceRecord, SessionRecord, SessionRef } from "../src/lib/types";
import { sessionKey, sessionRef } from "../src/lib/types";

const POLL_INTERVAL_MS = 5_000;

export default function PanelPage() {
  const [config, setConfig] = useState<PanelConfig | undefined>(undefined);
  const [devices, setDevices] = useState<DeviceRecord[]>([]);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [syncError, setSyncError] = useState<string | undefined>(undefined);
  const [lastSyncMs, setLastSyncMs] = useState<number | undefined>(undefined);
  const [nowMs, setNowMs] = useState<number>(() => Date.now());
  const [tab, setTab] = useState<SessionTab>("all");
  const [deviceFilter, setDeviceFilter] = useState<string | undefined>(
    undefined,
  );
  const [selected, setSelected] = useState<SessionRef | undefined>(undefined);
  const [timeline, setTimeline] = useState<SessionRecord[] | undefined>(
    undefined,
  );
  const [timelineError, setTimelineError] = useState<string | undefined>(
    undefined,
  );
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Config only exists in the browser; loading it in an effect keeps the
  // server-rendered markup deterministic.
  useEffect(() => {
    setConfig(loadConfig(window.localStorage));
  }, []);

  const refresh = useCallback(async () => {
    if (config === undefined) {
      return;
    }
    try {
      const [nextDevices, nextSessions] = await Promise.all([
        fetchDevices(config),
        fetchSessions(config),
      ]);
      setDevices(nextDevices);
      setSessions(nextSessions);
      setSyncError(undefined);
      setLastSyncMs(Date.now());
    } catch (error) {
      setSyncError(describeApiError(error));
    }
  }, [config]);

  useEffect(() => {
    if (config === undefined) {
      return;
    }
    void refresh();
    // The first fetch always runs (embedded webviews can report "hidden"
    // permanently); only the recurring poll pauses while not visible.
    const interval = window.setInterval(() => {
      if (!document.hidden) {
        void refresh();
      }
    }, POLL_INTERVAL_MS);
    const onVisibility = () => {
      if (!document.hidden) {
        void refresh();
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [config, refresh]);

  useEffect(() => {
    const interval = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, []);

  useEffect(() => {
    if (config === undefined || selected === undefined) {
      setTimeline(undefined);
      setTimelineError(undefined);
      return;
    }
    let cancelled = false;
    const load = async () => {
      try {
        const events = await fetchSessionEvents(config, selected);
        if (!cancelled) {
          setTimeline(events);
          setTimelineError(undefined);
        }
      } catch (error) {
        if (!cancelled) {
          setTimelineError(describeApiError(error));
        }
      }
    };
    setTimeline(undefined);
    setTimelineError(undefined);
    void load();
    const interval = window.setInterval(() => {
      if (!document.hidden) {
        void load();
      }
    }, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [config, selected]);

  const visibleSessions = useMemo(
    () => sortSessions(filterSessions(sessions, tab, deviceFilter)),
    [sessions, tab, deviceFilter],
  );
  const counts = useMemo(
    () => tabCounts(sessions, deviceFilter),
    [sessions, deviceFilter],
  );
  const selectedKey = selected === undefined ? undefined : sessionKey(selected);
  const selectedSession = useMemo(
    () =>
      selectedKey === undefined
        ? undefined
        : sessions.find(
            (session) => sessionKey(sessionRef(session)) === selectedKey,
          ),
    [sessions, selectedKey],
  );

  const handleSaveConfig = (next: PanelConfig) => {
    saveConfig(window.localStorage, next);
    setDevices([]);
    setSessions([]);
    setSelected(undefined);
    setSyncError(undefined);
    setLastSyncMs(undefined);
    setSettingsOpen(false);
    setConfig(next);
  };

  const showOnboarding =
    config !== undefined &&
    devices.length === 0 &&
    syncError === undefined &&
    lastSyncMs !== undefined;

  return (
    <div className="panel">
      <header className="panel-header">
        <div>
          <h1>Agent Platform</h1>
          <p className="panel-status">
            {config === undefined
              ? "初始化…"
              : syncError !== undefined
                ? syncError
                : lastSyncMs === undefined
                  ? "连接网关中…"
                  : `已更新 · ${formatRelative(
                      new Date(lastSyncMs).toISOString(),
                      nowMs,
                    )}`}
            {syncError !== undefined && <span className="status-dot-error" />}
          </p>
        </div>
        <button
          type="button"
          className="secondary"
          onClick={() => setSettingsOpen(true)}
        >
          设置
        </button>
      </header>

      <DeviceStrip
        devices={devices}
        selected={deviceFilter}
        nowMs={nowMs}
        onSelect={setDeviceFilter}
      />

      <nav className="tabs" aria-label="会话分类">
        {SESSION_TABS.map((entry) => (
          <button
            type="button"
            key={entry}
            className="tab"
            data-selected={entry === tab}
            onClick={() => setTab(entry)}
          >
            {TAB_LABELS[entry]}
            <span className="tab-count">{counts[entry]}</span>
          </button>
        ))}
      </nav>

      <main
        className="content"
        data-detail-open={selectedSession !== undefined}
      >
        <section className="list-pane">
          {showOnboarding ? (
            <div className="onboarding">
              <h2>还没有绑定设备</h2>
              <p>
                在要监控的电脑上启动 Runner 并完成绑定后，这里会显示所有 Claude
                Code / Codex 会话的实时状态：
              </p>
              <ol>
                <li>启动 Gateway 并配置一次性绑定码</li>
                <li>
                  <span className="mono">
                    agent-platform-runner device bind
                  </span>{" "}
                  登记设备
                </li>
                <li>
                  <span className="mono">agent-platform-runner run</span> 采集，
                  <span className="mono">sync push</span> 上传
                </li>
              </ol>
              <p>具体命令见仓库 README。</p>
            </div>
          ) : (
            <SessionList
              sessions={visibleSessions}
              selectedKey={selectedKey}
              nowMs={nowMs}
              onSelect={(session) => setSelected(sessionRef(session))}
            />
          )}
        </section>
        {selectedSession !== undefined && (
          <SessionDetail
            session={selectedSession}
            timeline={timeline}
            timelineError={timelineError}
            nowMs={nowMs}
            onClose={() => setSelected(undefined)}
          />
        )}
      </main>

      {settingsOpen && config !== undefined && (
        <SettingsPanel
          config={config}
          onSave={handleSaveConfig}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}
