package com.agentplatform.mobile.data

import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ModelsTest {
    @Test
    fun overlappingFiltersMatchTheSharedSessionTaxonomy() {
        val waiting = session("waiting", Presence.LIVE, ActivityStatus.WAITING_PERMISSION, 3)
        val completed = session("completed", Presence.PERSISTED, ActivityStatus.COMPLETED, 2)
        val working = session("working", Presence.LIVE, ActivityStatus.WORKING, 1)
        val sessions = listOf(completed, working, waiting)

        assertEquals(listOf("waiting", "working"), filterSessions(sessions, SessionFilter.LIVE).map { it.eventId })
        assertEquals(listOf("waiting"), filterSessions(sessions, SessionFilter.WAITING).map { it.eventId })
        assertEquals(listOf("completed"), filterSessions(sessions, SessionFilter.FINISHED).map { it.eventId })
        assertEquals(listOf("completed"), filterSessions(sessions, SessionFilter.HISTORY).map { it.eventId })
        assertEquals(1, countSessions(sessions).waiting)
    }

    @Test
    fun sortingKeepsWaitingFirstThenUsesObservationAndSequence() {
        val olderWaiting =
            session(
                "older-waiting",
                Presence.LIVE,
                ActivityStatus.WAITING_INPUT,
                1,
                "2026-07-29T08:00:00Z",
            )
        val newerWorking =
            session(
                "newer-working",
                Presence.LIVE,
                ActivityStatus.WORKING,
                9,
                "2026-07-29T09:00:00Z",
            )

        assertEquals(
            listOf("older-waiting", "newer-working"),
            filterSessions(listOf(newerWorking, olderWaiting), SessionFilter.ALL).map { it.eventId },
        )
    }

    @Test
    fun remoteGatewayRequiresHttpsButLoopbackDevelopmentMayUseHttp() {
        assertNull(
            AppConfig("https://agent.example.com", "token", "").validationError(),
        )
        assertNull(
            AppConfig("http://127.0.0.1:3001", "", "local-user").validationError(),
        )
        assertEquals(
            "远程 Gateway 必须使用 HTTPS",
            AppConfig("http://agent.example.com", "token", "").validationError(),
        )
        assertEquals(
            "生产 Gateway 必须填写访问令牌",
            AppConfig("https://agent.example.com", "", "local-user").validationError(),
        )
    }

    private fun session(
        id: String,
        presence: Presence,
        status: ActivityStatus,
        sequence: Long,
        observedAt: String = "2026-07-29T08:30:00Z",
    ) =
        SessionRecord(
            eventId = id,
            sequence = sequence,
            deviceId = "device-a",
            agentType = AgentType.CODEX,
            agentSessionId = id,
            observedAt = Instant.parse(observedAt),
            receivedAt = null,
            project = ProjectInfo(displayName = "Agent-Platform", pathFingerprint = null),
            state = SessionState(presence, status, null),
        )
}
