package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.AppearanceTheme
import com.agentplatform.mobile.data.CatalogModel
import com.agentplatform.mobile.data.ComposerMode
import com.agentplatform.mobile.data.DiscoveryCandidate
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.HostHello
import com.agentplatform.mobile.data.ModelCatalog
import com.agentplatform.mobile.data.PendingApproval
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.RemoteBinding
import com.agentplatform.mobile.data.RemoteCommandRecord
import com.agentplatform.mobile.data.RemoteEvent
import com.agentplatform.mobile.data.RemoteSessionRow
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.displayEngineLabel
import com.agentplatform.mobile.data.engineOfSession
import com.agentplatform.mobile.data.mergeHostHello
import com.agentplatform.mobile.data.modelsForEngine
import com.agentplatform.mobile.sanitizeDeploymentChange
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MobileParityTest {
    @Test
    fun `launchable engines exclude claude_desktop and deepseek_harness and gate plugin on plugin state`() {
        val withoutPlugin = EngineKind.launchable(enabledPluginIds = emptySet())
        assertEquals(
            listOf(
                EngineKind.CODEX,
                EngineKind.CLAUDE_CODE,
                EngineKind.ANTIGRAVITY,
            ),
            withoutPlugin,
        )
        assertFalse(EngineKind.CLAUDE_DESKTOP in withoutPlugin)
        assertFalse(EngineKind.DEEPSEEK_HARNESS in withoutPlugin)
        assertFalse(EngineKind.PLUGIN in withoutPlugin)

        val withPlugin = EngineKind.launchable(enabledPluginIds = setOf("grok_build"))
        assertTrue(EngineKind.PLUGIN in withPlugin)
        assertFalse(EngineKind.CLAUDE_DESKTOP in withPlugin)
    }

    @Test
    fun `displayEngineLabel and modelsForEngine isolate gemini_cli zcode and acp`() {
        val catalog =
            ModelCatalog(
                flat = listOf(CatalogModel("gpt-5-codex", "GPT-5 Codex")),
                agents =
                    mapOf(
                        "zcode" to listOf(CatalogModel("zcode-pro", "ZCode Pro")),
                        "gemini_cli" to listOf(CatalogModel("gemini-2.5-pro", "Gemini 2.5 Pro")),
                        "antigravity" to listOf(CatalogModel("gemini-2.5-pro-agy", "AGY Gemini")),
                    ),
            )

        assertEquals("Gemini CLI", displayEngineLabel(EngineKind.PLUGIN, "gemini_123", "gemini_cli"))
        assertEquals("ACP", displayEngineLabel(EngineKind.CODEX, "acp_999", "acp"))
        assertEquals("Antigravity", displayEngineLabel(EngineKind.ANTIGRAVITY, "antigravity_1", "antigravity"))

        assertEquals(
            listOf("gemini-2.5-pro"),
            modelsForEngine(catalog, EngineKind.PLUGIN, "gemini_cli", "gemini_123").map { it.id },
        )
        assertEquals(
            listOf("zcode-pro"),
            modelsForEngine(catalog, EngineKind.PLUGIN, "zcode", "sess_123").map { it.id },
        )
        assertTrue(modelsForEngine(catalog, EngineKind.CODEX, "acp", "acp_999").isEmpty())
        assertEquals(
            listOf("gemini-2.5-pro-agy"),
            modelsForEngine(catalog, EngineKind.ANTIGRAVITY, "antigravity", "antigravity_1").map { it.id },
        )
    }

    @Test
    fun `mergeHostHello keeps fresh host_generation and approvals while retaining cached models and candidates`() {
        val cached =
            HostHello(
                hostGeneration = "gen-old",
                deviceName = "OldName",
                provider = "codex",
                workspaces = listOf("/old"),
                candidates =
                    listOf(
                        DiscoveryCandidate(
                            providerSessionId = "antigravity_abc",
                            workspacePath = "/new",
                            title = "AGY 会话",
                            provider = "antigravity",
                            model = "gemini-2.5-pro",
                            totalTokens = 12_000,
                            lastActivityAtMs = 1000L,
                            resumable = true,
                        ),
                    ),
                models =
                    ModelCatalog(
                        flat = emptyList(),
                        agents = mapOf("antigravity" to listOf(CatalogModel("agy-1", "AGY 1"))),
                    ),
                pendingApprovals = 0,
                pendingQuestions = 0,
            )
        val projectedFresh =
            HostHello(
                hostGeneration = "gen-new-rotated",
                deviceName = "Portable-PC",
                provider = "codex",
                workspaces = listOf("/new"),
                candidates = emptyList(),
                models = ModelCatalog.Empty,
                pendingApprovals = 1,
                pendingQuestions = 0,
                pendingApprovalsList =
                    listOf(
                        PendingApproval.parse(
                            JSONObject()
                                .put("approval_id", "appr-9")
                                .put("hub_session_id", "hub-1")
                                .put("host_generation", "gen-new-rotated")
                                .put("command_preview", "git status"),
                        ),
                    ),
            )

        val merged = mergeHostHello(projectedFresh, cached)
        assertNotNull(merged)
        assertEquals("gen-new-rotated", merged!!.hostGeneration)
        assertEquals("Portable-PC", merged.deviceName)
        assertEquals(listOf("/new"), merged.workspaces)
        assertEquals(1, merged.pendingApprovals)
        assertEquals("appr-9", merged.pendingApprovalsList.single().approvalId)
        assertEquals(1, merged.candidates.size)
        assertEquals("agy-1", merged.models.agents["antigravity"]?.single()?.id)
    }

    @Test
    fun `sanitizeDeploymentChange strips sessionCookie and accessToken when switching servers`() {
        val oldServer =
            AppConfig(
                gatewayBase = "https://platform.example.com",
                sessionCookie = "__Host-agent_pulse_session=" + "a".repeat(43),
                accessToken = "secret-bearer",
            )
        val sameServer =
            sanitizeDeploymentChange(
                oldServer,
                oldServer.copy(gatewayBase = "https://platform.example.com/"),
            )
        assertTrue(sameServer.sessionCookie.isNotEmpty())
        assertEquals("secret-bearer", sameServer.accessToken)

        val movedServer =
            sanitizeDeploymentChange(
                oldServer,
                oldServer.copy(gatewayBase = "https://other.example.com"),
            )
        assertEquals("", movedServer.sessionCookie)
        assertEquals("", movedServer.accessToken)
    }

    @Test
    fun `sanitizeSessionTitle strips XML command wrappers and Markdown noise`() {
        assertEquals(
            "修复移动端底部抽屉",
            sanitizeSessionTitle("<command-message>compact</command-message><command-name>/compact</command-name>  # `修复移动端底部抽屉` "),
        )
        assertEquals(
            "/compact",
            sanitizeSessionTitle("<command-message>compact</command-message><command-name>/compact</command-name><command-args></command-args>"),
        )
        assertEquals(
            "查看 AgentHub 文档",
            sanitizeSessionTitle("查看 [AgentHub 文档](https://example.com/docs)"),
        )
    }

    @Test
    fun `workspaceLabel decodes triple-underscore paths and redacted CJK slugs`() {
        assertEquals("ClaudeSpace", workspaceLabel("ws-D___ClaudeSpace-0123456789abcdef"))
        assertEquals("agenthub", workspaceLabel("ws-home___tanner___Project___agenthub-fedcba9876543210"))
        assertEquals("本地目录 · abcd", workspaceLabel("ws-__-abcd1234567890ef"))
        assertEquals("本地目录 · 9876", workspaceLabel("ws-____-9876543210fedcba"))
        assertEquals("agenthub", workspaceLabel("\\\\?\\D:\\ClaudeSpace\\agenthub\\"))
    }

    @Test
    fun `codex native userMessage is deduplicated against turn_started command prompt`() {
        val cmd =
            RemoteCommandRecord.parse(
                JSONObject()
                    .put(
                        "command",
                        JSONObject()
                            .put("client_cmd_id", "cmd-codex-1")
                            .put("hub_session_id", "hub-1")
                            .put("host_generation", "gen-1")
                            .put(
                                "command",
                                JSONObject()
                                    .put("action", "send_prompt")
                                    .put("text", "检查移动端布局"),
                            ),
                    )
                    .put(
                        "receipt",
                        JSONObject()
                            .put("status", "accepted")
                            .put("result", JSONObject().put("turn", JSONObject().put("id", "turn-42"))),
                    ),
            )

        val turnStarted =
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
                                    .put("kind", "turn/started")
                                    .put(
                                        "raw",
                                        JSONObject().put(
                                            "params",
                                            JSONObject().put("turn", JSONObject().put("id", "turn-42")),
                                        ),
                                    ),
                            ),
                    ),
            )
        val codexEcho =
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
                                    .put("kind", "item/completed")
                                    .put(
                                        "raw",
                                        JSONObject().put(
                                            "params",
                                            JSONObject()
                                                .put("turnId", "turn-42")
                                                .put(
                                                    "item",
                                                    JSONObject()
                                                        .put("id", "item-user-42")
                                                        .put("type", "userMessage")
                                                        .put("text", "检查移动端布局"),
                                                ),
                                        ),
                                    ),
                            ),
                    ),
            )
        val reply =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 3)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "event")
                            .put(
                                "event",
                                JSONObject()
                                    .put("kind", "item/completed")
                                    .put(
                                        "raw",
                                        JSONObject().put(
                                            "params",
                                            JSONObject()
                                                .put("turnId", "turn-42")
                                                .put(
                                                    "item",
                                                    JSONObject()
                                                        .put("id", "item-agent-42")
                                                        .put("type", "agentMessage")
                                                        .put("text", "布局正常"),
                                                ),
                                        ),
                                    ),
                            ),
                    ),
            )

        val items = buildTranscript(listOf(turnStarted, codexEcho, reply), listOf(cmd), "hub-1")
        val userRows = items.filter { it.kind == TranscriptKind.USER }
        assertEquals(1, userRows.size)
        assertEquals("检查移动端布局", userRows.single().text)
    }

    @Test
    fun `turn_completed preserves final error banner until next turn_started and receipt clears question`() {
        val errEvent =
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
                                    .put("kind", "error")
                                    .put("binding", JSONObject().put("hub_session_id", "hub-1"))
                                    .put(
                                        "raw",
                                        JSONObject().put(
                                            "params",
                                            JSONObject()
                                                .put("turnId", "turn-1")
                                                .put("error", JSONObject().put("message", "API quota exceeded")),
                                        ),
                                    ),
                            ),
                    ),
            )
        val turnDone =
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
                                    .put("kind", "turn/completed")
                                    .put("binding", JSONObject().put("hub_session_id", "hub-1").put("execution_state", "error"))
                                    .put(
                                        "raw",
                                        JSONObject().put(
                                            "params",
                                            JSONObject().put("turn", JSONObject().put("id", "turn-1").put("status", "failed")),
                                        ),
                                    ),
                            ),
                    ),
            )
        val signals = analyzeSessionSignals(listOf(errEvent, turnDone), "hub-1")
        assertEquals("API quota exceeded", signals.turnFailure?.message)
        assertEquals(latestTurnFailure(listOf(errEvent, turnDone), "hub-1"), signals.turnFailure)

        val qRequested =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 3)
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
                                            .put("question_id", "q-99")
                                            .put("hub_session_id", "hub-1")
                                            .put("prompt", "继续吗？"),
                                    ),
                            ),
                    ),
            )
        val qReceipt =
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
                                            .put("question_id", "q-99")
                                            .put("response_sent", true),
                                    ),
                            ),
                    ),
            )
        assertTrue(pendingQuestions(listOf(qRequested, qReceipt), "hub-1").isEmpty())
    }

    @Test
    fun `active-turn prompt queueing merges messages and blocks auto-dispatch on interrupt or failure`() {
        val att = RemoteAttachment("spec.md", "text/markdown", "# Spec", "utf8")
        val q1 = mergeQueuedPrompt(null, "第一步检查接口", emptyList())
        val q2 = mergeQueuedPrompt(q1, "第二步跑单测", listOf(att))

        assertNotNull(q2)
        assertEquals(2, q2!!.count)
        assertEquals("第一步检查接口\n\n第二步跑单测", q2.text)
        assertEquals(1, q2.attachments.size)

        assertTrue(
            shouldAutoDispatchQueue(
                wasBusy = true,
                mode = ComposerMode.READY,
                sending = false,
                manualInterruptPending = false,
                hasTurnFailure = false,
            ),
        )
        assertFalse(
            shouldAutoDispatchQueue(
                wasBusy = true,
                mode = ComposerMode.READY,
                sending = false,
                manualInterruptPending = true,
                hasTurnFailure = false,
            ),
        )
        assertFalse(
            shouldAutoDispatchQueue(
                wasBusy = true,
                mode = ComposerMode.READY,
                sending = false,
                manualInterruptPending = false,
                hasTurnFailure = true,
            ),
        )
    }

    @Test
    fun `unclosed streaming code fence and multi-thinking extraction work incrementally`() {
        val cache = MarkdownSegmentCache()
        val s1 = cache.update("说明：\n```kotlin\nval a = 1")
        val s2 = cache.update("说明：\n```kotlin\nval a = 1\nval b = 2")
        assertEquals(2, s1.size)
        assertTrue(s1[1].isCodeBlock)
        assertTrue(s1[1].unclosed)
        assertEquals(parseMarkdownSegments("说明：\n```kotlin\nval a = 1\nval b = 2"), s2)

        val (thought, body, live) =
            splitThinking("<think>第一段</think>正文内容<thinking>第二段未闭合", streaming = true)
        assertEquals("第一段\n\n第二段未闭合", thought)
        assertEquals("正文内容", body)
        assertTrue(live)

        val structuredDiff = normalizeDiffText("""{"old_string":"val x = 1","new_string":"val x = 2"}""")
        assertTrue(structuredDiff.contains("- val x = 1"))
        assertTrue(structuredDiff.contains("+ val x = 2"))
    }

    @Test
    fun `buildAttentionQueue ignores providerFilter and pairs pending approvals`() {
        val approval =
            PendingApproval.parse(
                JSONObject()
                    .put("approval_id", "ap-1")
                    .put("hub_session_id", "hub-antigravity")
                    .put("host_generation", "g1")
                    .put("command_preview", "npm publish"),
            )
        val host =
            HostDevice(
                runnerId = "r-1",
                online = true,
                revoked = false,
                hello =
                    HostHello(
                        hostGeneration = "g1",
                        deviceName = "Studio-Mac",
                        provider = "antigravity",
                        workspaces = listOf("/repo"),
                        candidates = emptyList(),
                        pendingApprovals = 1,
                        pendingQuestions = 0,
                        pendingApprovalsList = listOf(approval),
                    ),
            )
        val row =
            RemoteSessionRow(
                runnerId = "r-1",
                online = true,
                binding =
                    RemoteBinding(
                        hubSessionId = "hub-antigravity",
                        providerSessionId = "antigravity_77",
                        workspacePath = "/repo",
                        hostGeneration = "g1",
                        controlConnected = true,
                        executionState = ExecutionState.AWAITING_PERMISSION,
                        currentTurnId = "t-1",
                        title = "<command-name>/deploy</command-name> 发布检查",
                    ),
            )
        val queue = buildAttentionQueue(Snapshot(1, listOf(host), listOf(row)))
        assertEquals(1, queue.size)
        assertEquals("发布检查", queue.single().entry.title)
        assertEquals("Studio-Mac", queue.single().entry.deviceNameLabel)
        assertEquals("Antigravity", queue.single().entry.agentLabel)
        assertEquals("npm publish", queue.single().approval?.summary)
    }

    @Test
    fun `history snapshot preserves thinking blocks and splits tool result markers and diffs`() {
        val itemsArray =
            org.json.JSONArray()
                .put(JSONObject().put("role", "user").put("kind", "message").put("text", "完善反重力"))
                .put(JSONObject().put("role", "assistant").put("kind", "thinking").put("text", "先分析 ConnectRPC 协议。"))
                .put(
                    JSONObject()
                        .put("role", "assistant")
                        .put("kind", "tool")
                        .put("label", "edit_file · daemon.rs")
                        .put("text", "--- a/daemon.rs\n+++ b/daemon.rs\n@@ -1 +1 @@\n-old\n+new\n\n〔结果〕\nEdited file.")
                        .put("status", "completed"),
                )
                .put(JSONObject().put("role", "assistant").put("kind", "thinking").put("text", "协议验证通过。"))
                .put(JSONObject().put("role", "assistant").put("kind", "message").put("text", "已完成改造。"))

        val snap =
            com.agentplatform.mobile.data.RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 10)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "event")
                            .put(
                                "event",
                                JSONObject()
                                    .put("kind", "history/snapshot")
                                    .put("raw", JSONObject().put("params", JSONObject().put("items", itemsArray))),
                            ),
                    ),
            )

        val transcript = buildTranscript(listOf(snap), emptyList(), "hub-agy")
        assertEquals(4, transcript.size)
        assertEquals("<think>先分析 ConnectRPC 协议。</think>", transcript[1].text)
        assertEquals("edit_file · daemon.rs", transcript[2].title)
        assertEquals("--- a/daemon.rs\n+++ b/daemon.rs\n@@ -1 +1 @@\n-old\n+new", transcript[2].command)
        assertEquals("Edited file.", transcript[2].output)
        assertEquals("<think>协议验证通过。</think>\n\n已完成改造。", transcript[3].text)
    }

    @Test
    fun `live codex reasoning deltas command output deltas fileChange changes and tokenUsage fold cleanly`() {
        fun rawEvt(cursor: Int, kind: String, params: JSONObject) =
            com.agentplatform.mobile.data.RemoteEvent.parse(
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
                    ),
            )

        val events =
            listOf(
                rawEvt(1, "turn/started", JSONObject().put("turn", JSONObject().put("id", "t1"))),
                rawEvt(2, "item/reasoning/summaryTextDelta", JSONObject().put("itemId", "r1").put("delta", "分析 ")),
                rawEvt(3, "item/reasoning/summaryTextDelta", JSONObject().put("itemId", "r1").put("delta", "SQLite")),
                rawEvt(
                    4,
                    "item/started",
                    JSONObject().put(
                        "item",
                        JSONObject().put("id", "c1").put("type", "commandExecution").put("command", "cargo test"),
                    ),
                ),
                rawEvt(5, "item/commandExecution/outputDelta", JSONObject().put("itemId", "c1").put("delta", "ok\n")),
                rawEvt(
                    6,
                    "item/completed",
                    JSONObject().put(
                        "item",
                        JSONObject()
                            .put("id", "f1")
                            .put("type", "fileChange")
                            .put("status", "completed")
                            .put(
                                "changes",
                                org.json.JSONArray().put(
                                    JSONObject()
                                        .put("path", "src/codex.rs")
                                        .put("diff", "@@ -1,1 +1,2 @@\n use std;\n+use serde;"),
                                ),
                            ),
                    ),
                ),
                rawEvt(
                    7,
                    "thread/tokenUsage/updated",
                    JSONObject().put(
                        "tokenUsage",
                        JSONObject()
                            .put("last", JSONObject().put("totalTokens", 52_300L))
                            .put("modelContextWindow", 400_000L),
                    ),
                ),
                rawEvt(8, "turn/completed", JSONObject().put("turn", JSONObject().put("id", "t1").put("status", "completed"))),
            )

        val transcript = buildTranscript(events, emptyList(), "hub-codex")
        assertEquals(3, transcript.size)
        assertEquals("<think>分析 SQLite</think>", transcript[0].text)
        assertEquals("ok\n", transcript[1].output)
        assertEquals(ToolState.COMPLETED, transcript[1].state)
        assertEquals("文件修改 · codex.rs", transcript[2].title)
        assertTrue(transcript[2].command.orEmpty().startsWith("--- a/src/codex.rs"))

        val usage = latestContextUsage(events, "hub-codex")
        assertNotNull(usage)
        assertEquals(52_300L, usage?.used)
        assertEquals(400_000L, usage?.window)

        assertEquals(
            1_000_000L,
            com.agentplatform.mobile.data.inferModelContextWindow("gemini-4-carbon-high", "antigravity"),
        )
        assertEquals(
            200_000L,
            com.agentplatform.mobile.data.inferModelContextWindow("claude-opus-4-6-thinking", "antigravity"),
        )
        assertEquals(
            "修复子代理折叠",
            sanitizeSessionTitle("<USER_SETTINGS_CHANGE>Model: Carbon</USER_SETTINGS_CHANGE><environment_context>cwd</environment_context>修复子代理折叠"),
        )
    }

    @Test
    fun `appearance theme defaults to system and rejected command receipts prune optimistic prompts`() {
        assertEquals(AppearanceTheme.SYSTEM, AppearanceTheme.fromWire(""))
        assertEquals(AppearanceTheme.SYSTEM, AppearanceTheme.fromWire("nonexistent"))
        assertEquals(AppearanceTheme.FOREST_DARK, AppearanceTheme.fromWire("forest_dark"))
        assertEquals(AppearanceTheme.MINT_LIGHT, AppearanceTheme.fromWire("mint_light"))
        assertEquals(AppearanceTheme.FOREST_DARK, AppearanceTheme.SYSTEM.resolve(systemDark = true))
        assertEquals(AppearanceTheme.MINT_LIGHT, AppearanceTheme.SYSTEM.resolve(systemDark = false))

        val rejectedCmd =
            RemoteCommandRecord(
                command =
                    JSONObject()
                        .put("client_cmd_id", "cmd-err-1")
                        .put(
                            "command",
                            JSONObject().put("action", "send_prompt").put("text", "坏消息"),
                        ),
                receipt =
                    JSONObject()
                        .put("status", "rejected")
                        .put("message", "runner offline"),
            )

        val preSeed = evaluateRejectedCommands(listOf(rejectedCmd), emptySet(), announceErrors = false)
        assertTrue("cmd-err-1" in preSeed.newIds)
        assertNull(preSeed.errorMessage)

        val live = evaluateRejectedCommands(listOf(rejectedCmd), emptySet(), announceErrors = true)
        assertEquals("指令被拒绝: runner offline", live.errorMessage)
        assertEquals("坏消息", live.prunedPromptText)

        val duplicate = evaluateRejectedCommands(listOf(rejectedCmd), live.newIds, announceErrors = true)
        assertTrue(duplicate.newIds.isEmpty())
        assertNull(duplicate.errorMessage)
    }
}
