"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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
import { openChangeStream } from "../src/lib/stream";
import type { DeviceRecord, SessionRecord, SessionRef } from "../src/lib/types";
import { parseSessionRef, sessionKey, sessionRef } from "../src/lib/types";

const POLL_INTERVAL_MS = 5_000;
/**
 * With the change stream connected, polling is only a safety net for missed
 * notifications (single-instance broker, sleeping tabs, proxies that buffer).
 */
const STREAMING_POLL_INTERVAL_MS = 30_000;

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
  const [streamConnected, setStreamConnected] = useState(false);

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

  const timelineRequest = useRef(0);
  const loadTimeline = useCallback(async () => {
    if (config === undefined || selected === undefined) {
      return;
    }
    timelineRequest.current += 1;
    const request = timelineRequest.current;
    try {
      const events = await fetchSessionEvents(config, selected);
      if (request === timelineRequest.current) {
        setTimeline(events);
        setTimelineError(undefined);
      }
    } catch (error) {
      if (request === timelineRequest.current) {
        setTimelineError(describeApiError(error));
      }
    }
  }, [config, selected]);

  const pollIntervalMs = streamConnected
    ? STREAMING_POLL_INTERVAL_MS
    : POLL_INTERVAL_MS;

  useEffect(() => {
    if (config === undefined) {
      return;
    }
    // The first fetch always runs (embedded webviews can report "hidden"
    // permanently); only the recurring poll pauses while not visible.
    void refresh();
    const onVisibility = () => {
      if (!document.hidden) {
        void refresh();
      }
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, [config, refresh]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (!document.hidden) {
        void refresh();
      }
    }, pollIntervalMs);
    return () => window.clearInterval(interval);
  }, [refresh, pollIntervalMs]);

  // The stream handlers must reach the current loaders without restarting the
  // connection every time a fetch closure is recreated.
  const refreshRef = useRef(refresh);
  const loadTimelineRef = useRef(loadTimeline);
  useEffect(() => {
    refreshRef.current = refresh;
    loadTimelineRef.current = loadTimeline;
  }, [refresh, loadTimeline]);

  useEffect(() => {
    if (config === undefined) {
      return;
    }
    setStreamConnected(false);
    return openChangeStream(config, {
      onChange: () => {
        void refreshRef.current();
        void loadTimelineRef.current();
      },
      onConnected: setStreamConnected,
    });
  }, [config]);

  useEffect(() => {
    const interval = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, []);

  // Clicking a push notification focuses this tab and hands the session over
  // in a message: identifiers stay out of the URL, history and shared links.
  useEffect(() => {
    if (!("serviceWorker" in navigator)) {
      return;
    }
    const onMessage = (event: MessageEvent) => {
      const message: unknown = event.data;
      if (
        typeof message !== "object" ||
        message === null ||
        (message as { type?: unknown }).type !== "select-session"
      ) {
        return;
      }
      const ref = parseSessionRef((message as { session?: unknown }).session);
      if (ref === undefined) {
        return;
      }
      // The session may sit outside the current view; reset the filters so the
      // notification always lands on something visible.
      setTab("all");
      setDeviceFilter(undefined);
      setSelected(ref);
    };
    navigator.serviceWorker.addEventListener("message", onMessage);
    return () =>
      navigator.serviceWorker.removeEventListener("message", onMessage);
  }, []);

  useEffect(() => {
    setTimeline(undefined);
    setTimelineError(undefined);
    if (config === undefined || selected === undefined) {
      // Invalidate any in-flight load so a late response cannot repopulate the
      // timeline of a session the user already left.
      timelineRequest.current += 1;
      return;
    }
    void loadTimeline();
  }, [config, selected, loadTimeline]);

  useEffect(() => {
    const interval = window.setInterval(() => {
      if (!document.hidden) {
        void loadTimeline();
      }
    }, pollIntervalMs);
    return () => window.clearInterval(interval);
  }, [loadTimeline, pollIntervalMs]);

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
                  : `${streamConnected ? "实时" : "轮询"} · 已更新 ${formatRelative(
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
