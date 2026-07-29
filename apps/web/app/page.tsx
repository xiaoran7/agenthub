"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { DeviceStrip } from "../src/components/DeviceStrip";
import { MobileDock } from "../src/components/MobileDock";
import { MobileFilters } from "../src/components/MobileFilters";
import { EmptySessions, PanelSkeleton } from "../src/components/PanelStates";
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
import { connectionStatus } from "../src/lib/connection-status";
import { deviceShortId, formatRelative } from "../src/lib/format";
import { parseNotificationPresentedAt } from "../src/lib/notifications";
import { usePwaInstall } from "../src/lib/pwa-install";
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
  const [mobileFiltersOpen, setMobileFiltersOpen] = useState(false);
  const [streamConnected, setStreamConnected] = useState(false);
  const [browserOnline, setBrowserOnline] = useState(true);
  const [notificationPresentedAt, setNotificationPresentedAt] = useState<
    number | undefined
  >(undefined);
  const pwaInstall = usePwaInstall();

  // Config only exists in the browser; loading it in an effect keeps the
  // server-rendered markup deterministic.
  useEffect(() => {
    setConfig(loadConfig(window.localStorage));
  }, []);

  const refresh = useCallback(async () => {
    if (config === undefined || !navigator.onLine) {
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
    const onOnline = () => {
      setBrowserOnline(true);
      void refreshRef.current();
    };
    const onOffline = () => {
      setBrowserOnline(false);
      setStreamConnected(false);
    };
    setBrowserOnline(navigator.onLine);
    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);
    return () => {
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    };
  }, []);

  useEffect(() => {
    if (config === undefined || !browserOnline) {
      setStreamConnected(false);
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
  }, [config, browserOnline]);

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
      const presentedAt = parseNotificationPresentedAt(message);
      if (presentedAt !== undefined) {
        setNotificationPresentedAt(presentedAt);
        return;
      }
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
  const globalCounts = useMemo(
    () => tabCounts(sessions, undefined),
    [sessions],
  );
  const activeDevices = devices.filter((device) => !device.revoked);
  const onlineDevices = activeDevices.filter((device) => device.online).length;
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
  const openSettings = useCallback(() => setSettingsOpen(true), []);
  const closeSettings = useCallback(() => setSettingsOpen(false), []);
  const openMobileFilters = useCallback(() => setMobileFiltersOpen(true), []);
  const closeMobileFilters = useCallback(() => setMobileFiltersOpen(false), []);

  const showOnboarding =
    config !== undefined &&
    devices.length === 0 &&
    syncError === undefined &&
    lastSyncMs !== undefined;
  const showInitialLoading =
    config === undefined ||
    (lastSyncMs === undefined && syncError === undefined);
  const connection = connectionStatus({
    configured: config !== undefined,
    online: browserOnline,
    syncError,
    lastSyncLabel:
      lastSyncMs === undefined
        ? undefined
        : formatRelative(new Date(lastSyncMs).toISOString(), nowMs),
    streamConnected,
  });

  const selectMobileTab = (nextTab: SessionTab) => {
    setTab(nextTab);
    window.requestAnimationFrame(() => {
      document
        .getElementById("sessions")
        ?.scrollIntoView({ behavior: "smooth", block: "start" });
    });
  };
  const applyMobileFilters = (
    nextTab: SessionTab,
    nextDevice: string | undefined,
  ) => {
    setTab(nextTab);
    setDeviceFilter(nextDevice);
    setMobileFiltersOpen(false);
    window.requestAnimationFrame(() => {
      document
        .getElementById("sessions")
        ?.scrollIntoView({ behavior: "smooth", block: "start" });
    });
  };

  return (
    <div className="panel">
      <header className="panel-header">
        <div className="brand-block">
          <p className="eyebrow">AGENT OPERATIONS</p>
          <h1>Agent Platform</h1>
          <p className="brand-subtitle">跨设备 Coding Agent 会话监控中心</p>
        </div>
        <div className="header-actions">
          <p
            className="connection-pill"
            role="status"
            aria-live="polite"
            data-state={connection.tone}
          >
            <span className="connection-dot" />
            {connection.label}
          </p>
          <button
            type="button"
            className="secondary settings-button"
            onClick={openSettings}
          >
            <span aria-hidden="true">⚙</span>
            设置
          </button>
        </div>
      </header>

      {(!browserOnline || syncError !== undefined) && (
        <section className="sync-banner" role="alert">
          <span className="sync-banner-icon" aria-hidden="true">
            !
          </span>
          <div>
            <strong>
              {browserOnline ? "暂时无法刷新数据" : "网络连接已断开"}
            </strong>
            <p>
              {browserOnline
                ? syncError
                : "已保留上次同步结果，恢复联网后会自动重试。"}
            </p>
          </div>
          {browserOnline && (
            <button
              type="button"
              className="secondary"
              onClick={() => void refresh()}
            >
              重新连接
            </button>
          )}
        </section>
      )}

      <section className="overview-grid" aria-label="运行概览">
        <article className="overview-card" data-tone="devices">
          <div className="overview-icon" aria-hidden="true">
            ◫
          </div>
          <div>
            <p>在线设备</p>
            {showInitialLoading ? (
              <span className="overview-value-skeleton" aria-hidden="true" />
            ) : (
              <strong>
                {onlineDevices}
                <span> / {activeDevices.length}</span>
              </strong>
            )}
          </div>
        </article>
        <article className="overview-card" data-tone="live">
          <div className="overview-icon" aria-hidden="true">
            ◉
          </div>
          <div>
            <p>活动会话</p>
            {showInitialLoading ? (
              <span className="overview-value-skeleton" aria-hidden="true" />
            ) : (
              <strong>{globalCounts.live}</strong>
            )}
          </div>
        </article>
        <article className="overview-card" data-tone="waiting">
          <div className="overview-icon" aria-hidden="true">
            !
          </div>
          <div>
            <p>等待处理</p>
            {showInitialLoading ? (
              <span className="overview-value-skeleton" aria-hidden="true" />
            ) : (
              <strong>{globalCounts.waiting}</strong>
            )}
          </div>
        </article>
        <article className="overview-card" data-tone="finished">
          <div className="overview-icon" aria-hidden="true">
            ✓
          </div>
          <div>
            <p>最近完成</p>
            {showInitialLoading ? (
              <span className="overview-value-skeleton" aria-hidden="true" />
            ) : (
              <strong>{globalCounts.finished}</strong>
            )}
          </div>
        </article>
      </section>

      <section
        className="control-surface"
        aria-label="面板筛选"
        data-loading={showInitialLoading}
      >
        <div className="control-row">
          <span className="control-label">设备范围</span>
          <DeviceStrip
            devices={devices}
            selected={deviceFilter}
            nowMs={nowMs}
            onSelect={setDeviceFilter}
          />
        </div>
        <div className="control-divider" />
        <div className="control-row">
          <span className="control-label">会话视图</span>
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
        </div>
      </section>

      <main
        id="sessions"
        className="content"
        data-detail-open={selectedSession !== undefined}
      >
        <section className="list-pane">
          <header className="list-header">
            <div>
              <p className="eyebrow">SESSIONS</p>
              <h2>{TAB_LABELS[tab]}会话</h2>
            </div>
            <div className="list-header-actions">
              <button
                type="button"
                className="mobile-filter-trigger"
                onClick={openMobileFilters}
              >
                <span aria-hidden="true">≡</span>
                筛选
                {(tab !== "all" || deviceFilter !== undefined) && (
                  <span className="mobile-filter-summary">
                    {TAB_LABELS[tab]}
                    {deviceFilter === undefined
                      ? ""
                      : ` · ${deviceShortId(deviceFilter)}`}
                  </span>
                )}
              </button>
              <span className="result-count" role="status" aria-live="polite">
                {showInitialLoading
                  ? "正在读取"
                  : `${visibleSessions.length} 条结果`}
              </span>
            </div>
          </header>
          {showInitialLoading ? (
            <PanelSkeleton />
          ) : showOnboarding ? (
            <div className="onboarding">
              <h2>还没有绑定设备</h2>
              <p>
                在要监控的电脑上启动 Runner 并完成绑定后，这里会显示所有 Claude
                Code / Codex 会话的实时状态：
              </p>
              <ol>
                <li>打开右上角设置，生成 10 分钟一次性绑定码</li>
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
          ) : visibleSessions.length === 0 ? (
            <EmptySessions
              filtered={tab !== "all" || deviceFilter !== undefined}
              onReset={() => {
                setTab("all");
                setDeviceFilter(undefined);
              }}
            />
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

      <MobileDock
        selected={tab}
        counts={counts}
        deviceFilterActive={deviceFilter !== undefined}
        onSelect={selectMobileTab}
        onOpenFilters={openMobileFilters}
        onOpenSettings={openSettings}
      />

      {mobileFiltersOpen && (
        <MobileFilters
          devices={devices}
          sessions={sessions}
          selectedTab={tab}
          selectedDevice={deviceFilter}
          nowMs={nowMs}
          onApply={applyMobileFilters}
          onClose={closeMobileFilters}
        />
      )}

      {settingsOpen && config !== undefined && (
        <SettingsPanel
          config={config}
          devices={devices}
          onSave={handleSaveConfig}
          onDevicesChanged={() => void refresh()}
          notificationPresentedAt={notificationPresentedAt}
          installState={pwaInstall.state}
          onInstall={pwaInstall.install}
          onClose={closeSettings}
        />
      )}
    </div>
  );
}
