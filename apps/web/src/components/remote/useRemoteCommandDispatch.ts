import { useEffect, useRef, useState, type MutableRefObject } from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import {
  remoteError,
  type OptimisticPrompt,
  type RemoteCommandRecord,
  type RemoteRequest,
  type RemoteRunner,
  type RemoteSession,
} from "../../lib/remote";
import { buildCommandAttemptKey } from "../../lib/remote-session-state";
import { safeRandomUUID } from "../../lib/uuid";
import { isCodexThreadId, sessionKey } from "./remote-model-options";
import type {
  ComposerDraft,
  SubmitCommandFn,
  TakeoverDraft,
} from "./useRemoteComposer";

export interface UseRemoteCommandDispatchParams {
  selected: string;
  session?: RemoteSession | undefined;
  runners: RemoteRunner[];
  commands: RemoteCommandRecord[];
  codexSession: boolean;
  sessionLoading: boolean;
  itemsCount: number;
  launchBusy: boolean;
  attempts: MutableRefObject<Map<string, RemoteCommand>>;
  requestWithIdentity: RemoteRequest;
  refresh: MutableRefObject<() => void>;
  refreshSessionDetail: MutableRefObject<() => void>;
  setError: (msg: string) => void;
  setDismissedFailure: (val: string | null) => void;
  supportsCodexControl: (runnerId: string) => boolean;
  composerCallbacksRef: MutableRefObject<{
    setTakeoverDrafts: React.Dispatch<
      React.SetStateAction<Record<string, TakeoverDraft>>
    >;
    forgetTakeoverDraft: (sKey: string) => void;
    updateOptimisticForSession: (
      sKey: string,
      updater: (cur: OptimisticPrompt[]) => OptimisticPrompt[],
    ) => void;
    restoreRollbackDraft: (sKey: string, draft: ComposerDraft) => void;
  }>;
}

export function useRemoteCommandDispatch({
  selected,
  session,
  runners,
  commands,
  codexSession,
  sessionLoading,
  itemsCount,
  launchBusy,
  attempts,
  requestWithIdentity,
  refresh,
  refreshSessionDetail,
  setError,
  setDismissedFailure,
  supportsCodexControl,
  composerCallbacksRef,
}: UseRemoteCommandDispatchParams) {
  const [submittingApproval, setSubmittingApproval] = useState<{
    id: string;
    action: "approve" | "decline";
  } | null>(null);
  const [submittingQuestionId, setSubmittingQuestionId] = useState<
    string | null
  >(null);
  const [submittingInterrupt, setSubmittingInterrupt] = useState(false);
  const [busyBySession, setBusyBySession] = useState<Record<string, number>>(
    {},
  );
  const previewedHistory = useRef("");
  const submitRef = useRef<SubmitCommandFn>(async () => false);

  const setSessionBusy = (sKey: string, delta: 1 | -1) => {
    setBusyBySession((prev) => {
      const nextCount = Math.max(0, (prev[sKey] ?? 0) + delta);
      if (nextCount === 0) {
        if (!(sKey in prev)) return prev;
        const copy = { ...prev };
        delete copy[sKey];
        return copy;
      }
      return { ...prev, [sKey]: nextCount };
    });
  };

  const busy = Boolean(
    launchBusy || (selected && (busyBySession[selected] ?? 0) > 0),
  );

  const submit: SubmitCommandFn = async (
    action,
    target = session,
    clientCmdId,
    rollbackDraft,
  ) => {
    if (!target) return false;
    const targetKey = sessionKey(target);
    setSessionBusy(targetKey, 1);
    if (action.action === "resolve_approval") {
      setSubmittingApproval({
        id: action.approval_id,
        action: action.approved ? "approve" : "decline",
      });
    } else if (action.action === "answer_question") {
      setSubmittingQuestionId(action.question_id);
    } else if (action.action === "interrupt") {
      setSubmittingInterrupt(true);
    }
    setError("");
    const targetRunner = runners.find(
      (item) => item.runner_id === target.runner_id,
    );
    const activeGeneration =
      targetRunner?.hello?.host_generation ?? target.binding.host_generation;
    const attemptKey = buildCommandAttemptKey(
      target.runner_id,
      target.binding.hub_session_id,
      action,
      activeGeneration,
    );
    const command =
      attempts.current.get(attemptKey) ??
      ({
        client_cmd_id: clientCmdId ?? safeRandomUUID(),
        hub_session_id: target.binding.hub_session_id,
        host_generation: activeGeneration,
        command: action,
      } satisfies RemoteCommand);
    attempts.current.set(attemptKey, command);

    const composerCbs = composerCallbacksRef.current;
    if (action.action === "send_prompt") {
      if (action.acquire_control && rollbackDraft) {
        composerCbs.setTakeoverDrafts((previous) => ({
          ...previous,
          [targetKey]: { id: command.client_cmd_id, ...rollbackDraft },
        }));
      }
      if (clientCmdId && clientCmdId !== command.client_cmd_id) {
        composerCbs.updateOptimisticForSession(targetKey, (previous) =>
          previous.map((entry) =>
            entry.id === clientCmdId
              ? { ...entry, id: command.client_cmd_id }
              : entry,
          ),
        );
      }
    }

    try {
      await requestWithIdentity(
        `/runners/${encodeURIComponent(target.runner_id)}/commands`,
        command,
      );
      attempts.current.delete(attemptKey);
      if (action.action === "send_prompt") {
        setDismissedFailure(null);
      }
      refresh.current();
      refreshSessionDetail.current();
      return true;
    } catch (err) {
      if (!(err instanceof Error) || err.name === "RemoteRequestRejected") {
        attempts.current.delete(attemptKey);
      }
      if (action.action === "send_prompt") {
        if (action.acquire_control) composerCbs.forgetTakeoverDraft(targetKey);
        composerCbs.updateOptimisticForSession(targetKey, (prev) =>
          prev.filter(
            (opt) =>
              opt.id !== command.client_cmd_id &&
              (clientCmdId === undefined || opt.id !== clientCmdId),
          ),
        );
        if (rollbackDraft) {
          composerCbs.restoreRollbackDraft(targetKey, rollbackDraft);
        }
      }
      setError(remoteError(err));
      return false;
    } finally {
      setSessionBusy(targetKey, -1);
      if (action.action === "resolve_approval") {
        setSubmittingApproval((current) =>
          current?.id === action.approval_id ? null : current,
        );
      } else if (action.action === "answer_question") {
        setSubmittingQuestionId((current) =>
          current === action.question_id ? null : current,
        );
      } else if (action.action === "interrupt") {
        setSubmittingInterrupt(false);
      }
    }
  };
  submitRef.current = submit;

  useEffect(() => {
    setSubmittingApproval(null);
    setSubmittingQuestionId(null);
  }, [selected]);

  useEffect(() => {
    const confirmed = new Set(
      commands
        .filter((item) => item.receipt)
        .map((item) => item.command.client_cmd_id),
    );
    for (const [k, command] of attempts.current)
      if (confirmed.has(command.client_cmd_id)) attempts.current.delete(k);
  }, [commands, attempts]);

  // History hydration never acquires native control.
  useEffect(() => {
    if (
      !session ||
      !session.online ||
      !codexSession ||
      sessionLoading ||
      itemsCount > 0
    )
      return;
    const previewKey =
      sessionKey(session) + "/" + session.binding.host_generation;
    if (previewedHistory.current === previewKey) return;
    previewedHistory.current = previewKey;
    void submitRef.current(
      {
        action: "connect",
        mode: "preview",
        workspace_path: session.binding.workspace_path,
        provider_session_id: session.binding.provider_session_id,
      },
      session,
    );
  }, [session, codexSession, sessionLoading, itemsCount]);

  const reconnectActiveSession = (activeSession: RemoteSession) => {
    void submit(
      {
        action: "connect",
        mode:
          supportsCodexControl(activeSession.runner_id) &&
          isCodexThreadId(activeSession.binding.provider_session_id)
            ? "take_control"
            : "resume",
        workspace_path: activeSession.binding.workspace_path,
        provider_session_id: activeSession.binding.provider_session_id,
      },
      activeSession,
    );
  };

  return {
    busy,
    submittingApproval,
    submittingQuestionId,
    submittingInterrupt,
    submit,
    submitRef,
    reconnectActiveSession,
  };
}
