import { useEffect, useMemo, useRef, useState } from "react";
import type {
  AgentPlugin,
  RemotePluginsResponse,
} from "@agent-pulse/contracts";
import type { AccountView } from "../../lib/account";
import type { PanelConfig } from "../../lib/config";
import { parseNotificationPresentedAt } from "../../lib/notifications";
import {
  isTransientRemoteError,
  isTransientRemoteMessage,
  remoteError,
  type DiscoveryCandidate,
  type RemoteCommandRecord,
  type RemoteEvent,
  type RemoteRequest,
  type RemoteRunner,
  type RemoteRunnerHelloResponse,
  type RemoteSession,
  type RemoteSessionDetail,
  type RemoteSnapshot,
} from "../../lib/remote";
import {
  mergeMonotonicEvents,
  updateBoundedSessionState,
  type BoundedSessionState,
} from "../../lib/remote-session-state";
import {
  computeResumeStartCursor,
  createRafBatcher,
  fetchSessionEventsSince,
  parseDirectSseBatch,
  type RemoteEventsBatchResponse,
} from "../../lib/remote-sse-sync";
import {
  RunnerHelloSyncController,
  mergeRunnerHelloEntries,
  type RunnerHelloEntry,
} from "../../lib/runner-hello-sync";
import {
  followContentStream,
  streamBatchAffectsSnapshot,
} from "../../lib/stream";
import { parseSessionRef, type SessionRef } from "../../lib/types";

const EMPTY_SNAPSHOT: RemoteSnapshot = { cursor: 0, runners: [], sessions: [] };
const EMPTY_EVENTS: RemoteEvent[] = [];
const EMPTY_COMMANDS: RemoteCommandRecord[] = [];
const SNAPSHOT_FALLBACK_MS = 30_000;
const DETAIL_FALLBACK_MS = 30_000;

export interface UseRemoteSyncParams {
  requestWithIdentity: RemoteRequest;
  selected: string;
  setSelected: (value: string) => void;
  onAuthenticationRequired?: (() => void) | undefined;
  panelConfig?: PanelConfig | undefined;
  account?: AccountView | null | undefined;
  settingsOpen: boolean;
  launchRunner: string;
}

export function useRemoteSync({
  requestWithIdentity,
  selected,
  setSelected,
  onAuthenticationRequired,
  panelConfig,
  account,
  settingsOpen,
  launchRunner,
}: UseRemoteSyncParams) {
  const [snapshot, setSnapshot] = useState<RemoteSnapshot>(EMPTY_SNAPSHOT);
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;

  const [eventsState, setEventsState] = useState<
    BoundedSessionState<RemoteEvent[]>
  >({ byKey: {}, order: [] });
  const eventsBySession = eventsState.byKey;
  const eventsBySessionRef = useRef(eventsBySession);
  eventsBySessionRef.current = eventsBySession;

  const [commandsState, setCommandsState] = useState<
    BoundedSessionState<RemoteCommandRecord[]>
  >({ byKey: {}, order: [] });
  const commandsBySession = commandsState.byKey;
  const sessionCursorRef = useRef<
    Map<string, { cursor: number; lastPolledGlobalCursor: number }>
  >(new Map());
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const pendingPushSessionRef = useRef<SessionRef | null>(null);
  const prevSelectedForPushRef = useRef(selected);

  const [error, setError] = useState("");
  const errorRef = useRef(error);
  errorRef.current = error;

  const [loading, setLoading] = useState(true);
  const [sessionLoading, setSessionLoading] = useState(false);
  const [notificationPresentedAt, setNotificationPresentedAt] = useState<
    number | undefined
  >(undefined);

  const [helloEntries, setHelloEntries] = useState<
    Record<string, RunnerHelloEntry>
  >({});
  const helloSyncRef = useRef<RunnerHelloSyncController | null>(null);

  const [pluginsResponse, setPluginsResponse] =
    useState<RemotePluginsResponse | null>(null);

  const refresh = useRef(() => {});
  const refreshSessionDetail = useRef(() => {});

  const events = selected
    ? (eventsBySession[selected] ?? EMPTY_EVENTS)
    : EMPTY_EVENTS;
  const commands = selected
    ? (commandsBySession[selected] ?? EMPTY_COMMANDS)
    : EMPTY_COMMANDS;

  const setEventsForSession = (sessionKey: string, next: RemoteEvent[]) => {
    setEventsState((prev) => updateBoundedSessionState(prev, sessionKey, next));
  };
  const setCommandsForSession = (
    sessionKey: string,
    next: RemoteCommandRecord[],
  ) => {
    setCommandsState((prev) =>
      updateBoundedSessionState(prev, sessionKey, next),
    );
  };

  useEffect(() => {
    for (const key of Array.from(sessionCursorRef.current.keys())) {
      if (key !== selectedRef.current && !(key in eventsBySession)) {
        sessionCursorRef.current.delete(key);
      }
    }
  }, [eventsBySession]);

  useEffect(() => {
    if (prevSelectedForPushRef.current !== selected) {
      prevSelectedForPushRef.current = selected;
      pendingPushSessionRef.current = null;
    }
  }, [selected]);

  useEffect(() => {
    if (!error || !isTransientRemoteMessage(error)) return;
    const timer = setTimeout(() => {
      setError((current) => (isTransientRemoteMessage(current) ? "" : current));
    }, 6000);
    return () => clearTimeout(timer);
  }, [error]);

  const runners = useMemo<RemoteRunner[]>(
    () => mergeRunnerHelloEntries(snapshot.runners, helloEntries),
    [snapshot.runners, helloEntries],
  );

  useEffect(() => {
    setHelloEntries({});
    const controller = new RunnerHelloSyncController({
      fetcher: (id, signal) =>
        requestWithIdentity<RemoteRunnerHelloResponse>(
          `/runners/${encodeURIComponent(id)}/hello`,
          undefined,
          25000,
          undefined,
          signal,
        ),
      onChange: setHelloEntries,
      ttlMs: SNAPSHOT_FALLBACK_MS,
    });
    helloSyncRef.current = controller;
    return () => {
      controller.dispose();
      if (helloSyncRef.current === controller) {
        helloSyncRef.current = null;
      }
    };
  }, [requestWithIdentity]);

  useEffect(() => {
    const targets = snapshot.runners
      .filter((runner) => !runner.revoked)
      .map((runner) => ({
        runnerId: runner.runner_id,
        hasThinHello: runner.hello !== null,
        hostGeneration: runner.hello?.host_generation ?? null,
      }));
    helloSyncRef.current?.sync(targets);
  }, [snapshot.runners, requestWithIdentity]);

  useEffect(() => {
    if (typeof document === "undefined") return;
    const onVisibilityChange = () => {
      if (!document.hidden) {
        refresh.current();
        refreshSessionDetail.current();
      }
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, []);

  useEffect(() => {
    if (!("serviceWorker" in navigator)) return;
    const onMessage = (event: MessageEvent) => {
      const presentedAt = parseNotificationPresentedAt(event.data);
      if (presentedAt !== undefined) setNotificationPresentedAt(presentedAt);
      if (
        event.data &&
        typeof event.data === "object" &&
        event.data.type === "select-session"
      ) {
        const rawSession = event.data.session;
        if (typeof rawSession === "string") {
          pendingPushSessionRef.current = null;
          setSelected(rawSession);
        } else {
          const parsed = parseSessionRef(rawSession);
          if (parsed) {
            const found = snapshotRef.current.sessions.find(
              (s) =>
                s.runner_id === parsed.device_id &&
                s.binding.provider_session_id === parsed.agent_session_id,
            );
            if (found) {
              pendingPushSessionRef.current = null;
              setSelected(`${found.runner_id}/${found.binding.hub_session_id}`);
            } else {
              pendingPushSessionRef.current = parsed;
            }
          }
        }
      }
    };
    navigator.serviceWorker.addEventListener("message", onMessage);
    navigator.serviceWorker.controller?.postMessage({
      type: "consume-pending-session",
    });
    void navigator.serviceWorker.ready
      .then((reg) =>
        reg.active?.postMessage({ type: "consume-pending-session" }),
      )
      .catch(() => undefined);
    return () =>
      navigator.serviceWorker.removeEventListener("message", onMessage);
  }, [setSelected]);

  useEffect(() => {
    const pending = pendingPushSessionRef.current;
    if (!pending) return;
    const found = snapshot.sessions.find(
      (s) =>
        s.runner_id === pending.device_id &&
        s.binding.provider_session_id === pending.agent_session_id,
    );
    if (found) {
      pendingPushSessionRef.current = null;
      setSelected(`${found.runner_id}/${found.binding.hub_session_id}`);
    }
  }, [snapshot.sessions, setSelected]);

  useEffect(() => {
    let cancelled = false;
    const loadPlugins = () => {
      requestWithIdentity<RemotePluginsResponse>("/plugins")
        .then((res) => {
          if (!cancelled && res && Array.isArray(res.plugins)) {
            setPluginsResponse(res);
          }
        })
        .catch(() => {});
    };
    loadPlugins();
    const unfollowPlugins = followContentStream({
      gatewayBase: panelConfig?.gatewayBase ?? "",
      event: "plugins",
      onChange: () => {
        if (!cancelled) loadPlugins();
      },
    });
    return () => {
      cancelled = true;
      unfollowPlugins();
    };
  }, [requestWithIdentity, settingsOpen, panelConfig?.gatewayBase]);

  const plugins = useMemo(() => {
    if (!pluginsResponse) return [];
    if (launchRunner && Array.isArray(pluginsResponse.runners)) {
      const snap = pluginsResponse.runners.find(
        (r) => r.runner === launchRunner,
      );
      if (snap) return snap.plugins;
      if (pluginsResponse.runners.length > 0) return [];
    }
    return pluginsResponse.plugins;
  }, [pluginsResponse, launchRunner]);

  const allKnownPlugins = useMemo(() => {
    if (!pluginsResponse) return [];
    const merged = new Map<string, AgentPlugin>();
    for (const r of pluginsResponse.runners ?? []) {
      for (const p of r.plugins) {
        if (!merged.has(p.id)) merged.set(p.id, p);
      }
    }
    for (const p of pluginsResponse.plugins) {
      if (!merged.has(p.id)) merged.set(p.id, p);
    }
    return [...merged.values()];
  }, [pluginsResponse]);

  const discovered = useMemo(() => {
    const candidates = new Map<string, DiscoveryCandidate>();
    for (const r of runners) {
      if (r.revoked) continue;
      for (const candidate of r.hello?.discovery?.candidates ?? [])
        candidates.set(
          `${r.runner_id}/${candidate.provider_session_id}`,
          candidate,
        );
    }
    return candidates;
  }, [runners]);

  useEffect(() => {
    let stopped = false;
    const abortController = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    let inFlight = false;
    let again = false;
    const poll = async () => {
      if (stopped) return;
      if (inFlight) {
        again = true;
        return;
      }
      inFlight = true;
      again = false;
      clearTimeout(timer);
      try {
        const value = await requestWithIdentity<RemoteSnapshot>(
          "/sessions",
          undefined,
          25000,
          undefined,
          abortController.signal,
        );
        if (!stopped) {
          setSnapshot(value);
          setLoading(false);
          if (isTransientRemoteMessage(errorRef.current)) {
            setError("");
          }
        }
      } catch (err) {
        if (
          !stopped &&
          !(err instanceof Error && err.message === "request_aborted")
        ) {
          if (
            typeof document !== "undefined" &&
            document.hidden &&
            isTransientRemoteError(err)
          ) {
            // Keep background poll silent when tab is suspended or hidden
          } else {
            setError(remoteError(err));
          }
          setLoading(false);
          if (err instanceof Error && err.message === "authentication_required")
            onAuthenticationRequired?.();
        }
      } finally {
        inFlight = false;
        if (!stopped) {
          if (again) {
            void poll();
          } else {
            timer = setTimeout(() => void poll(), SNAPSHOT_FALLBACK_MS);
          }
        }
      }
    };
    refresh.current = () => {
      clearTimeout(timer);
      void poll();
    };
    void poll();
    const unfollow = followContentStream({
      gatewayBase: panelConfig?.gatewayBase ?? "",
      event: "remote",
      onChange: (frames) => {
        if (streamBatchAffectsSnapshot(frames)) {
          void poll();
        }
      },
    });
    return () => {
      stopped = true;
      abortController.abort();
      clearTimeout(timer);
      unfollow();
    };
  }, [
    requestWithIdentity,
    onAuthenticationRequired,
    panelConfig?.gatewayBase,
    panelConfig?.userId,
    account?.user.user_id,
  ]);

  useEffect(() => {
    if (!selected) {
      setSessionLoading(false);
      return;
    }
    const slash = selected.indexOf("/");
    const runnerId = slash > 0 ? selected.slice(0, slash) : "";
    const hub = slash > 0 ? selected.slice(slash + 1) : "";
    if (!runnerId || !hub) {
      setSessionLoading(false);
      return;
    }
    const sKey = `${runnerId}/${hub}`;
    const abortController = new AbortController();
    const cachedEvents =
      sKey in eventsBySessionRef.current
        ? eventsBySessionRef.current[sKey]
        : [];
    let accumulated = cachedEvents ?? [];
    if (!(sKey in eventsBySessionRef.current) || accumulated.length === 0) {
      sessionCursorRef.current.delete(sKey);
      accumulated = [];
    }
    const existingMeta =
      accumulated.length > 0 ? sessionCursorRef.current.get(sKey) : undefined;
    setSessionLoading(accumulated.length === 0);
    let stopped = false;
    let inFlight = false;
    let again = false;
    let seeded = false;
    let httpCursor = existingMeta?.cursor ?? 0;
    let sseCursor = Math.max(
      httpCursor,
      accumulated.length > 0 ? accumulated[accumulated.length - 1]!.cursor : 0,
    );
    let lastPolledGlobalCursor = existingMeta?.lastPolledGlobalCursor ?? -1;
    let timer: ReturnType<typeof setTimeout>;
    const applyBindingUpdate = (resolvedBinding: RemoteSession["binding"]) => {
      setSnapshot((prev) => {
        const idx = prev.sessions.findIndex(
          (s) => s.runner_id === runnerId && s.binding.hub_session_id === hub,
        );
        const prevBinding = idx >= 0 ? prev.sessions[idx]?.binding : undefined;
        if (
          prevBinding &&
          (prevBinding.control_connected !==
            resolvedBinding.control_connected ||
            prevBinding.execution_state !== resolvedBinding.execution_state ||
            prevBinding.current_turn_id !== resolvedBinding.current_turn_id)
        ) {
          const updated = [...prev.sessions];
          updated[idx] = { ...updated[idx]!, binding: resolvedBinding };
          return { ...prev, sessions: updated };
        }
        return prev;
      });
    };
    const poll = async () => {
      if (stopped) return;
      if (inFlight) {
        again = true;
        return;
      }
      again = false;
      inFlight = true;
      let latestDetail: RemoteSessionDetail | null = null;
      try {
        const detail = await requestWithIdentity<RemoteSessionDetail>(
          `/sessions/${encodeURIComponent(runnerId)}/${encodeURIComponent(hub)}`,
          undefined,
          25000,
          undefined,
          abortController.signal,
        );
        if (stopped) return;
        latestDetail = detail;
        let resetCleared = false;
        if (
          lastPolledGlobalCursor >= 0 &&
          detail.cursor < lastPolledGlobalCursor
        ) {
          liveBatcher.cancel();
          accumulated = [];
          httpCursor = 0;
          sseCursor = 0;
          lastPolledGlobalCursor = -1;
          resetCleared = true;
        }
        if (detail.cursor > lastPolledGlobalCursor || sseCursor > httpCursor) {
          const startCursor =
            lastPolledGlobalCursor === -1 && httpCursor === 0 && detail.resume
              ? computeResumeStartCursor(detail.resume)
              : httpCursor;
          const result = await fetchSessionEventsSince({
            requestPage: (after) =>
              requestWithIdentity<RemoteEventsBatchResponse>(
                `/events?runner=${encodeURIComponent(runnerId)}&hub=${encodeURIComponent(hub)}&after=${after}`,
                undefined,
                25000,
                undefined,
                abortController.signal,
              ),
            getAccumulated: () => accumulated,
            setAccumulated: (next) => {
              accumulated = next;
            },
            startCursor,
            resume: detail.resume,
            isStopped: () => stopped,
            onTruncatedReset: () => {
              liveBatcher.cancel();
              httpCursor = 0;
              sseCursor = 0;
              lastPolledGlobalCursor = -1;
              resetCleared = true;
            },
          });
          if (stopped) return;
          httpCursor = result.truncatedReset
            ? result.nextHttpCursor
            : Math.max(httpCursor, result.nextHttpCursor);
          sseCursor = Math.max(sseCursor, result.maxSeenCursor, httpCursor);
          if (result.changed || resetCleared || result.truncatedReset) {
            setEventsForSession(sKey, accumulated);
          }
          lastPolledGlobalCursor = detail.cursor;
          sessionCursorRef.current.set(sKey, {
            cursor: httpCursor,
            lastPolledGlobalCursor,
          });
        } else if (resetCleared) {
          setEventsForSession(sKey, accumulated);
          sessionCursorRef.current.delete(sKey);
        }
        seeded = true;
        setCommandsForSession(sKey, detail.commands);
        setSessionLoading(false);
        if (isTransientRemoteMessage(errorRef.current)) {
          setError("");
        }
        if (detail.binding) applyBindingUpdate(detail.binding);
      } catch (err) {
        if (
          !stopped &&
          !(err instanceof Error && err.message === "request_aborted")
        ) {
          if (
            typeof document !== "undefined" &&
            document.hidden &&
            isTransientRemoteError(err)
          ) {
            // Keep background poll silent when tab is suspended or hidden
          } else {
            setError(remoteError(err));
          }
          setSessionLoading(false);
        }
      } finally {
        inFlight = false;
        if (!stopped) {
          if (again) {
            void poll();
          } else {
            const hasPending = latestDetail?.commands?.some((c) => !c.receipt);
            const nextDelay = hasPending ? 200 : DETAIL_FALLBACK_MS;
            timer = setTimeout(() => void poll(), nextDelay);
          }
        }
      }
    };
    const liveBatcher = createRafBatcher<RemoteEvent>((incoming) => {
      if (stopped) return;
      const merged = mergeMonotonicEvents(accumulated, incoming, httpCursor);
      if (!merged.changed) return;
      accumulated = merged.nextEvents;
      if (merged.nextCursor > sseCursor) sseCursor = merged.nextCursor;
      setEventsForSession(sKey, accumulated);
      setSessionLoading(false);
    });
    refreshSessionDetail.current = () => {
      clearTimeout(timer);
      void poll();
    };
    void poll();
    const unfollow = followContentStream({
      gatewayBase: panelConfig?.gatewayBase ?? "",
      event: "remote",
      onChange: (frames) => {
        const parsed = parseDirectSseBatch(frames, runnerId, hub);
        if (!parsed.relevant) return;
        if (seeded && parsed.events.length > 0) {
          const possibleRewind = parsed.events.some(
            (ev) => ev.cursor <= sseCursor,
          );
          if (!possibleRewind) {
            liveBatcher.push(parsed.events);
            if (parsed.latestBinding) applyBindingUpdate(parsed.latestBinding);
            if (!parsed.needsDetailReconcile) return;
          }
        }
        clearTimeout(timer);
        void poll();
      },
    });
    return () => {
      stopped = true;
      liveBatcher.cancel();
      abortController.abort();
      clearTimeout(timer);
      unfollow();
      refreshSessionDetail.current = () => {};
    };
  }, [requestWithIdentity, selected, panelConfig?.gatewayBase]);

  return {
    snapshot,
    runners,
    helloEntries,
    events,
    commands,
    loading,
    sessionLoading,
    error,
    setError,
    notificationPresentedAt,
    plugins,
    allKnownPlugins,
    discovered,
    refresh,
    refreshSessionDetail,
  };
}
