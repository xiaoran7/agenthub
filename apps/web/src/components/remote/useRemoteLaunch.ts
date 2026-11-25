import { useEffect, useMemo, useState, type MutableRefObject } from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import type { AgentPlugin } from "@agent-pulse/contracts";
import { safeRandomUUID } from "../../lib/uuid";
import {
  remoteError,
  type RemotePermissionMode,
  type RemoteRequest,
  type RemoteRunner,
  type RemoteSnapshot,
} from "../../lib/remote";
import type { ProviderKind } from "../../lib/session-tree";
import { CORE_LAUNCH_AGENTS } from "../remote-console-constants";
import type { LaunchAgentType } from "../RemoteLaunchModal";
import {
  buildLaunchModelOptions,
  buildLaunchSessionOptions,
  buildLaunchWorkspaceOptions,
  buildOnlineDeviceOptions,
  isCodexThreadId,
} from "./remote-model-options";

export interface UseRemoteLaunchParams {
  snapshot: RemoteSnapshot;
  runners: RemoteRunner[];
  plugins: AgentPlugin[];
  runner: string;
  setRunner: (id: string) => void;
  launchModalOpen: boolean;
  setLaunchModalOpen: (open: boolean) => void;
  activeRunnerId: string;
  providerFilter: ProviderKind;
  isMobile: boolean;
  attempts: MutableRefObject<Map<string, RemoteCommand>>;
  requestWithIdentity: RemoteRequest;
  refresh: MutableRefObject<() => void>;
  refreshSessionDetail: MutableRefObject<() => void>;
  setError: (msg: string) => void;
  setSelected: (key: string) => void;
  setSelectedRunnerId: (id: string) => void;
  setProviderFilter: (provider: ProviderKind) => void;
  userSelectedProviderRef: MutableRefObject<boolean>;
  setPermissionMode: (mode: RemotePermissionMode) => void;
  setPermissionModeTouched: (touched: boolean) => void;
  pendingLaunchPermissionRef: MutableRefObject<{
    sessionKey: string;
    mode: RemotePermissionMode;
  } | null>;
  setMobileDrawerOpen: (open: boolean) => void;
}

export function useRemoteLaunch({
  snapshot,
  runners,
  plugins,
  runner,
  setRunner,
  launchModalOpen,
  setLaunchModalOpen,
  activeRunnerId,
  providerFilter,
  isMobile,
  attempts,
  requestWithIdentity,
  refresh,
  refreshSessionDetail,
  setError,
  setSelected,
  setSelectedRunnerId,
  setProviderFilter,
  userSelectedProviderRef,
  setPermissionMode,
  setPermissionModeTouched,
  pendingLaunchPermissionRef,
  setMobileDrawerOpen,
}: UseRemoteLaunchParams) {
  const [workspace, setWorkspace] = useState("");
  const [agentType, setAgentType] = useState<LaunchAgentType>("codex");
  const [model, setModel] = useState("");
  const [sandbox, setSandbox] = useState<"read-only" | "workspace-write">(
    "read-only",
  );
  const [launchPermissionMode, setLaunchPermissionMode] =
    useState<RemotePermissionMode>("default");
  const [nativeId, setNativeId] = useState("");
  const [launchBusy, setLaunchBusy] = useState(false);
  const [resumingCandidateKey, setResumingCandidateKey] = useState<
    string | null
  >(null);

  const runnerInfo = runners.find((item) => item.runner_id === runner);

  const availableLaunchAgents = useMemo(() => {
    const list: Array<{
      value: string;
      label: string;
      catalog: readonly string[];
    }> = [...CORE_LAUNCH_AGENTS];
    const seen = new Set<string>(list.map((item) => item.value));
    for (const plugin of plugins) {
      if (
        plugin.installed !== false &&
        plugin.enabled &&
        plugin.available !== false &&
        plugin.capabilities?.launch !== false &&
        !seen.has(plugin.id)
      ) {
        seen.add(plugin.id);
        list.push({
          value: plugin.id,
          label: plugin.displayName || plugin.name || plugin.id,
          catalog: [plugin.id, ...(plugin.aliases ?? [])],
        });
      }
    }
    return list;
  }, [plugins]);

  const launchAgentOptions = useMemo(
    () => availableLaunchAgents.map(({ value, label }) => ({ value, label })),
    [availableLaunchAgents],
  );

  useEffect(() => {
    if (!availableLaunchAgents.some((a) => a.value === agentType)) {
      setAgentType(availableLaunchAgents[0]?.value ?? "codex");
    }
  }, [availableLaunchAgents, agentType]);

  const onlineDeviceOptions = useMemo(
    () => buildOnlineDeviceOptions(runners),
    [runners],
  );
  const launchWorkspaceOptions = useMemo(
    () => buildLaunchWorkspaceOptions(runnerInfo),
    [runnerInfo],
  );
  const launchSessionOptions = useMemo(
    () => buildLaunchSessionOptions(runnerInfo, workspace, plugins),
    [runnerInfo, workspace, plugins],
  );
  const launchModelOptions = useMemo(
    () => buildLaunchModelOptions(runnerInfo, agentType, availableLaunchAgents),
    [runnerInfo, agentType, availableLaunchAgents],
  );

  const openLaunchModal = (preferredWorkspace?: string) => {
    const available = runners.find(
      (item) => !item.revoked && item.online && item.hello?.workspaces.length,
    );
    const nextRunnerId =
      activeRunnerId || available?.runner_id || runners[0]?.runner_id || "";
    const rInfo = runners.find(
      (r) => !r.revoked && r.runner_id === nextRunnerId,
    );
    const authorized = rInfo?.hello?.workspaces ?? [];
    const nextWorkspace =
      preferredWorkspace && authorized.includes(preferredWorkspace)
        ? preferredWorkspace
        : (authorized[0] ?? "");
    const mappedAgent: LaunchAgentType =
      providerFilter === "claude" || providerFilter === "claude_desktop"
        ? "claude_code"
        : providerFilter === "antigravity"
          ? "antigravity"
          : providerFilter !== "all" && providerFilter !== "codex"
            ? providerFilter
            : "codex";
    if (availableLaunchAgents.some((a) => a.value === mappedAgent)) {
      setAgentType(mappedAgent);
    } else if (availableLaunchAgents[0]) {
      setAgentType(availableLaunchAgents[0].value as LaunchAgentType);
    }
    setRunner(nextRunnerId);
    setWorkspace(nextWorkspace);
    setNativeId("");
    setModel("");
    if (isMobile) setMobileDrawerOpen(false);
    setLaunchModalOpen(true);
  };

  const launch = async () => {
    const launchRunner = runners.find((item) => item.runner_id === runner);
    const generation = launchRunner?.hello?.host_generation;
    if (!runner || !workspace || !generation) return;
    setLaunchBusy(true);
    setError("");
    const effectiveSandbox =
      !nativeId && agentType !== "codex"
        ? launchPermissionMode === "plan"
          ? "read-only"
          : "workspace-write"
        : sandbox;
    const attemptKey = JSON.stringify([
      "launch",
      runner,
      workspace,
      nativeId,
      agentType,
      effectiveSandbox,
      launchPermissionMode,
      model,
      generation,
    ]);
    const existingSession = nativeId
      ? snapshot.sessions.find(
          (s) =>
            s.runner_id === runner &&
            s.binding.provider_session_id === nativeId,
        )
      : undefined;
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id:
          existingSession?.binding.hub_session_id ?? safeRandomUUID(),
        host_generation: generation,
        command: {
          action: "connect",
          mode: nativeId
            ? launchRunner?.codex_session_control &&
              isCodexThreadId(nativeId.trim())
              ? "take_control"
              : "resume"
            : "create",
          workspace_path: workspace,
          sandbox: effectiveSandbox,
          approval_policy:
            launchPermissionMode === "bypassPermissions"
              ? "never"
              : launchPermissionMode === "acceptEdits"
                ? "on-request"
                : "untrusted",
          ...(launchPermissionMode !== "default"
            ? { permission_mode: launchPermissionMode }
            : {}),
          ...(model ? { model } : {}),
          ...(nativeId
            ? { provider_session_id: nativeId }
            : { agent_type: agentType }),
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await requestWithIdentity(
        `/runners/${encodeURIComponent(runner)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      setSelectedRunnerId(runner);
      if (!nativeId) {
        const nextProvider: ProviderKind =
          agentType === "claude_code"
            ? "claude"
            : agentType === "antigravity"
              ? "antigravity"
              : agentType === "codex"
                ? "codex"
                : agentType;
        userSelectedProviderRef.current = true;
        setProviderFilter(nextProvider);
      }
      const launchedKey = `${runner}/${command.hub_session_id}`;
      pendingLaunchPermissionRef.current = {
        sessionKey: launchedKey,
        mode: launchPermissionMode,
      };
      setPermissionMode(launchPermissionMode);
      setPermissionModeTouched(launchPermissionMode !== "default");
      setSelected(launchedKey);
      setLaunchModalOpen(false);
      if (isMobile) setMobileDrawerOpen(false);
      refresh.current();
      refreshSessionDetail.current();
    } catch (err) {
      if (!(err instanceof Error) || err.name === "RemoteRequestRejected") {
        attempts.current.delete(attemptKey);
      }
      setError(remoteError(err));
    } finally {
      setLaunchBusy(false);
    }
  };

  const resumeCandidate = async (
    targetRunnerId: string,
    targetWorkspace: string,
    providerSessionId: string,
  ) => {
    const runnerObj = runners.find((item) => item.runner_id === targetRunnerId);
    const generation = runnerObj?.hello?.host_generation;
    if (!runnerObj || !generation || launchBusy) return;
    const candidateKey = `${targetRunnerId}/${providerSessionId}`;
    setResumingCandidateKey(candidateKey);
    setLaunchBusy(true);
    setError("");
    const attemptKey = JSON.stringify([
      "launch",
      targetRunnerId,
      targetWorkspace,
      providerSessionId,
      "read-only",
      "",
      generation,
    ]);
    const existingSession = snapshot.sessions.find(
      (s) =>
        s.runner_id === targetRunnerId &&
        s.binding.provider_session_id === providerSessionId,
    );
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: safeRandomUUID(),
        hub_session_id:
          existingSession?.binding.hub_session_id ??
          (isCodexThreadId(providerSessionId)
            ? providerSessionId
            : safeRandomUUID()),
        host_generation: generation,
        command: {
          action: "connect",
          mode:
            runnerObj.codex_session_control &&
            isCodexThreadId(providerSessionId)
              ? "preview"
              : "resume",
          workspace_path: targetWorkspace,
          sandbox: "read-only",
          approval_policy: "untrusted",
          provider_session_id: providerSessionId,
        },
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);
    try {
      await requestWithIdentity(
        `/runners/${encodeURIComponent(targetRunnerId)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      setSelected(`${targetRunnerId}/${command.hub_session_id}`);
      refresh.current();
      refreshSessionDetail.current();
    } catch (err) {
      if (!(err instanceof Error) || err.name === "RemoteRequestRejected") {
        attempts.current.delete(attemptKey);
      }
      setError(remoteError(err));
    } finally {
      setLaunchBusy(false);
      setResumingCandidateKey(null);
    }
  };

  return {
    workspace,
    setWorkspace,
    agentType,
    setAgentType,
    model,
    setModel,
    sandbox,
    setSandbox,
    launchPermissionMode,
    setLaunchPermissionMode,
    nativeId,
    setNativeId,
    launchModalOpen,
    launchBusy,
    resumingCandidateKey,
    launchAgentOptions,
    onlineDeviceOptions,
    launchWorkspaceOptions,
    launchSessionOptions,
    launchModelOptions,
    openLaunchModal,
    launch,
    resumeCandidate,
  };
}
