package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.HostHello
import com.agentplatform.mobile.data.RemoteBinding
import com.agentplatform.mobile.data.RemoteSessionRow
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.DiscoveryCandidate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Session-tree tests.
 *
 * The tree is what makes the phone usable with more than one machine: it is a
 * strict partition (no session shown twice) and its ordering is what puts the
 * session that needs attention above the one that is merely recent.
 */
class SessionTreeTest {
    // ----------------------------------------------------------------- helpers

    private fun session(
        runnerId: String,
        hubId: String,
        providerSessionId: String,
        workspace: String,
        state: ExecutionState = ExecutionState.IDLE,
        online: Boolean = true,
        controlConnected: Boolean = true,
    ) = RemoteSessionRow(
        runnerId = runnerId,
        online = online,
        binding =
            RemoteBinding(
                hubSessionId = hubId,
                providerSessionId = providerSessionId,
                workspacePath = workspace,
                hostGeneration = "gen-1",
                controlConnected = controlConnected,
                executionState = state,
                currentTurnId = null,
            ),
    )

    private fun host(
        runnerId: String,
        deviceName: String? = null,
        online: Boolean = true,
        workspaces: List<String> = listOf("/code/agenthub"),
        candidates: List<DiscoveryCandidate> = emptyList(),
    ) = HostDevice(
        runnerId = runnerId,
        online = online,
        revoked = false,
        hello =
            HostHello(
                hostGeneration = "gen-1",
                deviceName = deviceName,
                provider = null,
                workspaces = workspaces,
                candidates = candidates,
                pendingApprovals = 0,
                pendingQuestions = 0,
            ),
    )

    private fun candidate(
        providerSessionId: String,
        provider: String,
        workspace: String = "/code/agenthub",
        title: String? = null,
        activity: Long? = null,
    ) = DiscoveryCandidate(
        providerSessionId = providerSessionId,
        workspacePath = workspace,
        title = title,
        provider = provider,
        model = null,
        totalTokens = null,
        lastActivityAtMs = activity,
        resumable = true,
    )

    private fun flatten(devices: List<DeviceGroup>): List<TreeEntry> =
        devices.flatMap { device -> device.agents.flatMap { agent -> agent.workspaces.flatMap { it.entries } } }

    // ---------------------------------------------------------------- labelling

    @Test
    fun `a workspace slug is shown instead of the ws token`() {
        assertEquals("agenthub", workspaceLabel("ws-agenthub-0123456789abcdef"))
        assertEquals("my project", workspaceLabel("ws-my project-0123456789abcdef"))
        // A real path keeps only its last segment.
        assertEquals("agenthub", workspaceLabel("/home/tanner/code/agenthub"))
        assertEquals("agenthub", workspaceLabel("C:\\code\\agenthub"))
    }

    @Test
    fun `a device shows its own name and only falls back to its id`() {
        assertEquals("工作本", deviceLabel("runner-abcdef123", "工作本"))
        assertEquals("工作本", deviceLabel("runner-abcdef123", "  工作本  "))
        // No name reported: the tail of the id is enough to tell two machines
        // apart without printing a key-shaped identifier in full.
        assertEquals("电脑 · def123", deviceLabel("runner-abcdef123", null))
        assertEquals("电脑 · def123", deviceLabel("runner-abcdef123", "   "))
    }

    // ---------------------------------------------------------------- provider

    @Test
    fun `a reported provider wins over the id prefix`() {
        // The Host knows what it hosts; the prefix is only a guess.
        assertEquals(EngineKind.CODEX, com.agentplatform.mobile.data.engineOfSession("sess_abc", "codex"))
        assertEquals(EngineKind.PLUGIN, com.agentplatform.mobile.data.engineOfSession("claude_abc", "zcode"))
        assertEquals(EngineKind.CLAUDE_CODE, com.agentplatform.mobile.data.engineOfSession("sess_xyz", "claude_code"))
    }

    @Test
    fun `an undescribed session falls back to its id prefix`() {
        assertEquals(EngineKind.PLUGIN, com.agentplatform.mobile.data.engineOfSession("sess_abc", null))
        assertEquals(EngineKind.PLUGIN, com.agentplatform.mobile.data.engineOfSession("gemini_abc", null))
        assertEquals(EngineKind.CLAUDE_CODE, com.agentplatform.mobile.data.engineOfSession("claude_abc", null))
        assertEquals(EngineKind.ANTIGRAVITY, com.agentplatform.mobile.data.engineOfSession("antigravity_abc", null))
        assertEquals(EngineKind.DEEPSEEK_HARNESS, com.agentplatform.mobile.data.engineOfSession("deepseek_abc", null))
        assertEquals(EngineKind.PLUGIN, com.agentplatform.mobile.data.engineOfSession("grok_abc", null))
        assertEquals(EngineKind.CODEX, com.agentplatform.mobile.data.engineOfSession("something_else", null))
    }

    @Test
    fun `every engine the runner publishes resolves to itself`() {
        val reported =
            listOf(
                "zcode" to EngineKind.PLUGIN,
                "gemini_cli" to EngineKind.PLUGIN,
                "codex" to EngineKind.CODEX,
                "claude_code" to EngineKind.CLAUDE_CODE,
                "claude_desktop" to EngineKind.CLAUDE_DESKTOP,
                "antigravity" to EngineKind.ANTIGRAVITY,
                "deepseek_harness" to EngineKind.DEEPSEEK_HARNESS,
                "grok_build" to EngineKind.PLUGIN,
            )
        reported.forEach { (name, engine) ->
            assertEquals(engine, com.agentplatform.mobile.data.engineOfSession("id", name))
        }
    }

    @Test
    fun `every native engine reads a catalog key the runner actually publishes`() {
        val published = setOf("claude_code", "claude_desktop", "codex", "antigravity", "deepseek_harness")
        for (engine in EngineKind.entries.filter { it != EngineKind.PLUGIN }) {
            assertTrue(
                "$engine maps to no published catalog key",
                engine.catalogKeys.any { it in published },
            )
        }
        assertTrue(EngineKind.CLAUDE_DESKTOP.catalogKeys.contains("claude_desktop"))
    }

    // --------------------------------------------------------------------- tree

    @Test
    fun `the tree groups device then agent then workspace and counts every level`() {
        val snapshot =
            Snapshot(
                cursor = 0,
                hosts =
                    listOf(
                        host("runner-a", "工作本", workspaces = listOf("/code/agenthub", "/code/blog")),
                        host("runner-b", "客厅机"),
                    ),
                sessions =
                    listOf(
                        session("runner-a", "hub-1", "sess_a", "/code/agenthub"),
                        session("runner-a", "hub-2", "sess_b", "/code/blog"),
                        session("runner-b", "hub-3", "sess_c", "/code/agenthub"),
                    ),
            )

        val byLabel = buildSessionTree(snapshot).associateBy { it.label }

        assertEquals(setOf("工作本", "客厅机"), byLabel.keys)
        assertEquals(2, byLabel.getValue("工作本").count)
        assertEquals(1, byLabel.getValue("客厅机").count)
        // Two workspaces under one agent on the first machine, one session each.
        val workspaces = byLabel.getValue("工作本").agents.single().workspaces
        assertEquals(setOf("/code/agenthub", "/code/blog"), workspaces.map { it.id }.toSet())
        assertTrue(workspaces.all { it.entries.size == 1 })
    }

    @Test
    fun `an online device outranks an offline one whatever the order of the payload`() {
        val online = host("runner-a", "在线机")
        val offline = host("runner-b", "离线机", online = false)
        val sessions =
            listOf(
                session("runner-a", "hub-1", "sess_a", "/w", online = true),
                session("runner-b", "hub-2", "sess_b", "/w", online = false),
            )

        assertEquals(
            listOf("在线机", "离线机"),
            buildSessionTree(Snapshot(0, listOf(offline, online), sessions)).map { it.label },
        )
        assertEquals(
            listOf("在线机", "离线机"),
            buildSessionTree(Snapshot(0, listOf(online, offline), sessions)).map { it.label },
        )
    }

    @Test
    fun `a discovered conversation already under control is never listed twice`() {
        val snapshot =
            Snapshot(
                cursor = 0,
                hosts =
                    listOf(
                        host(
                            "runner-a",
                            candidates = listOf(candidate("sess_a", "zcode", title = "修登录")),
                        ),
                    ),
                sessions = listOf(session("runner-a", "hub-1", "sess_a", "/code/agenthub")),
            )

        val entries = flatten(buildSessionTree(snapshot))

        assertEquals(1, entries.size)
        assertTrue(entries.single() is TreeEntry.Active)
        // The discovery title still names the controllable row.
        assertEquals("修登录", entries.single().title)
    }

    @Test
    fun `a discovered conversation without a live session becomes a resumable stub`() {
        val snapshot =
            Snapshot(
                cursor = 0,
                hosts = listOf(host("runner-a", candidates = listOf(candidate("sess_old", "codex", title = "旧会话")))),
                sessions = emptyList(),
            )

        val entry = flatten(buildSessionTree(snapshot)).single()

        assertTrue(entry is TreeEntry.Candidate)
        assertEquals("旧会话", entry.title)
        assertEquals(EngineKind.CODEX, entry.provider)
        assertTrue((entry as TreeEntry.Candidate).resumable)
    }

    @Test
    fun `an empty host still appears so a session can be launched from it`() {
        val snapshot = Snapshot(cursor = 0, hosts = listOf(host("runner-a", "工作本")), sessions = emptyList())

        val device = buildSessionTree(snapshot).single()

        assertEquals("工作本", device.label)
        assertTrue(device.agents.isEmpty())
        assertEquals(0, device.count)
    }

    @Test
    fun `a revoked host and the sessions it held are left out`() {
        val revoked =
            HostDevice(runnerId = "runner-a", online = true, revoked = true, hello = host("runner-a").hello)
        val snapshot =
            Snapshot(
                cursor = 0,
                hosts = listOf(revoked, host("runner-b")),
                sessions =
                    listOf(
                        session("runner-a", "hub-1", "sess_a", "/code/agenthub"),
                        session("runner-b", "hub-2", "sess_b", "/code/agenthub"),
                    ),
            )

        val devices = buildSessionTree(snapshot)

        // The Gateway may still echo the revoked machine's sessions; offering
        // them would mean offering control of a device this account lost.
        assertEquals(listOf("runner-b"), devices.map { it.runnerId })
        assertEquals(listOf("hub-2"), flatten(devices).map { (it as TreeEntry.Active).hubSessionId })
    }

    @Test
    fun `a provider filter narrows the tree without dropping the other provider`() {
        // `sess_*` resolves to ZCode, so the live session is filed there too.
        val snapshot =
            Snapshot(
                cursor = 0,
                hosts =
                    listOf(
                        host(
                            "runner-a",
                            candidates =
                                listOf(
                                    candidate("sess_z", "zcode", title = "Z"),
                                    candidate("codex_one", "codex", title = "C"),
                                ),
                        ),
                    ),
                sessions = listOf(session("runner-a", "hub-1", "sess_live", "/code/agenthub")),
            )

        assertEquals(3, flatten(buildSessionTree(snapshot)).size)
        assertEquals(
            setOf("Z", "新会话"),
            flatten(buildSessionTree(snapshot, EngineKind.PLUGIN)).map { it.title }.toSet(),
        )
        assertEquals(
            listOf("C"),
            flatten(buildSessionTree(snapshot, EngineKind.CODEX)).map { it.title },
        )
    }

    // ---------------------------------------------------------------- ordering

    @Test
    fun `a controllable session outranks an online-only one, which outranks history`() {
        val controlled =
            session("runner-a", "hub-1", "sess_a", "/w", controlConnected = true, online = true)
                .let { it.copy(binding = it.binding) }
        val onlineOnly = session("runner-a", "hub-2", "sess_b", "/w", controlConnected = false, online = true)
        val offline = session("runner-a", "hub-3", "sess_c", "/w", controlConnected = false, online = false)

        assertTrue(
            entryActivity(activeEntry(controlled)) >
                entryActivity(activeEntry(onlineOnly)),
        )
        assertTrue(
            entryActivity(activeEntry(onlineOnly)) >
                entryActivity(activeEntry(offline)),
        )
        // Real timestamps still order sessions inside one tier.
        assertTrue(
            entryActivity(activeEntry(controlled).copy(lastActivityAtMs = 2_000)) >
                entryActivity(activeEntry(controlled).copy(lastActivityAtMs = 1_000)),
        )
    }

    private fun activeEntry(row: RemoteSessionRow) =
        TreeEntry.Active(
            key = row.key,
            runnerId = row.runnerId,
            providerSessionId = row.binding.providerSessionId,
            workspaceId = row.binding.workspacePath,
            provider = EngineKind.PLUGIN,
            title = "会话",
            lastActivityAtMs = null,
            hubSessionId = row.binding.hubSessionId,
            online = row.online,
            controlConnected = row.binding.controlConnected,
            executionState = row.binding.executionState,
        )

    @Test
    fun `workspaces are ordered by their most recent session`() {
        val snapshot =
            Snapshot(
                cursor = 0,
                hosts =
                    listOf(
                        host(
                            "runner-a",
                            workspaces = listOf("/w/old", "/w/new"),
                            candidates =
                                listOf(
                                    candidate("sess_old", "zcode", workspace = "/w/old", activity = 1_000),
                                    candidate("sess_new", "zcode", workspace = "/w/new", activity = 9_000),
                                ),
                        ),
                    ),
                sessions = emptyList(),
            )

        val workspaces = buildSessionTree(snapshot).single().agents.single().workspaces

        assertEquals(listOf("/w/new", "/w/old"), workspaces.map { it.id })
    }

    // ------------------------------------------------------------- attention

    @Test
    fun `attention lists only controllable sessions blocked on a person`() {
        val snapshot =
            Snapshot(
                cursor = 0,
                hosts = listOf(host("runner-a")),
                sessions =
                    listOf(
                        session("runner-a", "hub-1", "sess_a", "/w", ExecutionState.AWAITING_PERMISSION),
                        session("runner-a", "hub-2", "sess_b", "/w", ExecutionState.AWAITING_INPUT),
                        session("runner-a", "hub-3", "sess_c", "/w", ExecutionState.RUNNING),
                        // Waiting, but nobody can act on it: not attention.
                        session(
                            "runner-a",
                            "hub-4",
                            "sess_d",
                            "/w",
                            ExecutionState.AWAITING_INPUT,
                            controlConnected = false,
                        ),
                    ),
            )

        val attention = attentionEntries(buildSessionTree(snapshot))

        assertEquals(setOf("hub-1", "hub-2"), attention.map { (it as TreeEntry.Active).hubSessionId }.toSet())
    }

    @Test
    fun `entry keys address an active session by runner and hub`() {
        val active = session("runner-a", "hub-1", "sess_a", "/w")
        assertEquals(
            "runner-a/hub-1",
            entryKeyOf(
                TreeEntry.Active(
                    key = active.key,
                    runnerId = "runner-a",
                    providerSessionId = "sess_a",
                    workspaceId = "/w",
                    provider = EngineKind.PLUGIN,
                    title = "会话",
                    lastActivityAtMs = null,
                    hubSessionId = "hub-1",
                    online = true,
                    controlConnected = true,
                    executionState = ExecutionState.IDLE,
                ),
            ),
        )
        // A candidate has no hub session yet; its own key is what the launch
        // screen resumes from.
        assertEquals(
            "runner-a/sess_a",
            entryKeyOf(
                TreeEntry.Candidate(
                    key = "runner-a/sess_a",
                    runnerId = "runner-a",
                    providerSessionId = "sess_a",
                    workspaceId = "/w",
                    provider = EngineKind.PLUGIN,
                    title = "会话",
                    lastActivityAtMs = null,
                    resumable = true,
                ),
            ),
        )
    }
}
