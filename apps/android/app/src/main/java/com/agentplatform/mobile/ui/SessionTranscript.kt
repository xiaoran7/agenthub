package com.agentplatform.mobile.ui

import org.json.JSONArray
import org.json.JSONObject

// =========================================================================
// Remote transcript model
//
// Mirrors the conversation builder in `apps/web/src/lib/remote.ts` so the
// phone and the browser console tell the same story from the same events:
//
//   * `history/snapshot` replaces a stale replay and seeds resumed sessions;
//   * streaming deltas and the final `agentMessage` collapse into one item;
//   * a tool call is one card across start -> complete, not two rows;
//   * bare tool JSON embedded in an agent message becomes a tool card;
//   * failures keep the provider's reason instead of a bare state label.
//
// The parser is pure Kotlin (no Android classes) so it runs in unit tests.
// =========================================================================

enum class TranscriptKind { USER, ASSISTANT, TOOL, SYSTEM }

enum class ToolState { RUNNING, COMPLETED, FAILED }

data class TranscriptItem(
    val id: String,
    val kind: TranscriptKind,
    val text: String = "",
    val title: String? = null,
    val command: String? = null,
    val output: String? = null,
    val description: String? = null,
    val state: ToolState? = null,
    val turnId: String? = null,
    /** True while this is the live tail of an assistant turn. */
    val streaming: Boolean = false,
    /** True when the item came from the bounded history snapshot. */
    val history: Boolean = false,
)

/** One failed turn, with the reason the Runner was allowed to forward. */
data class TurnFailure(val turnId: String?, val message: String, val retrying: Boolean)

private fun JSONArray.objects(): List<JSONObject> =
    (0 until length()).mapNotNull { optJSONObject(it) }

/**
 * A tool-call JSON object the provider leaked into the message stream. Codex
 * used to emit these raw; a card is what the user should see, not `{...}`.
 */
private fun bareToolCall(text: String): JSONObject? {
    val trimmed = text.trim()
    if (!trimmed.startsWith("{\"command\":") || !trimmed.contains("\"description\"")) return null
    return try {
        JSONObject(trimmed).takeIf { it.opt("command") is String }
    } catch (_: Exception) {
        null
    }
}

/** `server · tool` so a tool row says where it ran as well as what it is. */
private fun toolLabel(server: String?, name: String?): String =
    listOfNotNull(server?.takeIf { it.isNotBlank() }, name?.takeIf { it.isNotBlank() })
        .joinToString(" · ")
        .ifBlank { "工具调用" }

private fun itemState(item: JSONObject, completed: Boolean): ToolState =
    when {
        item.optString("status") == "failed" -> ToolState.FAILED
        completed -> ToolState.COMPLETED
        else -> ToolState.RUNNING
    }

private fun textOf(item: JSONObject): String =
    item.optString("text").ifBlank {
        item.optJSONArray("content")
            ?.objects()
            ?.filter { it.optString("type") == "text" }
            ?.joinToString("\n") { it.optString("text") }
            .orEmpty()
    }

private fun fileName(path: String): String =
    path.substringAfterLast('/').substringAfterLast('\\').ifBlank { path }

/**
 * Builds the transcript for one session.
 *
 * `commands` is the session command log: a `send_prompt` receipt names the turn
 * it opened, which is how a prompt typed on another device still appears above
 * the answer it produced. `live` is the session's current execution state; it
 * lets a resumed mid-turn session show the streaming caret before any
 * `turn/started` event has been polled.
 */
fun buildTranscript(
    events: List<JSONObject>,
    commands: List<JSONObject>,
    hubSessionId: String,
    live: Boolean = false,
): List<TranscriptItem> {
    val prompts = HashMap<String, String>()
    for (record in commands) {
        val envelope = record.optJSONObject("command") ?: continue
        if (envelope.optString("hub_session_id").ifBlank { hubSessionId } != hubSessionId) continue
        val body = envelope.optJSONObject("command") ?: continue
        if (body.optString("action") != "send_prompt") continue
        val receipt = record.optJSONObject("receipt") ?: continue
        if (receipt.optString("status") != "accepted") continue
        val turn = receipt.optJSONObject("result")?.optJSONObject("turn")?.optString("id").orEmpty()
        val text = body.optString("text")
        if (turn.isNotEmpty() && text.isNotBlank()) prompts[turn] = text
    }

    val items = mutableListOf<TranscriptItem>()
    val positions = HashMap<String, Int>()

    /** Inserts or replaces by id, always leaving the row at its original slot. */
    fun put(item: TranscriptItem) {
        val position = positions[item.id]
        if (position == null || position >= items.size) {
            positions[item.id] = items.size
            items.add(item)
        } else {
            items[position] = item
        }
    }

    fun byId(id: String): TranscriptItem? = positions[id]?.let(items::get)

    var liveTurnId: String? = null

    // A managed resume replays a bounded snapshot. Everything before the last
    // one is stale replay and is dropped; events after it are live.
    val lastHistory =
        events.indexOfLast { envelope ->
            val event =
                envelope.optJSONObject("payload")
                    ?.takeIf { it.optString("type") == "event" }
                    ?.optJSONObject("event") ?: return@indexOfLast false
            event.optString("kind") == "history/snapshot" &&
                event.optJSONObject("raw")?.optJSONObject("params")
                    ?.optJSONArray("items")?.let { it.length() > 0 } == true
        }

    if (lastHistory >= 0) {
        val envelope = events[lastHistory]
        val history =
            envelope.optJSONObject("payload")
                ?.optJSONObject("event")
                ?.optJSONObject("raw")
                ?.optJSONObject("params")
                ?.optJSONArray("items")?.objects().orEmpty()
        val cursor = envelope.optLong("cursor")
        history.forEachIndexed { index, entry ->
            val kind = entry.optString("kind")
            if (kind !in setOf("message", "tool", "status")) return@forEachIndexed
            val text = entry.optString("text")
            if (text.isBlank() && kind != "tool") return@forEachIndexed
            val id = "history-$cursor-$index"
            val state =
                when (entry.optString("status")) {
                    "running", "pending" -> ToolState.RUNNING
                    "failed" -> ToolState.FAILED
                    else -> ToolState.COMPLETED
                }
            val item =
                when (kind) {
                    "tool" ->
                        TranscriptItem(
                            id = id,
                            kind = TranscriptKind.TOOL,
                            text = text,
                            title = entry.optString("label").ifBlank { "工具调用" },
                            output = text.ifBlank { null },
                            state = state,
                            history = true,
                        )
                    "status" ->
                        TranscriptItem(
                            id = id,
                            kind = TranscriptKind.SYSTEM,
                            text = text,
                            title = entry.optString("label").ifBlank { null },
                            history = true,
                        )
                    else ->
                        TranscriptItem(
                            id = id,
                            kind = if (entry.optString("role") == "user") TranscriptKind.USER else TranscriptKind.ASSISTANT,
                            text = text,
                            history = true,
                        )
                }
            put(item)
        }
    }

    for (index in (lastHistory + 1).coerceAtLeast(0) until events.size) {
        val envelope = events[index]
        val payload = envelope.optJSONObject("payload") ?: continue
        if (payload.optString("type") != "event") continue
        val event = payload.optJSONObject("event") ?: continue
        val kind = event.optString("kind")
        val params = event.optJSONObject("raw")?.optJSONObject("params") ?: continue
        val item = params.optJSONObject("item")
        val eventTurnId = params.optString("turnId").takeIf { it.isNotBlank() }
        val eventId = params.optString("itemId").ifBlank { item?.optString("id").orEmpty() }

        when (kind) {
            "turn/started" -> {
                val turn = params.optJSONObject("turn")?.optString("id").orEmpty()
                liveTurnId = turn
                prompts[turn]?.let { prompt ->
                    val id = "user:$turn"
                    if (positions[id] == null) {
                        put(TranscriptItem(id = id, kind = TranscriptKind.USER, text = prompt))
                    }
                }
            }
            "turn/completed" -> {
                liveTurnId = null
                // A turn only ends after its tools settled. Claude emits only a
                // generic activity marker, never a completion item, so rows
                // still marked running would otherwise stay amber forever.
                for (i in items.indices) {
                    val row = items[i]
                    if (row.kind == TranscriptKind.TOOL && row.state == ToolState.RUNNING) {
                        items[i] = row.copy(state = ToolState.COMPLETED, text = "工具执行已完成")
                    }
                }
            }
            "item/agentMessage/delta" -> {
                if (eventId.isEmpty()) continue
                val delta = params.optString("delta")
                if (delta.isEmpty()) continue
                val existing = byId(eventId)
                if (existing == null) {
                    val call = bareToolCall(delta)
                    if (call != null) {
                        put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.TOOL,
                                title = "命令执行",
                                command = call.optString("command"),
                                description = call.optString("description"),
                                state = ToolState.RUNNING,
                                turnId = eventTurnId,
                                text = "工具执行中…",
                            ),
                        )
                        continue
                    }
                }
                if (existing != null && existing.kind != TranscriptKind.ASSISTANT) continue
                put(
                    if (existing == null) {
                        TranscriptItem(id = eventId, kind = TranscriptKind.ASSISTANT, text = delta)
                    } else {
                        existing.copy(text = existing.text + delta, history = false)
                    },
                )
            }
            "item/started", "item/completed" -> {
                if (eventId.isEmpty() || item == null) continue
                val completed = kind == "item/completed"
                when (item.optString("type")) {
                    "userMessage" -> {
                        if (positions[eventId] != null) continue
                        val text = textOf(item)
                        if (text.isBlank()) continue
                        put(TranscriptItem(id = eventId, kind = TranscriptKind.USER, text = text))
                    }
                    "agentMessage" -> {
                        val existing = byId(eventId)
                        if (existing != null && existing.kind != TranscriptKind.ASSISTANT) continue
                        val text = item.optString("text")
                        val call = bareToolCall(text)
                        if (call != null) {
                            put(
                                TranscriptItem(
                                    id = eventId,
                                    kind = TranscriptKind.TOOL,
                                    title = "命令执行",
                                    command = call.optString("command"),
                                    description = call.optString("description"),
                                    state = if (completed) ToolState.COMPLETED else ToolState.RUNNING,
                                    turnId = eventTurnId,
                                    text = if (completed) "工具执行已完成" else "工具执行中…",
                                ),
                            )
                            continue
                        }
                        if (text.isBlank() && existing == null) continue
                        put(
                            if (existing == null) {
                                TranscriptItem(id = eventId, kind = TranscriptKind.ASSISTANT, text = text)
                            } else {
                                existing.copy(
                                    text = text.ifBlank { existing.text },
                                    streaming = !completed,
                                    history = false,
                                )
                            },
                        )
                    }
                    "reasoning", "thinking" -> {
                        val failure = item.optString("failureReason")
                        val text =
                            failure.ifBlank {
                                item.optString("text").ifBlank { reasoningText(item) }
                            }
                        if (text.isBlank()) continue
                        put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.ASSISTANT,
                                // `failureReason` is the one reasoning-shaped
                                // item whose text a user must see, so it is
                                // not folded into the accordion.
                                text = if (failure.isNotBlank()) text else "<think>$text</think>",
                                streaming = !completed,
                                history = false,
                            ),
                        )
                    }
                    "commandExecution" -> {
                        val command =
                            item.optString("command").ifBlank { item.optString("commandLine") }
                        val output =
                            item.optString("aggregatedOutput")
                                .ifBlank { item.optString("output") }
                                .ifBlank { item.optString("stdout") }
                        val state = itemState(item, completed)
                        val existing = byId(eventId)
                        if (existing != null) {
                            put(
                                existing.copy(
                                    command = command.ifBlank { existing.command },
                                    output = output.ifBlank { existing.output },
                                    state = state,
                                    text = if (completed) "工具执行已完成" else existing.text,
                                ),
                            )
                            continue
                        }
                        // A running row captured from a bare JSON delta is the
                        // same operation — merge instead of duplicating it.
                        val previous =
                            items.lastOrNull {
                                it.kind == TranscriptKind.TOOL &&
                                    it.state == ToolState.RUNNING &&
                                    it.command != null &&
                                    (command.isBlank() || it.command == command)
                            }
                        if (previous != null) {
                            val before = items.indexOf(previous)
                            positions.remove(previous.id)
                            positions[eventId] = before
                            items[before] =
                                previous.copy(
                                    id = eventId,
                                    command = command.ifBlank { previous.command },
                                    output = output.ifBlank { previous.output },
                                    state = state,
                                    text = if (completed) "工具执行已完成" else previous.text,
                                )
                        } else {
                            put(
                                TranscriptItem(
                                    id = eventId,
                                    kind = TranscriptKind.TOOL,
                                    title = "命令执行",
                                    command = command.ifBlank { null },
                                    output = output.ifBlank { null },
                                    state = state,
                                    turnId = eventTurnId,
                                    text = if (completed) "工具执行已完成" else "工具执行中…",
                                ),
                            )
                        }
                    }
                    "toolCall", "functionCall", "mcpToolCall", "tool" -> {
                        put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.TOOL,
                                title =
                                    toolLabel(
                                        item.optString("server").ifBlank { null },
                                        item.optString("tool").ifBlank { item.optString("name") }.ifBlank { null },
                                    ),
                                command =
                                    item.optString("input").ifBlank {
                                        item.optString("arguments").ifBlank { null }
                                    },
                                output = item.optString("output").ifBlank { null },
                                state = itemState(item, completed),
                                turnId = eventTurnId,
                            ),
                        )
                    }
                    "fileChange", "fileChangeStart" -> {
                        val path = item.optString("path")
                        val label =
                            if (path.isNotBlank()) "文件修改 · " + fileName(path)
                            else toolLabel(null, item.optString("name").ifBlank { null })
                        put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.TOOL,
                                title = label
                                    .ifBlank { "文件修改" },
                                command =
                                    item.optString("diff")
                                        .ifBlank { item.optString("patch") }
                                        .ifBlank { null },
                                state = itemState(item, completed),
                                turnId = eventTurnId,
                            ),
                        )
                    }
                }
            }
        }
    }

    // Only the tail of a live turn is still arriving; a finished turn is not.
    val tail = items.lastOrNull()
    val turnActive = live || liveTurnId != null
    if (turnActive && tail != null && tail.kind == TranscriptKind.ASSISTANT && !tail.history) {
        items[items.lastIndex] = tail.copy(streaming = true)
    }
    return items
}

internal fun reasoningText(item: JSONObject): String {
    val summary = item.optJSONArray("summary")
    if (summary != null && summary.length() > 0) {
        return (0 until summary.length())
            .mapNotNull { summary.optJSONObject(it) }
            .joinToString("\n") { it.optString("text") }
            .trim()
    }
    return item.optString("text").trim()
}

/**
 * The most recent failure that a turn did not supersede. A retrying error is
 * reported as such, never as the final outcome of the turn.
 */
fun latestTurnFailure(events: List<JSONObject>, hubSessionId: String): TurnFailure? {
    var failure: TurnFailure? = null
    for (envelope in events) {
        val payload = envelope.optJSONObject("payload") ?: continue
        if (payload.optString("type") != "event") continue
        val event = payload.optJSONObject("event") ?: continue
        val bindingHub =
            event.optJSONObject("binding")?.optString("hub_session_id")?.takeIf { it.isNotEmpty() }
        if (bindingHub != null && bindingHub != hubSessionId) continue
        when (event.optString("kind")) {
            "turn/started", "turn/completed" -> failure = null
            "error" -> {
                val params = event.optJSONObject("raw")?.optJSONObject("params") ?: continue
                val error = params.optJSONObject("error") ?: continue
                val message = error.optString("message").trim()
                if (message.isEmpty()) continue
                failure =
                    TurnFailure(
                        turnId = params.optString("turnId").takeIf { it.isNotEmpty() },
                        message = message,
                        retrying = params.optBoolean("willRetry", false),
                    )
            }
        }
    }
    return failure
}
