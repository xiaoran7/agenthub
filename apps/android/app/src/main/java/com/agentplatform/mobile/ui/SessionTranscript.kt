package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.RemoteCommandRecord
import com.agentplatform.mobile.data.RemoteEvent
import com.agentplatform.mobile.data.objects
import com.agentplatform.mobile.data.optNullableString
import com.agentplatform.mobile.data.optStringOrEmpty
import com.agentplatform.mobile.data.paramsOf
import org.json.JSONObject

// =========================================================================
// Remote transcript model
//
// Mirrors the conversation builder in `apps/web/src/lib/remote.ts` so the
// phone and the browser console tell the same story from the same events,
// including Codex `userMessage` deduplication against `turn/started` prompts.
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
    /** The row key survives reconciliation from a provisional tool ID to the provider's ID. */
    internal val displayKey: String = "item:$id",
)

/** One failed turn, with the reason the Runner was allowed to forward. */
data class TurnFailure(val turnId: String?, val message: String, val retrying: Boolean)

/** Context window fill, as reported by the provider after a turn. */
data class ContextUsage(val used: Long, val window: Long?)

internal fun bareToolCall(text: String): JSONObject? {
    val trimmed = text.trim()
    if (!trimmed.startsWith("{\"command\":") || !trimmed.contains("\"description\"")) return null
    return try {
        JSONObject(trimmed).takeIf { it.opt("command") is String }
    } catch (_: Exception) {
        null
    }
}

internal fun toolLabel(server: String?, name: String?): String =
    listOfNotNull(server?.takeIf { it.isNotBlank() }, name?.takeIf { it.isNotBlank() })
        .joinToString(" · ")
        .ifBlank { "工具调用" }

internal fun itemState(item: JSONObject, completed: Boolean): ToolState =
    when {
        item.optStringOrEmpty("status") == "failed" -> ToolState.FAILED
        completed -> ToolState.COMPLETED
        else -> ToolState.RUNNING
    }

internal fun textOf(item: JSONObject): String =
    item.optStringOrEmpty("text").ifBlank {
        item.optJSONArray("content")
            ?.objects()
            ?.filter { it.optStringOrEmpty("type") == "text" }
            ?.joinToString("\n") { it.optStringOrEmpty("text") }
            .orEmpty()
    }

internal fun extractEventTurnId(params: JSONObject): String? =
    params.optNullableString("turnId")
        ?: params.optJSONObject("turn")?.optNullableString("id")
        ?: params.optNullableString("turn_id")

internal fun formatCommandPromptText(commandObj: JSONObject?): String {
    if (commandObj == null) return ""
    val base = commandObj.optNullableString("text").orEmpty()
    val attLabels =
        commandObj
            .optJSONArray("attachments")
            ?.objects()
            ?.mapNotNull { it.optNullableString("name")?.trim()?.takeIf { name -> name.isNotEmpty() } }
            ?.map { "[附件: $it]" }
            .orEmpty()
    if (attLabels.isEmpty()) return base
    val suffix = attLabels.joinToString(" ")
    return if (base.isNotBlank()) "$base\n$suffix" else suffix
}

internal fun fileName(path: String): String =
    path.substringAfterLast('/').substringAfterLast('\\').ifBlank { path }

internal fun isRedactedPathOnly(value: String): Boolean =
    value.trim() == "[local_path_redacted]"

internal sealed interface ConversationBlock {
    val displayKey: String

    data class Single(val item: TranscriptItem) : ConversationBlock {
        override val displayKey: String get() = item.displayKey
    }

    data class Tools(val items: List<TranscriptItem>) : ConversationBlock {
        override val displayKey: String get() = items.first().displayKey
    }
}

internal fun groupTranscript(items: List<TranscriptItem>): List<ConversationBlock> {
    val blocks = ArrayList<ConversationBlock>(items.size)
    val tools = mutableListOf<TranscriptItem>()
    fun flush() {
        if (tools.size == 1) blocks.add(ConversationBlock.Single(tools.first()))
        if (tools.size > 1) blocks.add(ConversationBlock.Tools(tools.toList()))
        tools.clear()
    }
    for (item in items) {
        if (item.kind != TranscriptKind.TOOL && isRedactedPathOnly(item.text)) {
            continue
        }
        if (item.kind == TranscriptKind.TOOL) {
            tools.add(item)
        } else {
            flush()
            blocks.add(ConversationBlock.Single(item))
        }
    }
    flush()
    return blocks
}

internal class TranscriptFoldState(
    val prompts: LinkedHashMap<String, String>,
) {
    val items = mutableListOf<TranscriptItem>()
    val positions = HashMap<String, Int>()
    val runningToolIds = LinkedHashSet<String>()
    val runningToolByCmd = HashMap<String, String>()
    private val toolIdOccurrences = HashMap<String, Int>()
    val seenTurnIds = HashSet<String>()
    val promptedTurnIds = HashSet<String>()
    val trailingPromptIds = ArrayList<String>()
    var liveTurnId: String? = null

    fun put(item: TranscriptItem) {
        val position = positions[item.id]
        if (position == null || position >= items.size) {
            positions[item.id] = items.size
            items.add(if (item.kind == TranscriptKind.TOOL) withNewToolDisplayKey(item) else item)
        } else {
            val previous = items[position]
            if (previous.kind == TranscriptKind.TOOL && previous.state == ToolState.RUNNING) {
                runningToolIds.remove(previous.id)
                previous.command?.let { cmd ->
                    if (runningToolByCmd[cmd] == previous.id) runningToolByCmd.remove(cmd)
                }
            }
            items[position] =
                when {
                    item.kind != TranscriptKind.TOOL -> item
                    previous.kind != TranscriptKind.TOOL -> withNewToolDisplayKey(item)
                    item.displayKey == previous.displayKey -> item
                    else -> item.copy(displayKey = previous.displayKey)
                }
        }
        if (item.kind == TranscriptKind.TOOL) {
            if (item.state == ToolState.RUNNING) {
                runningToolIds.add(item.id)
                if (!item.command.isNullOrBlank()) {
                    runningToolByCmd[item.command] = item.id
                }
            } else {
                runningToolIds.remove(item.id)
                item.command?.let { cmd ->
                    if (runningToolByCmd[cmd] == item.id) runningToolByCmd.remove(cmd)
                }
            }
        }
    }

    private fun withNewToolDisplayKey(item: TranscriptItem): TranscriptItem {
        // A provisional ID can reappear after reconciliation frees it in the event index.
        val occurrence = (toolIdOccurrences[item.id] ?: 0) + 1
        toolIdOccurrences[item.id] = occurrence
        return item.copy(displayKey = "tool:$occurrence:${item.id}")
    }

    fun stripTrailingPrompts() {
        if (trailingPromptIds.isEmpty()) return
        val toRemove = trailingPromptIds.toHashSet()
        trailingPromptIds.clear()
        val retained = items.filterNot { it.id in toRemove }
        if (retained.size == items.size) return
        items.clear()
        positions.clear()
        retained.forEachIndexed { idx, row ->
            positions[row.id] = idx
            items.add(row)
        }
    }

    fun byId(id: String): TranscriptItem? = positions[id]?.let(items::get)

    fun closeOpenThinking() {
        for (idx in items.indices) {
            val row = items[idx]
            if (row.kind != TranscriptKind.ASSISTANT) continue
            val hasOpenThink =
                row.text.contains("<think>", ignoreCase = true) &&
                    !row.text.contains("</think>", ignoreCase = true)
            val hasOpenThinking =
                row.text.contains("<thinking>", ignoreCase = true) &&
                    !row.text.contains("</thinking>", ignoreCase = true)
            if (hasOpenThink) {
                items[idx] = row.copy(text = "${row.text}</think>", streaming = false)
            } else if (hasOpenThinking) {
                items[idx] = row.copy(text = "${row.text}</thinking>", streaming = false)
            }
        }
    }

    fun completeRunningTools() {
        if (runningToolIds.isEmpty()) return
        for (toolId in runningToolIds) {
            val pos = positions[toolId] ?: continue
            val row = items[pos]
            if (row.kind == TranscriptKind.TOOL && row.state == ToolState.RUNNING) {
                items[pos] = row.copy(state = ToolState.COMPLETED, text = "工具执行已完成")
            }
        }
        runningToolIds.clear()
        runningToolByCmd.clear()
    }

    fun mergeRunningCommand(
        eventId: String,
        command: String,
        output: String,
        state: ToolState,
        completed: Boolean,
    ): Boolean {
        val matchedId =
            if (command.isNotBlank()) {
                runningToolByCmd[command]
            } else {
                runningToolIds.lastOrNull { id ->
                    positions[id]?.let { items[it].command != null } == true
                }
            }
        val pos = matchedId?.let(positions::get) ?: return false
        val previous = items[pos]
        if (previous.kind != TranscriptKind.TOOL || previous.state != ToolState.RUNNING || previous.command == null) {
            return false
        }
        runningToolIds.remove(previous.id)
        if (runningToolByCmd[previous.command] == previous.id) {
            runningToolByCmd.remove(previous.command)
        }
        positions.remove(previous.id)
        positions[eventId] = pos
        val updated =
            previous.copy(
                id = eventId,
                command = command.ifBlank { previous.command },
                output = output.ifBlank { previous.output },
                state = state,
                text = if (completed) "工具执行已完成" else previous.text,
            )
        items[pos] = updated
        if (updated.state == ToolState.RUNNING) {
            runningToolIds.add(eventId)
            if (!updated.command.isNullOrBlank()) {
                runningToolByCmd[updated.command] = eventId
            }
        }
        return true
    }
}

private fun collectPrompts(
    commands: List<RemoteCommandRecord>,
    hubSessionId: String,
): LinkedHashMap<String, String> {
    val prompts = LinkedHashMap<String, String>()
    for (record in commands) {
        if (record.action != "send_prompt") continue
        if (record.hubSessionId.isNotBlank() && record.hubSessionId != hubSessionId) continue
        val receipt = record.receipt ?: continue
        if (receipt.optStringOrEmpty("status") != "accepted") continue
        val turn = receipt.optJSONObject("result")?.optJSONObject("turn")?.optStringOrEmpty("id").orEmpty()
        val text = formatCommandPromptText(record.command.optJSONObject("command"))
        if (turn.isNotEmpty() && text.isNotBlank()) prompts[turn] = text
    }
    return prompts
}

private fun buildTranscriptState(
    events: List<RemoteEvent>,
    commands: List<RemoteCommandRecord>,
    hubSessionId: String,
): TranscriptFoldState {
    val state = TranscriptFoldState(collectPrompts(commands, hubSessionId))

    val lastHistory =
        events.indexOfLast { envelope ->
            envelope.eventKind == "history/snapshot" &&
                envelope.paramsOf()?.optJSONArray("items")?.let { it.length() > 0 } == true
        }

    if (lastHistory >= 0) {
        val envelope = events[lastHistory]
        val history = envelope.paramsOf()?.optJSONArray("items")?.objects().orEmpty()
        val cursor = envelope.cursor
        val pendingThoughts = mutableListOf<String>()
        var pendingThoughtId = ""

        fun flushThoughts() {
            if (pendingThoughts.isEmpty()) return
            state.put(
                TranscriptItem(
                    id = pendingThoughtId,
                    kind = TranscriptKind.ASSISTANT,
                    text = "<think>${pendingThoughts.joinToString("\n\n")}</think>",
                    history = true,
                ),
            )
            pendingThoughts.clear()
            pendingThoughtId = ""
        }

        history.forEachIndexed { index, entry ->
            val kind = entry.optNullableString("kind").orEmpty()
            val text = entry.optNullableString("text") ?: entry.optNullableString("content").orEmpty()
            val id = "history-$cursor-$index"

            if (kind == "thinking") {
                val trimmed = text.trim()
                if (trimmed.isNotEmpty()) {
                    if (pendingThoughts.isEmpty()) pendingThoughtId = id
                    pendingThoughts += trimmed
                }
                return@forEachIndexed
            }

            if (kind !in setOf("message", "tool", "status")) return@forEachIndexed
            if (text.isBlank() && kind != "tool") return@forEachIndexed

            if (
                kind == "message" &&
                entry.optStringOrEmpty("role") != "user" &&
                pendingThoughts.isNotEmpty() &&
                !text.trimStart().startsWith("<think", ignoreCase = true)
            ) {
                val merged = "<think>${pendingThoughts.joinToString("\n\n")}</think>\n\n$text"
                pendingThoughts.clear()
                pendingThoughtId = ""
                state.put(
                    TranscriptItem(
                        id = id,
                        kind = TranscriptKind.ASSISTANT,
                        text = merged,
                        history = true,
                    ),
                )
                return@forEachIndexed
            }

            flushThoughts()

            val toolState =
                when (entry.optStringOrEmpty("status")) {
                    "running", "pending" -> ToolState.RUNNING
                    "failed" -> ToolState.FAILED
                    else -> ToolState.COMPLETED
                }
            val item =
                when (kind) {
                    "tool" -> {
                        val (cmd, out) = splitHistoricalToolValue(text)
                        TranscriptItem(
                            id = id,
                            kind = TranscriptKind.TOOL,
                            text = text,
                            title = entry.optNullableString("label") ?: "工具调用",
                            command = cmd,
                            output = out,
                            state = toolState,
                            history = true,
                        )
                    }
                    "status" ->
                        TranscriptItem(
                            id = id,
                            kind = TranscriptKind.SYSTEM,
                            text = text,
                            title = entry.optNullableString("label"),
                            history = true,
                        )
                    else ->
                        TranscriptItem(
                            id = id,
                            kind =
                                if (entry.optStringOrEmpty("role") == "user") TranscriptKind.USER
                                else TranscriptKind.ASSISTANT,
                            text = text,
                            history = true,
                        )
                }
            state.put(item)
        }
        flushThoughts()
    }

    foldEventsRange(state, events, (lastHistory + 1).coerceAtLeast(0))
    appendUnansweredPrompts(state, commands, hubSessionId)
    return state
}

internal fun optimisticPromptLanded(neededUserRows: Int, userRowCount: Int): Boolean =
    userRowCount >= neededUserRows

internal fun nextOptimisticPromptNeeded(pending: List<Int>, userRowCount: Int): Int {
    val head = userRowCount + 1
    val behindQueue = (pending.lastOrNull() ?: 0) + 1
    return maxOf(head, behindQueue)
}

private fun appendUnansweredPrompts(
    fold: TranscriptFoldState,
    commands: List<RemoteCommandRecord>,
    hubSessionId: String,
) {
    for (record in commands) {
        if (record.action != "send_prompt") continue
        if (record.hubSessionId.isNotBlank() && record.hubSessionId != hubSessionId) continue
        val receipt = record.receipt
        val acceptedTurnId =
            if (receipt?.optStringOrEmpty("status") == "accepted") {
                receipt.optJSONObject("result")?.optJSONObject("turn")?.optStringOrEmpty("id").orEmpty()
            } else {
                ""
            }
        if (receipt != null && (acceptedTurnId.isEmpty() || acceptedTurnId in fold.seenTurnIds)) {
            continue
        }
        val clientId = record.command.optStringOrEmpty("client_cmd_id")
        val id =
            if (acceptedTurnId.isNotEmpty()) "user:$acceptedTurnId"
            else if (clientId.isNotBlank()) "pending:$clientId"
            else continue
        if (fold.positions[id] != null) continue
        val text = formatCommandPromptText(record.command.optJSONObject("command"))
        if (text.isBlank()) continue
        fold.put(TranscriptItem(id = id, kind = TranscriptKind.USER, text = text, streaming = receipt == null))
        fold.trailingPromptIds.add(id)
    }
}

private fun applyTailStreaming(items: List<TranscriptItem>, turnActive: Boolean): List<TranscriptItem> {
    val tail = items.lastOrNull() ?: return items
    if (turnActive && tail.kind == TranscriptKind.ASSISTANT && !tail.history && !tail.streaming) {
        val copy = items.toMutableList()
        copy[copy.lastIndex] = tail.copy(streaming = true)
        return copy
    }
    return items
}

fun buildTranscript(
    events: List<RemoteEvent>,
    commands: List<RemoteCommandRecord>,
    hubSessionId: String,
    live: Boolean = false,
): List<TranscriptItem> {
    val state = buildTranscriptState(events, commands, hubSessionId)
    val turnActive = live || state.liveTurnId != null
    return applyTailStreaming(state.items, turnActive)
}

class IncrementalTranscriptFold {
    private var lastEvents: List<RemoteEvent> = emptyList()
    private var lastCommands: List<RemoteCommandRecord> = emptyList()
    private var lastHubSessionId: String = ""
    private var lastLive: Boolean = false
    private var fold: TranscriptFoldState? = null
    private var cachedItems: List<TranscriptItem> = emptyList()

    @Synchronized
    fun update(
        events: List<RemoteEvent>,
        commands: List<RemoteCommandRecord>,
        hubSessionId: String,
        live: Boolean,
    ): List<TranscriptItem> {
        val prevFold = fold
        if (
            prevFold != null &&
            lastHubSessionId == hubSessionId &&
            lastCommands == commands &&
            lastEvents.isNotEmpty() &&
            events.size >= lastEvents.size &&
            events.size - lastEvents.size <= INCREMENTAL_EVENT_LIMIT &&
            isPrefixByIdentity(lastEvents, events) &&
            !hasNonEmptySnapshotInRange(events, lastEvents.size)
        ) {
            if (events.size == lastEvents.size) {
                if (live == lastLive) return cachedItems
                val result = applyTailStreaming(prevFold.items.toList(), live || prevFold.liveTurnId != null)
                lastLive = live
                cachedItems = result
                return result
            }
            prevFold.stripTrailingPrompts()
            foldEventsRange(prevFold, events, lastEvents.size)
            appendUnansweredPrompts(prevFold, commands, hubSessionId)
            val result = applyTailStreaming(prevFold.items.toList(), live || prevFold.liveTurnId != null)
            lastEvents = events
            lastCommands = commands
            lastLive = live
            cachedItems = result
            return result
        }

        val fresh = buildTranscriptState(events, commands, hubSessionId)
        val result = applyTailStreaming(fresh.items.toList(), live || fresh.liveTurnId != null)
        fold = fresh
        lastEvents = events
        lastCommands = commands
        lastHubSessionId = hubSessionId
        lastLive = live
        cachedItems = result
        return result
    }

    @Synchronized
    fun reset() {
        lastEvents = emptyList()
        lastCommands = emptyList()
        lastHubSessionId = ""
        lastLive = false
        fold = null
        cachedItems = emptyList()
    }

    private companion object {
        const val INCREMENTAL_EVENT_LIMIT = 512

        fun isPrefixByIdentity(prefix: List<RemoteEvent>, all: List<RemoteEvent>): Boolean {
            for (i in prefix.indices) {
                if (prefix[i] !== all[i]) return false
            }
            return true
        }

        fun hasNonEmptySnapshotInRange(events: List<RemoteEvent>, startIndex: Int): Boolean {
            for (i in startIndex until events.size) {
                val env = events[i]
                if (
                    env.eventKind == "history/snapshot" &&
                    env.paramsOf()?.optJSONArray("items")?.let { it.length() > 0 } == true
                ) {
                    return true
                }
            }
            return false
        }
    }
}
