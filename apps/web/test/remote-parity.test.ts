import assert from "node:assert/strict";
import test from "node:test";
import {
  conversationItems,
  inferModelContextWindow,
  latestContextUsage,
  type RemoteEvent,
} from "../src/lib/remote";
import { sanitizeSessionTitle } from "../src/lib/session-tree";

function evt(
  kind: string,
  params: Record<string, unknown>,
  cursor = 1,
): RemoteEvent {
  return {
    cursor,
    runner_id: "runner-parity",
    payload: {
      type: "event",
      event: {
        kind,
        raw: { params },
      },
    },
  };
}

test("history/snapshot preserves thinking blocks and splits tool result markers + diffs", () => {
  const snapshot = evt("history/snapshot", {
    items: [
      { role: "user", kind: "message", text: "重构模块并运行测试" },
      {
        role: "assistant",
        kind: "thinking",
        label: "思考过程",
        text: "先阅读源码再生成补丁。",
      },
      {
        role: "assistant",
        kind: "tool",
        label: "edit_file · src/lib.rs",
        text: "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n\n〔结果〕\nEdited successfully.",
        status: "completed",
      },
      {
        role: "assistant",
        kind: "thinking",
        label: "思考过程",
        text: "补丁应用完毕，总结修改结果。",
      },
      {
        role: "assistant",
        kind: "message",
        text: "已完成修改并通过验证。",
      },
    ],
  });

  const items = conversationItems([snapshot], [], "parity-hist:");
  assert.equal(items.length, 4);
  assert.equal(items[0]?.kind, "user");
  assert.equal(items[1]?.kind, "assistant");
  assert.equal(items[1]?.text, "<thinking>先阅读源码再生成补丁。</thinking>");
  assert.equal(items[2]?.kind, "tool");
  assert.equal(items[2]?.title, "edit_file · src/lib.rs");
  assert.equal(
    items[2]?.command,
    "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new",
  );
  assert.equal(items[2]?.output, "Edited successfully.");
  assert.equal(items[3]?.kind, "assistant");
  assert.equal(
    items[3]?.text,
    "<thinking>补丁应用完毕，总结修改结果。</thinking>\n\n已完成修改并通过验证。",
  );
});

test("live streaming folds reasoning deltas, command output deltas, and Codex fileChange changes", () => {
  const events: RemoteEvent[] = [
    evt("turn/started", { turn: { id: "turn-codex-1" } }, 1),
    evt(
      "item/reasoning/summaryTextDelta",
      { itemId: "rsn-1", delta: "正在检查 " },
      2,
    ),
    evt(
      "item/reasoning/summaryTextDelta",
      { itemId: "rsn-1", delta: "Cargo.toml" },
      3,
    ),
    evt(
      "item/started",
      {
        item: {
          id: "cmd-1",
          type: "commandExecution",
          command: "cargo test",
          status: "inProgress",
        },
      },
      4,
    ),
    evt(
      "item/commandExecution/outputDelta",
      { itemId: "cmd-1", delta: "running 5 tests\n" },
      5,
    ),
    evt(
      "item/commandExecution/outputDelta",
      { itemId: "cmd-1", delta: "test result: ok.\n" },
      6,
    ),
    evt(
      "item/completed",
      {
        item: {
          id: "patch-1",
          type: "fileChange",
          status: "completed",
          changes: [
            {
              path: "crates/runner/src/main.rs",
              diff: "@@ -10,2 +10,3 @@\n fn main() {\n+    init();\n }",
            },
          ],
        },
      },
      7,
    ),
    evt(
      "turn/completed",
      { turn: { id: "turn-codex-1", status: "completed" } },
      8,
    ),
  ];

  const items = conversationItems(events, [], "parity-live:");
  assert.equal(items.length, 3);
  assert.equal(items[0]?.text, "<thinking>正在检查 Cargo.toml</thinking>");
  assert.equal(items[1]?.kind, "tool");
  assert.equal(items[1]?.command, "cargo test");
  assert.equal(items[1]?.output, "running 5 tests\ntest result: ok.\n");
  assert.equal(items[1]?.status, "completed");
  assert.equal(items[2]?.title, "文件修改 · main.rs");
  assert.match(
    items[2]?.command ?? "",
    /^--- a\/crates\/runner\/src\/main\.rs/u,
  );
});

test("Antigravity & Codex context windows and title sanitization strip metadata wrappers", () => {
  assert.equal(
    inferModelContextWindow("gemini-4-carbon-high", "antigravity"),
    1_000_000,
  );
  assert.equal(
    inferModelContextWindow("claude-opus-4-6-thinking", "antigravity"),
    200_000,
  );

  const usageEvt = evt("thread/tokenUsage/updated", {
    tokenUsage: {
      last: { totalTokens: 64_200 },
      modelContextWindow: 400_000,
    },
  });
  assert.deepEqual(latestContextUsage([usageEvt]), {
    used: 64_200,
    window: 400_000,
    source: "reported",
  });

  assert.equal(
    sanitizeSessionTitle(
      "<USER_SETTINGS_CHANGE>Model Selection: Gemini 4 Carbon</USER_SETTINGS_CHANGE><USER_REQUEST>完善 Codex 与 Antigravity</USER_REQUEST>",
    ),
    "完善 Codex 与 Antigravity",
  );
  assert.equal(
    sanitizeSessionTitle(
      "<environment_context><cwd>/repo</cwd></environment_context>修复 subagent 折叠展示",
    ),
    "修复 subagent 折叠展示",
  );
});
