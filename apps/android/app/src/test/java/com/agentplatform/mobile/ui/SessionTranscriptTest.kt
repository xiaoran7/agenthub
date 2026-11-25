package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.DiscoveryCandidate
import com.agentplatform.mobile.data.RemoteCommandRecord
import com.agentplatform.mobile.data.RemoteEvent
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
    private fun envelope(kind: String, params: JSONObject, cursor: Number = 1): RemoteEvent =
        RemoteEvent.parse(
            JSONObject()
                .put("cursor", cursor)
                .put("runner_id", "runner-a")
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

    /** `ready` frame: the authoritative full set of pending interventions. */
    private fun ready(approvals: JSONArray, questions: JSONArray? = null, cursor: Number = 1): RemoteEvent =
        RemoteEvent.parse(
            JSONObject()
                .put("cursor", cursor)
                .put(
                    "payload",
                    JSONObject()
                        .put("type", "ready")
                        .put("pending_approvals", approvals)
                        .apply { if (questions != null) put("pending_questions", questions) },
                ),
        )

    /** Mirrors `RemoteCommandRecord`: `{ command: RemoteCommand, receipt }`. */
    private fun promptCommand(turn: String, text: String): RemoteCommandRecord =
        RemoteCommandRecord.parse(
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
    fun `a bare tool JSON delta retains its display key when the real event arrives`() {
        val call = """{"command":"cargo test","description":"Run tests"}"""
        val provisional = delta("tool-1", call)
        for (completedId in listOf("tool-1", "command-1")) {
            val done =
                completed(
                    JSONObject()
                        .put("id", completedId)
                        .put("type", "commandExecution")
                        .put("command", "cargo test")
                        .put("output", "ok")
                        .put("status", "completed"),
                    cursor = 2,
                )
            val fold = IncrementalTranscriptFold()
            val opened = fold.update(listOf(provisional), emptyList(), "hub-one", live = true).single()
            val events = listOf(provisional, done)
            val items = fold.update(events, emptyList(), "hub-one", live = false)
            val tool = items.single()
            assertEquals(buildTranscript(events, emptyList(), "hub-one"), items)
            assertEquals(completedId, tool.id)
            assertEquals(opened.displayKey, tool.displayKey)
            assertEquals(TranscriptKind.TOOL, tool.kind)
            assertEquals("cargo test", tool.command)
            assertEquals("ok", tool.output)
            assertEquals(ToolState.COMPLETED, tool.state)
            if (completedId != "tool-1") {
                val reused = events + delta("tool-1", call, cursor = 3)
                val revised = fold.update(reused, emptyList(), "hub-one", live = true)
                assertEquals(buildTranscript(reused, emptyList(), "hub-one", live = true), revised)
                assertEquals(2, revised.map { it.displayKey }.distinct().size)
            }
        }
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
        val mine =
            JSONObject()
                .put("approval_id", "approval-a")
                .put("hub_session_id", "hub-one")
        val other =
            JSONObject()
                .put("approval_id", "approval-b")
                .put("hub_session_id", "hub-two")
        val frame = ready(JSONArray().put(mine).put(other))

        assertEquals(listOf("approval-a"), pendingApprovals(listOf(frame), "hub-one").map { it.approvalId })
        assertEquals(listOf("approval-b"), pendingApprovals(listOf(frame), "hub-two").map { it.approvalId })
    }

    @Test
    fun `an empty ready frame clears approvals resolved on another device`() {
        val approval =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 1)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "event")
                            .put(
                                "event",
                                JSONObject()
                                    .put("kind", "approval_requested")
                                    .put(
                                        "approval",
                                        JSONObject()
                                            .put("approval_id", "approval-a")
                                            .put("hub_session_id", "hub-one"),
                                    ),
                            ),
                    ),
            )

        assertEquals(1, pendingApprovals(listOf(approval), "hub-one").size)
        // The authoritative set is empty: the button is gone everywhere.
        assertEquals(0, pendingApprovals(listOf(approval, ready(JSONArray(), cursor = 2)), "hub-one").size)
    }

    @Test
    fun `an accepted receipt retires its approval`() {
        val requested =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 1)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "event")
                            .put(
                                "event",
                                JSONObject()
                                    .put("kind", "approval_requested")
                                    .put(
                                        "approval",
                                        JSONObject()
                                            .put("approval_id", "approval-a")
                                            .put("hub_session_id", "hub-one"),
                                    ),
                            ),
                    ),
            )
        val accepted =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 2)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "receipt")
                            .put(
                                "receipt",
                                JSONObject()
                                    .put("status", "accepted")
                                    .put(
                                        "result",
                                        JSONObject()
                                            .put("approval_id", "approval-a")
                                            .put("response_sent", true),
                                    ),
                            ),
                    ),
            )

        assertEquals(0, pendingApprovals(listOf(requested, accepted), "hub-one").size)
    }

    @Test
    fun `a loose approval_resolved event also retires the approval`() {
        val requested =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 1)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "event")
                            .put(
                                "event",
                                JSONObject()
                                    .put("kind", "approval_requested")
                                    .put(
                                        "approval",
                                        JSONObject()
                                            .put("approval_id", "approval-a")
                                            .put("hub_session_id", "hub-one"),
                                    ),
                            ),
                    ),
            )
        val resolved =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 2)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "event")
                            .put(
                                "event",
                                JSONObject()
                                    .put("kind", "approval_resolved")
                                    .put("approval_id", "approval-a"),
                            ),
                    ),
            )

        assertEquals(0, pendingApprovals(listOf(requested, resolved), "hub-one").size)
    }

    @Test
    fun `questions are tracked separately from approvals`() {
        val question =
            JSONObject()
                .put("question_id", "q-1")
                .put("hub_session_id", "hub-one")
                .put("prompt", "用哪个分支？")
                .put(
                    "options",
                    JSONArray().put(JSONObject().put("label", "main")),
                )
        val frame = ready(JSONArray(), JSONArray().put(question))

        assertEquals(1, pendingQuestions(listOf(frame), "hub-one").size)
        assertEquals(0, pendingApprovals(listOf(frame), "hub-one").size)
        assertEquals("用哪个分支？", pendingQuestions(listOf(frame), "hub-one").single().prompt)
        assertEquals(0, pendingQuestions(listOf(frame), "hub-two").size)
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
        val scoped = usageFrame("hub-one", used = 42_000, window = 200_000, cursor = 1)
        val other = usageFrame("hub-two", used = 900_000, window = 1_000_000, cursor = 2)
        val newest = usageFrame("hub-one", used = 51_000, window = 200_000, cursor = 3)

        assertEquals(ContextUsage(42_000, 200_000), latestContextUsage(listOf(scoped, other), "hub-one"))
        assertEquals(ContextUsage(51_000, 200_000), latestContextUsage(listOf(scoped, other, newest), "hub-one"))
        assertNull(latestContextUsage(listOf(scoped), "hub-three"))
    }

    @Test
    fun `a usage report without a window still reports what was used`() {
        assertEquals(
            ContextUsage(12_345, null),
            latestContextUsage(listOf(usageFrame("hub-one", used = 12_345, window = null)), "hub-one"),
        )
    }

    @Test
    fun `a session with no usage frame falls back to what discovery measured`() {
        val candidate =
            DiscoveryCandidate(
                providerSessionId = "claude_abc",
                workspacePath = "/w/one",
                title = null,
                provider = "claude_code",
                model = "claude-sonnet-5",
                totalTokens = null,
                lastActivityAtMs = null,
                resumable = true,
                contextWindowTokens = 200_000,
                contextUsedTokens = 61_000,
            )

        assertEquals(
            ContextUsage(61_000, 200_000),
            latestContextUsage(emptyList(), "hub-one", candidate),
        )
    }

    @Test
    fun `the discovery fallback infers a window when the host measured none`() {
        val candidate =
            DiscoveryCandidate(
                providerSessionId = "gemini_abc",
                workspacePath = "/w/one",
                title = null,
                provider = "gemini_cli",
                model = "gemini-3.8-flash",
                totalTokens = null,
                lastActivityAtMs = null,
                resumable = true,
            )

        assertEquals(ContextUsage(4_000, 1_000_000), latestContextUsage(emptyList(), "hub-one", candidate.copy(promptTokens = 4_000)))
    }

    @Test
    fun `a live usage report wins over the discovery fallback`() {
        val candidate =
            DiscoveryCandidate(
                providerSessionId = "claude_abc",
                workspacePath = "/w/one",
                title = null,
                provider = "claude_code",
                model = "claude-sonnet-5",
                totalTokens = null,
                lastActivityAtMs = null,
                resumable = true,
                contextWindowTokens = 200_000,
                contextUsedTokens = 61_000,
            )
        val live = usageFrame("hub-one", used = 5_000, window = 200_000)

        assertEquals(ContextUsage(5_000, 200_000), latestContextUsage(listOf(live), "hub-one", candidate))
    }

    @Test
    fun `a discovery record with no token count yields nothing rather than a fake zero`() {
        val candidate =
            DiscoveryCandidate(
                providerSessionId = "claude_abc",
                workspacePath = "/w/one",
                title = null,
                provider = "claude_code",
                model = "claude-sonnet-5",
                totalTokens = null,
                lastActivityAtMs = null,
                resumable = true,
            )

        assertNull(latestContextUsage(emptyList(), "hub-one", candidate))
    }

    @Test
    fun `no usage and no discovery record means no ring at all`() {
        assertNull(latestContextUsage(emptyList(), "hub-one", null))
    }

    private fun usageFrame(hub: String, used: Long, window: Long?, cursor: Number = 1): RemoteEvent {
        val tokenUsage = JSONObject().put("used", used)
        if (window != null) tokenUsage.put("window", window)
        return RemoteEvent.parse(
            JSONObject()
                .put("cursor", cursor)
                .put(
                    "payload",
                    JSONObject()
                        .put("type", "event")
                        .put(
                            "event",
                            JSONObject()
                                .put("kind", "thread/tokenUsage/updated")
                                .put("binding", JSONObject().put("hub_session_id", hub))
                                .put(
                                    "raw",
                                    JSONObject()
                                        .put("params", JSONObject().put("tokenUsage", tokenUsage)),
                                ),
                        ),
                ),
        )
    }

    @Test
    fun `incremental fold matches full buildTranscript across delta appends and snapshots`() {
        val fold = IncrementalTranscriptFold()
        val snap =
            history(
                JSONArray()
                    .put(JSONObject().put("role", "user").put("kind", "message").put("text", "你好"))
                    .put(JSONObject().put("role", "assistant").put("kind", "message").put("text", "请说")),
                cursor = 1,
            )
        val e1 = delta("msg-2", "正在", cursor = 2)
        val e2 = delta("msg-2", "处理", cursor = 3)
        val e3 = completed(JSONObject().put("id", "msg-2").put("type", "agentMessage").put("text", "正在处理"), cursor = 4)

        val step1 = fold.update(listOf(snap, e1), emptyList(), "hub-one", live = true)
        assertEquals(buildTranscript(listOf(snap, e1), emptyList(), "hub-one", live = true), step1)

        val step2 = fold.update(listOf(snap, e1, e2), emptyList(), "hub-one", live = true)
        assertEquals(buildTranscript(listOf(snap, e1, e2), emptyList(), "hub-one", live = true), step2)

        val step3 = fold.update(listOf(snap, e1, e2, e3), emptyList(), "hub-one", live = false)
        assertEquals(buildTranscript(listOf(snap, e1, e2, e3), emptyList(), "hub-one", live = false), step3)

        // New snapshot supersedes earlier buffer
        val snap2 =
            history(
                JSONArray().put(JSONObject().put("role", "user").put("kind", "message").put("text", "下一轮")),
                cursor = 5,
            )
        val step4 = fold.update(listOf(snap, e1, e2, e3, snap2), emptyList(), "hub-one", live = false)
        assertEquals(buildTranscript(listOf(snap, e1, e2, e3, snap2), emptyList(), "hub-one", live = false), step4)
    }

    @Test
    fun `groupTranscript filters isolated redacted path bubbles`() {
        val items =
            listOf(
                TranscriptItem(id = "u1", kind = TranscriptKind.USER, text = "查看目录"),
                TranscriptItem(id = "a1", kind = TranscriptKind.ASSISTANT, text = "[local_path_redacted]"),
                TranscriptItem(id = "a2", kind = TranscriptKind.ASSISTANT, text = "已完成"),
            )
        val blocks = groupTranscript(items)
        assertEquals(2, blocks.size)
        assertEquals(listOf("u1", "a2"), blocks.map { (it as ConversationBlock.Single).item.id })
    }

    @Test
    fun `analyzeSessionSignals matches individual signal extractors in one pass`() {
        val appr =
            envelope(
                "approval_requested",
                JSONObject(),
                cursor = 1,
            ).let {
                RemoteEvent.parse(
                    JSONObject()
                        .put("cursor", 1)
                        .put(
                            "payload",
                            JSONObject()
                                .put("type", "event")
                                .put(
                                    "event",
                                    JSONObject()
                                        .put("kind", "approval_requested")
                                        .put(
                                            "approval",
                                            JSONObject()
                                                .put("approval_id", "appr-1")
                                                .put("hub_session_id", "hub-one")
                                                .put("host_generation", "g1")
                                                .put("turn_id", "t1"),
                                        ),
                                ),
                        ),
                )
            }
        val usage = usageFrame("hub-one", used = 9_000, window = 200_000, cursor = 2)
        val events = listOf(appr, usage)

        val signals = analyzeSessionSignals(events, "hub-one")
        assertEquals(pendingApprovals(events, "hub-one"), signals.approvals)
        assertEquals(pendingQuestions(events, "hub-one"), signals.questions)
        assertEquals(latestTurnFailure(events, "hub-one"), signals.turnFailure)
        assertEquals(latestContextUsage(events, "hub-one"), signals.usage)
    }

    // ---------------------------------------------------------------------
    // A repeated prompt is confirmed by count, never by text
    // ---------------------------------------------------------------------

    @Test
    fun `a repeated prompt is not confirmed by earlier rows with the same text`() {
        // Two "你好" rows already answered, then the same phrase sent again.
        // The old rule matched the text against the recent user rows and
        // dropped the new bubble (and its waiting skeleton) immediately. The
        // threshold sits one past the rows already on screen.
        assertFalse(optimisticPromptLanded(neededUserRows = 3, userRowCount = 2))
        assertTrue(optimisticPromptLanded(neededUserRows = 3, userRowCount = 3))
    }

    @Test
    fun `two prompts sent in the same frame get distinct thresholds`() {
        // Both sends read the same on-screen count, so only the queue term can
        // keep them apart: sharing a threshold would let one landed row confirm
        // both, which is the same "one row, two bubbles gone" defect.
        val first = nextOptimisticPromptNeeded(pending = emptyList(), userRowCount = 1)
        val second = nextOptimisticPromptNeeded(pending = listOf(first), userRowCount = 1)
        assertEquals(2, first)
        assertEquals(3, second)

        // One landed row retires the first bubble and only the first.
        assertTrue(optimisticPromptLanded(first, userRowCount = 2))
        assertFalse(optimisticPromptLanded(second, userRowCount = 2))
        assertTrue(optimisticPromptLanded(second, userRowCount = 3))
    }

    @Test
    fun `a prompt queued behind another does not jump ahead of it`() {
        // The transcript already had one user row when the first prompt was
        // sent and no new row has landed since; a second send must still wait
        // its turn even though the on-screen count now exceeds the first
        // threshold.
        val first = nextOptimisticPromptNeeded(pending = emptyList(), userRowCount = 1)
        val second = nextOptimisticPromptNeeded(pending = listOf(first), userRowCount = 3)
        assertEquals(2, first)
        assertEquals(4, second)
    }

    @Test
    fun `a landed threshold is met exactly, never one row early`() {
        assertFalse(optimisticPromptLanded(neededUserRows = 5, userRowCount = 4))
        assertTrue(optimisticPromptLanded(neededUserRows = 5, userRowCount = 5))
        assertTrue(optimisticPromptLanded(neededUserRows = 5, userRowCount = 9))
        // An empty transcript can still owe nothing.
        assertTrue(optimisticPromptLanded(neededUserRows = 0, userRowCount = 0))
    }

    // ---------------------------------------------------------------------
    // Dispatched-but-unanswered prompts and the identity handover
    // ---------------------------------------------------------------------

    private fun unansweredPrompt(clientId: String, text: String): RemoteCommandRecord =
        RemoteCommandRecord.parse(
            JSONObject()
                .put(
                    "command",
                    JSONObject()
                        .put("client_cmd_id", clientId)
                        .put("hub_session_id", "hub-one")
                        .put("host_generation", "generation-one")
                        .put(
                            "command",
                            JSONObject()
                                .put("action", "send_prompt")
                                .put("text", text),
                        ),
                )
                .put("receipt", JSONObject.NULL),
        )

    @Test
    fun `a dispatched prompt shows before its turn opens`() {
        val items =
            buildTranscript(
                emptyList(),
                listOf(unansweredPrompt("cmd-1", "你好")),
                "hub-one",
            )

        assertEquals(1, items.size)
        assertEquals(TranscriptKind.USER, items.first().kind)
        assertEquals("你好", items.first().text)
    }

    @Test
    fun `the answered prompt appears once the receipt names its turn`() {
        // The dispatcher's row exists only while the receipt is missing, and
        // the answered row exists only once it arrives — both keyed off the
        // same record, so a prompt is never drawn twice.
        val turnStarted =
            envelope(
                "turn/started",
                JSONObject().put("turn", JSONObject().put("id", "turn-9")),
                cursor = 3,
            )

        val pendingOnly =
            buildTranscript(
                listOf(turnStarted),
                listOf(unansweredPrompt("cmd-1", "你好")),
                "hub-one",
            )
        assertEquals(1, pendingOnly.size)
        assertEquals("pending:cmd-1", pendingOnly.first().id)

        val answered =
            buildTranscript(
                listOf(turnStarted),
                listOf(promptCommand("turn-9", "你好")),
                "hub-one",
            )
        assertEquals(1, answered.size)
        assertEquals("你好", answered.first().text)
        assertEquals("user:turn-9", answered.first().id)
    }
}
