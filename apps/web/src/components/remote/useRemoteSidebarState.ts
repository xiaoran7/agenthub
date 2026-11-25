import { useEffect, useMemo, useRef, useState } from "react";
import type { AgentPlugin } from "@agent-pulse/contracts";
import { copyTextToClipboard } from "../../lib/clipboard";
import type {
  DiscoveryCandidate,
  RemoteRunner,
  RemoteSnapshot,
} from "../../lib/remote-types";
import {
  isRunnerHelloBlocking,
  type RunnerHelloEntry,
} from "../../lib/runner-hello-sync";
import {
  attentionEntries,
  buildSessionTree,
  getDeviceLabel,
  getProviderInfo,
  getWorkspaceActivityMs,
  groupPendingApprovals,
  type ProviderKind,
  type TreeSessionItem,
} from "../../lib/session-tree";
import { filterDeviceGroups, sessionKey } from "./remote-model-options";

export interface SessionBreadcrumb {
  key: string;
  label: string;
  runnerId?: string;
  provider?: ProviderKind;
  sessionKey?: string;
}

export interface UseRemoteSidebarStateParams {
  snapshot: RemoteSnapshot;
  runners: RemoteRunner[];
  helloEntries: Record<string, RunnerHelloEntry>;
  discovered: Map<string, DiscoveryCandidate>;
  allKnownPlugins: AgentPlugin[];
  selected: string;
  isMobile: boolean;
  mobileDrawerOpen: boolean;
  openMobileDrawer: () => void;
  closeMobileDrawer: (restoreFocus?: boolean) => void;
  settingsOpen: boolean;
  setSettingsOpen: (open: boolean) => void;
}

export function useRemoteSidebarState({
  snapshot,
  runners,
  helloEntries,
  discovered,
  allKnownPlugins,
  selected,
  isMobile,
  mobileDrawerOpen,
  openMobileDrawer,
  closeMobileDrawer,
  settingsOpen,
  setSettingsOpen,
}: UseRemoteSidebarStateParams) {
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    if (typeof window !== "undefined") {
      try {
        const saved = localStorage.getItem("agenthub_remote_sidebar_width");
        if (saved) {
          const parsed = parseInt(saved, 10);
          if (!isNaN(parsed) && parsed >= 180 && parsed <= 520) return parsed;
        }
      } catch {
        // ignore storage errors in restricted contexts
      }
    }
    return 280;
  });
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    if (typeof window !== "undefined") {
      try {
        return (
          localStorage.getItem("agenthub_remote_sidebar_collapsed") === "true"
        );
      } catch {
        // ignore storage errors in restricted contexts
      }
    }
    return false;
  });

  const currentSidebarWidthRef = useRef(sidebarWidth);
  currentSidebarWidthRef.current = sidebarWidth;
  const isResizingRef = useRef(false);
  const cleanResizerListenersRef = useRef<(() => void) | null>(null);
  const mobileDrawerRef = useRef<HTMLElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  const [providerFilter, setProviderFilter] = useState<ProviderKind>("claude");
  const userSelectedProviderRef = useRef(false);
  const [selectedRunnerId, setSelectedRunnerId] = useState<string>("");
  const [deviceMenuOpen, setDeviceMenuOpen] = useState(false);
  const [agentMenuOpen, setAgentMenuOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [sessionSearchQuery, setSessionSearchQuery] = useState("");
  const [copiedSessionId, setCopiedSessionId] = useState<string | null>(null);
  const [collapsedWorkspaces, setCollapsedWorkspaces] = useState<
    Record<string, boolean>
  >({});
  const [workspaceVisibleCounts, setWorkspaceVisibleCounts] = useState<
    Record<string, number>
  >({});

  const deviceMenuRef = useRef<HTMLDivElement>(null);
  const deviceMenuButtonRef = useRef<HTMLButtonElement>(null);
  const agentMenuRef = useRef<HTMLDivElement>(null);
  const agentMenuButtonRef = useRef<HTMLButtonElement>(null);
  const copyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const mountedRef = useRef(true);
  const lastSelectedForSyncRef = useRef<string>("");

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      isResizingRef.current = false;
      cleanResizerListenersRef.current?.();
      if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current);
    };
  }, []);

  useEffect(() => {
    if (!isMobile || !mobileDrawerOpen) return;
    const drawer = mobileDrawerRef.current;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeMobileDrawer(true);
        return;
      }
      if (event.key !== "Tab" || drawer === null) return;
      const focusable = drawer.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      );
      if (focusable.length === 0) return;
      const first = focusable[0]!;
      const last = focusable[focusable.length - 1]!;
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !drawer.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [isMobile, mobileDrawerOpen, closeMobileDrawer]);

  const toggleSidebar = () => {
    setSidebarCollapsed((prev) => {
      const next = !prev;
      try {
        localStorage.setItem("agenthub_remote_sidebar_collapsed", String(next));
      } catch {
        // ignore storage errors in restricted contexts
      }
      return next;
    });
  };

  const handleResizerMouseDown = (e: React.MouseEvent) => {
    e.preventDefault();
    isResizingRef.current = true;
    const startX = e.clientX;
    const startWidth = currentSidebarWidthRef.current;
    let latestWidth = startWidth;

    const onMouseMove = (moveEvent: MouseEvent) => {
      if (!isResizingRef.current) return;
      const delta = moveEvent.clientX - startX;
      latestWidth = Math.min(Math.max(startWidth + delta, 180), 520);
      setSidebarWidth(latestWidth);
    };

    const onMouseUp = () => {
      isResizingRef.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
      cleanResizerListenersRef.current = null;
      try {
        localStorage.setItem(
          "agenthub_remote_sidebar_width",
          String(latestWidth),
        );
      } catch {
        // ignore storage errors in restricted contexts
      }
    };

    cleanResizerListenersRef.current = () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  };

  useEffect(() => {
    const onGlobalKeyDown = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        if (settingsOpen) setSettingsOpen(false);
        if (!isMobile && sidebarCollapsed) toggleSidebar();
        if (isMobile && !mobileDrawerOpen) {
          openMobileDrawer();
        }
        setSearchOpen(true);
        requestAnimationFrame(() => searchInputRef.current?.focus());
        return;
      }
      if (
        (e.ctrlKey || e.metaKey) &&
        e.key.toLowerCase() === "b" &&
        !isMobile
      ) {
        e.preventDefault();
        toggleSidebar();
        return;
      }
      if (e.key === "Escape") {
        if (settingsOpen) {
          setSettingsOpen(false);
          return;
        }
        if (searchOpen || sessionSearchQuery) {
          setSessionSearchQuery("");
          setSearchOpen(false);
        }
      }
    };
    window.addEventListener("keydown", onGlobalKeyDown);
    return () => window.removeEventListener("keydown", onGlobalKeyDown);
  }, [
    settingsOpen,
    searchOpen,
    sessionSearchQuery,
    isMobile,
    sidebarCollapsed,
    mobileDrawerOpen,
    openMobileDrawer,
    setSettingsOpen,
  ]);

  const handleCopySessionId = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    void copyTextToClipboard(id).then((ok) => {
      if (!ok || !mountedRef.current) return;
      setCopiedSessionId(id);
      if (copyTimeoutRef.current) clearTimeout(copyTimeoutRef.current);
      copyTimeoutRef.current = setTimeout(() => {
        if (mountedRef.current) {
          setCopiedSessionId((cur) => (cur === id ? null : cur));
        }
      }, 1500);
    });
  };

  const toggleWorkspaceCollapsed = (workspaceKey: string) => {
    setCollapsedWorkspaces((prev) => ({
      ...prev,
      [workspaceKey]: !prev[workspaceKey],
    }));
  };

  const showMoreInWorkspace = (
    workspaceKey: string,
    initialVisibleCount: number,
    stepCount: number,
  ) => {
    setWorkspaceVisibleCounts((prev) => ({
      ...prev,
      [workspaceKey]: (prev[workspaceKey] ?? initialVisibleCount) + stepCount,
    }));
  };

  const allDeviceGroups = useMemo(
    () =>
      buildSessionTree({
        discovered,
        devices: runners
          .filter((runner) => !runner.revoked)
          .map((runner) => ({
            runner_id: runner.runner_id,
            online: runner.online,
            hello: runner.hello as { device_name?: unknown } | null,
          })),
        activeSessions: snapshot.sessions.map((item) => ({
          runner_id: item.runner_id,
          hub_session_id: item.binding.hub_session_id,
          provider_session_id: item.binding.provider_session_id,
          workspace_path: item.binding.workspace_path,
          title: discovered.get(
            `${item.runner_id}/${item.binding.provider_session_id}`,
          )?.title,
          provider: item.provider,
          online: item.online,
          control_connected: item.binding.control_connected,
          execution_state: item.binding.execution_state,
        })),
        candidates: runners
          .filter((runner) => !runner.revoked)
          .flatMap((runner) =>
            (runner.hello?.discovery?.candidates ?? []).map((candidate) => ({
              runner_id: runner.runner_id,
              provider_session_id: candidate.provider_session_id,
              workspace_path: candidate.workspace_path,
              title: candidate.title,
              provider: candidate.provider,
              resumable: candidate.resumable,
              model: candidate.model,
              total_tokens: candidate.total_tokens,
              last_activity_at_ms: candidate.last_activity_at_ms,
            })),
          ),
        providerFilter: "all",
        plugins: allKnownPlugins,
      }),
    [snapshot.sessions, runners, discovered, allKnownPlugins],
  );

  const deviceGroups = useMemo(() => {
    if (providerFilter === "all") return allDeviceGroups;
    return allDeviceGroups.map((device) => {
      const workspaces = device.workspaces.filter(
        (agent) => agent.provider === providerFilter,
      );
      const totalCount = workspaces.reduce(
        (sum, agent) => sum + agent.totalCount,
        0,
      );
      return { ...device, workspaces, totalCount };
    });
  }, [allDeviceGroups, providerFilter]);

  const filteredDeviceGroups = useMemo(
    () => filterDeviceGroups(deviceGroups, sessionSearchQuery),
    [deviceGroups, sessionSearchQuery],
  );

  const deviceList = useMemo(
    () =>
      runners
        .filter((r) => !r.revoked)
        .map((r) => ({
          runnerId: r.runner_id,
          label: getDeviceLabel(r.runner_id, r.hello),
          online: r.online,
          workspaces: r.hello?.workspaces ?? [],
        }))
        .sort((a, b) => {
          if (a.online !== b.online) return a.online ? -1 : 1;
          return a.label.localeCompare(b.label, "zh-Hans-CN");
        }),
    [runners],
  );

  const activeRunnerId = useMemo(() => {
    if (
      selectedRunnerId &&
      deviceList.some((d) => d.runnerId === selectedRunnerId)
    ) {
      return selectedRunnerId;
    }
    const firstOnline = deviceList.find((d) => d.online);
    if (firstOnline) return firstOnline.runnerId;
    return deviceList[0]?.runnerId ?? "";
  }, [selectedRunnerId, deviceList]);

  const activeDevice = useMemo(
    () => deviceList.find((d) => d.runnerId === activeRunnerId),
    [deviceList, activeRunnerId],
  );

  const activeSnapshotRunner = useMemo(
    () => snapshot.runners.find((r) => r.runner_id === activeRunnerId),
    [snapshot.runners, activeRunnerId],
  );

  const isActiveRunnerHelloPending = isRunnerHelloBlocking(
    activeRunnerId ? helloEntries[activeRunnerId] : undefined,
    activeSnapshotRunner,
  );

  const availableAgentsForCurrentDevice = useMemo(() => {
    if (!activeRunnerId) return [];
    const map = new Map<
      ProviderKind,
      { type: ProviderKind; name: string; count: number }
    >();
    const countedKeys = new Set<string>();

    for (const s of snapshot.sessions) {
      if (s.runner_id !== activeRunnerId) continue;
      const k = `${s.runner_id}/${s.binding.provider_session_id}`;
      countedKeys.add(k);
      const disc = discovered.get(k);
      const info = getProviderInfo(
        s.binding.provider_session_id,
        disc?.provider ?? s.provider,
        allKnownPlugins,
      );
      const existing = map.get(info.type);
      map.set(info.type, {
        type: info.type,
        name: info.name,
        count: (existing?.count ?? 0) + 1,
      });
    }

    const activeRunner = runners.find(
      (r) => r.runner_id === activeRunnerId && !r.revoked,
    );
    for (const c of activeRunner?.hello?.discovery?.candidates ?? []) {
      const k = `${activeRunnerId}/${c.provider_session_id}`;
      if (countedKeys.has(k)) continue;
      countedKeys.add(k);
      const info = getProviderInfo(
        c.provider_session_id,
        c.provider,
        allKnownPlugins,
      );
      const existing = map.get(info.type);
      map.set(info.type, {
        type: info.type,
        name: info.name,
        count: (existing?.count ?? 0) + 1,
      });
    }

    const runnerHello = activeRunner?.hello as
      Record<string, unknown> | null | undefined;
    if (
      runnerHello &&
      typeof runnerHello.provider === "string" &&
      runnerHello.provider.trim()
    ) {
      const info = getProviderInfo(
        "",
        runnerHello.provider.trim(),
        allKnownPlugins,
      );
      if (!map.has(info.type)) {
        map.set(info.type, { type: info.type, name: info.name, count: 0 });
      }
    }

    return Array.from(map.values()).sort((a, b) => {
      if (a.count !== b.count) return b.count - a.count;
      return a.name.localeCompare(b.name, "zh-Hans-CN");
    });
  }, [activeRunnerId, runners, snapshot.sessions, discovered, allKnownPlugins]);

  const currentAgentInfo = useMemo(() => {
    if (!activeRunnerId) {
      return { type: "" as ProviderKind, name: "请先选择电脑", count: 0 };
    }
    if (isActiveRunnerHelloPending) {
      return { type: "" as ProviderKind, name: "正在同步 Agent…", count: 0 };
    }
    const found = availableAgentsForCurrentDevice.find(
      (a) => a.type === providerFilter,
    );
    if (found) return found;
    if (availableAgentsForCurrentDevice.length > 0) {
      return availableAgentsForCurrentDevice[0]!;
    }
    return { type: "" as ProviderKind, name: "暂无可用 Agent", count: 0 };
  }, [
    activeRunnerId,
    isActiveRunnerHelloPending,
    availableAgentsForCurrentDevice,
    providerFilter,
  ]);

  useEffect(() => {
    if (
      !activeRunnerId ||
      isActiveRunnerHelloPending ||
      availableAgentsForCurrentDevice.length === 0
    ) {
      return;
    }
    const exists = availableAgentsForCurrentDevice.some(
      (a) => a.type === providerFilter,
    );
    if (!exists || (!userSelectedProviderRef.current && !selected)) {
      const first = availableAgentsForCurrentDevice[0];
      if (first && first.type !== providerFilter) {
        setProviderFilter(first.type);
      }
    }
  }, [
    activeRunnerId,
    isActiveRunnerHelloPending,
    availableAgentsForCurrentDevice,
    providerFilter,
    selected,
  ]);

  const currentDeviceGroup = useMemo(
    () => filteredDeviceGroups.find((d) => d.runnerId === activeRunnerId),
    [filteredDeviceGroups, activeRunnerId],
  );

  const currentAgentWorkspaces = useMemo(() => {
    const agent = currentDeviceGroup?.workspaces.find(
      (a) => a.provider === providerFilter,
    );
    return agent?.workspaces ?? [];
  }, [currentDeviceGroup, providerFilter]);

  const allWorkspacesForDevice = useMemo(() => {
    const map = new Map<
      string,
      { id: string; label: string; items: TreeSessionItem[] }
    >();
    for (const ws of currentAgentWorkspaces) {
      if (ws.items.length > 0) {
        map.set(ws.id, { id: ws.id, label: ws.label, items: [...ws.items] });
      }
    }
    return Array.from(map.values()).sort((a, b) => {
      const timeA = getWorkspaceActivityMs(a);
      const timeB = getWorkspaceActivityMs(b);
      if (timeA !== timeB) return timeB - timeA;
      return a.label.localeCompare(b.label, "zh-Hans-CN");
    });
  }, [currentAgentWorkspaces]);

  useEffect(() => {
    if (!deviceMenuOpen && !agentMenuOpen) return;
    const onClickOutside = (e: MouseEvent) => {
      if (
        deviceMenuOpen &&
        deviceMenuRef.current &&
        !deviceMenuRef.current.contains(e.target as Node)
      ) {
        setDeviceMenuOpen(false);
      }
      if (
        agentMenuOpen &&
        agentMenuRef.current &&
        !agentMenuRef.current.contains(e.target as Node)
      ) {
        setAgentMenuOpen(false);
      }
    };
    window.addEventListener("pointerdown", onClickOutside);
    return () => window.removeEventListener("pointerdown", onClickOutside);
  }, [deviceMenuOpen, agentMenuOpen]);

  useEffect(() => {
    if (!deviceMenuOpen && !agentMenuOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      if (deviceMenuOpen) {
        setDeviceMenuOpen(false);
        deviceMenuButtonRef.current?.focus();
      }
      if (agentMenuOpen) {
        setAgentMenuOpen(false);
        agentMenuButtonRef.current?.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [deviceMenuOpen, agentMenuOpen]);

  useEffect(() => {
    if (!selected) {
      lastSelectedForSyncRef.current = "";
      return;
    }
    if (selected === lastSelectedForSyncRef.current) return;
    lastSelectedForSyncRef.current = selected;

    const s = snapshot.sessions.find((entry) => sessionKey(entry) === selected);
    if (s && s.runner_id && s.runner_id !== selectedRunnerId) {
      setSelectedRunnerId(s.runner_id);
    }
    if (s) {
      const disc = discovered.get(
        `${s.runner_id}/${s.binding.provider_session_id}`,
      );
      const info = getProviderInfo(
        s.binding.provider_session_id,
        disc?.provider ?? s.provider,
        allKnownPlugins,
      );
      if (info.type) {
        setProviderFilter(info.type);
      }
    }
  }, [
    selected,
    snapshot.sessions,
    selectedRunnerId,
    discovered,
    allKnownPlugins,
  ]);

  const pendingApprovalsBySessionKey = useMemo(
    () => groupPendingApprovals(runners),
    [runners],
  );

  const pendingApprovalCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const [k, list] of pendingApprovalsBySessionKey) {
      counts.set(k, list.length);
    }
    return counts;
  }, [pendingApprovalsBySessionKey]);

  const attention = useMemo(
    () => attentionEntries(allDeviceGroups, pendingApprovalsBySessionKey),
    [allDeviceGroups, pendingApprovalsBySessionKey],
  );

  const selectedSessionPath = useMemo<SessionBreadcrumb[] | undefined>(() => {
    if (!selected) return undefined;
    for (const device of allDeviceGroups) {
      for (const agent of device.workspaces) {
        for (const ws of agent.workspaces) {
          for (const item of ws.items) {
            if (item.kind !== "active" || item.sessionKey !== selected) {
              continue;
            }
            return [
              {
                key: device.runnerId,
                label: device.label,
                runnerId: device.runnerId,
              },
              {
                key: `${device.runnerId}/${agent.provider}`,
                label: agent.label,
                runnerId: device.runnerId,
                provider: agent.provider,
              },
              {
                key: `${device.runnerId}/${agent.provider}/${ws.id}`,
                label: ws.label,
                runnerId: device.runnerId,
                provider: agent.provider,
              },
              { key: item.key, label: item.title, sessionKey: selected },
            ];
          }
        }
      }
    }
    return undefined;
  }, [allDeviceGroups, selected]);

  return {
    sidebarWidth,
    sidebarCollapsed,
    toggleSidebar,
    handleResizerMouseDown,
    mobileDrawerRef,
    searchInputRef,
    providerFilter,
    setProviderFilter,
    userSelectedProviderRef,
    setSelectedRunnerId,
    deviceMenuOpen,
    setDeviceMenuOpen,
    agentMenuOpen,
    setAgentMenuOpen,
    deviceMenuRef,
    deviceMenuButtonRef,
    agentMenuRef,
    agentMenuButtonRef,
    searchOpen,
    setSearchOpen,
    sessionSearchQuery,
    setSessionSearchQuery,
    copiedSessionId,
    handleCopySessionId,
    collapsedWorkspaces,
    toggleWorkspaceCollapsed,
    workspaceVisibleCounts,
    showMoreInWorkspace,
    deviceList,
    activeRunnerId,
    activeDevice,
    isActiveRunnerHelloPending,
    availableAgentsForCurrentDevice,
    currentAgentInfo,
    allWorkspacesForDevice,
    pendingApprovalsBySessionKey,
    pendingApprovalCounts,
    attention,
    selectedSessionPath,
  };
}
