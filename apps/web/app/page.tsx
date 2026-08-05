"use client";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { SettingsPanel } from "../src/components/SettingsPanel";
import { DeviceConnectDialog } from "../src/components/DeviceConnectDialog";
import { SessionInspector } from "../src/components/SessionInspector";
import {
  ActivityView,
  DevicesView,
  DiagnosticsView,
  HelpView,
  OverviewView,
  SessionsView,
} from "../src/components/PlatformViews";
import { PlatformShell } from "../src/components/PlatformShell";
import {
  describeApiError,
  fetchDeviceAuth,
  fetchDevices,
  fetchSessionEvents,
  fetchSessions,
} from "../src/lib/api";
import type { PanelConfig } from "../src/lib/config";
import { loadConfig, saveConfig } from "../src/lib/config";
import {
  AGENT_LABELS,
  type ProviderFilter,
  type RangeFilter,
  type SectionKey,
  type SessionFilter,
  formatRelativeCompact,
  matchesSessionFilter,
  sessionTask,
  sortPlatformSessions,
} from "../src/lib/platform-view";
import { connectionStatus } from "../src/lib/connection-status";
import { parseNotificationPresentedAt } from "../src/lib/notifications";
import { usePwaInstall } from "../src/lib/pwa-install";
import { openChangeStream } from "../src/lib/stream";
import type {
  AuthRecord,
  DeviceRecord,
  SessionRecord,
  SessionRef,
} from "../src/lib/types";
import { parseSessionRef, sessionKey, sessionRef } from "../src/lib/types";
import {
  PREVIEW_DEVICES,
  PREVIEW_SESSIONS,
  previewTimelineFor,
} from "../src/lib/preview-data";
import { Icon } from "../src/components/Icon";

const POLL_INTERVAL_MS = 5_000;
const STREAMING_POLL_INTERVAL_MS = 30_000;

function LoadingView() {
  return (
    <div
      className="loading-view"
      role="status"
      aria-label="正在加载 AgentPulse"
    >
      <div className="loading-title" />
      <div className="loading-copy" />
      <div className="loading-metrics">
        <span />
        <span />
        <span />
        <span />
      </div>
      <div className="loading-grid">
        <div />
        <div />
      </div>
    </div>
  );
}

function SyncAlert({
  browserOnline,
  syncError,
  onRefresh,
}: {
  browserOnline: boolean;
  syncError: string | undefined;
  onRefresh: () => void;
}) {
  if (browserOnline && syncError === undefined) return null;
  return (
    <section className="system-alert" role="alert">
      <span className="system-alert-icon">
        <Icon name="warning" size={17} />
      </span>
      <div>
        <strong>{browserOnline ? "网关同步不可用" : "网络已断开"}</strong>
        <p>
          {browserOnline
            ? syncError
            : "已保留最近一次状态；连接恢复后会自动重试。"}
        </p>
      </div>
      {browserOnline && (
        <button className="secondary" onClick={onRefresh} type="button">
          重试连接
        </button>
      )}
    </section>
  );
}

export default function PanelPage() {
  const [config, setConfig] = useState<PanelConfig | undefined>();
  const [previewMode, setPreviewMode] = useState<boolean | undefined>();
  const [devices, setDevices] = useState<DeviceRecord[]>([]);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [authSnapshots, setAuthSnapshots] = useState<AuthRecord[]>([]);
  const [syncError, setSyncError] = useState<string | undefined>();
  const [lastSyncMs, setLastSyncMs] = useState<number | undefined>();
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [section, setSection] = useState<SectionKey>("overview");
  const [sessionFilter, setSessionFilter] = useState<SessionFilter>("all");
  const [deviceFilter, setDeviceFilter] = useState<string | undefined>();
  const [providerFilter, setProviderFilter] = useState<ProviderFilter>("all");
  const [rangeFilter, setRangeFilter] = useState<RangeFilter>("24h");
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<SessionRef | undefined>();
  const [timeline, setTimeline] = useState<SessionRecord[] | undefined>();
  const [timelineError, setTimelineError] = useState<string | undefined>();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [deviceConnectOpen, setDeviceConnectOpen] = useState(false);
  const [fullSession, setFullSession] = useState(false);
  const [streamConnected, setStreamConnected] = useState(false);
  const [browserOnline, setBrowserOnline] = useState(true);
  const [notificationPresentedAt, setNotificationPresentedAt] = useState<
    number | undefined
  >();
  const pwaInstall = usePwaInstall();
  const refreshRef = useRef<() => Promise<void>>(async () => undefined);
  const timelineRequest = useRef(0);

  useEffect(() => {
    setConfig(loadConfig(window.localStorage));
    setPreviewMode(
      new URLSearchParams(window.location.search).get("preview") === "1",
    );
  }, []);

  const refresh = useCallback(async () => {
    if (config === undefined || previewMode === undefined || !navigator.onLine)
      return;
    if (previewMode) {
      setDevices(PREVIEW_DEVICES);
      setSessions(PREVIEW_SESSIONS);
      setAuthSnapshots([]);
      setSyncError(undefined);
      setLastSyncMs(Date.now());
      return;
    }
    try {
      const [nextDevices, nextSessions, nextAuthSnapshots] = await Promise.all([
        fetchDevices(config),
        fetchSessions(config),
        fetchDeviceAuth(config),
      ]);
      setDevices(nextDevices);
      setSessions(nextSessions);
      setAuthSnapshots(nextAuthSnapshots);
      setSyncError(undefined);
      setLastSyncMs(Date.now());
    } catch (error) {
      setSyncError(describeApiError(error));
    }
  }, [config, previewMode]);

  refreshRef.current = refresh;

  const loadTimeline = useCallback(async () => {
    if (
      config === undefined ||
      previewMode === undefined ||
      selected === undefined
    )
      return;
    timelineRequest.current += 1;
    const request = timelineRequest.current;
    const current = sessions.find(
      (session) => sessionKey(sessionRef(session)) === sessionKey(selected),
    );
    if (current === undefined) return;
    if (previewMode) {
      setTimeline(previewTimelineFor(current));
      setTimelineError(undefined);
      return;
    }
    try {
      const events = await fetchSessionEvents(config, selected);
      if (request === timelineRequest.current) {
        setTimeline(events);
        setTimelineError(undefined);
      }
    } catch (error) {
      if (request === timelineRequest.current)
        setTimelineError(describeApiError(error));
    }
  }, [config, previewMode, selected, sessions]);

  useEffect(() => {
    if (config === undefined || previewMode === undefined) return;
    void refresh();
    const onVisibility = () => {
      if (!document.hidden) void refresh();
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, [config, previewMode, refresh]);

  useEffect(() => {
    if (previewMode === undefined || previewMode) return;
    const interval = window.setInterval(
      () => {
        if (!document.hidden) void refresh();
      },
      streamConnected ? STREAMING_POLL_INTERVAL_MS : POLL_INTERVAL_MS,
    );
    return () => window.clearInterval(interval);
  }, [previewMode, refresh, streamConnected]);

  useEffect(() => {
    if (
      previewMode === undefined ||
      previewMode ||
      config === undefined ||
      !browserOnline
    ) {
      setStreamConnected(false);
      return;
    }
    setStreamConnected(false);
    return openChangeStream(config, {
      onChange: () => {
        void refreshRef.current();
        void loadTimeline();
      },
      onConnected: setStreamConnected,
    });
  }, [browserOnline, config, loadTimeline, previewMode]);

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
    const interval = window.setInterval(() => setNowMs(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, []);

  useEffect(() => {
    if (!("serviceWorker" in navigator)) return;
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
      )
        return;
      const ref = parseSessionRef((message as { session?: unknown }).session);
      if (ref === undefined) return;
      setSection("sessions");
      setSessionFilter("all");
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
    timelineRequest.current += 1;
    if (selected !== undefined) void loadTimeline();
  }, [loadTimeline, selected]);

  const baseSessions = useMemo(() => {
    const rangeMs =
      rangeFilter === "24h"
        ? 24 * 60 * 60_000
        : rangeFilter === "7d"
          ? 7 * 24 * 60 * 60_000
          : undefined;
    const query = search.trim().toLowerCase();
    return sessions.filter((session) => {
      if (deviceFilter !== undefined && session.device_id !== deviceFilter)
        return false;
      if (providerFilter !== "all" && session.agent_type !== providerFilter)
        return false;
      if (
        rangeMs !== undefined &&
        Date.parse(session.observed_at) < nowMs - rangeMs
      )
        return false;
      if (query === "") return true;
      const searchable = [
        session.payload.project.display_name,
        session.agent_session_id,
        session.device_id,
        AGENT_LABELS[session.agent_type],
        sessionTask(session),
      ]
        .join(" ")
        .toLowerCase();
      return searchable.includes(query);
    });
  }, [deviceFilter, nowMs, providerFilter, rangeFilter, search, sessions]);

  useEffect(() => {
    if (
      !previewMode ||
      selected !== undefined ||
      baseSessions.length === 0 ||
      window.innerWidth < 920
    )
      return;
    const first = sortPlatformSessions(baseSessions)[0];
    if (first !== undefined) setSelected(sessionRef(first));
  }, [baseSessions, previewMode, selected]);

  const selectedKey = selected === undefined ? undefined : sessionKey(selected);
  const selectedSession = useMemo(
    () =>
      selectedKey === undefined
        ? undefined
        : baseSessions.find(
            (session) => sessionKey(sessionRef(session)) === selectedKey,
          ),
    [baseSessions, selectedKey],
  );

  useEffect(() => {
    if (
      selectedKey !== undefined &&
      !baseSessions.some(
        (session) => sessionKey(sessionRef(session)) === selectedKey,
      )
    ) {
      setSelected(undefined);
      setFullSession(false);
    }
  }, [baseSessions, selectedKey]);
  const visibleSessionIndex =
    selectedKey === undefined
      ? -1
      : baseSessions.findIndex(
          (session) => sessionKey(sessionRef(session)) === selectedKey,
        );

  const navigateDetail = (direction: -1 | 1) => {
    const next = visibleSessionIndex + direction;
    const session =
      next >= 0 && next < baseSessions.length ? baseSessions[next] : undefined;
    if (session !== undefined) setSelected(sessionRef(session));
  };

  const handleSelectSession = (session: SessionRecord) => {
    setSelected(sessionRef(session));
    setFullSession(false);
  };

  const handleSaveConfig = (next: PanelConfig) => {
    saveConfig(window.localStorage, next);
    setDevices([]);
    setSessions([]);
    setAuthSnapshots([]);
    setSelected(undefined);
    setFullSession(false);
    setSyncError(undefined);
    setLastSyncMs(undefined);
    setSettingsOpen(false);
    setConfig(next);
    setPreviewMode(false);
  };

  const showInitialLoading =
    previewMode === undefined ||
    (lastSyncMs === undefined && syncError === undefined);
  const connection = previewMode
    ? { label: "网关已连接", tone: "live" }
    : connectionStatus({
        configured: config !== undefined,
        online: browserOnline,
        syncError,
        lastSyncLabel:
          lastSyncMs === undefined
            ? undefined
            : formatRelativeCompact(new Date(lastSyncMs).toISOString(), nowMs),
        streamConnected,
      });

  let view: ReactNode;
  if (showInitialLoading) {
    view = <LoadingView />;
  } else if (section === "overview") {
    view = (
      <OverviewView
        devices={devices}
        nowMs={nowMs}
        onNavigate={setSection}
        onSelect={handleSelectSession}
        selectedKey={selectedKey}
        sessions={baseSessions}
        sessionFilter={sessionFilter}
        onSessionFilterChange={setSessionFilter}
      />
    );
  } else if (section === "sessions") {
    view = (
      <SessionsView
        devices={devices}
        filter={sessionFilter}
        nowMs={nowMs}
        onFilterChange={setSessionFilter}
        onSelect={handleSelectSession}
        selectedKey={selectedKey}
        sessions={baseSessions}
      />
    );
  } else if (section === "devices") {
    view = (
      <DevicesView
        devices={devices}
        nowMs={nowMs}
        onAddDevice={() => setDeviceConnectOpen(true)}
        onSelect={handleSelectSession}
        sessions={baseSessions}
        authSnapshots={authSnapshots}
      />
    );
  } else if (section === "activity") {
    view = (
      <ActivityView devices={devices} nowMs={nowMs} sessions={baseSessions} />
    );
  } else if (section === "diagnostics") {
    view = (
      <DiagnosticsView
        connectionLabel={connection.label}
        connectionTone={connection.tone}
        devices={devices}
        authSnapshots={authSnapshots}
        streamConnected={streamConnected}
        syncError={syncError}
      />
    );
  } else {
    view = <HelpView onNavigate={setSection} />;
  }

  const inspector =
    selectedSession === undefined ? undefined : (
      <SessionInspector
        devices={devices}
        expanded={fullSession}
        nowMs={nowMs}
        onClose={() => {
          setSelected(undefined);
          setFullSession(false);
        }}
        onExitFull={() => setFullSession(false)}
        onNext={
          visibleSessionIndex >= 0 &&
          visibleSessionIndex < baseSessions.length - 1
            ? () => navigateDetail(1)
            : undefined
        }
        onOpenFull={() => {
          setFullSession(true);
          setSection("sessions");
        }}
        onPrevious={
          visibleSessionIndex > 0 ? () => navigateDetail(-1) : undefined
        }
        position={Math.max(1, visibleSessionIndex + 1)}
        session={selectedSession}
        timeline={timeline}
        timelineError={timelineError}
        total={baseSessions.length}
      />
    );

  return (
    <>
      <PlatformShell
        connectionLabel={connection.label}
        connectionTone={connection.tone}
        deviceFilter={deviceFilter}
        devices={devices}
        inspector={inspector}
        onDeviceFilterChange={setDeviceFilter}
        onNavigate={(next) => {
          setSection(next);
          if (next !== "sessions") setSessionFilter("all");
        }}
        onOpenSettings={() => setSettingsOpen(true)}
        onProviderFilterChange={setProviderFilter}
        onRangeFilterChange={setRangeFilter}
        onSearchChange={setSearch}
        providerFilter={providerFilter}
        rangeFilter={rangeFilter}
        search={search}
        section={section}
      >
        <SyncAlert
          browserOnline={browserOnline}
          onRefresh={() => void refresh()}
          syncError={syncError}
        />
        {view}
      </PlatformShell>
      {settingsOpen && config !== undefined && (
        <SettingsPanel
          config={config}
          devices={devices}
          installState={pwaInstall.state}
          notificationPresentedAt={notificationPresentedAt}
          onClose={() => setSettingsOpen(false)}
          onDevicesChanged={() => void refresh()}
          onInstall={pwaInstall.install}
          onSave={handleSaveConfig}
        />
      )}
      {deviceConnectOpen && config !== undefined && (
        <DeviceConnectDialog
          config={config}
          onClose={() => setDeviceConnectOpen(false)}
        />
      )}
    </>
  );
}
