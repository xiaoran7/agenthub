import assert from "node:assert/strict";
import test from "node:test";
import type {
  RemoteBinding,
  RemotePendingApproval,
} from "@agent-pulse/contracts/remote";
import {
  cleanBannerText,
  conversationItems,
  groupConversationItems,
  isRedactedPathOnly,
  latestContextUsage,
  latestTurnFailure,
  nextOptimisticPromptNeeded,
  pendingApprovals,
  pendingOptimisticPrompts,
  pendingQuestions,
  pruneOptimisticPrompts,
  readableRemoteText,
  remoteError,
  remoteRequest,
  isTransientRemoteError,
  isTransientRemoteMessage,
  trimSupersededEvents,
  type OptimisticPrompt,
  type RemoteCommandRecord,
  type RemoteEvent,
} from "../src/lib/remote";

const event = (
  kind: string,
  params: Record<string, unknown>,
  options: {
    approval_id?: string;
    approval?: RemotePendingApproval;
    binding?: RemoteBinding;
  } = {},
): RemoteEvent => ({
  cursor: 1,
  runner_id: "runner",
  payload: {
    type: "event",
    event: {
      kind,
      raw: { params },
      ...options,
    },
  },
});

const approval = (
  fill: string,
  hubSessionId = "hub-one",
): RemotePendingApproval => ({
  approval_id: `approval-${fill.repeat(64)}`,
  hub_session_id: hubSessionId,
  host_generation: "generation-one",
  turn_id: "turn-one",
  can_approve: true,
  can_decline: true,
});

const ready = (
  approvals: RemotePendingApproval[],
  cursor: number,
): RemoteEvent => ({
  cursor,
  runner_id: "runner",
  payload: { type: "ready", pending_approvals: approvals },
});

test("native final messages replace deltas without duplicating the answer", () => {
  const items = conversationItems(
    [
      event("item/agentMessage/delta", { itemId: "a", delta: "Hel" }),
      event("item/agentMessage/delta", { itemId: "a", delta: "lo" }),
      event("item/completed", {
        item: { id: "a", type: "agentMessage", text: "Hello!" },
      }),
      event("item/completed", {
        item: {
          id: "tool",
          type: "commandExecution",
          command: "pwd",
          aggregatedOutput: "/workspace",
        },
      }),
    ],
    [],
  );
  assert.deepEqual(
    items.map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "assistant", text: "Hello!" },
      { kind: "tool", text: "工具执行已完成" },
    ],
  );
});

test("legacy Claude deltas render in their own turn as they arrive", () => {
  const scope = "device/claude-stream/";
  const history = event("history/snapshot", {
    items: [{ kind: "message", role: "user", text: "Earlier prompt" }],
  });
  const commands: RemoteCommandRecord[] = ["one", "two"].map((id) => ({
    command: {
      client_cmd_id: `cmd-${id}`,
      hub_session_id: "hub",
      host_generation: "generation",
      command: { action: "send_prompt", text: `Prompt ${id}` },
    },
    receipt: { status: "accepted", result: { turn: { id } } },
  }));
  const events = [
    history,
    event("turn/started", { turn: { id: "one" } }),
    event("item/agentMessage/delta", { itemId: "claude_msg", delta: "First" }),
    event("turn/completed", { turn: { id: "one" } }),
    event("turn/started", { turn: { id: "two" } }),
    event("item/agentMessage/delta", { itemId: "claude_msg", delta: "Sec" }),
  ];
  const partial = conversationItems(events, commands, scope, "two");
  assert.deepEqual(
    partial.slice(-4).map(({ kind, text }) => [kind, text]),
    [
      ["user", "Prompt one"],
      ["assistant", "First"],
      ["user", "Prompt two"],
      ["assistant", "Sec"],
    ],
  );

  for (const itemId of ["claude_msg", "claude_msg:two"]) {
    const remaining = event("item/agentMessage/delta", {
      itemId,
      delta: "ond",
    });
    const full = conversationItems(
      [...events, remaining],
      commands,
      scope,
      "two",
    );
    assert.equal(partial.at(-1)?.text, "Sec");
    assert.equal(full.at(-1)?.text, "Second");
    assert.equal(full.at(-3)?.text, "First");
  }
});

test("remote tool events merge once and consecutive calls group like Desktop", () => {
  const items = conversationItems(
    [
      event("item/agentMessage/delta", {
        itemId: "tool-intent",
        turnId: "turn-1",
        delta: '{"command":"pwd","description":"检查目录"}',
      }),
      event("item/started", {
        turnId: "turn-1",
        item: { id: "command-1", type: "commandExecution", command: "pwd" },
      }),
      event("item/completed", {
        turnId: "turn-1",
        item: {
          id: "command-1",
          type: "commandExecution",
          command: "pwd",
          aggregatedOutput: "/workspace",
        },
      }),
      event("item/completed", {
        turnId: "turn-1",
        item: { id: "command-2", type: "commandExecution", command: "ls" },
      }),
    ],
    [],
  );
  assert.equal(items.length, 2);
  assert.equal(items[0]?.title, "命令执行");
  assert.equal(items[0]?.status, "completed");
  assert.equal(items[0]?.output, "/workspace");
  const groups = groupConversationItems(items);
  assert.equal(groups[0]?.type, "tool_group");
  if (groups[0]?.type === "tool_group") assert.equal(groups[0].items.length, 2);
});

test("redacted local paths read as privacy notices", () => {
  assert.equal(isRedactedPathOnly("[local_path_redacted]"), true);
  assert.equal(
    readableRemoteText("已成功[local_path_redacted]"),
    "已成功（本机路径已隐藏）",
  );
});

test("context ring uses reported values and stays empty without usage", () => {
  assert.equal(latestContextUsage([]), null);
  assert.deepEqual(
    latestContextUsage([
      event("thread/tokenUsage/updated", {
        tokenUsage: {
          last: { inputTokens: 42_000 },
          modelContextWindow: 200_000,
        },
      }),
    ]),
    { used: 42_000, window: 200_000, source: "reported" },
  );
  assert.deepEqual(
    latestContextUsage([
      event("thread/tokenUsage/updated", {
        tokenUsage: {
          used: 68_000,
          window: 200_000,
        },
      }),
    ]),
    { used: 68_000, window: 200_000, source: "reported" },
  );
  assert.deepEqual(
    latestContextUsage([], {
      provider_session_id: "claude_abc",
      workspace_path: "ws-agent-0123456789abcdef",
      title: "课表爬虫",
      context_used_tokens: 54_000,
      context_window_tokens: 200_000,
    }),
    { used: 54_000, window: 200_000, source: "reported" },
  );
  assert.deepEqual(
    latestContextUsage([], {
      provider_session_id: "claude_legacy",
      workspace_path: "ws-agent-0123456789abcdef",
      title: "旧版Runner兼容",
      model: "claude-opus-4-6",
      prompt_tokens: 90_000,
      calls: 3,
    }),
    { used: 30_000, window: 200_000, source: "reported" },
  );
});

test("managed history snapshots replace stale replay and retain later live messages", () => {
  const first = event("item/completed", {
    item: { id: "old", type: "agentMessage", text: "stale" },
  });
  const history = {
    ...event("history/snapshot", {
      items: [
        { role: "user", kind: "message", text: "开始" },
        { role: "assistant", kind: "message", text: "已完成" },
        {
          role: "assistant",
          kind: "tool",
          label: "Read",
          text: "读取结果",
          status: "completed",
        },
      ],
    }),
    cursor: 2,
  };
  const live = {
    ...event("item/completed", {
      item: { id: "new", type: "agentMessage", text: "继续执行" },
    }),
    cursor: 3,
  };
  const items = conversationItems([first, history, live], []);
  assert.deepEqual(
    items.map(({ kind, text }) => [kind, text]),
    [
      ["user", "开始"],
      ["assistant", "已完成"],
      ["tool", "读取结果"],
      ["assistant", "继续执行"],
    ],
  );
  assert.equal(items[2]?.title, "Read");
});

test("ready snapshots restore, scope, and clear opaque approvals", () => {
  const target = approval("a");
  const other = approval("b", "hub-other");
  assert.deepEqual(pendingApprovals([ready([target, other], 1)], "hub-one"), [
    target,
  ]);
  const requested = event(
    "approval_requested",
    {},
    {
      approval_id: target.approval_id,
      approval: target,
    },
  );
  assert.deepEqual(pendingApprovals([requested, ready([], 2)], "hub-one"), []);
});

test("opaque resolution and completed turns remove exact approvals", () => {
  const target = approval("a");
  const requested = event(
    "approval_requested",
    {},
    {
      approval_id: target.approval_id,
      approval: target,
    },
  );
  assert.equal(pendingApprovals([requested], "hub-one").length, 1);
  assert.equal(
    pendingApprovals(
      [
        requested,
        event("approval_resolved", {}, { approval_id: target.approval_id }),
      ],
      "hub-one",
    ).length,
    0,
  );
  const binding: RemoteBinding = {
    hub_session_id: "hub-one",
    provider_session_id: "native-one",
    workspace_path: "ws-project-0123456789abcdef",
    host_generation: "generation-one",
    control_connected: true,
    execution_state: "idle",
    current_turn_id: null,
  };
  assert.equal(
    pendingApprovals(
      [requested, event("turn/completed", {}, { binding })],
      "hub-one",
    ).length,
    0,
  );
});

test("accepted approval receipts remove the exact opaque control", () => {
  const target = approval("b");
  const requested = event(
    "approval_requested",
    {},
    {
      approval_id: target.approval_id,
      approval: target,
    },
  );
  const receipt: RemoteEvent = {
    cursor: 2,
    runner_id: "runner",
    payload: {
      type: "receipt",
      receipt: {
        client_cmd_id: "command-1",
        status: "accepted",
        result: { approval_id: target.approval_id, response_sent: true },
      },
    },
  };
  const rejected: RemoteEvent = {
    ...receipt,
    payload: {
      ...receipt.payload,
      receipt: { ...receipt.payload.receipt!, status: "rejected" },
    },
  };

  assert.equal(pendingApprovals([requested, rejected], "hub-one").length, 1);
  assert.equal(pendingApprovals([requested, receipt], "hub-one").length, 0);
});

const errorEvent = (
  message: string,
  options: { willRetry?: boolean; turnId?: string; hubSessionId?: string } = {},
): RemoteEvent =>
  event("error", {
    turnId: options.turnId ?? "turn-one",
    willRetry: options.willRetry ?? false,
    error: { message },
  });

test("a failed turn reports the cause the runner forwarded", () => {
  const failure = latestTurnFailure(
    [
      errorEvent(
        "The 'gpt-6-luna' model is not supported when using Codex with a ChatGPT account.",
      ),
    ],
    "hub-one",
  );

  assert.ok(failure, "the console must be able to explain a failed turn");
  assert.equal(failure.retrying, false);
  assert.equal(failure.turnId, "turn-one");
  assert.match(failure.message, /gpt-6-luna/);
});

test("a retrying provider error is not the turn's final outcome", () => {
  const failure = latestTurnFailure(
    [errorEvent("stream stalled", { willRetry: true })],
    "hub-one",
  );

  assert.equal(failure?.retrying, true);
});

test("a new turn clears an earlier failure and a completed turn clears it too", () => {
  const failed = errorEvent("model unavailable");
  const restarted = event("turn/started", { turn: { id: "turn-two" } });
  assert.equal(latestTurnFailure([failed, restarted], "hub-one"), null);

  const completed = event("turn/completed", { turn: { id: "turn-two" } });
  assert.equal(latestTurnFailure([failed, completed], "hub-one"), null);

  const failedCompletion: RemoteEvent = {
    cursor: 3,
    runner_id: "runner-one",
    payload: {
      type: "event",
      event: {
        kind: "turn/completed",
        raw: { params: { turn: { id: "turn-one" } } },
        binding: {
          hub_session_id: "hub-one",
          provider_session_id: "native-one",
          workspace_path: "ws-app-0123456789abcdef",
          host_generation: "generation-one",
          control_connected: true,
          execution_state: "error",
          current_turn_id: null,
        },
      },
    },
  };
  assert.equal(
    latestTurnFailure([failed, failedCompletion], "hub-one")?.message,
    "model unavailable",
  );
});

test("another session's turn must not clear this session's failure", () => {
  const failed = errorEvent("model unavailable");
  for (const kind of ["turn/started", "turn/completed"]) {
    const otherSession: RemoteEvent = {
      ...event(kind, { turn: { id: "turn-other" } }),
      payload: {
        type: "event",
        event: {
          kind,
          raw: { params: { turn: { id: "turn-other" } } },
          binding: {
            hub_session_id: "hub-other",
            provider_session_id: "native-one",
            workspace_path: "ws-app-0123456789abcdef",
            host_generation: "generation-one",
            control_connected: true,
            execution_state: "idle",
            current_turn_id: null,
          },
        },
      },
    };
    const failure = latestTurnFailure([failed, otherSession], "hub-one");
    assert.equal(
      failure?.message,
      "model unavailable",
      `${kind} for another session is not this session's outcome`,
    );
  }
});

test("an error the runner could not describe yields no invented cause", () => {
  for (const params of [
    {},
    { error: {} },
    { error: { message: "" } },
    { error: { message: 42 } },
  ]) {
    assert.equal(
      latestTurnFailure([event("error", params)], "hub-one"),
      null,
      `unrecognised error ${JSON.stringify(params)} must not fabricate text`,
    );
  }
});

test("an error belonging to another session is ignored", () => {
  const otherSession: RemoteEvent = {
    ...errorEvent("model unavailable"),
    payload: {
      type: "event",
      event: {
        kind: "error",
        raw: { params: { error: { message: "model unavailable" } } },
        binding: {
          hub_session_id: "hub-other",
          provider_session_id: "native-one",
          workspace_path: "ws-app-0123456789abcdef",
          host_generation: "generation-one",
          control_connected: true,
          execution_state: "error",
          current_turn_id: null,
        },
      },
    },
  };

  assert.equal(latestTurnFailure([otherSession], "hub-one"), null);
});

test("conversationItems generates stable IDs and guards unclosed JSON deltas", () => {
  const history = {
    ...event("history/snapshot", {
      items: [
        { role: "user", kind: "message", text: "历史提问" },
        {
          id: "custom-id-1",
          role: "assistant",
          kind: "message",
          text: "历史回答",
        },
      ],
    }),
    cursor: 10,
  };
  const unclosedDelta = event("item/agentMessage/delta", {
    itemId: "msg-1",
    delta: '{"command": "echo 1", "description": "partial...',
  });
  const closedDelta = event("item/agentMessage/delta", {
    itemId: "msg-2",
    turnId: "turn-1",
    delta: '{"command": "echo 2", "description": "full"}',
  });
  const items = conversationItems([history, unclosedDelta, closedDelta], []);
  assert.equal(items[0]?.id, "history-0");
  assert.equal(items[1]?.id, "custom-id-1");
  assert.equal(items[2]?.kind, "assistant");
  assert.equal(
    items[2]?.text,
    '{"command": "echo 1", "description": "partial...',
  );
  assert.equal(items[3]?.kind, "tool");
  assert.equal(items[3]?.command, "echo 2");
  assert.equal(items[3]?.description, "full");
});

test("item ids are scoped per session and references survive an unchanged replay", () => {
  const textOf = (items: Array<{ text: string }>) => items.map((i) => i.text);
  const snapshotAt = (cursor: number, assistant: string) => ({
    ...event("history/snapshot", {
      items: [
        { role: "user", kind: "message", text: "问题" },
        { role: "assistant", kind: "message", text: assistant },
      ],
    }),
    cursor,
  });
  const scope = "device/session-a/";

  const before = conversationItems([snapshotAt(10, "回答")], [], scope);
  const replay = conversationItems([snapshotAt(10, "回答")], [], scope);
  // Same session, same events: every item is the very object from last time,
  // which is what lets the message cards and MarkdownView skip re-rendering.
  assert.ok(replay[0] === before[0], "unchanged user message is reused");
  assert.ok(replay[1] === before[1], "unchanged answer is reused");

  // A changed message keeps its id but must be a new object, so the card and
  // its Markdown actually update.
  const grown = conversationItems([snapshotAt(11, "回答得更长")], [], scope);
  assert.ok(grown[0] === before[0], "the untouched message is still reused");
  assert.ok(grown[1] !== before[1], "the edited answer is rebuilt");
  assert.equal(grown[1]?.id, before[1]?.id);
  assert.deepEqual(textOf(grown), ["问题", "回答得更长"]);

  // A different session must not reuse the ids of the previous one: React
  // would otherwise keep the old DOM nodes and re-key the transcript onto
  // content that belongs to another conversation.
  const other = conversationItems(
    [snapshotAt(10, "回答")],
    [],
    "device/session-b/",
  );
  const same = conversationItems([snapshotAt(10, "回答")], [], scope);
  const ids = new Set([...other, ...same].map((item) => item.id));
  assert.equal(ids.size, 4);
  assert.ok(other.every((item) => item.id.startsWith("device/session-b/")));
  assert.ok(same.every((item) => item.id.startsWith(scope)));
});

test("a streamed delta grows the message without rewriting the one already painted", () => {
  const scope = "device/session-a/";
  const delta = (text: string, cursor: number) => ({
    ...event("item/agentMessage/delta", { itemId: "live", delta: text }),
    cursor,
  });
  const first = conversationItems([delta("A", 1)], [], scope);
  const second = conversationItems([delta("A", 1), delta("B", 2)], [], scope);
  const third = conversationItems(
    [delta("A", 1), delta("B", 2), delta("C", 3)],
    [],
    scope,
  );

  // The text the reader sees accumulates...
  assert.equal(first[0]?.text, "A");
  assert.equal(second[0]?.text, "AB");
  assert.equal(third[0]?.text, "ABC");
  // ...and every step is a new object, or `memo` would not repaint it.
  assert.ok(first[0] !== second[0]);
  assert.ok(second[0] !== third[0]);
  // The earlier results are values the caller may still be rendering: a later
  // fold must never have written into them.
  assert.equal(first[0]?.text, "A");
  assert.equal(second[0]?.text, "AB");
});

test("concurrent running tools merge with command execution without O(N^2) mispairing", () => {
  const toolDelta1 = event("item/agentMessage/delta", {
    itemId: "msg-t1",
    turnId: "turn-a",
    delta: '{"command": "cat a.txt", "description": "read a"}',
  });
  const toolDelta2 = event("item/agentMessage/delta", {
    itemId: "msg-t2",
    turnId: "turn-b",
    delta: '{"command": "cat b.txt", "description": "read b"}',
  });
  const exec2 = event("item/completed", {
    itemId: "exec-2",
    item: {
      type: "commandExecution",
      command: "cat b.txt",
      output: "content of b",
      status: "completed",
    },
    raw: { params: { turnId: "turn-b" } },
  });
  const exec1 = event("item/completed", {
    itemId: "exec-1",
    item: {
      type: "commandExecution",
      command: "cat a.txt",
      output: "content of a",
      status: "completed",
    },
    raw: { params: { turnId: "turn-a" } },
  });
  const items = conversationItems([toolDelta1, toolDelta2, exec2, exec1], []);
  assert.equal(items.length, 2);
  assert.equal(items[0]?.command, "cat a.txt");
  assert.equal(items[0]?.output, "content of a");
  assert.equal(items[0]?.status, "completed");
  assert.equal(items[1]?.command, "cat b.txt");
  assert.equal(items[1]?.output, "content of b");
  assert.equal(items[1]?.status, "completed");
});

test("a prompt still awaiting its turn is appended once, not once per poll", () => {
  const scope = "device/session-a/";
  const snapshot = {
    ...event("history/snapshot", {
      items: [{ role: "user", kind: "message", text: "q" }],
    }),
    cursor: 1,
  };
  const delta = (cursor: number) => ({
    ...event("item/agentMessage/delta", { itemId: "m", delta: "x" }),
    cursor,
  });
  const prompt = {
    command: {
      client_cmd_id: "cmd-1",
      command: { action: "send_prompt", text: "hello" },
    },
    receipt: null,
  } as never;

  const first = conversationItems([snapshot, delta(2)], [prompt], scope);
  assert.deepEqual(
    first.map((item) => item.id),
    [`${scope}history-0`, "m", "cmd-1"],
  );
  // The prompt is derived from the command list, not from an event, so an
  // incremental fold must lift it off before folding the new tail.
  const second = conversationItems(
    [snapshot, delta(2), delta(3)],
    [prompt],
    scope,
  );
  assert.deepEqual(
    second.map((item) => item.id),
    [`${scope}history-0`, "m", "cmd-1"],
  );
  const third = conversationItems(
    [snapshot, delta(2), delta(3), delta(4)],
    [prompt],
    scope,
  );
  assert.deepEqual(
    third.map((item) => item.id),
    [`${scope}history-0`, "m", "cmd-1"],
  );
});

test("trimming superseded events keeps a request the snapshot does not answer", () => {
  const approval = {
    approval_id: `approval-${"a".repeat(64)}`,
    hub_session_id: "hub",
    host_generation: "generation",
    turn_id: "turn-1",
    can_approve: true,
    can_decline: true,
  };
  const readyFrame = (
    cursor: number,
    approvals: RemotePendingApproval[],
  ): RemoteEvent => ({
    cursor,
    runner_id: "runner",
    payload: { type: "ready", pending_approvals: approvals },
  });
  const request: RemoteEvent = {
    cursor: 2,
    runner_id: "runner",
    payload: {
      type: "event",
      event: {
        kind: "approval_requested",
        hub_session_id: "hub",
        approval_id: approval.approval_id,
        approval,
      } as never,
    },
  };
  const tail = Array.from({ length: 100 }, (_, index) => ({
    ...event("item/agentMessage/delta", { itemId: `m${index}`, delta: "x" }),
    cursor: 10 + index,
  }));
  const snapshot = {
    ...event("history/snapshot", {
      items: [{ role: "user", kind: "message", text: "q" }],
    }),
    cursor: 500,
  };

  const events = [readyFrame(1, []), request, ...tail, snapshot];
  assert.equal(pendingApprovals(events, "hub").length, 1);
  const trimmed = trimSupersededEvents(events);
  assert.ok(trimmed.length < events.length, "the superseded tail is dropped");
  // The snapshot says nothing about approvals: keeping only the older `ready`
  // would reset the set to empty and hide the request.
  assert.equal(pendingApprovals(trimmed, "hub").length, 1);

  // A `ready` that arrives after the request and clears it must still win.
  const cleared = [...events];
  cleared.splice(50, 0, readyFrame(51, []));
  const clearedTrimmed = trimSupersededEvents(cleared);
  assert.equal(pendingApprovals(clearedTrimmed, "hub").length, 0);
});

test("two transcripts rendered side by side each keep their incremental state", () => {
  const build = (id: string) => {
    const events: RemoteEvent[] = [
      {
        ...event("history/snapshot", {
          items: [{ role: "user", kind: "message", text: "q" }],
        }),
        cursor: 1,
      },
    ];
    for (let index = 0; index < 20; index++)
      events.push({
        ...event("item/agentMessage/delta", { itemId: id, delta: id }),
        cursor: index + 2,
      });
    return events;
  };
  const a = build("a");
  const b = build("b");
  conversationItems(a, [], "A/");
  conversationItems(b, [], "B/");

  const aBefore = conversationItems(a, [], "A/");
  const bBefore = conversationItems(b, [], "B/");
  // Interleave the two sessions, the way a side-by-side view would.
  const aNext = conversationItems(
    [
      ...a,
      {
        ...event("item/agentMessage/delta", { itemId: "a", delta: "!" }),
        cursor: 99,
      },
    ],
    [],
    "A/",
  );
  const bNext = conversationItems(
    [
      ...b,
      {
        ...event("item/agentMessage/delta", { itemId: "b", delta: "!" }),
        cursor: 98,
      },
    ],
    [],
    "B/",
  );
  // Each session continued from its own folded state, so its untouched
  // history is still the same objects...
  const aHistory = aBefore.find((item) => item.id.endsWith("history-0"));
  const bHistory = bBefore.find((item) => item.id.endsWith("history-0"));
  assert.ok(aNext.some((item) => item === aHistory));
  assert.ok(bNext.some((item) => item === bHistory));
  // ...and each kept its own content instead of the other's.
  assert.equal(aNext.at(-1)?.text, "a".repeat(20) + "!");
  assert.equal(bNext.at(-1)?.text, "b".repeat(20) + "!");
  assert.equal(aNext.at(-1)?.id, "a");
  assert.equal(bNext.at(-1)?.id, "b");
});

test("pending approvals and questions resume from their own fold", () => {
  const approval = {
    approval_id: `approval-${"b".repeat(64)}`,
    hub_session_id: "hub",
    host_generation: "generation",
    turn_id: "turn-1",
    can_approve: true,
    can_decline: true,
  };
  const base: RemoteEvent[] = [
    {
      cursor: 1,
      runner_id: "runner",
      payload: { type: "ready", pending_approvals: [] },
    },
    {
      cursor: 2,
      runner_id: "runner",
      payload: {
        type: "event",
        event: {
          kind: "approval_requested",
          hub_session_id: "hub",
          approval_id: approval.approval_id,
          approval,
        } as never,
      },
    },
  ];
  assert.equal(pendingApprovals(base, "hub").length, 1);
  // Appending unrelated traffic must not resurrect or drop the request.
  const grown: RemoteEvent[] = [
    ...base,
    {
      ...event("item/agentMessage/delta", { itemId: "m", delta: "x" }),
      cursor: 3,
    },
  ];
  assert.equal(pendingApprovals(grown, "hub").length, 1);
  // A resolution appended later must be applied to the resumed fold.
  const resolved: RemoteEvent[] = [
    ...grown,
    {
      cursor: 4,
      runner_id: "runner",
      payload: {
        type: "event",
        event: {
          kind: "approval_resolved",
          hub_session_id: "hub",
          approval_id: approval.approval_id,
        } as never,
      },
    },
  ];
  assert.equal(pendingApprovals(resolved, "hub").length, 0);
  assert.equal(pendingQuestions(resolved, "hub").length, 0);
});

test("isTransientRemoteError identifies auto-retrying timeout and unreachable errors", () => {
  assert.equal(isTransientRemoteError(new Error("request_timeout")), true);
  assert.equal(isTransientRemoteError(new Error("gateway_unreachable")), true);
  assert.equal(
    isTransientRemoteError(new Error("session_control_lost")),
    false,
  );
  assert.equal(isTransientRemoteError(new Error("unknown")), false);
  assert.equal(isTransientRemoteError("string error"), false);
});

test("isTransientRemoteMessage identifies auto-retrying transient UI notices", () => {
  assert.equal(
    isTransientRemoteMessage(remoteError(new Error("request_timeout"))),
    true,
  );
  assert.equal(
    isTransientRemoteMessage(remoteError(new Error("gateway_unreachable"))),
    true,
  );
  assert.equal(
    isTransientRemoteMessage(remoteError(new Error("session_control_lost"))),
    false,
  );
  assert.equal(isTransientRemoteMessage(""), false);
});

// ---------------------------------------------------------------------------
// Optimistic prompt settlement
//
// The queue has to answer the same way as Android's
// `optimisticPromptLanded`/`nextOptimisticPromptNeeded` in SessionTranscript.kt:
// one landed row retires exactly one bubble, a prompt sent while another is
// still in flight waits for its own row, and one prompt ageing out never
// removes another.
// ---------------------------------------------------------------------------

/** Exactly `count` user rows — the helper the assertions below count on. */
const userRows = (count: number) =>
  Array.from({ length: count }, () => ({ kind: "user" }));

const prompt = (
  id: string,
  neededUserRows: number,
  createdAt: number,
): OptimisticPrompt => ({ id, text: `text-${id}`, neededUserRows, createdAt });

test("a repeated prompt is not confirmed by earlier rows with the same text", () => {
  // Two "你好" rows already answered, then the same phrase sent again: the
  // threshold is past the rows already there, so the new bubble survives.
  const sent = [prompt("second-hello", 3, 0)];
  assert.equal(pendingOptimisticPrompts(sent, userRows(2)).length, 1);
  // The third row is its own, and it retires the bubble.
  assert.equal(pendingOptimisticPrompts(sent, userRows(3)).length, 0);
});

test("two prompts sent in the same frame get distinct thresholds", () => {
  // Both sends read the same on-screen count, so only the queue term can keep
  // them apart; sharing a threshold would let one row confirm both.
  const first = nextOptimisticPromptNeeded([], 1);
  const second = nextOptimisticPromptNeeded([first], 1);
  assert.equal(first, 2);
  assert.equal(second, 3);

  const sent = [prompt("one", first, 0), prompt("two", second, 0)];
  assert.equal(pendingOptimisticPrompts(sent, userRows(2)).length, 1);
  assert.equal(pendingOptimisticPrompts(sent, userRows(3)).length, 0);
});

test("a prompt queued behind another does not jump ahead of it", () => {
  // The transcript already had one user row when the first prompt was sent and
  // no new row has landed since; a second send must still wait its turn even
  // though the on-screen count is now higher than the first threshold.
  const first = nextOptimisticPromptNeeded([], 1);
  const second = nextOptimisticPromptNeeded([first], 3);
  assert.equal(first, 2);
  assert.equal(second, 4);
});

test("one prompt ageing out keeps the prompt sent after it", () => {
  const sent = [prompt("old", 2, 0), prompt("fresh", 3, 89_000)];
  // Two minutes on, the first is well past the TTL; the second is 31s old.
  const { visible, kept } = pruneOptimisticPrompts(sent, userRows(1), 120_000);
  assert.deepEqual(
    kept.map((item) => item.id),
    ["fresh"],
  );
  assert.deepEqual(
    visible.map((item) => item.id),
    ["fresh"],
  );
});

test("a landed prompt leaves the queue even before its TTL", () => {
  const sent = [prompt("landed", 2, 0)];
  const { kept } = pruneOptimisticPrompts(sent, userRows(2), 1_000);
  assert.equal(kept.length, 0);
});

test("an empty queue settles without consulting the transcript", () => {
  assert.deepEqual(pendingOptimisticPrompts([], userRows(9)), []);
  assert.deepEqual(pruneOptimisticPrompts([], userRows(9), 0).kept, []);
});

test("an accepted active send_prompt awaiting turn/started stays visible without duplicating on arrival", () => {
  const scope = "device/session-gap/";
  const command = {
    command: {
      client_cmd_id: "cmd-live-1",
      command: { action: "send_prompt", text: "你好" },
    },
    receipt: {
      status: "accepted",
      result: { turn: { id: "turn-live-1" } },
    },
  } as never;

  const beforeTurnStarted = conversationItems(
    [],
    [command],
    scope,
    "turn-live-1",
  );
  assert.deepEqual(
    beforeTurnStarted.map((item) => [item.id, item.kind, item.text]),
    [["cmd-live-1", "user", "你好"]],
  );

  const turnStarted = event("turn/started", { turn: { id: "turn-live-1" } });
  const afterTurnStarted = conversationItems(
    [turnStarted],
    [command],
    scope,
    "turn-live-1",
  );
  assert.deepEqual(
    afterTurnStarted.map((item) => [item.id, item.kind, item.text]),
    [["cmd-live-1", "user", "你好"]],
  );
});

test("codex native userMessage does not duplicate a prompt already rendered from turn/started", () => {
  const scope = "device/codex-dedupe/";
  const cmd1 = {
    command: {
      client_cmd_id: "cmd-codex-1",
      command: { action: "send_prompt", text: "你好" },
    },
    receipt: {
      status: "accepted",
      result: { turn: { id: "turn-codex-1" } },
    },
  } as never;
  const cmd2 = {
    command: {
      client_cmd_id: "cmd-codex-2",
      command: { action: "send_prompt", text: "你好" },
    },
    receipt: {
      status: "accepted",
      result: { turn: { id: "turn-codex-2" } },
    },
  } as never;

  const snapshot = event("history/snapshot", {
    items: [{ id: "h0", kind: "message", role: "assistant", text: "就绪" }],
  });
  const t1Start = event("turn/started", { turn: { id: "turn-codex-1" } });
  const t1UserStarted = event("item/started", {
    turnId: "turn-codex-1",
    item: {
      id: "msg-user-1",
      type: "userMessage",
      content: [{ type: "text", text: "你好" }],
    },
  });
  const t1UserCompleted = event("item/completed", {
    turnId: "turn-codex-1",
    item: {
      id: "msg-user-1",
      type: "userMessage",
      content: [{ type: "text", text: "你好" }],
    },
  });
  const t1Reply = event("item/completed", {
    turnId: "turn-codex-1",
    item: { id: "msg-agent-1", type: "agentMessage", text: "你好！" },
  });
  const t1Done = event("turn/completed", { turn: { id: "turn-codex-1" } });

  // Incremental fold across turn/started -> userMessage -> agentMessage
  conversationItems([snapshot, t1Start], [cmd1], scope, "turn-codex-1");
  const afterTurn1 = conversationItems(
    [snapshot, t1Start, t1UserStarted, t1UserCompleted, t1Reply, t1Done],
    [cmd1],
    scope,
    null,
  );
  assert.deepEqual(
    afterTurn1.map((item) => [item.kind, item.text]),
    [
      ["assistant", "就绪"],
      ["user", "你好"],
      ["assistant", "你好！"],
    ],
  );

  // Sending the same text in a second turn still renders a second user bubble
  const t2Start = event("turn/started", { turn: { id: "turn-codex-2" } });
  const t2User = event("item/completed", {
    item: { id: "msg-user-2", type: "userMessage", text: "你好" },
  });
  const afterTurn2 = conversationItems(
    [
      snapshot,
      t1Start,
      t1UserStarted,
      t1UserCompleted,
      t1Reply,
      t1Done,
      t2Start,
      t2User,
    ],
    [cmd1, cmd2],
    scope,
    "turn-codex-2",
  );
  assert.equal(afterTurn2.filter((item) => item.kind === "user").length, 2);

  // External terminal turns (no send_prompt command) still render native userMessage
  const extStart = event("turn/started", { turn: { id: "turn-ext-3" } });
  const extUser = event("item/completed", {
    turnId: "turn-ext-3",
    item: { id: "msg-ext-3", type: "userMessage", text: "终端输入" },
  });
  const afterExternal = conversationItems(
    [snapshot, extStart, extUser],
    [],
    `${scope}ext/`,
    null,
  );
  assert.deepEqual(
    afterExternal.map((item) => [item.kind, item.text]),
    [
      ["assistant", "就绪"],
      ["user", "终端输入"],
    ],
  );
});

test("cleanBannerText strips markdown inline backticks for plain alert banners", () => {
  assert.equal(
    cleanBannerText("Failed to spawn `agy` in [local_path_redacted]"),
    "Failed to spawn agy in （本机路径已隐藏）",
  );
});

import {
  buildCommandAttemptKey,
  checkAttachmentSize,
  matchesSelectedOption,
  mergeMonotonicEvents,
  setBoundedSessionRecord,
  updateBoundedSessionState,
  type BoundedSessionState,
} from "../src/lib/remote-session-state";
import { parseQuestionData } from "../src/components/RemoteToolCard";
import { splitThinkingContent } from "../src/components/RemoteMessageCard";
import {
  isSafeRasterDataUrl,
  safeMarkdownUrlTransform,
} from "../src/components/MarkdownView";

test("markdown and thinking helpers protect fences, extract multiple blocks, and gate data URLs", () => {
  const md = [
    "<thinking>first thought</thinking>",
    "```xml\n<thinking>example only</thinking>\n```",
    "<thought>second thought</thought>",
    "Final answer.",
  ].join("\n\n");
  const parsed = splitThinkingContent(md, false);
  assert.equal(parsed.thinking, "first thought\n\nsecond thought");
  assert.equal(parsed.isThinkingLive, false);
  assert.ok(parsed.cleanText.includes("<thinking>example only</thinking>"));
  assert.ok(parsed.cleanText.includes("Final answer."));

  assert.equal(isSafeRasterDataUrl("data:image/png;base64,AAAA"), true);
  assert.equal(
    isSafeRasterDataUrl("data:image/svg+xml;base64,PHN2Zz4="),
    false,
  );
  assert.equal(
    safeMarkdownUrlTransform("data:image/svg+xml;base64,PHN2Zz4="),
    "",
  );
  assert.equal(safeMarkdownUrlTransform("javascript:alert(1)"), "");
  assert.equal(
    safeMarkdownUrlTransform("https://example.com/a.png"),
    "https://example.com/a.png",
  );
});

test("session-state helpers bound LRU records purely under StrictMode double-invoke, filter stale cursors, validate attachment sizes, and strip base64 from attemptKey", () => {
  let order: readonly string[] = Object.freeze([]);
  let store: Readonly<Record<string, number>> = Object.freeze({});
  const r1 = setBoundedSessionRecord(store, order, "s1", 1, 2);
  store = Object.freeze(r1.next);
  order = Object.freeze(r1.nextOrder);
  const r2 = setBoundedSessionRecord(store, order, "s2", 2, 2);
  store = Object.freeze(r2.next);
  order = Object.freeze(r2.nextOrder);

  // React StrictMode invokes state updaters twice with identical prev inputs.
  const firstPass = setBoundedSessionRecord(store, order, "s3", 3, 2);
  const secondPass = setBoundedSessionRecord(store, order, "s3", 3, 2);
  assert.deepEqual(firstPass, secondPass);
  assert.deepEqual(Object.keys(secondPass.next).sort(), ["s2", "s3"]);
  assert.deepEqual(secondPass.nextOrder, ["s2", "s3"]);
  assert.deepEqual(secondPass.evictedKeys, ["s1"]);
  assert.deepEqual(order, ["s1", "s2"]);

  const bounded: BoundedSessionState<number> = {
    byKey: Object.freeze({ s1: 1, s2: 2 }),
    order: Object.freeze(["s1", "s2"]),
  };
  const u1 = updateBoundedSessionState(bounded, "s3", 3, 2);
  const u2 = updateBoundedSessionState(bounded, "s3", 3, 2);
  assert.deepEqual(u1, u2);
  assert.deepEqual(u1.order, ["s2", "s3"]);
  assert.equal("s1" in u1.byKey, false);

  const e1 = { ...event("turn/started", { turn: { id: "t1" } }), cursor: 10 };
  const e2 = { ...e1, cursor: 15 };
  const eStale = { ...e1, cursor: 9 };
  const merged = mergeMonotonicEvents([e1], [eStale, e2], 10);
  assert.equal(merged.changed, true);
  assert.equal(merged.nextCursor, 15);
  assert.equal(merged.nextEvents.length, 2);

  assert.equal(checkAttachmentSize(0), "empty");
  assert.equal(checkAttachmentSize(7 * 1024 * 1024), "ok");
  assert.equal(checkAttachmentSize(8 * 1024 * 1024), "too_large");

  const largeData = "A".repeat(2048);
  const k = buildCommandAttemptKey(
    "r1",
    "h1",
    {
      action: "send_prompt",
      text: "hi",
      attachments: [
        {
          name: "img.png",
          media_type: "image/png",
          data: largeData,
          encoding: "base64",
        },
      ],
    },
    "gen1",
  );
  assert.ok(!k.includes(largeData));
  assert.equal(matchesSelectedOption("Yesterday", "Yes"), false);
  assert.equal(matchesSelectedOption("Yes", "Yes"), true);
  assert.equal(matchesSelectedOption('{"answers":{"q1":"Yes"}}', "Yes"), true);
  assert.equal(
    matchesSelectedOption("Yes", undefined as unknown as string),
    false,
  );
  assert.equal(matchesSelectedOption('{"answers":null}', "Yes"), false);
});

test("parseQuestionData rejects malformed questions JSON and sanitizes non-string optional fields", () => {
  assert.equal(parseQuestionData(undefined), null);
  assert.equal(parseQuestionData("not json"), null);
  assert.equal(parseQuestionData('{"questions":[]}'), null);
  assert.equal(parseQuestionData('{"questions":[null]}'), null);
  assert.equal(parseQuestionData('{"questions":[{"question":123}]}'), null);
  assert.equal(
    parseQuestionData('{"questions":[{"question":"Pick","options":"bad"}]}'),
    null,
  );
  assert.equal(
    parseQuestionData('{"questions":[{"question":"Pick","options":[null]}]}'),
    null,
  );
  assert.equal(
    parseQuestionData(
      '{"questions":[{"question":"Pick","options":[{"label":99}]}]}',
    ),
    null,
  );

  const parsed = parseQuestionData(
    JSON.stringify({
      questions: [
        {
          question: "Continue?",
          header: { unexpected: true },
          options: [
            { label: "Yes", description: "Proceed", preview: { bad: 1 } },
          ],
          multiSelect: false,
        },
      ],
    }),
  );
  assert.deepEqual(parsed, [
    {
      question: "Continue?",
      options: [{ label: "Yes", description: "Proceed" }],
      multiSelect: false,
    },
  ]);
});
test("remoteRequest aborts and throws request_timeout when response.text() stalls past timeoutMs", async (t) => {
  const originalFetch = globalThis.fetch;
  t.after(() => {
    globalThis.fetch = originalFetch;
  });

  globalThis.fetch = (async (_input: unknown, init?: RequestInit) =>
    ({
      ok: true,
      status: 200,
      text: () =>
        new Promise<string>((_resolve, reject) => {
          init?.signal?.addEventListener(
            "abort",
            () => reject(new DOMException("Aborted", "AbortError")),
            { once: true },
          );
        }),
    }) as unknown as Response) as typeof fetch;

  await assert.rejects(
    () => remoteRequest("/runners/dev_a/hello", undefined, 25),
    (err: unknown) => err instanceof Error && err.message === "request_timeout",
  );
});

test("trimSupersededEvents retains pre-snapshot turn/completed and accepted response receipts so resolved approvals/questions never resurrect", () => {
  const approval = {
    approval_id: "apr-pre-snap",
    hub_session_id: "hub-trim",
    host_generation: "gen-1",
    turn_id: "turn-1",
    can_approve: true,
    can_decline: true,
  };
  const question = {
    question_id: "q-pre-snap",
    hub_session_id: "hub-trim",
    host_generation: "gen-1",
    turn_id: "turn-1",
    prompt: "Proceed?",
  };
  const unresolvedApproval = {
    approval_id: "apr-still-open",
    hub_session_id: "hub-trim",
    host_generation: "gen-1",
    turn_id: "turn-2",
    can_approve: true,
    can_decline: true,
  };

  const preEvents: RemoteEvent[] = [
    {
      cursor: 1,
      runner_id: "runner",
      payload: {
        type: "event",
        event: {
          kind: "approval_requested",
          hub_session_id: "hub-trim",
          approval_id: approval.approval_id,
          approval,
        } as never,
      },
    },
    {
      cursor: 2,
      runner_id: "runner",
      payload: {
        type: "receipt",
        receipt: {
          client_cmd_id: "cmd-apr-1",
          hub_session_id: "hub-trim",
          status: "accepted",
          result: { response_sent: true, approval_id: approval.approval_id },
        } as never,
      },
    },
    {
      cursor: 3,
      runner_id: "runner",
      payload: {
        type: "event",
        event: {
          kind: "question_requested",
          hub_session_id: "hub-trim",
          question_id: question.question_id,
          question,
        } as never,
      },
    },
    {
      cursor: 4,
      runner_id: "runner",
      payload: {
        type: "event",
        event: {
          kind: "turn/completed",
          binding: { hub_session_id: "hub-trim" },
          raw: { params: { turn: { id: "turn-1", status: "completed" } } },
        } as never,
      },
    },
    {
      cursor: 5,
      runner_id: "runner",
      payload: {
        type: "event",
        event: {
          kind: "approval_requested",
          hub_session_id: "hub-trim",
          approval_id: unresolvedApproval.approval_id,
          approval: unresolvedApproval,
        } as never,
      },
    },
  ];
  for (let i = 0; i < 70; i++) {
    preEvents.push({
      ...event("item/agentMessage/delta", { itemId: "m1", delta: "x" }),
      cursor: 10 + i,
    });
  }
  const snapshotEvt: RemoteEvent = {
    ...event("history/snapshot", {
      items: [{ id: "h1", kind: "message", role: "assistant", text: "done" }],
    }),
    cursor: 200,
  };
  const trimmed = trimSupersededEvents([...preEvents, snapshotEvt]);
  assert.ok(trimmed.length < preEvents.length + 1);
  assert.deepEqual(
    pendingApprovals(trimmed, "hub-trim").map((a) => a.approval_id),
    ["apr-still-open"],
  );
  assert.deepEqual(pendingQuestions(trimmed, "hub-trim"), []);
});

test("incremental conversationItems clones frozen fallback tool items without mutating prior render objects", () => {
  const scope = "device/frozen-tool-fallback/";
  const snapshotEvt: RemoteEvent = {
    ...event("history/snapshot", {
      items: [{ id: "h0", kind: "message", role: "user", text: "run check" }],
    }),
    cursor: 1,
  };
  const turnStart: RemoteEvent = {
    ...event("turn/started", { turn: { id: "turn-freeze-1" } }),
    cursor: 2,
  };
  const bareToolDelta: RemoteEvent = {
    ...event("item/agentMessage/delta", {
      itemId: "bare-1",
      turnId: "turn-freeze-1",
      delta: JSON.stringify({ command: "echo hi", description: "greet" }),
    }),
    cursor: 3,
  };

  const first = conversationItems(
    [snapshotEvt, turnStart, bareToolDelta],
    [],
    scope,
    "turn-freeze-1",
  );
  const frozenBefore = first.find((item) => item.kind === "tool");
  assert.ok(frozenBefore);
  assert.equal(frozenBefore.status, "running");
  Object.freeze(frozenBefore);

  // Match via runningToolsByTurn with a different item id; must copy instead of mutating frozenBefore.
  const cmdStartByTurn: RemoteEvent = {
    ...event("item/started", {
      turnId: "turn-freeze-1",
      item: {
        id: "exec-turn-1",
        type: "commandExecution",
        command: "echo hi",
        output: "starting...\n",
        status: "inProgress",
      },
    }),
    cursor: 4,
  };
  const second = conversationItems(
    [snapshotEvt, turnStart, bareToolDelta, cmdStartByTurn],
    [],
    scope,
    "turn-freeze-1",
  );
  const afterStart = second.find((item) => item.kind === "tool");
  assert.ok(afterStart);
  assert.notEqual(afterStart, frozenBefore);
  assert.equal(frozenBefore.output, undefined);
  assert.equal(afterStart.output, "starting...\n");
  Object.freeze(afterStart);

  // Now match via runningToolsByCmd (without turnId) on completion.
  const cmdDoneByCmd: RemoteEvent = {
    ...event("item/completed", {
      item: {
        id: "exec-cmd-2",
        type: "commandExecution",
        command: "echo hi",
        aggregatedOutput: "hi\n",
        status: "completed",
      },
    }),
    cursor: 5,
  };
  const third = conversationItems(
    [snapshotEvt, turnStart, bareToolDelta, cmdStartByTurn, cmdDoneByCmd],
    [],
    scope,
    "turn-freeze-1",
  );
  const afterDone = third.find((item) => item.kind === "tool");
  assert.ok(afterDone);
  assert.notEqual(afterDone, afterStart);
  assert.equal(frozenBefore.status, "running");
  assert.equal(afterStart.status, "running");
  assert.equal(afterDone.status, "completed");
  assert.equal(afterDone.output, "hi\n");
});
