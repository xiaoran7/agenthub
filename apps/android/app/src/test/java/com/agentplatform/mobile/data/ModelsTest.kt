package com.agentplatform.mobile.data

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire-contract tests for the Remote v3 payloads.
 *
 * Every field parsed here is defined in `packages/contracts/src/remote.ts`;
 * if the Gateway renames one, these tests are what fails before the app ships
 * a screen that silently shows nothing.
 */
class ModelsTest {
    // ------------------------------------------------------------- execution

    @Test
    fun `every execution state maps to a wire value and an unknown one degrades safely`() {
        assertEquals(ExecutionState.AWAITING_PERMISSION, ExecutionState.fromWire("awaiting_permission"))
        assertEquals(ExecutionState.RUNNING, ExecutionState.fromWire("running"))
        // An unfamiliar state must never crash a list: it becomes UNKNOWN.
        assertEquals(ExecutionState.UNKNOWN, ExecutionState.fromWire("quantum_superposition"))
        assertEquals(ExecutionState.UNKNOWN, ExecutionState.fromWire(""))

        assertTrue(ExecutionState.AWAITING_INPUT.waiting)
        assertTrue(ExecutionState.AWAITING_PERMISSION.waiting)
        assertFalse(ExecutionState.RUNNING.waiting)
        assertTrue(ExecutionState.RUNNING.busy)
        assertTrue(ExecutionState.COMPLETED.finished)
        assertTrue(ExecutionState.TERMINATED.finished)
        assertFalse(ExecutionState.IDLE.finished)
    }

    // --------------------------------------------------------------- snapshot

    @Test
    fun `snapshot parses runners and sessions and counts only live ones`() {
        val json =
            JSONObject()
                .put("cursor", 42)
                .put(
                    "runners",
                    JSONArray()
                        .put(
                            JSONObject()
                                .put("runner_id", "runner-a")
                                .put("online", true)
                                .put("revoked", false)
                                .put(
                                    "hello",
                                    JSONObject()
                                        .put("host_generation", "gen-1")
                                        .put("device_name", "工作本")
                                        .put("workspaces", JSONArray().put("/home/tanner/code")),
                                ),
                        )
                        .put(
                            JSONObject()
                                .put("runner_id", "runner-b")
                                .put("online", false)
                                .put("revoked", false),
                        )
                        .put(
                            JSONObject()
                                .put("runner_id", "runner-c")
                                .put("online", true)
                                .put("revoked", true),
                        ),
                )
                .put(
                    "sessions",
                    JSONArray()
                        .put(sessionRow("runner-a", "hub-1", "running", online = true))
                        .put(sessionRow("runner-a", "hub-2", "awaiting_input", online = true))
                        .put(sessionRow("runner-b", "hub-3", "running", online = false)),
                )

        val snapshot = Snapshot.parse(json)

        assertEquals(42L, snapshot.cursor)
        assertEquals(3, snapshot.hosts.size)
        // A revoked runner is not an online runner, however its flag reads.
        assertEquals(1, snapshot.onlineHostCount)
        assertEquals(1, snapshot.runningCount)
        assertEquals(1, snapshot.waitingCount)
        assertEquals("工作本", snapshot.host("runner-a")?.hello?.deviceName)
        assertEquals("gen-1", snapshot.host("runner-a")?.hostGeneration)
        assertEquals("/home/tanner/code", snapshot.host("runner-a")?.workspaces?.single())
    }

    @Test
    fun `a malformed runner row is skipped instead of blanking the fleet`() {
        val snapshot =
            Snapshot.parse(
                JSONObject()
                    .put("runners", JSONArray().put(JSONObject().put("online", true)))
                    .put("sessions", JSONArray().put(JSONObject().put("online", true))),
            )

        assertTrue(snapshot.hosts.isEmpty())
        assertTrue(snapshot.sessions.isEmpty())
    }

    @Test
    fun `an empty snapshot is a valid value and safe to compare by identity`() {
        assertEquals(0L, Snapshot.Empty.cursor)
        assertTrue(Snapshot.Empty.hosts.isEmpty())
        assertEquals(0, Snapshot.Empty.waitingCount)
        assertNull(Snapshot.Empty.session("runner-a", "hub-1"))
    }

    // --------------------------------------------------------------- approvals

    @Test
    fun `an approval falls back through command description and path`() {
        val withCommand =
            PendingApproval.parse(
                JSONObject()
                    .put("approval_id", "approval-1")
                    .put("hub_session_id", "hub-1")
                    .put("command_preview", "rm -rf build")
                    .put("description", "删除构建目录"),
            )
        assertEquals("rm -rf build", withCommand.summary)

        val onlyPath =
            PendingApproval.parse(
                JSONObject()
                    .put("approval_id", "approval-2")
                    .put("hub_session_id", "hub-1")
                    .put("path", "/repo/src/main.kt"),
            )
        assertEquals("/repo/src/main.kt", onlyPath.summary)

        // Nothing optional reported: the card still has to say something real.
        val bare = PendingApproval.parse(JSONObject().put("approval_id", "approval-3"))
        assertTrue(bare.summary.isNotBlank())
    }

    @Test
    fun `approval permissions default to allowed and honour an explicit refusal`() {
        val defaulted = PendingApproval.parse(JSONObject().put("approval_id", "a"))
        assertTrue(defaulted.canApprove)
        assertTrue(defaulted.canDecline)

        val refused =
            PendingApproval.parse(
                JSONObject()
                    .put("approval_id", "b")
                    .put("can_approve", false)
                    .put("can_decline", false),
            )
        assertFalse(refused.canApprove)
        assertFalse(refused.canDecline)
    }

    @Test
    fun `question options parse with optional descriptions and multi-select`() {
        val question =
            PendingQuestion.parse(
                JSONObject()
                    .put("question_id", "q-1")
                    .put("hub_session_id", "hub-1")
                    .put("prompt", "用哪个分支？")
                    .put("header", "分支")
                    .put("multi_select", true)
                    .put(
                        "options",
                        JSONArray()
                            .put(JSONObject().put("label", "main").put("description", "直接改主线"))
                            .put(JSONObject().put("label", "feature")),
                    ),
            )

        assertEquals("分支", question.header)
        assertTrue(question.multiSelect)
        assertEquals(listOf("main", "feature"), question.options.map { it.label })
        assertEquals("直接改主线", question.options[0].description)
        assertNull(question.options[1].description)
    }

    // ---------------------------------------------------------------- hosts

    @Test
    fun `host hello carries workspaces and discovery candidates`() {
        val host =
            HostDevice.parse(
                JSONObject()
                    .put("runner_id", "runner-a")
                    .put("online", true)
                    .put(
                        "hello",
                        JSONObject()
                            .put("host_generation", "gen-9")
                            .put("workspaces", JSONArray().put("/w/one").put("/w/two"))
                            .put(
                                "discovery",
                                JSONObject()
                                    .put(
                                        "candidates",
                                        JSONArray()
                                            .put(
                                                JSONObject()
                                                    .put("provider_session_id", "sess_abc")
                                                    .put("workspace_path", "/w/one")
                                                    .put("title", "修登录")
                                                    .put("provider", "zcode")
                                                    .put("last_activity_at_ms", 1_700_000_000_000L)
                                                    .put("resumable", true),
                                            )
                                            // Missing workspace: unusable, must be dropped.
                                            .put(JSONObject().put("provider_session_id", "sess_broken")),
                                    ),
                            ),
                    ),
            )

        assertEquals("gen-9", host?.hostGeneration)
        assertEquals(listOf("/w/one", "/w/two"), host?.workspaces)
        assertEquals(1, host?.hello?.candidates?.size)
        assertEquals("修登录", host?.hello?.candidates?.single()?.title)
        assertEquals(1_700_000_000_000L, host?.hello?.candidates?.single()?.lastActivityAtMs)
    }

    @Test
    fun `a host without hello still lists, with empty workspaces`() {
        val host = HostDevice.parse(JSONObject().put("runner_id", "runner-a").put("online", true))

        assertEquals("", host?.hostGeneration)
        assertTrue(host?.workspaces?.isEmpty() == true)
        assertNull(host?.hello)
    }

    // --------------------------------------------------------------- sessions

    @Test
    fun `session binding parses control state and execution state`() {
        val row =
            RemoteSessionRow.parse(
                sessionRow("runner-a", "hub-1", "awaiting_permission", online = true)
                    .put(
                        "binding",
                        JSONObject()
                            .put("hub_session_id", "hub-1")
                            .put("provider_session_id", "sess_abc")
                            .put("workspace_path", "ws-agenthub-0123456789abcdef")
                            .put("host_generation", "gen-1")
                            .put("control_connected", true)
                            .put("execution_state", "awaiting_permission"),
                    ),
            )

        assertEquals("runner-a", row?.runnerId)
        assertTrue(row?.online == true)
        assertEquals("runner-a/hub-1", row?.key)
        assertEquals(ExecutionState.AWAITING_PERMISSION, row?.binding?.executionState)
        assertTrue(row?.binding?.controllable(hostOnline = true) == true)
        // A host the Gateway reports offline cannot be controlled even if the
        // binding still claims a live channel.
        assertFalse(row?.binding?.controllable(hostOnline = false) == true)
    }

    // ----------------------------------------------------------- session detail

    @Test
    fun `resume point is the earlier of transcript and approvals`() {
        val both =
            SessionDetail.parse(
                JSONObject()
                    .put("cursor", 500)
                    .put(
                        "resume",
                        JSONObject().put("transcript", 400).put("approvals", 120),
                    ),
            )
        // Starting after transcript alone would skip approvals that arrived
        // earlier and were never repeated.
        assertEquals(120L, both.resumeAfter)

        val transcriptOnly =
            SessionDetail.parse(JSONObject().put("resume", JSONObject().put("transcript", 88)))
        assertEquals(88L, transcriptOnly.resumeAfter)

        val approvalsOnly =
            SessionDetail.parse(JSONObject().put("resume", JSONObject().put("approvals", 7)))
        assertEquals(7L, approvalsOnly.resumeAfter)

        val fresh = SessionDetail.parse(JSONObject())
        // No history at all: read from the beginning.
        assertNull(fresh.resumeAfter)
    }

    @Test
    fun `session detail parses the command log`() {
        // Wire shape is `{ command: RemoteCommand, receipt }`, and the action
        // lives one level deeper still: `command.command.action`.
        val detail =
            SessionDetail.parse(
                JSONObject()
                    .put(
                        "commands",
                        JSONArray()
                            .put(
                                JSONObject()
                                    .put(
                                        "command",
                                        JSONObject()
                                            .put("client_cmd_id", "cmd-1")
                                            .put("hub_session_id", "hub-1")
                                            .put(
                                                "command",
                                                JSONObject().put("action", "send_prompt"),
                                            ),
                                    )
                                    .put("receipt", JSONObject().put("status", "accepted")),
                            )
                            .put(
                                JSONObject()
                                    .put(
                                        "command",
                                        JSONObject()
                                            .put("client_cmd_id", "cmd-2")
                                            .put("hub_session_id", "hub-1")
                                            .put(
                                                "command",
                                                JSONObject().put("action", "interrupt"),
                                            ),
                                    ),
                            ),
                    ),
            )

        assertEquals(listOf("send_prompt", "interrupt"), detail.commands.map { it.action })
        assertEquals(listOf("hub-1", "hub-1"), detail.commands.map { it.hubSessionId })
        assertEquals("accepted", detail.commands[0].receipt?.optString("status"))
        assertNull(detail.commands[1].receipt)
    }

    // ----------------------------------------------------------------- events

    @Test
    fun `event frames expose kind and params while receipts stay separate`() {
        val event =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 12)
                    .put("runner_id", "runner-a")
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "event")
                            .put("event", JSONObject().put("kind", "turn/started")),
                    ),
            )
        assertEquals("event", event.payloadType)
        assertEquals("turn/started", event.eventKind)
        assertNull(event.receipt)

        val receipt =
            RemoteEvent.parse(
                JSONObject()
                    .put("cursor", 13)
                    .put(
                        "payload",
                        JSONObject()
                            .put("type", "receipt")
                            .put("receipt", JSONObject().put("status", "accepted")),
                    ),
            )
        assertEquals("", receipt.eventKind)
        assertEquals("accepted", receipt.receipt?.optString("status"))
    }

    // ---------------------------------------------------------------- account

    @Test
    fun `account view falls back to email and tolerates a missing workspace`() {
        val account =
            AccountView.parse(
                JSONObject()
                    .put(
                        "user",
                        JSONObject().put("user_id", "u-1").put("email", "tanner@example.com"),
                    ),
            )

        assertEquals("u-1", account.userId)
        assertEquals("tanner@example.com", account.displayName)
        assertNull(account.workspaceName)
        assertNull(account.plan)
    }

    // ------------------------------------------------------------------ config

    @Test
    fun `remote gateway requires https but loopback development may use http`() {
        assertNull(AppConfig("https://agent.example.com", sessionCookie = "agent_pulse_session=abc").validationError())
        assertNull(AppConfig("http://127.0.0.1:3001", developmentUserId = "local-user").validationError())
        assertNull(AppConfig("http://localhost:3001", developmentUserId = "local-user").validationError())

        assertEquals(
            "远程 Gateway 必须使用 HTTPS",
            AppConfig("http://agent.example.com", sessionCookie = "agent_pulse_session=abc").validationError(),
        )
    }

    @Test
    fun `a remote gateway with no credential is refused before the call is made`() {
        assertEquals(
            "生产 Gateway 必须登录账号或填写访问令牌",
            AppConfig("https://agent.example.com").validationError(),
        )
        assertEquals(
            "请登录账号或填写访问令牌；本机开发模式可填写开发用户",
            AppConfig("http://127.0.0.1:3001").validationError(),
        )
    }

    @Test
    fun `gateway address rejects embedded credentials and query strings`() {
        // A credential smuggled into the URL would end up in logs and in the
        // saved config; the parser refuses it rather than storing it.
        assertEquals(
            "Gateway 地址不能包含凭据、查询参数或片段",
            AppConfig("https://user:pass@agent.example.com", accessToken = "t").validationError(),
        )
        assertEquals(
            "Gateway 地址不能包含凭据、查询参数或片段",
            AppConfig("https://agent.example.com?token=abc", accessToken = "t").validationError(),
        )
        assertEquals("Gateway 地址缺少主机名", AppConfig("not-a-url", accessToken = "t").validationError())
    }

    @Test
    fun `normalizing strips the trailing slash so paths never double up`() {
        val config = AppConfig("  https://agent.example.com///  ", accessToken = " tok ").normalized()

        assertEquals("https://agent.example.com", config.gatewayBase)
        assertEquals("tok", config.accessToken)
        assertTrue(config.hasCredential)
    }

    @Test
    fun `a control character in any credential is refused`() {
        assertTrue(
            AppConfig("https://agent.example.com", accessToken = "tok\nen")
                .validationError()
                ?.contains("控制字符") == true,
        )
        assertTrue(
            AppConfig("https://agent.example.com", sessionCookie = "agent_pulse_session=a\nb")
                .validationError() != null,
        )
    }

    @Test
    fun `remote plugins response parses gateway runner field`() {
        val json =
            JSONObject()
                .put(
                    "plugins",
                    JSONArray().put(
                        JSONObject()
                            .put("id", "zcode")
                            .put("name", "ZCode")
                            .put("installed", true)
                            .put("enabled", true)
                            .put("available", true)
                            .put("runtimeStatus", "ready"),
                    ),
                )
                .put(
                    "runners",
                    JSONArray().put(
                        JSONObject()
                            .put("runner", "dev_runner_1")
                            .put("registry_url", "https://example.com/registry.json")
                            .put(
                                "plugins",
                                JSONArray().put(
                                    JSONObject()
                                        .put("id", "zcode")
                                        .put("name", "ZCode")
                                        .put("sessionPrefix", "zcode_")
                                        .put("sessionPrefixes", JSONArray().put("zcode_").put("sess_"))
                                        .put("installed", true)
                                        .put("enabled", true),
                                ),
                            ),
                    ),
                )
        val parsed = RemotePluginsResponse.parse(json)
        assertEquals(1, parsed.runners.size)
        assertEquals("dev_runner_1", parsed.runners.single().runnerId)
        val zcodePlugin = parsed.runners.single().plugins.single()
        assertEquals("zcode", zcodePlugin.id)
        assertEquals(listOf("zcode_", "sess_"), zcodePlugin.sessionPrefixes)
        assertEquals(listOf("zcode_", "sess_"), zcodePlugin.allSessionPrefixes)
    }
}

/** One `sessions[]` entry as `GET /v3/remote/sessions` emits it. */
private fun sessionRow(
    runnerId: String,
    hubId: String,
    state: String,
    online: Boolean,
): JSONObject =
    JSONObject()
        .put("runner_id", runnerId)
        .put("online", online)
        .put(
            "binding",
            JSONObject()
                .put("hub_session_id", hubId)
                .put("provider_session_id", "sess_$hubId")
                .put("workspace_path", "ws-agenthub-0123456789abcdef")
                .put("host_generation", "gen-1")
                .put("control_connected", online)
                .put("execution_state", state),
        )
