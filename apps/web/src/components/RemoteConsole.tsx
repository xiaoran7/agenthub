import { Suspense, lazy, useEffect, useMemo, useRef, useState } from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import type { AccountView } from "../lib/account";
import type { PanelConfig } from "../lib/config";
import { loadConfig, saveConfig } from "../lib/config";
import {
  cleanBannerText,
  conversationItems,
  groupConversationItems,
  latestTurnFailure,
  pendingApprovals,
  pendingQuestions,
  pruneOptimisticPrompts,
  remoteRequest,
  settledConversationItems,
  type ConversationItem,
  type RemotePermissionMode,
  type RemoteRequest,
} from "../lib/remote";
import {
  PERMISSION_MODE_OPTIONS,
  SANDBOX_OPTIONS,
} from "./remote-console-constants";
import { RemoteLaunchModal } from "./RemoteLaunchModal";
import { useMobileConsoleLayout } from "./useMobileConsoleLayout";
import {
  buildSessionModelOptions,
  isCodexThreadId,
  sessionKey,
} from "./remote/remote-model-options";
import { RemoteComposer } from "./remote/RemoteComposer";
import { RemoteHeader } from "./remote/RemoteHeader";
import { RemoteSidebar } from "./remote/RemoteSidebar";
import { RemoteTimeline } from "./remote/RemoteTimeline";
import { RemoteWelcome } from "./remote/RemoteWelcome";
import { useRemoteCommandDispatch } from "./remote/useRemoteCommandDispatch";
import { useRemoteComposer } from "./remote/useRemoteComposer";
import { useRemoteLaunch } from "./remote/useRemoteLaunch";
import { useRemoteSidebarState } from "./remote/useRemoteSidebarState";
import { useRemoteSync } from "./remote/useRemoteSync";
import "./remote.css";

const SettingsPage = lazy(() =>
  import("./SettingsPage").then((m) => ({ default: m.SettingsPage })),
);

export function RemoteConsole({
  request = remoteRequest,
  onAuthenticationRequired,
  account,
  onAccountUpdated,
  onLogout,
  onLoggedOut,
  config,
}: {
  request?: RemoteRequest;
  onAuthenticationRequired?: () => void;
  account?: AccountView | null;
  onAccountUpdated?: (next: AccountView) => void;
  onLogout?: () => void;
  /** Fired when Settings revokes the current session, so the page can sign out. */
  onLoggedOut?: () => void;
  /** Gateway config the settings view reads and writes. */
  config?: PanelConfig | undefined;
}) {
  const [selected, setSelected] = useState("");
  const [runner, setRunner] = useState("");
  const [launchModalOpen, setLaunchModalOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [panelConfig, setPanelConfig] = useState<PanelConfig | undefined>(
    config,
  );
  const [permissionMode, setPermissionMode] =
    useState<RemotePermissionMode>("default");
  const [permissionModeTouched, setPermissionModeTouched] = useState(false);
  const [followBottom, setFollowBottom] = useState(true);
  const [dismissedFailure, setDismissedFailure] = useState<string | null>(null);

  const attempts = useRef(new Map<string, RemoteCommand>());
  const pendingLaunchPermissionRef = useRef<{
    sessionKey: string;
    mode: RemotePermissionMode;
  } | null>(null);
  const timelineRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setPanelConfig(
      config !== undefined ? config : loadConfig(window.localStorage),
    );
  }, [config]);

  useEffect(() => {
    setFollowBottom(true);
    setDismissedFailure(null);
  }, [selected]);

  const requestWithIdentity = useMemo<RemoteRequest>(
    () =>
      (path, body, timeoutMs = 25000, _userId?: string, signal?: AbortSignal) =>
        request(
          path,
          body,
          timeoutMs,
          account?.user.user_id ?? panelConfig?.userId,
          signal,
        ),
    [request, account?.user.user_id, panelConfig?.userId],
  );

  const mobile = useMobileConsoleLayout();
  const sync = useRemoteSync({
    requestWithIdentity,
    selected,
    setSelected,
    onAuthenticationRequired,
    panelConfig,
    account,
    settingsOpen,
    launchRunner: runner,
  });

  const sidebar = useRemoteSidebarState({
    snapshot: sync.snapshot,
    runners: sync.runners,
    helloEntries: sync.helloEntries,
    discovered: sync.discovered,
    allKnownPlugins: sync.allKnownPlugins,
    selected,
    isMobile: mobile.isMobile,
    mobileDrawerOpen: mobile.mobileDrawerOpen,
    openMobileDrawer: mobile.openMobileDrawer,
    closeMobileDrawer: mobile.closeMobileDrawer,
    settingsOpen,
    setSettingsOpen,
  });

  const launchState = useRemoteLaunch({
    snapshot: sync.snapshot,
    runners: sync.runners,
    plugins: sync.plugins,
    runner,
    setRunner,
    launchModalOpen,
    setLaunchModalOpen,
    activeRunnerId: sidebar.activeRunnerId,
    providerFilter: sidebar.providerFilter,
    isMobile: mobile.isMobile,
    attempts,
    requestWithIdentity,
    refresh: sync.refresh,
    refreshSessionDetail: sync.refreshSessionDetail,
    setError: sync.setError,
    setSelected,
    setSelectedRunnerId: sidebar.setSelectedRunnerId,
    setProviderFilter: sidebar.setProviderFilter,
    userSelectedProviderRef: sidebar.userSelectedProviderRef,
    setPermissionMode,
    setPermissionModeTouched,
    pendingLaunchPermissionRef,
    setMobileDrawerOpen: mobile.setMobileDrawerOpen,
  });

  const session = sync.snapshot.sessions.find(
    (item) => sessionKey(item) === selected,
  );
  const connected = !!session?.online && !!session.binding.control_connected;
  const supportsCodexControl = (runnerId: string) =>
    sync.runners.find((item) => item.runner_id === runnerId)
      ?.codex_session_control === true;
  const codexSession = Boolean(
    session &&
    isCodexThreadId(session.binding.provider_session_id) &&
    supportsCodexControl(session.runner_id),
  );
  const releasingControl = Boolean(
    session?.online && session.binding.execution_state === "releasing_control",
  );
  const canTakeControl = Boolean(
    codexSession && session?.online && !connected && !releasingControl,
  );

  const activeTurnId = session?.binding.current_turn_id ?? null;
  const items = useMemo(
    () =>
      conversationItems(
        sync.events,
        sync.commands,
        `${selected}/`,
        activeTurnId,
      ),
    [sync.events, sync.commands, selected, activeTurnId],
  );
  const settledItems = useMemo(
    () =>
      settledConversationItems(items, sync.events, sync.commands, activeTurnId),
    [items, sync.events, sync.commands, activeTurnId],
  );
  const approvals = useMemo(
    () => pendingApprovals(sync.events, session?.binding.hub_session_id ?? ""),
    [sync.events, session?.binding.hub_session_id],
  );
  const questions = useMemo(
    () => pendingQuestions(sync.events, session?.binding.hub_session_id ?? ""),
    [sync.events, session?.binding.hub_session_id],
  );
  const turnFailure = useMemo(
    () => latestTurnFailure(sync.events, session?.binding.hub_session_id ?? ""),
    [sync.events, session?.binding.hub_session_id],
  );
  const hasInFlightPrompt = useMemo(
    () =>
      sync.commands.some(
        (c) => c.command.command.action === "send_prompt" && c.receipt === null,
      ),
    [sync.commands],
  );
  const baseTurnExecuting = Boolean(
    session &&
    connected &&
    (session.binding.execution_state === "running" ||
      Boolean(session.binding.current_turn_id) ||
      hasInFlightPrompt),
  );

  const composerCallbacksRef = useRef({
    setTakeoverDrafts: (() => {}) as React.Dispatch<
      React.SetStateAction<
        Record<string, { id: string; text: string; attachments: never[] }>
      >
    >,
    forgetTakeoverDraft: (_sKey: string) => {},
    updateOptimisticForSession: (
      _sKey: string,
      _updater: (cur: never[]) => never[],
    ) => {},
    restoreRollbackDraft: (
      _sKey: string,
      _draft: { text: string; attachments: never[] },
    ) => {},
  });

  const dispatch = useRemoteCommandDispatch({
    selected,
    session,
    runners: sync.runners,
    commands: sync.commands,
    codexSession,
    sessionLoading: sync.sessionLoading,
    itemsCount: items.length,
    launchBusy: launchState.launchBusy,
    attempts,
    requestWithIdentity,
    refresh: sync.refresh,
    refreshSessionDetail: sync.refreshSessionDetail,
    setError: sync.setError,
    setDismissedFailure,
    supportsCodexControl,
    composerCallbacksRef: composerCallbacksRef as never,
  });

  const composer = useRemoteComposer({
    selected,
    session,
    runners: sync.runners,
    commands: sync.commands,
    settledItems,
    connected,
    canTakeControl,
    busy: dispatch.busy,
    baseTurnExecuting,
    turnFailure,
    permissionMode,
    setPermissionMode,
    permissionModeTouched,
    setPermissionModeTouched,
    pendingLaunchPermissionRef,
    submitRef: dispatch.submitRef,
    setError: sync.setError,
    setFollowBottom,
    setDismissedFailure,
  });
  composerCallbacksRef.current = {
    setTakeoverDrafts: composer.setTakeoverDrafts as never,
    forgetTakeoverDraft: composer.forgetTakeoverDraft,
    updateOptimisticForSession: composer.updateOptimisticForSession as never,
    restoreRollbackDraft: composer.restoreRollbackDraft as never,
  };

  const effectiveItems = useMemo(() => {
    if (composer.optimisticPrompts.length === 0) return items;
    const { visible } = pruneOptimisticPrompts(
      composer.optimisticPrompts,
      settledItems,
      Date.now(),
    );
    const renderedIds = new Set(items.map((item) => item.id));
    const rejectedIds = new Set(
      sync.commands
        .filter((c) => c.receipt && c.receipt.status !== "accepted")
        .map((c) => c.command.client_cmd_id),
    );
    const pendingVisible = visible.filter(
      (opt) => !renderedIds.has(opt.id) && !rejectedIds.has(opt.id),
    );
    if (pendingVisible.length === 0) return items;
    const optItems: ConversationItem[] = pendingVisible.map((opt) => ({
      id: opt.id,
      kind: "user",
      text: opt.text,
      status: "running",
    }));
    return [...items, ...optItems];
  }, [items, settledItems, sync.commands, composer.optimisticPrompts]);

  const itemGroups = useMemo(
    () => groupConversationItems(effectiveItems),
    [effectiveItems],
  );
  const sessionModelOptions = useMemo(
    () =>
      buildSessionModelOptions(
        session,
        sync.runners,
        sync.discovered,
        sync.allKnownPlugins,
      ),
    [session, sync.runners, sync.discovered, sync.allKnownPlugins],
  );

  const lastUserIndex = effectiveItems.findLastIndex(
    (item) => item.kind === "user",
  );
  const isWaitingFirstAssistant = Boolean(
    (composer.optimisticPrompts.length > 0 || composer.isTurnExecuting) &&
    lastUserIndex >= 0 &&
    !effectiveItems
      .slice(lastUserIndex + 1)
      .some((item) => item.kind === "assistant") &&
    approvals.length === 0 &&
    questions.length === 0,
  );

  useEffect(() => {
    const pane = timelineRef.current;
    if (pane && followBottom) pane.scrollTop = pane.scrollHeight;
  }, [effectiveItems, composer.isTurnExecuting, turnFailure, followBottom]);

  const lastCompletedCommand = [...sync.commands]
    .reverse()
    .find((item) => item.receipt !== null);
  const latestFailure =
    lastCompletedCommand &&
    ["rejected", "indeterminate"].includes(
      lastCompletedCommand.receipt?.status ?? "",
    ) &&
    !["session_already_bound", "native_session_already_bound"].includes(
      lastCompletedCommand.receipt?.code ?? "",
    )
      ? lastCompletedCommand
      : undefined;

  return (
    <div className="remote-console" ref={mobile.consoleRootRef}>
      <RemoteSidebar
        isMobile={mobile.isMobile}
        mobileDrawerOpen={mobile.mobileDrawerOpen}
        drawerAnimated={mobile.drawerAnimated}
        sidebarCollapsed={sidebar.sidebarCollapsed}
        sidebarWidth={sidebar.sidebarWidth}
        mobileDrawerRef={sidebar.mobileDrawerRef}
        searchInputRef={sidebar.searchInputRef}
        deviceMenuRef={sidebar.deviceMenuRef}
        deviceMenuButtonRef={sidebar.deviceMenuButtonRef}
        agentMenuRef={sidebar.agentMenuRef}
        agentMenuButtonRef={sidebar.agentMenuButtonRef}
        onSidebarTouchStart={mobile.onSidebarTouchStart}
        onSidebarTouchMove={mobile.onSidebarTouchMove}
        onSidebarTouchEnd={mobile.onSidebarTouchEnd}
        onCloseMobileDrawer={mobile.closeMobileDrawer}
        onToggleSidebar={sidebar.toggleSidebar}
        onResizerMouseDown={sidebar.handleResizerMouseDown}
        searchOpen={sidebar.searchOpen}
        setSearchOpen={sidebar.setSearchOpen}
        sessionSearchQuery={sidebar.sessionSearchQuery}
        setSessionSearchQuery={sidebar.setSessionSearchQuery}
        attention={sidebar.attention}
        pendingApprovalsBySessionKey={sidebar.pendingApprovalsBySessionKey}
        submittingApproval={dispatch.submittingApproval}
        onOpenAttention={(entry) => {
          sidebar.setSelectedRunnerId(entry.runnerId);
          setSelected(entry.sessionKey);
          sync.setError("");
          if (mobile.isMobile) mobile.setMobileDrawerOpen(false);
        }}
        onResolveAttentionApproval={(entry, approval, approved) => {
          const target = sync.snapshot.sessions.find(
            (item) => sessionKey(item) === entry.sessionKey,
          );
          if (target === undefined) {
            sync.setError("该会话已不在当前快照中，请刷新后重试");
            return;
          }
          void dispatch.submit(
            {
              action: "resolve_approval",
              approval_id: approval.approval_id,
              approved,
            },
            target,
          );
        }}
        deviceList={sidebar.deviceList}
        activeRunnerId={sidebar.activeRunnerId}
        activeDevice={sidebar.activeDevice}
        deviceMenuOpen={sidebar.deviceMenuOpen}
        setDeviceMenuOpen={sidebar.setDeviceMenuOpen}
        agentMenuOpen={sidebar.agentMenuOpen}
        setAgentMenuOpen={sidebar.setAgentMenuOpen}
        isActiveRunnerHelloPending={sidebar.isActiveRunnerHelloPending}
        availableAgentsForCurrentDevice={
          sidebar.availableAgentsForCurrentDevice
        }
        currentAgentInfo={sidebar.currentAgentInfo}
        providerFilter={sidebar.providerFilter}
        onSelectDevice={(runnerId) => {
          if (runnerId !== sidebar.activeRunnerId) {
            setSelected("");
            sidebar.userSelectedProviderRef.current = false;
          }
          sidebar.setSelectedRunnerId(runnerId);
          sidebar.setDeviceMenuOpen(false);
        }}
        onSelectAgent={(nextAgentType) => {
          if (nextAgentType !== sidebar.providerFilter) setSelected("");
          sidebar.userSelectedProviderRef.current = true;
          sidebar.setProviderFilter(nextAgentType);
          sidebar.setAgentMenuOpen(false);
        }}
        allWorkspacesForDevice={sidebar.allWorkspacesForDevice}
        sessions={sync.snapshot.sessions}
        runners={sync.runners}
        selected={selected}
        busy={dispatch.busy}
        resumingCandidateKey={launchState.resumingCandidateKey}
        copiedSessionId={sidebar.copiedSessionId}
        collapsedWorkspaces={sidebar.collapsedWorkspaces}
        workspaceVisibleCounts={sidebar.workspaceVisibleCounts}
        pendingApprovalCounts={sidebar.pendingApprovalCounts}
        supportsCodexControl={supportsCodexControl}
        onToggleWorkspace={sidebar.toggleWorkspaceCollapsed}
        onShowMore={sidebar.showMoreInWorkspace}
        onOpenLaunchModal={launchState.openLaunchModal}
        onSelectActiveSession={(sKey) => {
          if (mobile.isMobile) mobile.setMobileDrawerOpen(false);
          if (sKey !== selected) {
            setSelected(sKey);
            sync.setError("");
          }
        }}
        onResumeCandidate={(rId, wsId, providerSid) => {
          if (mobile.isMobile) mobile.setMobileDrawerOpen(false);
          void launchState.resumeCandidate(rId, wsId, providerSid);
        }}
        onCopySessionId={sidebar.handleCopySessionId}
        account={account}
        onOpenSettings={() => {
          if (mobile.isMobile) mobile.setMobileDrawerOpen(false);
          setSettingsOpen(true);
        }}
        onLogout={onLogout}
      />

      <main
        className="remote-main"
        inert={mobile.isMobile && mobile.mobileDrawerOpen ? true : undefined}
        onTouchStart={mobile.onMainTouchStart}
        onTouchMove={mobile.onMainTouchMove}
        onTouchEnd={mobile.onMainTouchEnd}
      >
        <RemoteHeader
          isMobile={mobile.isMobile}
          mobileDrawerOpen={mobile.mobileDrawerOpen}
          sidebarCollapsed={sidebar.sidebarCollapsed}
          mobileMenuBtnRef={mobile.mobileMenuBtnRef}
          attention={sidebar.attention}
          selected={selected}
          selectedSessionPath={sidebar.selectedSessionPath}
          session={session}
          discovered={sync.discovered}
          allKnownPlugins={sync.allKnownPlugins}
          releasingControl={releasingControl}
          sessionLoading={sync.sessionLoading}
          itemsCount={items.length}
          connected={connected}
          copiedSessionId={sidebar.copiedSessionId}
          onOpenMobileDrawer={mobile.openMobileDrawer}
          onToggleSidebar={sidebar.toggleSidebar}
          onSelectSession={setSelected}
          onSelectRunnerAndProvider={(runnerId, provider) => {
            if (runnerId !== undefined) sidebar.setSelectedRunnerId(runnerId);
            if (provider !== undefined) {
              sidebar.userSelectedProviderRef.current = true;
              sidebar.setProviderFilter(provider);
            }
          }}
          onCloseMobileDrawer={() => mobile.setMobileDrawerOpen(false)}
          onCopySessionId={sidebar.handleCopySessionId}
          onNewSession={launchState.openLaunchModal}
          onOpenAttentionSingle={(entry) => {
            sidebar.setSelectedRunnerId(entry.runnerId);
            setSelected(entry.sessionKey);
            sync.setError("");
          }}
        />

        {sync.error && (
          <div role="alert" className="remote-notice">
            {cleanBannerText(sync.error)}
            <button aria-label="关闭提示" onClick={() => sync.setError("")}>
              ×
            </button>
          </div>
        )}

        {!selected ? (
          <RemoteWelcome
            loading={sync.loading}
            isMobile={mobile.isMobile}
            onNewSession={() => launchState.openLaunchModal()}
            onOpenSettings={() => setSettingsOpen(true)}
          />
        ) : (
          <div key={selected} className="remote-session-view remote-view-enter">
            <RemoteTimeline
              timelineRef={timelineRef}
              session={session}
              discovered={sync.discovered}
              allKnownPlugins={sync.allKnownPlugins}
              connected={connected}
              codexSession={codexSession}
              releasingControl={releasingControl}
              busy={dispatch.busy}
              sessionLoading={sync.sessionLoading}
              itemsCount={items.length}
              effectiveItems={effectiveItems}
              itemGroups={itemGroups}
              isTurnExecuting={composer.isTurnExecuting}
              isWaitingFirstAssistant={isWaitingFirstAssistant}
              optimisticPrompts={composer.optimisticPrompts}
              hasInFlightPrompt={hasInFlightPrompt}
              turnFailure={turnFailure}
              latestFailure={latestFailure}
              dismissedFailure={dismissedFailure}
              approvals={approvals}
              questions={questions}
              submittingApproval={dispatch.submittingApproval}
              submittingQuestionId={dispatch.submittingQuestionId}
              followBottom={followBottom}
              onScroll={() => {
                const pane = timelineRef.current;
                if (!pane) return;
                setFollowBottom(
                  pane.scrollHeight - pane.scrollTop - pane.clientHeight < 80,
                );
              }}
              onScrollToBottom={() => {
                const pane = timelineRef.current;
                if (pane)
                  pane.scrollTo({ top: pane.scrollHeight, behavior: "smooth" });
                setFollowBottom(true);
              }}
              onDismissFailure={setDismissedFailure}
              onReconnect={dispatch.reconnectActiveSession}
              onReleaseControl={() =>
                void dispatch.submit({ action: "release_control" })
              }
              onResolveApproval={(approvalId, approved) =>
                void dispatch.submit({
                  action: "resolve_approval",
                  approval_id: approvalId,
                  approved,
                })
              }
              onAnswerQuestion={(questionId, answer) =>
                dispatch.submit({
                  action: "answer_question",
                  question_id: questionId,
                  answer,
                })
              }
            />
            <RemoteComposer
              session={session}
              events={sync.events}
              candidate={
                session
                  ? sync.discovered.get(
                      `${session.runner_id}/${session.binding.provider_session_id}`,
                    )
                  : undefined
              }
              itemsCount={items.length}
              isMobile={mobile.isMobile}
              connected={connected}
              canTakeControl={canTakeControl}
              canSend={composer.canSend}
              busy={dispatch.busy}
              isTurnExecuting={composer.isTurnExecuting}
              submittingInterrupt={dispatch.submittingInterrupt}
              text={composer.text}
              setText={composer.setText}
              attachments={composer.attachments}
              onRemoveAttachment={(idx) =>
                composer.setAttachments((prev) =>
                  prev.filter((_, i) => i !== idx),
                )
              }
              activeQueuedPrompt={composer.activeQueuedPrompt}
              onSendQueuedNow={composer.sendQueuedNow}
              onEditQueued={composer.editQueuedPrompt}
              onCancelQueued={composer.cancelQueuedPrompt}
              textareaRef={composer.textareaRef}
              fileInputRef={composer.fileInputRef}
              attachMenuRef={composer.attachMenuRef}
              attachMenuOpen={composer.attachMenuOpen}
              setAttachMenuOpen={composer.setAttachMenuOpen}
              attachQuickPromptsOpen={composer.attachQuickPromptsOpen}
              setAttachQuickPromptsOpen={composer.setAttachQuickPromptsOpen}
              permissionMode={permissionMode}
              onPermissionModeChange={(next) => {
                setPermissionMode(next);
                setPermissionModeTouched(true);
              }}
              sessionModel={composer.sessionModel}
              sessionModelOptions={sessionModelOptions}
              onSessionModelChange={composer.setSessionModel}
              effort={composer.effort}
              onEffortChange={composer.setEffort}
              adjustTextareaHeight={composer.adjustTextareaHeight}
              onFilesSelected={(files) =>
                void composer.handleFilesSelected(files)
              }
              onPasteFromClipboard={() =>
                void composer.handlePasteFromClipboardMenu()
              }
              onInsertCodeSnippet={composer.handleInsertCodeSnippet}
              onSendCurrentPrompt={composer.sendCurrentPrompt}
              onEnqueueCurrentPrompt={composer.enqueueCurrentPrompt}
              onInterrupt={composer.interruptCurrentTurn}
            />
          </div>
        )}
      </main>

      <RemoteLaunchModal
        open={launchModalOpen}
        busy={launchState.launchBusy}
        runner={runner}
        workspace={launchState.workspace}
        nativeId={launchState.nativeId}
        agentType={launchState.agentType}
        model={launchState.model}
        sandbox={launchState.sandbox}
        launchPermissionMode={launchState.launchPermissionMode}
        onlineDeviceOptions={launchState.onlineDeviceOptions}
        launchWorkspaceOptions={launchState.launchWorkspaceOptions}
        launchSessionOptions={launchState.launchSessionOptions}
        launchAgentOptions={launchState.launchAgentOptions}
        launchModelOptions={launchState.launchModelOptions}
        sandboxOptions={SANDBOX_OPTIONS}
        permissionModeOptions={PERMISSION_MODE_OPTIONS}
        onClose={() => setLaunchModalOpen(false)}
        onSubmit={() => void launchState.launch()}
        onRunnerChange={(next) => {
          setRunner(next);
          launchState.setWorkspace(
            sync.runners.find((item) => item.runner_id === next)?.hello
              ?.workspaces[0] ?? "",
          );
          launchState.setNativeId("");
          launchState.setModel("");
        }}
        onWorkspaceChange={(next) => {
          launchState.setWorkspace(next);
          launchState.setNativeId("");
        }}
        onNativeIdChange={launchState.setNativeId}
        onAgentTypeChange={(next) => {
          launchState.setAgentType(next);
          launchState.setModel("");
        }}
        onModelChange={launchState.setModel}
        onSandboxChange={launchState.setSandbox}
        onPermissionModeChange={launchState.setLaunchPermissionMode}
      />

      {settingsOpen && panelConfig !== undefined && (
        <Suspense fallback={null}>
          <SettingsPage
            account={account ?? undefined}
            config={panelConfig}
            notificationPresentedAt={sync.notificationPresentedAt}
            onAccountUpdated={onAccountUpdated}
            onClose={() => setSettingsOpen(false)}
            onLoggedOut={() => {
              setSettingsOpen(false);
              onLoggedOut?.();
            }}
            onSave={(next) => {
              saveConfig(window.localStorage, next);
              setPanelConfig(next);
              setSettingsOpen(false);
              sync.refresh.current();
            }}
            onWorkspaceChanged={() => sync.refresh.current()}
          />
        </Suspense>
      )}
    </div>
  );
}
