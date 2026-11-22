package com.agentplatform.mobile.data

import java.nio.charset.StandardCharsets
import java.security.MessageDigest
import java.util.UUID

// =========================================================================
// Session logic that does not need Android
//
// Two pieces of behaviour decide whether the phone is trustworthy, and both
// are pure functions so they are pinned by unit tests:
//
//   * EventWindow — which frames a reader still holds. Dropping too much
//     loses the seeded history; keeping too much makes a long session slow.
//   * CommandLedger — the idempotency keys. A retry after a dropped response
//     must replay the same key or the Agent runs the action twice.
// =========================================================================

/**
 * The bounded window of frames one open session keeps.
 *
 * Trimming is not a plain `takeLast`: the transcript is built from the newest
 * `history/snapshot` inside the window, so dropping that frame would silently
 * turn a resumed conversation into a partial one. Two rules keep both halves
 * of that promise:
 *
 *   * the newest non-empty `history/snapshot` and everything after it always
 *     survive;
 *   * the approval/question frames before it survive too, because they are the
 *     only other thing that is not superseded replay — a `ready` frame is the
 *     authoritative reset point for pending approvals, and an outstanding
 *     request older than the snapshot would otherwise vanish from the cards
 *     while the turn stays blocked.
 *
 * The hard cap still applies to everything else, so a long session cannot grow
 * the window without bound.
 */
class EventWindow(private val capacity: Int = 1_200) {
    private val events = ArrayDeque<RemoteEvent>()

    /** Highest cursor this window has consumed. */
    var cursor: Long = 0L
        private set

    val size: Int
        get() = events.size

    fun snapshot(): List<RemoteEvent> = events.toList()

    /**
     * Appends a page of frames.
     *
     * Frames at or below the consumed cursor are a replay (the Gateway may
     * re-send after a reconnect) and are ignored. Returns true when the window
     * actually changed, so an idle poll costs no recomposition.
     */
    fun append(batch: List<RemoteEvent>): Boolean {
        if (batch.isEmpty()) return false
        var changed = false
        for (event in batch) {
            if (event.cursor <= cursor) continue
            events.addLast(event)
            cursor = event.cursor
            changed = true
        }
        if (!changed) return false
        trimSuperseded()
        trim()
        return true
    }

    /** Seeds the window from the resume point, replacing whatever was held. */
    fun reset(initialCursor: Long, batch: List<RemoteEvent>) {
        events.clear()
        cursor = initialCursor
        append(batch)
    }

    fun clear() {
        events.clear()
        cursor = 0L
    }

    /**
     * Drops superseded frames before the newest non-empty `history/snapshot`
     * without waiting for the hard capacity ceiling, keeping state-carrying
     * frames (`ready`, `host_lost`, `models`, `receipt`, approvals/questions).
     */
    private fun trimSuperseded() {
        if (events.size <= 64) return
        val snapshot = indexOfLastHistorySnapshot()
        if (snapshot <= 64) return
        val latestReplaceableIndex = HashMap<String, Int>(4)
        for (index in 0 until snapshot) {
            val type = events[index].payloadType
            if (type in REPLACEABLE_PAYLOAD_TYPES) {
                latestReplaceableIndex[type] = index
            }
        }
        val retained = ArrayList<RemoteEvent>(events.size - snapshot + 12)
        for (index in 0 until snapshot) {
            val event = events[index]
            val type = event.payloadType
            val keepReplaceable = type in REPLACEABLE_PAYLOAD_TYPES && latestReplaceableIndex[type] == index
            val keepEvent = type == "event" && (event.eventKind in APPROVAL_KINDS || event.eventKind in TURN_RETENTION_KINDS)
            if (keepReplaceable || keepEvent) {
                retained.add(event)
            }
        }
        for (index in snapshot until events.size) {
            retained.add(events[index])
        }
        if (retained.size < events.size) {
            events.clear()
            events.addAll(retained)
        }
    }

    private fun trim() {
        if (events.size <= capacity) return
        val keep = BooleanArray(events.size)
        val snapshot = indexOfLastHistorySnapshot()
        // Pinned frames claim their slots first, newest first, and are capped at
        // the capacity so a pathological stream of requests still cannot grow
        // the window. The old rule pinned everything from the snapshot onwards,
        // so once a seeded read put the snapshot first it pinned the whole
        // window and the cap silently disappeared for the session's lifetime.
        var pinnedKept = 0
        for (index in events.indices.reversed()) {
            if (!pinned(events[index], index == snapshot)) continue
            if (pinnedKept >= capacity) continue
            keep[index] = true
            pinnedKept++
        }
        // Whatever is left keeps the newest ordinary frames, so the transcript
        // tail stays live and the total never exceeds the cap.
        var budget = capacity - pinnedKept
        for (index in events.indices.reversed()) {
            if (budget <= 0) break
            if (keep[index]) continue
            keep[index] = true
            budget--
        }
        var kept = 0
        for (flag in keep) if (flag) kept++
        if (kept == events.size) return
        val retained = ArrayList<RemoteEvent>(kept)
        for (index in events.indices) if (keep[index]) retained.add(events[index])
        events.clear()
        events.addAll(retained)
    }

    /**
     * True for a frame the screen cannot rebuild without.
     *
     * The newest history snapshot is the whole basis of the transcript before
     * it, and an outstanding approval or question is the only record that the
     * turn is blocked waiting for this person — either one dropping silently
     * loses the thing the screen exists to show.
     */
    private fun pinned(event: RemoteEvent, isNewestSnapshot: Boolean): Boolean {
        if (isNewestSnapshot) return true
        if (event.payload.optString("type") != "event") return false
        return event.eventKind in APPROVAL_KINDS
    }

    private fun indexOfLastHistorySnapshot(): Int {
        for (index in events.indices.reversed()) {
            val event = events[index]
            if (
                event.eventKind == "history/snapshot" &&
                event.paramsOf()?.optJSONArray("items")?.length()?.let { it > 0 } == true
            ) {
                return index
            }
        }
        return -1
    }

    private companion object {
        val APPROVAL_KINDS =
            setOf("approval_requested", "approval_resolved", "question_requested", "question_resolved")
        val TURN_RETENTION_KINDS = setOf("turn/started", "turn/completed", "error")
        val REPLACEABLE_PAYLOAD_TYPES = setOf("ready", "host_lost", "models", "receipt")
    }
}

/** `params` of an `event` frame; kept here so the window stays self-contained. */
internal fun RemoteEvent.paramsOf(): org.json.JSONObject? =
    event?.optJSONObject("raw")?.optJSONObject("params")

/**
 * Idempotency keys for commands.
 *
 * The Gateway persists `client_cmd_id` and replays the original receipt for a
 * repeat of the same key. Two rules follow:
 *
 *   * a retry of the *same* action in the *same* session reuses the key, and a
 *     different action gets a new one;
 *   * a refusal that means "this will never be accepted" clears the key, so a
 *     deliberate second attempt is not swallowed as a duplicate of a failed
 *     one.
 *
 * The key is scoped by runner and hub session because the ledger outlives one
 * screen: `{"action":"interrupt"}` is byte-identical in every session, and
 * reusing the key across sessions would make the Gateway replay another
 * session's receipt instead of running the command.
 */
class CommandLedger(private val maxEntries: Int = MAX_ENTRIES) {
    private val keys =
        object : LinkedHashMap<String, String>(16, 0.75f, true) {
            override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, String>?): Boolean =
                size > maxEntries
        }

    private fun key(scope: String, action: String, payload: String): String {
        val digest = MessageDigest.getInstance("SHA-256")
        digest.update(scope.toByteArray(StandardCharsets.UTF_8))
        digest.update(0)
        digest.update(action.toByteArray(StandardCharsets.UTF_8))
        digest.update(0)
        digest.update(payload.toByteArray(StandardCharsets.UTF_8))
        val bytes = digest.digest()
        val hex = StringBuilder(bytes.size * 2)
        for (b in bytes) {
            val v = b.toInt() and 0xff
            hex.append(HEX_CHARS[v ushr 4])
            hex.append(HEX_CHARS[v and 0x0f])
        }
        return hex.toString()
    }

    /** Key for this exact action in this session, minting one on first use. */
    @Synchronized
    fun keyFor(scope: String, action: String, payload: String): String =
        keys.getOrPut(key(scope, action, payload)) { UUID.randomUUID().toString() }

    /** The Gateway accepted (or rejected) it, so the key has served its purpose. */
    @Synchronized
    fun forget(scope: String, action: String, payload: String) {
        keys.remove(key(scope, action, payload))
    }

    /**
     * A permanent refusal: retrying the same key would replay the refusal.
     *
     * Only the codes the Gateway uses for a decision about *this command* count
     * as permanent. The Gateway answers 400 for every retryable state too —
     * `runner_offline`, `session_control_lost`, `host_generation_mismatch`,
     * `approval_not_pending`, `runner_backpressure` — and 429/408 come from
     * layers in front of it, so a blanket 4xx rule would mint a fresh key on a
     * retry and let the same action run twice.
     */
    fun forgetIfPermanent(scope: String, action: String, payload: String, status: Int) {
        if (status == 401 || status == 403 || status == 404) forget(scope, action, payload)
    }

    val size: Int
        @Synchronized get() = keys.size

    private companion object {
        const val MAX_ENTRIES = 512
        const val HEX_CHARS = "0123456789abcdef"
    }
}

/** `hub_session_id` prefix is fixed, so a fresh hub id needs no round trip. */
fun newHubSessionId(): String = UUID.randomUUID().toString()

/**
 * What the send button should do for a given session state.
 *
 * Kept as data rather than inline conditions so the composer, the hint text
 * and the tests all read the same rule: a prompt may only be sent when the
 * Host is online, the control channel is up and the Agent is not mid-turn.
 */
enum class ComposerMode {
    /** No usable control channel: the box explains why. */
    BLOCKED,

    /** Connected but mid-turn: stop first, or wait. */
    BUSY,

    /** Ready for a prompt. */
    READY,
    /** An explicit send can acquire an idle Codex session. */
    TAKE_CONTROL,
    ;

    companion object {
        fun of(hostOnline: Boolean, controlConnected: Boolean, state: ExecutionState): ComposerMode =
            when {
                !hostOnline || !controlConnected -> BLOCKED
                state.busy || state.waiting -> BUSY
                else -> READY
            }
    }
}

/**
 * The one reading of "what can this screen do right now".
 *
 * The composer, its hint, the stop button, the empty state and the approval
 * cards all used to translate `(online, controlConnected, execution_state)`
 * on their own, which is how they drifted: `awaiting_permission` was told to
 * "wait for the agent to finish" even though the agent is waiting on the
 * person, and the approval cards stayed enabled for a host the Gateway had
 * already reported offline.
 *
 * [connected] is the gate every write goes through. [composer] is only about
 * typing a prompt. [waitingOnPerson] names the third behaviour — the turn is
 * blocked on an approval or a question, which the rail above the transcript
 * owns, not the composer.
 */
data class SessionPhase(
    val connected: Boolean,
    val composer: ComposerMode,
    val waitingOnPerson: Boolean,
    /** A turn is running or blocked, so the stop button means something. */
    val stoppable: Boolean,
    val hint: String,
) {
    companion object {
        fun of(hostOnline: Boolean, controlConnected: Boolean, state: ExecutionState): SessionPhase {
            val connected = hostOnline && controlConnected
            val waiting =
                state == ExecutionState.AWAITING_PERMISSION || state == ExecutionState.AWAITING_INPUT
            val composer =
                when {
                    !connected -> ComposerMode.BLOCKED
                    state.busy || waiting -> ComposerMode.BUSY
                    else -> ComposerMode.READY
                }
            val hint =
                when {
                    !connected -> "电脑离线或控制已断开，点上方重新连接后可发送。"
                    waiting -> "处理上方请求，或先输入下一条加入待发队列…"
                    state.busy -> "Agent 执行中，输入指令点“排队”加入待发…"
                    else -> "输入指令，或点上方常用指令…"
                }
            return SessionPhase(
                connected = connected,
                composer = composer,
                waitingOnPerson = waiting,
                stoppable = connected && !state.settled,
                hint = hint,
            )
        }
    }
}

internal data class QueuedPromptDraft(
    val text: String,
    val attachments: List<RemoteAttachment> = emptyList(),
    val count: Int = 1,
)
