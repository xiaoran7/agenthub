import { useEffect, useRef, useState, type MutableRefObject } from "react";
import type { RemoteCommand } from "@agent-pulse/contracts/remote";
import { safeRandomUUID } from "../../lib/uuid";
import {
  nextOptimisticPromptNeeded,
  pendingOptimisticPrompts,
  pruneOptimisticPrompts,
  type ConversationItem,
  type OptimisticPrompt,
  type RemoteAttachment,
  type RemoteCommandRecord,
  type RemotePermissionMode,
  type RemoteRunner,
  type RemoteSession,
  type TurnFailure,
} from "../../lib/remote";
import {
  updateBoundedSessionState,
  type BoundedSessionState,
} from "../../lib/remote-session-state";
import type { QueuedPrompt } from "../QueuedPromptBar";
import { readSelectedAttachments, sessionKey } from "./remote-model-options";

const EMPTY_OPTIMISTIC: OptimisticPrompt[] = [];

export type EffortLevel = "default" | "low" | "medium" | "high" | "max";

export interface ComposerDraft {
  text: string;
  attachments: RemoteAttachment[];
}

export interface TakeoverDraft extends ComposerDraft {
  id: string;
}

export interface SubmitCommandFn {
  (
    action: RemoteCommand["command"],
    target?: RemoteSession,
    clientCmdId?: string,
    rollbackDraft?: ComposerDraft,
  ): Promise<boolean>;
}

export interface UseRemoteComposerParams {
  selected: string;
  session: RemoteSession | undefined;
  runners: RemoteRunner[];
  commands: RemoteCommandRecord[];
  settledItems: ConversationItem[];
  connected: boolean;
  canTakeControl: boolean;
  busy: boolean;
  baseTurnExecuting: boolean;
  turnFailure: TurnFailure | null;
  permissionMode: RemotePermissionMode;
  setPermissionMode: (mode: RemotePermissionMode) => void;
  permissionModeTouched: boolean;
  setPermissionModeTouched: (touched: boolean) => void;
  pendingLaunchPermissionRef: MutableRefObject<{
    sessionKey: string;
    mode: RemotePermissionMode;
  } | null>;
  submitRef: MutableRefObject<SubmitCommandFn>;
  setError: (msg: string) => void;
  setFollowBottom: (follow: boolean) => void;
  setDismissedFailure: (val: string | null) => void;
}

export function useRemoteComposer({
  selected,
  session,
  runners,
  commands,
  settledItems,
  connected,
  canTakeControl,
  busy,
  baseTurnExecuting,
  turnFailure,
  permissionMode,
  setPermissionMode,
  permissionModeTouched,
  setPermissionModeTouched,
  pendingLaunchPermissionRef,
  submitRef,
  setError,
  setFollowBottom,
  setDismissedFailure,
}: UseRemoteComposerParams) {
  const [text, setText] = useState("");
  const textRef = useRef(text);
  textRef.current = text;

  const [attachments, setAttachments] = useState<RemoteAttachment[]>([]);
  const attachmentsRef = useRef(attachments);
  attachmentsRef.current = attachments;

  const [optimisticState, setOptimisticState] = useState<
    BoundedSessionState<OptimisticPrompt[]>
  >({ byKey: {}, order: [] });
  const optimisticBySession = optimisticState.byKey;

  const [queuedPrompts, setQueuedPrompts] = useState<
    Record<string, QueuedPrompt>
  >({});
  const [takeoverDrafts, setTakeoverDrafts] = useState<
    Record<string, TakeoverDraft>
  >({});

  const [effort, setEffort] = useState<EffortLevel>("default");
  const [sessionModel, setSessionModel] = useState("");

  const [attachMenuOpen, setAttachMenuOpen] = useState(false);
  const [attachQuickPromptsOpen, setAttachQuickPromptsOpen] = useState(false);

  const attachMenuRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const draftsBySessionRef = useRef<Record<string, ComposerDraft>>({});
  const wasInterruptedBySessionRef = useRef<Record<string, boolean>>({});
  const wasTurnExecutingBySessionRef = useRef<Record<string, boolean>>({});
  const prevSelectedSessionRef = useRef("");
  const selectedRef = useRef(selected);
  selectedRef.current = selected;

  const optimisticPrompts = selected
    ? (optimisticBySession[selected] ?? EMPTY_OPTIMISTIC)
    : EMPTY_OPTIMISTIC;

  const isTurnExecuting = Boolean(
    session && connected && (baseTurnExecuting || optimisticPrompts.length > 0),
  );

  const updateOptimisticForSession = (
    sKey: string,
    updater: (cur: OptimisticPrompt[]) => OptimisticPrompt[],
  ) => {
    setOptimisticState((prev) => {
      const cur = prev.byKey[sKey] ?? EMPTY_OPTIMISTIC;
      const nextVal = updater(cur);
      if (nextVal === cur && sKey in prev.byKey) return prev;
      return updateBoundedSessionState(prev, sKey, nextVal);
    });
  };

  const forgetTakeoverDraft = (sKey: string) => {
    setTakeoverDrafts((previous) => {
      const next = { ...previous };
      delete next[sKey];
      return next;
    });
  };

  const clearComposerInput = () => {
    setText("");
    setAttachments([]);
    if (selectedRef.current) {
      delete draftsBySessionRef.current[selectedRef.current];
    }
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
      textareaRef.current.style.overflowY = "hidden";
    }
  };

  const restoreRollbackDraft = (
    targetKey: string,
    rollbackDraft: ComposerDraft,
  ) => {
    if (selectedRef.current === targetKey) {
      if (!textRef.current.trim() && attachmentsRef.current.length === 0) {
        setText(rollbackDraft.text);
        setAttachments([...rollbackDraft.attachments]);
      }
    } else {
      const existing = draftsBySessionRef.current[targetKey];
      const hasExistingDraft =
        Boolean(existing?.text.trim()) ||
        (existing?.attachments.length ?? 0) > 0;
      if (!hasExistingDraft) {
        draftsBySessionRef.current[targetKey] = {
          text: rollbackDraft.text,
          attachments: [...rollbackDraft.attachments],
        };
      }
    }
  };

  const adjustTextareaHeight = () => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const scrollH = el.scrollHeight;
    const clamped = Math.min(Math.max(scrollH, 28), 220);
    el.style.height = `${clamped}px`;
    el.style.overflowY = scrollH > 220 ? "auto" : "hidden";
  };

  useEffect(() => {
    const previousKey = prevSelectedSessionRef.current;
    if (previousKey && previousKey !== selected) {
      draftsBySessionRef.current[previousKey] = {
        text: textRef.current,
        attachments: [...attachmentsRef.current],
      };
    }
    prevSelectedSessionRef.current = selected;
    const savedDraft = selected
      ? draftsBySessionRef.current[selected]
      : undefined;
    const nextText = savedDraft?.text ?? "";
    const nextAttachments = savedDraft ? [...savedDraft.attachments] : [];
    textRef.current = nextText;
    attachmentsRef.current = nextAttachments;
    setText(nextText);
    setAttachments(nextAttachments);
    setSessionModel("");
    const pendingPerm = pendingLaunchPermissionRef.current;
    if (pendingPerm && pendingPerm.sessionKey === selected) {
      setPermissionMode(pendingPerm.mode);
      setPermissionModeTouched(pendingPerm.mode !== "default");
    } else {
      if (pendingPerm) {
        pendingLaunchPermissionRef.current = null;
      }
      setPermissionMode("default");
      setPermissionModeTouched(false);
    }
    setAttachMenuOpen(false);
    setAttachQuickPromptsOpen(false);
  }, [
    selected,
    setPermissionMode,
    setPermissionModeTouched,
    pendingLaunchPermissionRef,
  ]);

  useEffect(() => {
    if (!attachMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (
        attachMenuRef.current &&
        !attachMenuRef.current.contains(event.target as Node)
      ) {
        setAttachMenuOpen(false);
        setAttachQuickPromptsOpen(false);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setAttachMenuOpen(false);
        setAttachQuickPromptsOpen(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [attachMenuOpen]);

  useEffect(() => {
    if (optimisticPrompts.length === 0) return;
    const rejectedIds = new Set(
      commands
        .filter((c) => c.receipt && c.receipt.status !== "accepted")
        .map((c) => c.command.client_cmd_id),
    );
    const pruneKey = selected;
    if (!pruneKey) return;
    const tick = () => {
      const now = Date.now();
      updateOptimisticForSession(pruneKey, (prev) => {
        const { kept } = pruneOptimisticPrompts(prev, settledItems, now);
        const filtered = kept.filter((opt) => !rejectedIds.has(opt.id));
        return filtered.length === prev.length ? prev : filtered;
      });
    };
    tick();
    const timer = setInterval(tick, 1_000);
    return () => clearInterval(timer);
  }, [selected, settledItems, commands, optimisticPrompts]);

  useEffect(() => {
    const draft = takeoverDrafts[selected];
    if (!draft) return;
    const receipt = commands.find(
      (item) => item.command.client_cmd_id === draft.id,
    )?.receipt;
    if (!receipt) return;
    forgetTakeoverDraft(selected);
    if (receipt.status !== "accepted") return;
    const matchesTarget = (
      candidateText: string,
      candidateAttachments: readonly RemoteAttachment[],
    ) =>
      candidateText === draft.text &&
      candidateAttachments.length === draft.attachments.length &&
      candidateAttachments.every(
        (item, index) => item === draft.attachments[index],
      );
    const saved = draftsBySessionRef.current[selected];
    if (saved && matchesTarget(saved.text, saved.attachments)) {
      delete draftsBySessionRef.current[selected];
    }
    if (matchesTarget(textRef.current, attachmentsRef.current)) {
      clearComposerInput();
    }
  }, [commands, selected, takeoverDrafts]);

  const handleFilesSelected = async (fileList: FileList | File[]) => {
    const loaded = await readSelectedAttachments(fileList, setError);
    if (loaded.length > 0) {
      setAttachments((prev) => [...prev, ...loaded].slice(0, 16));
    }
  };

  const handlePasteFromClipboardMenu = async () => {
    setAttachMenuOpen(false);
    setAttachQuickPromptsOpen(false);
    if (typeof navigator === "undefined" || !navigator.clipboard) {
      setError("当前浏览器环境不支持直接读取剪贴板，请使用 Ctrl+V 粘贴。");
      return;
    }
    try {
      if (typeof navigator.clipboard.read === "function") {
        const items = await navigator.clipboard.read();
        const files: File[] = [];
        for (const clipItem of items) {
          const imgType = clipItem.types.find((t) => t.startsWith("image/"));
          if (imgType) {
            const blob = await clipItem.getType(imgType);
            const ext = imgType.split("/")[1] || "png";
            files.push(
              new File([blob], `clipboard-${Date.now()}.${ext}`, {
                type: imgType,
              }),
            );
          }
        }
        if (files.length > 0) {
          await handleFilesSelected(files);
          textareaRef.current?.focus();
          return;
        }
      }
      const clipText = await navigator.clipboard.readText();
      if (clipText) {
        if (clipText.length > 8192) {
          const textFile = new File([clipText], `clipboard-${Date.now()}.txt`, {
            type: "text/plain",
          });
          await handleFilesSelected([textFile]);
        } else {
          setText((prev) => (prev ? `${prev}\n${clipText}` : clipText));
          requestAnimationFrame(adjustTextareaHeight);
        }
        textareaRef.current?.focus();
      }
    } catch {
      setError("浏览器未授权读取剪贴板，请直接在输入框按 Ctrl+V 粘贴。");
    }
  };

  const handleInsertCodeSnippet = () => {
    setAttachMenuOpen(false);
    setAttachQuickPromptsOpen(false);
    const el = textareaRef.current;
    const start = el?.selectionStart ?? text.length;
    const end = el?.selectionEnd ?? text.length;
    const selectedSlice = text.slice(start, end);
    const prefix = start > 0 && text[start - 1] !== "\n" ? "\n" : "";
    const block = `${prefix}\`\`\`\n${selectedSlice}\n\`\`\`\n`;
    const nextText = text.slice(0, start) + block + text.slice(end);
    setText(nextText);
    const cursorOffset = start + prefix.length + 4 + selectedSlice.length;
    requestAnimationFrame(() => {
      adjustTextareaHeight();
      if (textareaRef.current) {
        textareaRef.current.focus();
        textareaRef.current.setSelectionRange(cursorOffset, cursorOffset);
      }
    });
  };

  const canSend =
    !takeoverDrafts[selected] &&
    (canTakeControl ||
      (connected &&
        !isTurnExecuting &&
        ["idle", "completed", "error", "failed", "interrupted"].includes(
          session?.binding.execution_state ?? "",
        )));

  const dispatchPromptPayload = (
    rawText: string,
    rawAttachments: RemoteAttachment[],
    acquireControl = false,
  ) => {
    if (!session || (!rawText.trim() && rawAttachments.length === 0)) return;
    const targetKey = sessionKey(session);
    wasInterruptedBySessionRef.current[targetKey] = false;
    const sessionRunner = runners.find(
      (item) => item.runner_id === session.runner_id,
    );
    const supportsModernCommandFields = Boolean(
      sessionRunner?.hello?.models?.agents?.claude_desktop !== undefined ||
      sessionRunner?.hello?.models?.agents?.antigravity !== undefined ||
      sessionRunner?.hello?.discovery?.candidates?.some(
        (c) => c.context_window_tokens !== undefined,
      ),
    );
    let promptText =
      rawText.trim() ||
      `请查看附件：${rawAttachments.map((a) => a.name).join("、")}`;
    if (!supportsModernCommandFields && rawAttachments.length > 0) {
      const inlineParts = rawAttachments.map((att) =>
        att.encoding === "utf8"
          ? `[附件 ${att.name}]:\n\`\`\`\n${att.data.slice(0, 64 * 1024)}\n\`\`\``
          : `[附件 ${att.name} (${att.media_type})]`,
      );
      promptText = `${inlineParts.join("\n\n")}\n\n${promptText}`;
    }

    const clientCmdId = safeRandomUUID();
    const sentAttachments = [...rawAttachments];
    const userRowCount = settledItems.filter(
      (item) => item.kind === "user",
    ).length;
    const neededUserRows = nextOptimisticPromptNeeded(
      pendingOptimisticPrompts(optimisticPrompts, settledItems).map(
        (opt) => opt.neededUserRows,
      ),
      userRowCount,
    );
    updateOptimisticForSession(targetKey, (prev) => [
      ...prev,
      {
        id: clientCmdId,
        text: promptText,
        createdAt: Date.now(),
        neededUserRows,
      },
    ]);
    setFollowBottom(true);
    setDismissedFailure(null);

    const shouldSendPermissionMode =
      permissionMode !== "default" || permissionModeTouched;
    void submitRef.current(
      {
        action: "send_prompt",
        text: promptText,
        ...(acquireControl ? { acquire_control: true } : {}),
        ...(effort !== "default" ? { effort } : {}),
        ...(sessionModel ? { model: sessionModel } : {}),
        ...(shouldSendPermissionMode
          ? { permission_mode: permissionMode }
          : {}),
        ...(supportsModernCommandFields && sentAttachments.length > 0
          ? { attachments: sentAttachments }
          : {}),
      },
      session,
      clientCmdId,
      { text: rawText, attachments: sentAttachments },
    );
  };

  const dispatchPromptPayloadRef = useRef(dispatchPromptPayload);
  dispatchPromptPayloadRef.current = dispatchPromptPayload;

  const enqueueCurrentPrompt = () => {
    if (!selected || !connected || (!text.trim() && attachments.length === 0))
      return;
    wasTurnExecutingBySessionRef.current[selected] = true;
    const draftText = text;
    const draftAttachments = [...attachments];
    clearComposerInput();
    setQueuedPrompts((prev) => {
      const existing = prev[selected];
      const mergedText = existing
        ? [existing.text.trim(), draftText.trim()].filter(Boolean).join("\n")
        : draftText;
      const mergedAttachments = existing
        ? [...existing.attachments, ...draftAttachments].slice(0, 16)
        : draftAttachments;
      return {
        ...prev,
        [selected]: {
          text: mergedText,
          attachments: mergedAttachments,
          queuedAt: Date.now(),
        },
      };
    });
  };

  const sendCurrentPrompt = () => {
    if (busy) return;
    if (!text.trim() && attachments.length === 0) return;
    if (isTurnExecuting && connected) {
      enqueueCurrentPrompt();
      return;
    }
    if (!canSend) return;
    const draftText = text;
    const draftAttachments = [...attachments];
    if (!canTakeControl) clearComposerInput();
    dispatchPromptPayload(draftText, draftAttachments, canTakeControl);
  };

  const activeQueuedPrompt = selected ? queuedPrompts[selected] : undefined;

  useEffect(() => {
    if (!selected) return;
    const previouslyExecuting =
      wasTurnExecutingBySessionRef.current[selected] === true;
    if (isTurnExecuting) {
      wasTurnExecutingBySessionRef.current[selected] = true;
      return;
    }
    if (!previouslyExecuting) return;
    if (!activeQueuedPrompt) {
      wasTurnExecutingBySessionRef.current[selected] = false;
      return;
    }
    if (!session) return;
    const hasFailed =
      Boolean(turnFailure && !turnFailure.retrying) ||
      session.binding.execution_state === "error" ||
      session.binding.execution_state === "failed" ||
      session.binding.execution_state === "interrupted" ||
      wasInterruptedBySessionRef.current[selected] === true;
    if (hasFailed) {
      wasTurnExecutingBySessionRef.current[selected] = false;
      return;
    }
    if (!connected || !canSend) return;
    wasTurnExecutingBySessionRef.current[selected] = false;
    const queued = activeQueuedPrompt;
    setQueuedPrompts((prev) => {
      const next = { ...prev };
      delete next[selected];
      return next;
    });
    dispatchPromptPayloadRef.current(queued.text, queued.attachments);
  }, [
    selected,
    isTurnExecuting,
    activeQueuedPrompt,
    session,
    turnFailure,
    connected,
    canSend,
  ]);

  const sendQueuedNow = () => {
    if (!selected || !connected || !canSend || !activeQueuedPrompt) return;
    const queued = activeQueuedPrompt;
    setQueuedPrompts((prev) => {
      const next = { ...prev };
      delete next[selected];
      return next;
    });
    dispatchPromptPayload(queued.text, queued.attachments);
  };

  const editQueuedPrompt = () => {
    if (!selected || !activeQueuedPrompt) return;
    setText((prev) =>
      prev.trim()
        ? `${activeQueuedPrompt.text}\n${prev}`
        : activeQueuedPrompt.text,
    );
    if (activeQueuedPrompt.attachments.length > 0) {
      setAttachments((prev) =>
        [...activeQueuedPrompt.attachments, ...prev].slice(0, 16),
      );
    }
    setQueuedPrompts((prev) => {
      const next = { ...prev };
      delete next[selected];
      return next;
    });
    requestAnimationFrame(() => {
      adjustTextareaHeight();
      textareaRef.current?.focus();
    });
  };

  const cancelQueuedPrompt = () => {
    if (!selected) return;
    setQueuedPrompts((prev) => {
      const next = { ...prev };
      delete next[selected];
      return next;
    });
  };

  const interruptCurrentTurn = () => {
    if (selected) {
      wasInterruptedBySessionRef.current[selected] = true;
    }
    void submitRef.current({ action: "interrupt" });
  };

  return {
    text,
    setText,
    attachments,
    setAttachments,
    optimisticPrompts,
    isTurnExecuting,
    updateOptimisticForSession,
    setTakeoverDrafts,
    forgetTakeoverDraft,
    restoreRollbackDraft,
    effort,
    setEffort,
    sessionModel,
    setSessionModel,
    attachMenuOpen,
    setAttachMenuOpen,
    attachQuickPromptsOpen,
    setAttachQuickPromptsOpen,
    attachMenuRef,
    fileInputRef,
    textareaRef,
    canSend,
    activeQueuedPrompt,
    adjustTextareaHeight,
    handleFilesSelected,
    handlePasteFromClipboardMenu,
    handleInsertCodeSnippet,
    enqueueCurrentPrompt,
    sendCurrentPrompt,
    sendQueuedNow,
    editQueuedPrompt,
    cancelQueuedPrompt,
    interruptCurrentTurn,
  };
}
