package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.RemoteEvent
import com.agentplatform.mobile.data.objects
import com.agentplatform.mobile.data.optNullableString
import com.agentplatform.mobile.data.optStringOrEmpty
import com.agentplatform.mobile.data.paramsOf
import org.json.JSONObject

internal const val TOOL_RESULT_MARKER = "\n\n〔结果〕\n"

internal fun looksLikeStructuredToolInput(value: String): Boolean {
    val trimmed = value.trim()
    if (trimmed.startsWith("--- ") || trimmed.startsWith("@@") || trimmed.contains("\n@@ ")) {
        return true
    }
    return trimmed.startsWith("{") &&
        trimmed.endsWith("}") &&
        (
            trimmed.contains("\"old_string\"") ||
                trimmed.contains("\"new_string\"") ||
                trimmed.contains("\"questions\"")
            )
}

internal fun splitHistoricalToolValue(value: String): Pair<String?, String?> {
    val idx = value.indexOf(TOOL_RESULT_MARKER)
    if (idx >= 0) {
        val head = value.substring(0, idx).trim().ifBlank { null }
        val tail = value.substring(idx + TOOL_RESULT_MARKER.length).ifBlank { null }
        return head to tail
    }
    if (looksLikeStructuredToolInput(value)) {
        return value to null
    }
    return null to value.ifBlank { null }
}

internal fun extractFileChangeDetails(item: JSONObject): Pair<String, String?> {
    val directPath = item.optStringOrEmpty("path").trim()
    val changes = item.optJSONArray("changes")?.objects().orEmpty()
    val firstChangePath = changes.firstOrNull()?.optStringOrEmpty("path")?.trim().orEmpty()
    val primaryPath = directPath.ifBlank { firstChangePath }
    val label =
        if (primaryPath.isNotBlank()) "文件修改 · ${fileName(primaryPath)}"
        else toolLabel(null, item.optStringOrEmpty("name").ifBlank { null }).ifBlank { "文件修改" }

    val directDiff = item.optStringOrEmpty("diff").ifBlank { item.optStringOrEmpty("patch") }
    if (directDiff.isNotBlank()) return label to directDiff

    if (changes.isNotEmpty()) {
        val chunks = mutableListOf<String>()
        for (change in changes) {
            val rawDiff = change.optStringOrEmpty("diff").ifBlank { change.optStringOrEmpty("patch") }
            if (rawDiff.isBlank()) continue
            val changePath = change.optStringOrEmpty("path").trim().ifBlank { primaryPath }
            if (rawDiff.startsWith("--- ") || changePath.isBlank()) {
                chunks += rawDiff
            } else if (rawDiff.startsWith("@@")) {
                chunks += "--- a/$changePath\n+++ b/$changePath\n$rawDiff"
            } else {
                chunks += rawDiff
            }
        }
        if (chunks.isNotEmpty()) return label to chunks.joinToString("\n")
    }

    val fallbackInput = item.optStringOrEmpty("input").ifBlank { null }
    return label.ifBlank { "文件修改" } to fallbackInput
}

internal fun reasoningText(item: JSONObject): String {
    val direct = item.optStringOrEmpty("text").trim()
    if (direct.isNotEmpty()) return direct
    val summary = item.optJSONArray("summary")
    if (summary != null && summary.length() > 0) {
        val joined =
            (0 until summary.length())
                .mapNotNull { summary.optJSONObject(it)?.optStringOrEmpty("text")?.trim() }
                .filter { it.isNotEmpty() }
                .joinToString("\n")
        if (joined.isNotEmpty()) return joined
    }
    val content = item.optJSONArray("content")
    if (content != null && content.length() > 0) {
        return (0 until content.length())
            .mapNotNull { content.optJSONObject(it)?.optStringOrEmpty("text")?.trim() }
            .filter { it.isNotEmpty() }
            .joinToString("\n")
    }
    return ""
}

internal fun foldEventsRange(
    fold: TranscriptFoldState,
    events: List<RemoteEvent>,
    startIndex: Int,
) {
    for (index in startIndex until events.size) {
        val envelope = events[index]
        if (envelope.event == null) continue
        val kind = envelope.eventKind
        val params = envelope.paramsOf() ?: continue
        val item = params.optJSONObject("item")
        val eventTurnId = extractEventTurnId(params)
        if (eventTurnId != null) fold.seenTurnIds.add(eventTurnId)
        val rawEventId = params.optNullableString("itemId") ?: item?.optNullableString("id").orEmpty()
        val scopeTurnId =
            eventTurnId
                ?: envelope.event?.optJSONObject("binding")?.optNullableString("current_turn_id")
                ?: fold.liveTurnId
        val eventId =
            if (scopeTurnId != null && (rawEventId == "claude_msg" || rawEventId == "claude_thinking")) {
                "$scopeTurnId:$rawEventId"
            } else {
                rawEventId
            }

        when (kind) {
            "turn/started" -> {
                val turn = params.optJSONObject("turn")?.optStringOrEmpty("id").orEmpty()
                if (turn.isNotEmpty()) fold.seenTurnIds.add(turn)
                fold.liveTurnId = turn.ifEmpty { null }
                fold.prompts[turn]?.let { prompt ->
                    fold.promptedTurnIds.add(turn)
                    val id = "user:$turn"
                    if (fold.positions[id] == null) {
                        fold.put(TranscriptItem(id = id, kind = TranscriptKind.USER, text = prompt))
                    } else {
                        val pos = fold.positions[id]
                        if (pos != null && fold.items[pos].streaming) {
                            fold.items[pos] = fold.items[pos].copy(streaming = false)
                        }
                    }
                }
            }
            "turn/completed" -> {
                fold.liveTurnId = null
                fold.closeOpenThinking()
                fold.completeRunningTools()
            }
            "item/reasoning/delta",
            "item/reasoning/textDelta",
            "item/reasoning/summaryTextDelta",
            -> {
                if (eventId.isEmpty()) continue
                val delta = params.optStringOrEmpty("delta").ifEmpty { params.optStringOrEmpty("text") }
                if (delta.isEmpty()) continue
                val existing = fold.byId(eventId)
                if (existing == null) {
                    fold.put(
                        TranscriptItem(
                            id = eventId,
                            kind = TranscriptKind.ASSISTANT,
                            text = "<think>$delta",
                            streaming = true,
                        ),
                    )
                } else if (existing.kind == TranscriptKind.ASSISTANT) {
                    val updatedText =
                        when {
                            existing.text.endsWith("</think>") ->
                                existing.text.removeSuffix("</think>") + delta
                            existing.text.endsWith("</thinking>") ->
                                existing.text.removeSuffix("</thinking>") + delta
                            !existing.text.contains("<think", ignoreCase = true) ->
                                "<think>$delta</think>\n\n${existing.text}"
                            else -> existing.text + delta
                        }
                    fold.put(existing.copy(text = updatedText, streaming = true, history = false))
                }
            }
            "item/agentMessage/delta" -> {
                if (eventId.isEmpty()) continue
                val delta = params.optStringOrEmpty("delta")
                if (delta.isEmpty()) continue
                val existing = fold.byId(eventId)
                if (existing == null) {
                    val call = bareToolCall(delta)
                    if (call != null) {
                        fold.put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.TOOL,
                                title = "命令执行",
                                command = call.optStringOrEmpty("command"),
                                description = call.optStringOrEmpty("description"),
                                state = ToolState.RUNNING,
                                turnId = eventTurnId,
                                text = "工具执行中…",
                            ),
                        )
                        continue
                    }
                }
                if (existing != null && existing.kind != TranscriptKind.ASSISTANT) continue
                fold.put(
                    if (existing == null) {
                        TranscriptItem(id = eventId, kind = TranscriptKind.ASSISTANT, text = delta)
                    } else {
                        existing.copy(text = existing.text + delta, history = false)
                    },
                )
            }
            "item/commandExecution/outputDelta" -> {
                if (eventId.isEmpty()) continue
                val delta = params.optStringOrEmpty("delta")
                if (delta.isEmpty()) continue
                val existing = fold.byId(eventId)
                if (existing == null) {
                    fold.put(
                        TranscriptItem(
                            id = eventId,
                            kind = TranscriptKind.TOOL,
                            title = "命令执行",
                            output = delta,
                            state = ToolState.RUNNING,
                            turnId = eventTurnId,
                            text = "工具执行中…",
                        ),
                    )
                } else if (existing.kind == TranscriptKind.TOOL) {
                    fold.put(existing.copy(output = existing.output.orEmpty() + delta))
                }
            }
            "item/fileChange/outputDelta" -> {
                if (eventId.isEmpty()) continue
                val delta = params.optStringOrEmpty("delta")
                if (delta.isEmpty()) continue
                val existing = fold.byId(eventId)
                if (existing == null) {
                    fold.put(
                        TranscriptItem(
                            id = eventId,
                            kind = TranscriptKind.TOOL,
                            title = "文件修改",
                            output = delta,
                            state = ToolState.RUNNING,
                            turnId = eventTurnId,
                            text = "工具执行中…",
                        ),
                    )
                } else if (existing.kind == TranscriptKind.TOOL) {
                    fold.put(existing.copy(output = existing.output.orEmpty() + delta))
                }
            }
            "item/started", "item/completed" -> {
                if (eventId.isEmpty() || item == null) continue
                val completed = kind == "item/completed"
                when (item.optStringOrEmpty("type")) {
                    "userMessage" -> {
                        if (fold.positions[eventId] != null) continue
                        val text = textOf(item)
                        if (text.isBlank()) continue
                        val effectiveTurnId = eventTurnId ?: fold.liveTurnId
                        val alreadyRenderedFromCommand =
                            (effectiveTurnId != null && effectiveTurnId in fold.promptedTurnIds) ||
                                (
                                    effectiveTurnId == null &&
                                        fold.items.lastOrNull()?.let { prev ->
                                            prev.kind == TranscriptKind.USER &&
                                                prev.id.startsWith("user:") &&
                                                prev.text.trim() == text.trim()
                                        } == true
                                    )
                        if (alreadyRenderedFromCommand) continue
                        fold.put(TranscriptItem(id = eventId, kind = TranscriptKind.USER, text = text))
                    }
                    "agentMessage" -> {
                        val existing = fold.byId(eventId)
                        if (existing != null && existing.kind != TranscriptKind.ASSISTANT) continue
                        val text = item.optStringOrEmpty("text")
                        val call = bareToolCall(text)
                        if (call != null) {
                            fold.put(
                                TranscriptItem(
                                    id = eventId,
                                    kind = TranscriptKind.TOOL,
                                    title = "命令执行",
                                    command = call.optStringOrEmpty("command"),
                                    description = call.optStringOrEmpty("description"),
                                    state = if (completed) ToolState.COMPLETED else ToolState.RUNNING,
                                    turnId = eventTurnId,
                                    text = if (completed) "工具执行已完成" else "工具执行中…",
                                ),
                            )
                            continue
                        }
                        if (text.isBlank() && existing == null) continue
                        fold.put(
                            if (existing == null) {
                                TranscriptItem(
                                    id = eventId,
                                    kind = TranscriptKind.ASSISTANT,
                                    text = text,
                                )
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
                        val failure = item.optStringOrEmpty("failureReason")
                        val existing = fold.byId(eventId)
                        val prevInner =
                            existing?.text
                                ?.removePrefix("<think>")
                                ?.removePrefix("<thinking>")
                                ?.removeSuffix("</think>")
                                ?.removeSuffix("</thinking>")
                                .orEmpty()
                        val text =
                            failure.ifBlank {
                                reasoningText(item).ifBlank { prevInner }
                            }
                        if (text.isBlank()) continue
                        val formatted =
                            when {
                                failure.isNotBlank() -> text
                                completed -> "<think>$text</think>"
                                else -> "<think>$text"
                            }
                        fold.put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.ASSISTANT,
                                text = formatted,
                                streaming = !completed,
                                history = false,
                            ),
                        )
                    }
                    "commandExecution" -> {
                        val command =
                            item.optNullableString("command")
                                ?: item.optNullableString("commandLine")
                                ?: item.optJSONObject("input")?.optNullableString("command")
                                ?: ""
                        val output =
                            item.optNullableString("aggregatedOutput")
                                ?: item.optNullableString("output")
                                ?: item.optNullableString("stdout")
                                ?: ""
                        val toolNameLabel =
                            if (command.isBlank()) item.optStringOrEmpty("toolName").trim() else ""
                        val toolState = itemState(item, completed)
                        val existing = fold.byId(eventId)
                        if (existing != null) {
                            fold.put(
                                existing.copy(
                                    title = if (command.isNotBlank()) "命令执行" else existing.title,
                                    command = command.ifBlank { existing.command },
                                    output = output.ifBlank { existing.output },
                                    state = toolState,
                                    text = if (completed) "工具执行已完成" else existing.text,
                                ),
                            )
                            continue
                        }
                        if (!fold.mergeRunningCommand(eventId, command, output, toolState, completed)) {
                            fold.put(
                                TranscriptItem(
                                    id = eventId,
                                    kind = TranscriptKind.TOOL,
                                    title = toolNameLabel.ifBlank { "命令执行" },
                                    command = command.ifBlank { null },
                                    output = output.ifBlank { null },
                                    state = toolState,
                                    turnId = eventTurnId,
                                    text = if (completed) "工具执行已完成" else "工具执行中…",
                                ),
                            )
                        }
                    }
                    "toolCall", "functionCall", "mcpToolCall", "customToolCall", "tool" -> {
                        val existing = fold.byId(eventId)
                        val cmd =
                            item.optStringOrEmpty("input")
                                .ifBlank { item.optStringOrEmpty("arguments").ifBlank { existing?.command.orEmpty() } }
                                .ifBlank { null }
                        val out =
                            item.optStringOrEmpty("output")
                                .ifBlank { existing?.output.orEmpty() }
                                .ifBlank { null }
                        fold.put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.TOOL,
                                title =
                                    toolLabel(
                                        item.optStringOrEmpty("server").ifBlank { null },
                                        item.optStringOrEmpty("tool")
                                            .ifBlank { item.optStringOrEmpty("name") }
                                            .ifBlank { null },
                                    ),
                                command = cmd,
                                output = out,
                                state = itemState(item, completed),
                                turnId = eventTurnId,
                            ),
                        )
                    }
                    "fileChange", "fileChangeStart" -> {
                        val existing = fold.byId(eventId)
                        val (label, diff) = extractFileChangeDetails(item)
                        val out =
                            item.optStringOrEmpty("output")
                                .ifBlank { existing?.output.orEmpty() }
                                .ifBlank { null }
                        fold.put(
                            TranscriptItem(
                                id = eventId,
                                kind = TranscriptKind.TOOL,
                                title = label,
                                command = diff ?: existing?.command,
                                output = out,
                                state = itemState(item, completed),
                                turnId = eventTurnId,
                            ),
                        )
                    }
                }
            }
        }
    }
}
