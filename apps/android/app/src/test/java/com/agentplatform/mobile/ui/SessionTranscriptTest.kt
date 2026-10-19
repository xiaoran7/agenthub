package com.agentplatform.mobile.ui

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Transcript contract tests. Every case mirrors a scenario the browser console
 * (`apps/web/test/remote.test.ts`) already covers, so a regression on either
 * client shows up in both test suites.
 */
class SessionTranscriptTest {
    private fun envelope(kind: String, params: JSONObject, cursor: Number = 1): JSONObject =
        JSONObject()
            .put("cursor", cursor)
            .put(
                "payload",
                JSONObject()
                    .put("type", "event")
                    .put(
                        "event",
                        JSONObject()
                            .put("kind", kind)
                            .put("raw", JSONObject().put("params", params)),
                    ),
            )

    private fun delta(itemId: String, text: String, cursor: Number = 1) =
        envelope(
            "item/agentMessage/delta",
            JSONObject().put("itemId", itemId).put("delta", text),
            cursor,
        )

    private fun completed(item: JSONObject, cursor: Number = 1) =
        envelope("item/completed", JSONObject().put("item", item), cursor)

    private fun started(item: JSONObject, cursor: Number = 1) =
        envelope("item/started", JSONObject().put("item", item), cursor)

    private fun history(items: JSONArray, cursor: Number = 2) =
        envelope("history/snapshot", JSONObject().put("items", items), cursor)

    /** Mirrors `RemoteCommandRecord`: `{ command: RemoteCommand, receipt }`. */
    private fun promptCommand(turn: String, text: String): JSONObject =
        JSONObject()
            .put(
                "command",
                JSONObject()
                    .put("client_cmd_id", "cmd-" + turn)
                    .put("hub_session_id", "hub-one")
                    .put("host_generation", "generation-one")
                    .put(
                        "command",
                        JSONObject()
                            .put("action", "send_prompt")
                            .put("text", text),
                    ),
            )
            .put(
                "receipt",
                JSONObject()
                    .put("status", "accepted")
                    .put(
                        "result",
                        JSONObject().put("turn", JSONObject().put("id", turn)),
                    ),
            )

    @Test
    fun `final agent message replaces deltas instead of duplicating the answer`() {
        val items =
            buildTranscript(
                listOf(
                    delta("msg-1", "你"),
                    delta("msg-1", "好"),
                    completed(JSONObject().put("id", "msg-1").put("type", "agentMessage").put("text", "你好")),
                ),
                emptyList(),
                "hub-one",
                live = true,
            )

        assertEquals(1, items.size)
        assertEquals("你好", items[0].text)
        assertEquals(TranscriptKind.ASSISTANT, items[0].kind)
        assertTrue(items[0].streaming)
    }

    @Test
    fun `turn completion stops the streaming caret`() {
        val events =
            listOf(
                envelope("turn/started", JSONObject().put("turn", JSONObject().put("id", "turn-one"))),
                delta("msg-1", "完成"),
                envelope("turn/completed", JSONObject().put("turn", JSONObject().put("id", "turn-one"))),
            )
        val items = buildTranscript(events, emptyList(), "hub-one", live = false)

        assertFalse(items.single { it.kind == TranscriptKind.ASSISTANT }.streaming)
    }

    @Test
    fun `history snapshot seeds a resumed session and drops stale replay`() {
        val stale = completed(JSONObject().put("id", "old").put("type", "agentMessage").put("text", "stale"))
        val snapshot =
            history(
                JSONArray()
                    .put(JSONObject().put("role", "user").put("kind", "message").put("text", "开始"))
                    .put(JSONObject().put("role", "assistant").put("kind", "message").put("text", "已完成"))
                    .put(
                        JSONObject()
                            .put("role", "assistant")
                            .put("kind", "tool")
                            .put("label", "Read")
                            .put("text", "读取结果")
                            .put("status", "completed"),
                    ),
                cursor = 2,
            )
        val live =
            completed(JSONObject().put("id", "new").put("type", "agentMessage").put("text", "继续执行"), cursor = 3)

        val items = buildTranscript(listOf(stale, snapshot, live), emptyList(), "hub-one")

        assertEquals(listOf("开始", "已完成", "读取结果", "继续执行"), items.map { it.text })
        assertEquals(TranscriptKind.USER, items[0].kind)
        assertEquals("Read", items[2].title)
        assertEquals(ToolState.COMPLETED, items[2].state)
    }

    @Test
    fun `a bare tool JSON delta becomes one tool card merged with its completion`() {
        val call =
            """{"command":"cargo test","description":"Run tests"}"""
        val events =
            listOf(
                delta("tool-1", call),
                completed(
                    JSONObject()
                        .put("id", "tool-1")
                        .put("type", "commandExecution")
                        .put("command", "cargo test")
                        .put("output", "ok")
                        .put("status", "completed"),
                ),
            )

        val items = buildTranscript(events, emptyList(), "hub-one")

        assertEquals(1, items.size)
        assertEquals(TranscriptKind.TOOL, items.single().kind)
        assertEquals("cargo test", items.single().command)
        assertEquals("ok", items.single().output)
        assertEquals(ToolState.COMPLETED, items.single().state)
    }

    @Test
    fun `started and completed executions are one row, never a stuck running duplicate`() {
        val running =
            started(
                JSONObject()
                    .put("id", "cmd-one")
                    .put("type", "commandExecution")
                    .put("command", "npm test"),
            )
        val done =
            completed(
                JSONObject()
                    .put("id", "cmd-one")
                    .put("type", "commandExecution")
                    .put("command", "npm test")
                    .put("status", "failed")
                    .put("exitCode", 1),
                cursor = 2,
            )

        val items = buildTranscript(listOf(running, done), emptyList(), "hub-one")

        assertEquals(1, items.size)
        assertEquals(ToolState.FAILED, items.single().state)
    }

    @Test
    fun `a turn ending settles any running tool marker`() {
        // Claude emits a generic activity marker but never a completion item.
        val events =
            listOf(
                started(JSONObject().put("id", "claude_tool").put("type", "commandExecution")),
                envelope("turn/completed", JSONObject().put("turn", JSONObject().put("id", "turn-one"))),
            )

        val items = buildTranscript(events, emptyList(), "hub-one")

        assertEquals(ToolState.COMPLETED, items.single().state)
    }

    @Test
    fun `tool calls carry server and tool in the label`() {
        val row =
            completed(
                JSONObject()
                    .put("id", "tool-call")
                    .put("type", "toolCall")
                    .put("server", "mcp")
                    .put("tool", "fetch")
                    .put("status", "completed"),
            )

        val items = buildTranscript(listOf(row), emptyList(), "hub-one")

        assertEquals("mcp · fetch", items.single().title)
        assertEquals(ToolState.COMPLETED, items.single().state)
    }

    @Test
    fun `a prompt receipt names the turn that produced the answer`() {
        val events =
            listOf(
                envelope("turn/started", JSONObject().put("turn", JSONObject().put("id", "turn-7"))),
                completed(JSONObject().put("id", "msg-1").put("type", "agentMessage").put("text", "done")),
            )
        val commands = listOf(promptCommand("turn-7", "修一下登录"))

        val items = buildTranscript(events, commands, "hub-one")

        assertEquals(2, items.size)
        assertEquals(TranscriptKind.USER, items[0].kind)
        assertEquals("修一下登录", items[0].text)
        assertEquals(TranscriptKind.ASSISTANT, items[1].kind)
    }

    @Test
    fun `approval and question state is scoped to the open session`() {
        val approval =
            JSONObject()
                .put("approval_id", "approval-a")
                .put("hub_session_id", "hub-one")
        val other =
            JSONObject()
                .put("approval_id", "approval-b")
                .put("hub_session_id", "hub-two")
        val ready =
            JSONObject()
                .put("cursor", 1)
                .put(
                    "payload",
                    JSONObject()
                        .put("type", "ready")
                        .put("pending_approvals", JSONArray().put(approval).put(other)),
                )

        assertEquals(setOf("approval-a"), remoteApprovals(listOf(ready), "hub-one").keys)
        assertTrue(remoteApprovals(listOf(ready), "hub-two").containsKey("approval-b"))
    }

    @Test
    fun `a failed turn keeps the provider reason and clears on the next turn`() {
        val failure =
            envelope(
                "error",
                JSONObject()
                    .put("turnId", "turn-9")
                    .put(
                        "error",
                        JSONObject().put("message", "usage limit reached"),
                    ),
            )
        val startedTurn = envelope("turn/started", JSONObject().put("turn", JSONObject().put("id", "turn-10")))

        val reported = latestTurnFailure(listOf(failure), "hub-one")
        assertEquals("usage limit reached", reported?.message)
        assertFalse(reported?.retrying ?: true)
        assertNull(latestTurnFailure(listOf(failure, startedTurn), "hub-one"))
    }

    @Test
    fun `a retrying error is marked as retrying, not as the final outcome`() {
        val retrying =
            envelope(
                "error",
                JSONObject()
                    .put("willRetry", true)
                    .put("error", JSONObject().put("message", "rate limited")),
            )

        assertTrue(latestTurnFailure(listOf(retrying), "hub-one")?.retrying == true)
    }

    @Test
    fun `context usage keeps the newest report and stays scoped to its session`() {
        val scoped =
            JSONObject()
                .put("payload", JSONObject()
                    .put("type", "event")
                    .put("event", JSONObject()
                        .put("kind", "thread/tokenUsage/updated")
                        .put("binding", JSONObject().put("hub_session_id", "hub-one"))
                        .put("raw", JSONObject().put("params", JSONObject().put(
                            "tokenUsage",
                            JSONObject().put("used", 42_000).put("window", 200_000),
                        )))))
        val other =
            JSONObject()
                .put("payload", JSONObject()
                    .put("type", "event")
                    .put("event", JSONObject()
                        .put("kind", "thread/tokenUsage/updated")
                        .put("binding", JSONObject().put("hub_session_id", "hub-two"))
                        .put("raw", JSONObject().put("params", JSONObject().put(
                            "tokenUsage",
                            JSONObject().put("used", 900_000).put("window", 1_000_000),
                        )))))
        val newest =
            JSONObject()
                .put("payload", JSONObject()
                    .put("type", "event")
                    .put("event", JSONObject()
                        .put("kind", "thread/tokenUsage/updated")
                        .put("binding", JSONObject().put("hub_session_id", "hub-one"))
                        .put("raw", JSONObject().put("params", JSONObject().put(
                            "tokenUsage",
                            JSONObject().put("used", 51_000).put("window", 200_000),
                        )))))

        assertEquals(ContextUsage(42_000, 200_000), latestContextUsage(listOf(scoped, other), "hub-one"))
        assertEquals(ContextUsage(51_000, 200_000), latestContextUsage(listOf(scoped, other, newest), "hub-one"))
        assertNull(latestContextUsage(listOf(scoped), "hub-three"))
    }

    @Test
    fun `a usage report without a window still reports what was used`() {
        val report =
            JSONObject()
                .put("payload", JSONObject()
                    .put("type", "event")
                    .put("event", JSONObject()
                        .put("kind", "thread/tokenUsage/updated")
                        .put("raw", JSONObject().put("params", JSONObject().put(
                            "tokenUsage",
                            JSONObject().put("used", 12_345),
                        )))))

        assertEquals(ContextUsage(12_345, null), latestContextUsage(listOf(report), "hub-one"))
    }
}
