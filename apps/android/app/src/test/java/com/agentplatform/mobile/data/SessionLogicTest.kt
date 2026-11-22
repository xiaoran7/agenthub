package com.agentplatform.mobile.data

import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Event-window and idempotency tests.
 *
 * These two classes decide whether the phone double-runs a command and whether
 * a resumed conversation is complete, so both are pinned here rather than left
 * to a screen to exercise them by hand.
 */
class SessionLogicTest {
    // ------------------------------------------------------------ event window

    private fun event(cursor: Long, kind: String = "item/completed"): RemoteEvent =
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
                                .put("raw", JSONObject().put("params", JSONObject())),
                        ),
                ),
        )

    private fun snapshot(cursor: Long, items: Int = 2): RemoteEvent =
        RemoteEvent.parse(
            JSONObject()
                .put("cursor", cursor)
                .put(
                    "payload",
                    JSONObject()
                        .put("type", "event")
                        .put(
                            "event",
                            JSONObject()
                                .put("kind", "history/snapshot")
                                .put(
                                    "raw",
                                    JSONObject()
                                        .put(
                                            "params",
                                            JSONObject()
                                                .put("items", JSONArray().apply { repeat(items) { put(JSONObject()) } }),
                                        ),
                                ),
                        ),
                ),
        )

    private fun approvalRequested(cursor: Long, approvalId: String): RemoteEvent =
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
                                .put("kind", "approval_requested")
                                .put(
                                    "raw",
                                    JSONObject()
                                        .put("params", JSONObject().put("approval_id", approvalId)),
                                ),
                        ),
                ),
        )

    @Test
    fun `appended frames advance the cursor and report change`() {
        val window = EventWindow()
        assertTrue(window.append(listOf(event(1), event(2))))
        assertEquals(2L, window.cursor)
        assertEquals(2, window.size)
    }

    @Test
    fun `a replayed page below the cursor is ignored rather than duplicated`() {
        val window = EventWindow()
        window.append(listOf(event(1), event(2)))

        // The Gateway may resend on reconnect; a transcript must not repeat it.
        assertFalse(window.append(listOf(event(1), event(2))))
        assertEquals(2, window.size)
        assertEquals(2L, window.cursor)
    }

    @Test
    fun `an empty page costs no recomposition`() {
        val window = EventWindow()
        assertFalse(window.append(emptyList()))
        assertEquals(0L, window.cursor)
    }

    @Test
    fun `reset seeds from the resume point and replaces held frames`() {
        val window = EventWindow()
        window.append(listOf(event(1), event(2)))

        window.reset(initialCursor = 100, batch = listOf(event(101)))

        assertEquals(1, window.size)
        assertEquals(101L, window.cursor)
    }

    @Test
    fun `a window at capacity keeps everything from the newest history snapshot`() {
        // Seeded from a resume point, so the first frames after it are live.
        val window = EventWindow(capacity = 10)
        window.append(listOf(event(1)))
        window.append(listOf(snapshot(2)))
        for (cursor in 3L..40L) window.append(listOf(event(cursor)))

        // The snapshot is the only thing that makes the transcript complete, so
        // trimming must never reach past it however many deltas follow.
        assertTrue(window.size >= 2)
        assertTrue(window.snapshot().any { it.eventKind == "history/snapshot" })
        assertEquals(40L, window.cursor)
    }

    @Test
    fun `a snapshot that reaches the front does not uncap the window`() {
        // The snapshot is the first frame of a seeded read, so it walks to the
        // front within a few appends. Pinning everything before it then became
        // pinning nothing, and the window grew without bound from there.
        val window = EventWindow(capacity = 10)
        window.append(listOf(snapshot(1)))
        for (cursor in 2L..200L) window.append(listOf(event(cursor)))

        assertEquals(10, window.size)
        assertEquals(200L, window.cursor)
    }

    @Test
    fun `an approval older than the snapshot survives the trim`() {
        // A pending approval from before the snapshot is the only thing that
        // keeps the cards on screen when a long stretch of deltas pushes the
        // window over its cap; dropping it leaves a blocked turn with no
        // button to unblock it.
        val window = EventWindow(capacity = 6)
        window.append(listOf(approvalRequested(1, "appr-1"), snapshot(2)))
        for (cursor in 3L..40L) window.append(listOf(event(cursor)))

        val kept = window.snapshot()
        assertTrue(kept.any { it.eventKind == "approval_requested" })
        assertTrue(kept.any { it.eventKind == "history/snapshot" })
        // ...but the cap still holds.
        assertTrue(window.size <= 6)
    }

    @Test
    fun `an empty history snapshot does not pin the window`() {
        // `items: []` is a snapshot of nothing; keeping it would waste the
        // budget that live frames need.
        val window = EventWindow(capacity = 4)
        window.append(listOf(snapshot(1, items = 0)))
        for (cursor in 2L..20L) window.append(listOf(event(cursor)))

        assertEquals(4, window.size)
        assertEquals(20L, window.cursor)
        assertFalse(window.snapshot().any { it.eventKind == "history/snapshot" })
    }

    @Test
    fun `clear drops frames and the cursor`() {
        val window = EventWindow()
        window.append(listOf(event(5)))
        window.clear()

        assertEquals(0, window.size)
        assertEquals(0L, window.cursor)
    }

    @Test
    fun `superseded deltas before a large history snapshot are trimmed immediately`() {
        val window = EventWindow(capacity = 1_200)
        val preSnapshot = (1L..80L).map { cursor ->
            if (cursor == 5L) approvalRequested(cursor, "appr-keep") else event(cursor)
        }
        window.append(preSnapshot)
        window.append(listOf(snapshot(81), event(82), event(83)))

        val kept = window.snapshot()
        assertTrue(kept.size < 10)
        assertTrue(kept.any { it.eventKind == "approval_requested" })
        assertTrue(kept.any { it.eventKind == "history/snapshot" })
        assertEquals(83L, window.cursor)
    }

    // ---------------------------------------------------------- command ledger

    @Test
    fun `the same action payload reuses its key so a retry is idempotent`() {
        val ledger = CommandLedger()
        val first = ledger.keyFor("r/h", "send_prompt", """{"text":"继续"}""")
        val second = ledger.keyFor("r/h", "send_prompt", """{"text":"继续"}""")

        // A dropped response must replay the original receipt, not run twice.
        assertEquals(first, second)
    }

    @Test
    fun `a different payload gets a different key`() {
        val ledger = CommandLedger()
        assertNotEquals(
            ledger.keyFor("r/h", "send_prompt", """{"text":"继续"}"""),
            ledger.keyFor("r/h", "send_prompt", """{"text":"停止"}"""),
        )
        // Same body, different action: still a different command.
        assertNotEquals(
            ledger.keyFor("r/h", "interrupt", """{"text":"继续"}"""),
            ledger.keyFor("r/h", "send_prompt", """{"text":"继续"}"""),
        )
    }

    @Test
    fun `forgetting a key mints a fresh one for the next deliberate attempt`() {
        val ledger = CommandLedger()
        val first = ledger.keyFor("r/h", "send_prompt", "body")
        ledger.forget("r/h", "send_prompt", "body")
        val second = ledger.keyFor("r/h", "send_prompt", "body")

        assertNotEquals(first, second)
    }

    @Test
    fun `the ledger holds one key per live payload and forget drops it`() {
        val ledger = CommandLedger()
        ledger.keyFor("r/h", "send_prompt", "body")
        assertEquals(1, ledger.size)

        // A second call for the same payload is a lookup, not a new entry.
        ledger.keyFor("r/h", "send_prompt", "body")
        assertEquals(1, ledger.size)

        ledger.forget("r/h", "send_prompt", "body")
        assertEquals(0, ledger.size)
    }

    @Test
    fun `only a permanent refusal clears the key`() {
        val ledger = CommandLedger()
        val first = ledger.keyFor("r/h", "send_prompt", "body")

        // 5xx and 0 (network) are transient: keep the key so the retry stays a
        // replay of the same command.
        ledger.forgetIfPermanent("r/h", "send_prompt", "body", 500)
        ledger.forgetIfPermanent("r/h", "send_prompt", "body", 0)
        assertEquals(first, ledger.keyFor("r/h", "send_prompt", "body"))

        // The Gateway answers 400 for retryable states too (runner_offline,
        // session_control_lost, host_generation_mismatch, approval_not_pending,
        // runner_backpressure), so a blanket 4xx rule would let the retry run
        // the same action twice. Only a verdict about this command is final.
        ledger.forgetIfPermanent("r/h", "send_prompt", "body", 400)
        ledger.forgetIfPermanent("r/h", "send_prompt", "body", 408)
        ledger.forgetIfPermanent("r/h", "send_prompt", "body", 409)
        ledger.forgetIfPermanent("r/h", "send_prompt", "body", 429)
        assertEquals(first, ledger.keyFor("r/h", "send_prompt", "body"))

        ledger.forgetIfPermanent("r/h", "send_prompt", "body", 403)
        assertNotEquals(first, ledger.keyFor("r/h", "send_prompt", "body"))
    }

    @Test
    fun `the same command in another session gets its own key`() {
        val ledger = CommandLedger()

        // `{"action":"interrupt"}` is byte-identical in every session; a shared
        // key would make the Gateway replay the first session's receipt
        // instead of running the second one's command.
        assertNotEquals(
            ledger.keyFor("runner-a/hub-1", "interrupt", """{"action":"interrupt"}"""),
            ledger.keyFor("runner-a/hub-2", "interrupt", """{"action":"interrupt"}"""),
        )
    }

    @Test
    fun `the ledger does not grow without bound`() {
        val ledger = CommandLedger()
        val key = ledger.keyFor("r/h", "send_prompt", "body")
        ledger.forget("r/h", "send_prompt", "body")

        assertEquals(0, ledger.size)
        assertNotEquals(key, ledger.keyFor("r/h", "send_prompt", "body"))
        assertEquals(1, ledger.size)
    }

    // -------------------------------------------------------------- composer

    @Test
    fun `a prompt is only sendable when the host is online, controlled and settled`() {
        assertEquals(
            ComposerMode.BLOCKED,
            ComposerMode.of(hostOnline = false, controlConnected = true, ExecutionState.IDLE),
        )
        assertEquals(
            ComposerMode.BLOCKED,
            ComposerMode.of(hostOnline = true, controlConnected = false, ExecutionState.IDLE),
        )
        assertEquals(
            ComposerMode.BUSY,
            ComposerMode.of(hostOnline = true, controlConnected = true, ExecutionState.RUNNING),
        )
        // Waiting on a person is still not a moment to send another prompt.
        assertEquals(
            ComposerMode.BUSY,
            ComposerMode.of(hostOnline = true, controlConnected = true, ExecutionState.AWAITING_PERMISSION),
        )
        assertEquals(
            ComposerMode.READY,
            ComposerMode.of(hostOnline = true, controlConnected = true, ExecutionState.IDLE),
        )
        assertEquals(
            ComposerMode.READY,
            ComposerMode.of(hostOnline = true, controlConnected = true, ExecutionState.COMPLETED),
        )
    }

    @Test
    fun `a fresh hub session id is a uuid`() {
        val id = newHubSessionId()
        assertTrue(
            id.matches(Regex("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")),
        )
        assertNotEquals(id, newHubSessionId())
    }

    @Test
    fun `command ledger hashes large payloads distinctly and evicts LRU entries`() {
        val ledger = CommandLedger(maxEntries = 3)
        val prefix = "a".repeat(70_000)
        val k1 = ledger.keyFor("s1", "prompt", prefix + "1")
        val k2 = ledger.keyFor("s1", "prompt", prefix + "2")
        assertNotEquals(k1, k2)
        assertEquals(k1, ledger.keyFor("s1", "prompt", prefix + "1"))

        val k3 = ledger.keyFor("s1", "prompt", "third")
        // Access k1 again so k2 becomes the least-recently-used entry
        assertEquals(k1, ledger.keyFor("s1", "prompt", prefix + "1"))
        ledger.keyFor("s1", "prompt", "fourth")

        // k2 should have been evicted while k1 stayed
        assertEquals(k1, ledger.keyFor("s1", "prompt", prefix + "1"))
        assertEquals(k3, ledger.keyFor("s1", "prompt", "third"))
        assertNotEquals(k2, ledger.keyFor("s1", "prompt", prefix + "2"))
    }

    @Test
    fun `event window retains turn lifecycle markers across pre-snapshot supersede trim and cursor reset`() {
        val window = EventWindow(capacity = 120)
        val batch =
            buildList {
                add(event(1, "turn/started"))
                for (c in 2L..70L) {
                    add(event(c, "item/delta"))
                }
                add(event(71, "turn/completed"))
                add(event(72, "error"))
                add(snapshot(73))
                add(event(74, "item/delta"))
            }
        assertTrue(window.append(batch))
        val cursors = window.snapshot().map { it.cursor }.toSet()
        assertTrue(1L in cursors)
        assertTrue(71L in cursors)
        assertTrue(72L in cursors)
        assertTrue(73L in cursors)
        assertFalse(2L in cursors)

        window.reset(initialCursor = 0L, batch = emptyList())
        assertEquals(0L, window.cursor)
        assertTrue(window.snapshot().isEmpty())
    }
}
