package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.AgentPlugin
import com.agentplatform.mobile.data.AttachmentReader
import com.agentplatform.mobile.data.DiscoveryCandidate
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.GatewayException
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.HostHello
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.RemoteBinding
import com.agentplatform.mobile.data.RemoteCommandRecord
import com.agentplatform.mobile.data.RemoteEvent
import com.agentplatform.mobile.data.RemoteSessionRow
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.describeGatewayError
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Targeted regression tests for Medium/Low UI & sync logic fixes:
 * M1 rejected send_prompt/code routing, M2 string+object turn errors,
 * M3 attachment total decoded + wire budget merging, M6 session provider propagation,
 * M7 JSON null handling, M9 streaming fence tag split, and M10 claude_msg turn scoping,
 * response_sent receipt gating, and input.command fallback.
 */
class ReviewFixesUiTest {
    private fun eventFrame(
        kind: String,
        params: JSONObject,
        cursor: Long,
        binding: JSONObject? = null,
    ): RemoteEvent =
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
                                .apply { if (binding != null) put("binding", binding) }
                                .put("raw", JSONObject().put("params", params)),
                        ),
                ),
        )

    // ------------------------------------------------------------------ M1

    @Test
    fun `evaluateRejectedCommands prunes send_prompt and routes any receipt code via describeGatewayError`() {
        val rejected =
            RemoteCommandRecord(
                command =
                    JSONObject()
                        .put("client_cmd_id", "cmd-rej-1")
                        .put("hub_session_id", "hub-1")
                        .put(
                            "command",
                            JSONObject()
                                .put("action", "send_prompt")
                                .put("text", "请帮我检查构建"),
                        ),
                receipt =
                    JSONObject()
                        .put("status", "rejected")
                        .put("code", "provider_rejected")
                        .put("message", JSONObject.NULL),
            )
        val outcome = evaluateRejectedCommands(listOf(rejected), emptySet(), announceErrors = true)
        assertEquals(describeGatewayError(GatewayException(409, "provider_rejected")), outcome.errorMessage)
        assertEquals("请帮我检查构建", outcome.prunedPromptText)

        val nonPrompt =
            RemoteCommandRecord(
                command =
                    JSONObject()
                        .put("client_cmd_id", "cmd-rej-2")
                        .put("command", JSONObject().put("action", "prompt").put("text", "legacy")),
                receipt =
                    JSONObject()
                        .put("status", "rejected")
                        .put("code", JSONObject.NULL)
                        .put("message", JSONObject.NULL),
            )
        val legacyOutcome = evaluateRejectedCommands(listOf(nonPrompt), emptySet(), announceErrors = true)
        assertNull(legacyOutcome.prunedPromptText)
        assertEquals("指令被拒绝: rejected", legacyOutcome.errorMessage)
    }

    // ------------------------------------------------------------------ M2

    @Test
    fun `turn completion and error signals support both string and object error payloads`() {
        val stringTurnError =
            eventFrame(
                kind = "turn/completed",
                params =
                    JSONObject().put(
                        "turn",
                        JSONObject()
                            .put("id", "t-1")
                            .put("status", "failed")
                            .put("error", "context window exceeded"),
                    ),
                cursor = 1L,
            )
        val f1 = latestTurnFailure(listOf(stringTurnError), "hub-1")
        assertEquals("context window exceeded", f1?.message)
        assertEquals(f1, analyzeSessionSignals(listOf(stringTurnError), "hub-1").turnFailure)

        val objectTurnError =
            eventFrame(
                kind = "turn/completed",
                params =
                    JSONObject().put(
                        "turn",
                        JSONObject()
                            .put("id", "t-2")
                            .put("status", "failed")
                            .put("error", JSONObject().put("message", "upstream 502")),
                    ),
                cursor = 2L,
            )
        assertEquals("upstream 502", latestTurnFailure(listOf(objectTurnError), "hub-1")?.message)

        val stringParamsError =
            eventFrame(
                kind = "error",
                params = JSONObject().put("turnId", JSONObject.NULL).put("error", "plain error text"),
                cursor = 3L,
            )
        val f3 = latestTurnFailure(listOf(stringParamsError), "hub-1")
        assertEquals("plain error text", f3?.message)
        assertNull(f3?.turnId)
        assertEquals(f3, analyzeSessionSignals(listOf(stringParamsError), "hub-1").turnFailure)
    }

    // ------------------------------------------------------------------ M3

    @Test
    fun `mergeAttachmentsWithinBudget and mergeQueuedPrompt enforce decoded wire and escape byte limits`() {
        // Wire limit: 2 * 4_800_000 = 9_600_000 > 9 MiB (9_437_184), while decoded is 7_200_000 <= 7 MiB (7_340_032)
        val bigBinary1 = RemoteAttachment("a.bin", "application/octet-stream", "A".repeat(4_800_000), "base64")
        val bigBinary2 = RemoteAttachment("b.bin", "application/octet-stream", "B".repeat(4_800_000), "base64")
        val wireMerged = mergeAttachmentsWithinBudget(listOf(bigBinary1), listOf(bigBinary2))
        assertEquals(1, wireMerged.attachments.size)
        assertEquals("a.bin", wireMerged.attachments.single().name)
        assertEquals(1, wireMerged.droppedCount)

        // Decoded limit: 2 * 4_000_000 = 8_000_000 > 7 MiB (7_340_032), while wire stays under 9 MiB
        val bigText1 = RemoteAttachment("a.txt", "text/plain", "x".repeat(4_000_000), "utf8")
        val bigText2 = RemoteAttachment("b.txt", "text/plain", "y".repeat(4_000_000), "utf8")
        val decodedMerged = mergeAttachmentsWithinBudget(listOf(bigText1), listOf(bigText2))
        assertEquals(1, decodedMerged.attachments.size)
        assertEquals("a.txt", decodedMerged.attachments.single().name)
        assertEquals(1, decodedMerged.droppedCount)

        // JSON escape overhead: 2 * 2_500_000 quotes = 5_000_000 decoded <= 7 MiB, but ~10_000_000 wire > 9 MiB
        val escaped1 = RemoteAttachment("q1.json", "application/json", "\"".repeat(2_500_000), "utf8")
        val escaped2 = RemoteAttachment("q2.json", "application/json", "\"".repeat(2_500_000), "utf8")
        val escapedMerged = mergeAttachmentsWithinBudget(listOf(escaped1), listOf(escaped2))
        assertEquals(1, escapedMerged.attachments.size)
        assertEquals(1, escapedMerged.droppedCount)

        val q1 = mergeQueuedPrompt(null, "第一条", listOf(bigBinary1))
        val qNoOp = mergeQueuedPrompt(q1, "   ", listOf(bigBinary2))
        assertEquals(1, qNoOp!!.count)
        assertEquals(1, qNoOp.attachments.size)

        val q2 = mergeQueuedPrompt(q1, "第二条", listOf(bigBinary2))
        assertNotNull(q2)
        assertEquals(2, q2!!.count)
        assertEquals(1, q2.attachments.size)
        assertEquals("a.bin", q2.attachments.single().name)
        assertTrue(q2.attachments.sumOf { it.estimatedDecodedBytes() } <= AttachmentReader.MAX_TOTAL_BYTES)
        assertTrue(q2.attachments.sumOf { it.estimatedWireBytes() } <= MAX_ATTACHMENT_WIRE_BYTES)
    }

    // ------------------------------------------------------------------ M6

    @Test
    fun `buildSessionTree uses top-level RemoteSessionRow provider when no candidate exists`() {
        val host =
            HostDevice(
                runnerId = "r-1",
                online = true,
                revoked = false,
                hello = HostHello("gen-1", "Mac", null, listOf("/repo"), emptyList(), pendingApprovals = 0, pendingQuestions = 0),
            )
        val row =
            RemoteSessionRow(
                runnerId = "r-1",
                online = true,
                binding =
                    RemoteBinding(
                        hubSessionId = "hub-1",
                        providerSessionId = "opaque-id-123",
                        workspacePath = "/repo",
                        hostGeneration = "gen-1",
                        controlConnected = true,
                        executionState = ExecutionState.IDLE,
                        currentTurnId = null,
                        title = "AGY 任务",
                    ),
                provider = "antigravity",
            )
        val entries = flattenTree(buildSessionTree(Snapshot(1, listOf(host), listOf(row))))
        assertEquals(1, entries.size)
        assertEquals(EngineKind.ANTIGRAVITY, entries.single().provider)
        assertEquals("Antigravity", entries.single().agentLabel)
    }

    // ------------------------------------------------------------------ M7

    @Test
    fun `history snapshot and normalizeDiffText ignore json null without literal null text`() {
        val snap =
            eventFrame(
                kind = "history/snapshot",
                params =
                    JSONObject().put(
                        "items",
                        JSONArray().put(
                            JSONObject()
                                .put("kind", "message")
                                .put("role", "assistant")
                                .put("text", JSONObject.NULL)
                                .put("content", "真实正文")
                                .put("label", JSONObject.NULL),
                        ),
                    ),
                cursor = 1L,
            )
        val items = buildTranscript(listOf(snap), emptyList(), "hub-1")
        assertEquals(1, items.size)
        assertEquals("真实正文", items.single().text)

        val diffWithNulls =
            normalizeDiffText(
                JSONObject()
                    .put("old_string", JSONObject.NULL)
                    .put("new_string", "val y = 2")
                    .toString(),
            )
        assertFalse(diffWithNulls.contains("null"))
        assertTrue(diffWithNulls.contains("+ val y = 2"))
    }

    // ------------------------------------------------------------------ M9

    @Test
    fun `markdown segment cache detects fence language tag split across streaming deltas`() {
        val cache = MarkdownSegmentCache()
        val s1 = cache.update("如下：\n```kot")
        assertEquals(1, s1.size)
        assertFalse(s1[0].isCodeBlock)

        val full = "如下：\n```kotlin\nval x = 1"
        val s2 = cache.update(full)
        assertEquals(parseMarkdownSegments(full), s2)
        assertEquals(2, s2.size)
        assertTrue(s2[1].isCodeBlock)
        assertEquals("kotlin", s2[1].language)
        assertTrue(s2[1].unclosed)
    }

    // ------------------------------------------------------------------ M10

    @Test
    fun `bare claude_msg and claude_thinking are turn-scoped across multiple turns`() {
        val events =
            listOf(
                eventFrame("turn/started", JSONObject().put("turn", JSONObject().put("id", "turn-1")), 1L),
                eventFrame("item/reasoning/delta", JSONObject().put("itemId", "claude_thinking").put("delta", "思考一"), 2L),
                eventFrame("item/agentMessage/delta", JSONObject().put("itemId", "claude_msg").put("delta", "回答一"), 3L),
                eventFrame("turn/completed", JSONObject().put("turn", JSONObject().put("id", "turn-1")), 4L),
                eventFrame("turn/started", JSONObject().put("turn", JSONObject().put("id", "turn-2")), 5L),
                eventFrame("item/reasoning/delta", JSONObject().put("itemId", "claude_thinking").put("delta", "思考二"), 6L),
                eventFrame("item/agentMessage/delta", JSONObject().put("itemId", "claude_msg").put("delta", "回答二"), 7L),
                eventFrame("turn/completed", JSONObject().put("turn", JSONObject().put("id", "turn-2")), 8L),
            )
        val full = buildTranscript(events, emptyList(), "hub-1")
        assertEquals(4, full.size)
        assertEquals(listOf("turn-1:claude_thinking", "turn-1:claude_msg", "turn-2:claude_thinking", "turn-2:claude_msg"), full.map { it.id })
        assertEquals("回答一", full[1].text)
        assertEquals("回答二", full[3].text)

        val fold = IncrementalTranscriptFold()
        var last = emptyList<TranscriptItem>()
        for (i in 1..events.size) {
            last = fold.update(events.take(i), emptyList(), "hub-1", live = i < events.size)
        }
        assertEquals(full, last)
    }

    @Test
    fun `receipt without response_sent true does not dismiss pending approvals or questions`() {
        val apprReq =
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
                                            .put("approval_id", "ap-1")
                                            .put("hub_session_id", "hub-1"),
                                    ),
                            ),
                    ),
            )
        val qReq =
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
                                    .put("kind", "question_requested")
                                    .put(
                                        "question",
                                        JSONObject()
                                            .put("question_id", "q-1")
                                            .put("hub_session_id", "hub-1")
                                            .put("prompt", "继续？"),
                                    ),
                            ),
                    ),
            )
        val notSentReceipt =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 3)
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
                                            .put("approval_id", "ap-1")
                                            .put("question_id", "q-1")
                                            .put("response_sent", false),
                                    ),
                            ),
                    ),
            )
        val beforeSent = listOf(apprReq, qReq, notSentReceipt)
        assertEquals(1, pendingApprovals(beforeSent, "hub-1").size)
        assertEquals(1, pendingQuestions(beforeSent, "hub-1").size)
        val sigBefore = analyzeSessionSignals(beforeSent, "hub-1")
        assertEquals(1, sigBefore.approvals.size)
        assertEquals(1, sigBefore.questions.size)

        val sentReceipt =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 4)
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
                                            .put("approval_id", "ap-1")
                                            .put("question_id", "q-1")
                                            .put("response_sent", true),
                                    ),
                            ),
                    ),
            )
        val afterSent = beforeSent + sentReceipt
        assertTrue(pendingApprovals(afterSent, "hub-1").isEmpty())
        assertTrue(pendingQuestions(afterSent, "hub-1").isEmpty())
        val sigAfter = analyzeSessionSignals(afterSent, "hub-1")
        assertTrue(sigAfter.approvals.isEmpty())
        assertTrue(sigAfter.questions.isEmpty())
    }

    @Test
    fun `commandExecution falls back to nested input command field`() {
        val cmdEvent =
            eventFrame(
                kind = "item/completed",
                params =
                    JSONObject().put(
                        "item",
                        JSONObject()
                            .put("id", "cmd-input-1")
                            .put("type", "commandExecution")
                            .put("input", JSONObject().put("command", "git status --short"))
                            .put("status", "completed"),
                    ),
                cursor = 1L,
            )
        val items = buildTranscript(listOf(cmdEvent), emptyList(), "hub-1")
        assertEquals(1, items.size)
        assertEquals("git status --short", items.single().command)
    }

    // -------------------------------------------------------------- AND-P0-1 / P1-4 / P2-2 / P2-5 / P2-8

    @Test
    fun `buildSessionTree assigns unique keys across duplicate active sessions and candidates and passes plugins`() {
        val customPlugin =
            AgentPlugin(
                id = "my_acp",
                name = "My ACP",
                displayName = "My ACP",
                version = "1.0.0",
                description = "",
                category = "agent",
                enabled = true,
                installed = true,
                available = true,
                runtimeStatus = "ready",
                error = null,
                sessionPrefix = "myacp_",
            )
        val candDup1 = DiscoveryCandidate("cand-1", "/repo", "历史任务", null, null, null, 100L, true)
        val candDup2 = DiscoveryCandidate("cand-1", "/repo", "重复历史任务", null, null, null, 200L, true)
        val host =
            HostDevice(
                runnerId = "r-1",
                online = true,
                revoked = false,
                hello =
                    HostHello(
                        hostGeneration = "gen-1",
                        deviceName = "Mac",
                        provider = null,
                        workspaces = listOf("/repo"),
                        candidates = listOf(candDup1, candDup2),
                        pendingApprovals = 0,
                        pendingQuestions = 0,
                        plugins = listOf(customPlugin),
                    ),
            )
        val row1 =
            RemoteSessionRow(
                runnerId = "r-1",
                online = true,
                binding =
                    RemoteBinding(
                        hubSessionId = "hub-1",
                        providerSessionId = "myacp_shared",
                        workspacePath = "/repo",
                        hostGeneration = "gen-1",
                        controlConnected = true,
                        executionState = ExecutionState.IDLE,
                        currentTurnId = null,
                        title = "活跃一",
                    ),
            )
        val row2 =
            RemoteSessionRow(
                runnerId = "r-1",
                online = true,
                binding =
                    RemoteBinding(
                        hubSessionId = "hub-2",
                        providerSessionId = "myacp_shared",
                        workspacePath = "/repo",
                        hostGeneration = "gen-1",
                        controlConnected = true,
                        executionState = ExecutionState.IDLE,
                        currentTurnId = null,
                        title = "活跃二",
                    ),
            )
        val entries = flattenTree(buildSessionTree(Snapshot(1, listOf(host), listOf(row1, row1, row2))))
        assertEquals(3, entries.size)
        val keys = entries.map { it.key }
        assertEquals(keys.size, keys.distinct().size)
        assertTrue("active:r-1/hub-1" in keys)
        assertTrue("active:r-1/hub-2" in keys)
        assertTrue("candidate:r-1/cand-1" in keys)
        val actives = entries.filterIsInstance<TreeEntry.Active>()
        assertEquals(2, actives.size)
        assertNotEquals(actives[0].key, actives[1].key)
        assertTrue(actives.all { it.provider == EngineKind.PLUGIN && it.agentLabel == "My ACP" })
    }

    @Test
    fun `transcript events and rejected commands ignore explicit json null in failureReason diff and status`() {
        val events =
            listOf(
                eventFrame(
                    kind = "item/completed",
                    params =
                        JSONObject().put(
                            "item",
                            JSONObject()
                                .put("id", "r-1")
                                .put("type", "reasoning")
                                .put("failureReason", JSONObject.NULL)
                                .put("text", "真实思考内容"),
                        ),
                    cursor = 1L,
                ),
                eventFrame(
                    kind = "item/completed",
                    params =
                        JSONObject().put(
                            "item",
                            JSONObject()
                                .put("id", "fc-1")
                                .put("type", "fileChange")
                                .put("path", JSONObject.NULL)
                                .put("diff", JSONObject.NULL)
                                .put("patch", "@@ -1 +1 @@\n-old\n+new")
                                .put("output", JSONObject.NULL)
                                .put("status", JSONObject.NULL),
                        ),
                    cursor = 2L,
                ),
            )
        val items = buildTranscript(events, emptyList(), "hub-1")
        assertEquals(2, items.size)
        assertEquals("<think>真实思考内容</think>", items[0].text)
        assertFalse(items[0].text.contains("null"))
        assertEquals("@@ -1 +1 @@\n-old\n+new", items[1].command)
        assertNull(items[1].output)
        assertEquals(ToolState.COMPLETED, items[1].state)

        val nullStatusCmd =
            RemoteCommandRecord(
                command = JSONObject().put("client_cmd_id", "cmd-null-status").put("command", JSONObject().put("action", "send_prompt").put("text", "hi")),
                receipt = JSONObject().put("status", JSONObject.NULL),
            )
        val outcome = evaluateRejectedCommands(listOf(nullStatusCmd), emptySet(), announceErrors = true)
        assertTrue(outcome.newIds.isEmpty())
        assertNull(outcome.errorMessage)
    }

    @Test
    fun `normalizeDiffText handles pure addition and pure deletion without blank counterpart lines`() {
        val pureAdd =
            normalizeDiffText(
                JSONObject()
                    .put("old_string", "")
                    .put("new_string", "val a = 1\nval b = 2")
                    .toString(),
            )
        assertEquals("+ val a = 1\n+ val b = 2", pureAdd)

        val pureDelete =
            normalizeDiffText(
                JSONObject()
                    .put("old_string", "val old = 0")
                    .put("new_string", "")
                    .toString(),
            )
        assertEquals("- val old = 0", pureDelete)
    }

    @Test
    fun `evaluateRejectedCommands and pruneRejectedOptimisticPrompts match clientCmdId across batches`() {
        val rejPrompt =
            RemoteCommandRecord(
                command =
                    JSONObject()
                        .put("client_cmd_id", "cid-1")
                        .put(
                            "command",
                            JSONObject()
                                .put("action", "send_prompt")
                                .put("text", "发给模型")
                                .put("attachments", JSONArray().put(JSONObject().put("name", "log.txt"))),
                        ),
                receipt = JSONObject().put("status", "rejected").put("code", "runner_offline"),
            )
        val rejInterrupt =
            RemoteCommandRecord(
                command =
                    JSONObject()
                        .put("client_cmd_id", "cid-2")
                        .put("command", JSONObject().put("action", "interrupt")),
                receipt = JSONObject().put("status", "rejected").put("code", "session_not_bound"),
            )
        val outcome = evaluateRejectedCommands(listOf(rejPrompt, rejInterrupt), emptySet(), announceErrors = true)
        assertEquals(setOf("cid-1", "cid-2"), outcome.newIds)
        assertEquals(1, outcome.prunedPrompts.size)
        assertEquals("cid-1", outcome.prunedPrompts.single().clientCmdId)

        val prompts =
            listOf(
                OptimisticPrompt(id = "opt-1", text = "发给模型", neededUserRows = 1, clientCmdId = "cid-1"),
                OptimisticPrompt(id = "opt-2", text = "保留消息", neededUserRows = 2, clientCmdId = "cid-3"),
            )
        val kept = pruneRejectedOptimisticPrompts(prompts, outcome)
        assertEquals(listOf("opt-2"), kept.map { it.id })
    }
}
