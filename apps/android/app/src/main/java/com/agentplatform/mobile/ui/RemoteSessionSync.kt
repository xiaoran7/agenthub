package com.agentplatform.mobile.ui

import android.content.Context
import android.net.Uri
import com.agentplatform.mobile.data.AgentGateway
import com.agentplatform.mobile.data.AttachmentReader
import com.agentplatform.mobile.data.AttachmentResult
import com.agentplatform.mobile.data.EventWindow
import com.agentplatform.mobile.data.RemoteBinding
import com.agentplatform.mobile.data.GatewayException
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.RemoteCommandRecord
import com.agentplatform.mobile.data.RemoteEvent
import com.agentplatform.mobile.data.SessionDetail
import com.agentplatform.mobile.data.describeGatewayError
import com.agentplatform.mobile.data.summarizeAttachmentResults
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.coroutineContext
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

// =========================================================================
// Session SSE + cursor synchronization & off-main-thread SAF attachment I/O
// =========================================================================

internal suspend fun runRemoteSessionStream(
    gateway: AgentGateway,
    runnerId: String,
    hubId: String,
    window: EventWindow,
    wakeChannel: Channel<Unit>,
    initiallySeeded: Boolean,
    currentFallbackBusy: () -> Boolean,
    currentCommands: () -> List<RemoteCommandRecord>,
    onCommandsUpdated: (List<RemoteCommandRecord>) -> Unit,
    onDetailBindingUpdated: (RemoteBinding?) -> Unit,
    onEventsUpdated: (List<RemoteEvent>) -> Unit,
    onSeeded: () -> Unit,
    onSeedError: (String) -> Unit,
    onAuthExpired: () -> Unit = {},
) = coroutineScope {
    var seededOnce = initiallySeeded
    var failures = 0
    var lastDetailCursor = -1L
    val sseLive = AtomicBoolean(false)
    var sseJob: Job? = null

    fun CoroutineScope.ensureSseStarted(startAfter: Long) {
        if (sseJob != null) return
        sseJob =
            launch {
                var sseCursor = startAfter
                var attempt = 0
                while (isActive) {
                    val closer = AtomicReference<(() -> Unit)?>(null)
                    val job = coroutineContext[Job]
                    val reg = job?.invokeOnCompletion { closer.getAndSet(null)?.invoke() }
                    var connectedAtMs = 0L
                    var authExpired = false
                    try {
                        withContext(Dispatchers.IO) {
                            gateway.streamEvents(
                                after = sseCursor,
                                onOpen = { closeFn -> closer.set(closeFn) },
                                onConnected = { streamUp ->
                                    sseLive.set(streamUp)
                                    if (streamUp) connectedAtMs = System.currentTimeMillis()
                                },
                                onFrame = { frame ->
                                    if (frame.cursor > sseCursor) sseCursor = frame.cursor
                                    if (
                                        frame.event == "remote" &&
                                        (frame.runnerId == null || frame.runnerId == runnerId)
                                    ) {
                                        wakeChannel.trySend(Unit)
                                    }
                                },
                            )
                        }
                    } catch (cancel: CancellationException) {
                        throw cancel
                    } catch (failure: Exception) {
                        if (failure is GatewayException && (failure.status == 401 || failure.status == 403)) {
                            authExpired = true
                        }
                    } finally {
                        sseLive.set(false)
                        reg?.dispose()
                        closer.getAndSet(null)?.invoke()
                    }
                    if (authExpired) {
                        onAuthExpired()
                        break
                    }
                    if (!isActive) break
                    if (connectedAtMs > 0L && System.currentTimeMillis() - connectedAtMs >= 10_000L) {
                        attempt = 0
                    }
                    attempt += 1
                    val backoffMs = minOf(1_000L shl minOf(attempt - 1, 4), 15_000L)
                    delay(backoffMs)
                }
            }
    }

    try {
        if (seededOnce) ensureSseStarted(window.cursor)
        while (true) {
            var activity = false
            try {
                if (!seededOnce) {
                    val detail =
                        withContext(Dispatchers.IO) { gateway.fetchSession(runnerId, hubId) }
                    onCommandsUpdated(detail.commands)
                    onDetailBindingUpdated(detail.binding)
                    lastDetailCursor = detail.cursor
                    seedResume(gateway, runnerId, hubId, detail, window) { progressive ->
                        onEventsUpdated(progressive)
                    }
                    seededOnce = true
                    onSeeded()
                    onSeedError("")
                    activity = true
                    ensureSseStarted(maxOf(lastDetailCursor, window.cursor))
                } else {
                    val detail =
                        withContext(Dispatchers.IO) { gateway.fetchSession(runnerId, hubId) }
                    if (detail.commands != currentCommands()) {
                        onCommandsUpdated(detail.commands)
                        activity = true
                    }
                    onDetailBindingUpdated(detail.binding)

                    if (detail.cursor < lastDetailCursor) {
                        seedResume(gateway, runnerId, hubId, detail, window) { progressive ->
                            onEventsUpdated(progressive)
                        }
                        lastDetailCursor = detail.cursor
                        activity = true
                    } else if (detail.cursor > lastDetailCursor || window.cursor == 0L) {
                        while (true) {
                            val before = window.cursor
                            val page =
                                withContext(Dispatchers.IO) {
                                    gateway.fetchEvents(runnerId, hubId, before, limit = 1000)
                                }
                            if (!window.append(page) || window.cursor <= before) break
                            activity = true
                        }
                        lastDetailCursor = detail.cursor
                    }
                }
                failures = 0
                onSeedError("")
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (failure: Exception) {
                if (failure is GatewayException && (failure.status == 401 || failure.status == 403)) {
                    onAuthExpired()
                    break
                }
                if (failure is GatewayException && failure.code == "invalid_cursor") {
                    window.reset(initialCursor = 0L, batch = emptyList())
                    lastDetailCursor = -1L
                    seededOnce = false
                    continue
                }
                failures += 1
                if (failures >= 3) onSeedError(describeGatewayError(failure))
            }
            if (activity) onEventsUpdated(window.snapshot())

            val busy = currentFallbackBusy()
            val hasPendingCommand = currentCommands().any { it.receipt == null }
            val baseWaitMs =
                when {
                    !seededOnce -> 1_500L
                    hasPendingCommand -> 300L
                    sseLive.get() -> 15_000L
                    busy -> 300L
                    else -> 2_500L
                }
            val waitMs =
                if (failures > 0) {
                    maxOf(baseWaitMs, minOf(1_500L shl minOf(failures - 1, 4), 15_000L))
                } else {
                    baseWaitMs
                }
            withTimeoutOrNull(waitMs) { wakeChannel.receive() }
        }
    } finally {
        sseJob?.cancel()
    }
}

private suspend fun seedResume(
    gateway: AgentGateway,
    runnerId: String,
    hubId: String,
    detail: SessionDetail,
    window: EventWindow,
    onProgress: (List<RemoteEvent>) -> Unit = {},
) = withContext(Dispatchers.IO) {
    val mark = detail.resumeAfter
    val start = (mark ?: 0L).let { if (it > 0) it - 1 else 0L }
    window.reset(initialCursor = start, batch = emptyList())
    while (true) {
        val before = window.cursor
        val page = gateway.fetchEvents(runnerId, hubId, before, limit = 1000)
        if (!window.append(page) || window.cursor <= before) break
        val snapshot = window.snapshot()
        withContext(Dispatchers.Main.immediate) {
            onProgress(snapshot)
        }
    }
}

internal fun RemoteAttachment.estimatedDecodedBytes(): Long =
    if (encoding == "base64") {
        (data.length.toLong() * 3L) / 4L
    } else {
        data.toByteArray(Charsets.UTF_8).size.toLong()
    }

internal suspend fun readPickedAttachments(
    context: Context,
    uris: List<Uri>,
    existingCount: Int,
    existingAttachments: List<RemoteAttachment> = emptyList(),
): Pair<List<RemoteAttachment>, String?> {
    val room = AttachmentReader.MAX_ATTACHMENTS - existingCount
    if (room <= 0) {
        return emptyList<RemoteAttachment>() to "最多附带 ${AttachmentReader.MAX_ATTACHMENTS} 个文件"
    }
    val overflow = (uris.size - room).coerceAtLeast(0)
    var usedBytes = existingAttachments.sumOf { it.estimatedDecodedBytes() }
    val results =
        withContext(Dispatchers.IO) {
            uris.take(room).map { uri ->
                var displayName = ""
                runCatching {
                    val name = context.queryDisplayName(uri).orEmpty()
                    displayName = name
                    val type = context.contentResolver.getType(uri).orEmpty()
                    val size = context.querySize(uri) ?: 0L
                    if (size > 0L && usedBytes + size > AttachmentReader.MAX_TOTAL_BYTES) {
                        return@runCatching AttachmentResult.Rejected(
                            "${name.ifBlank { "文件" }} 会使总附件超出 7MB 上限",
                        )
                    }
                    val stream =
                        context.contentResolver.openInputStream(uri)
                            ?: return@runCatching AttachmentResult.Rejected("附件读取失败")
                    stream.use { input ->
                        val parsed =
                            AttachmentReader.read(
                                displayName = name,
                                mediaType = type,
                                declaredSize = size,
                                stream = input,
                            )
                        if (parsed is AttachmentResult.Ready) {
                            val itemBytes = parsed.attachment.estimatedDecodedBytes()
                            if (usedBytes + itemBytes > AttachmentReader.MAX_TOTAL_BYTES) {
                                AttachmentResult.Rejected(
                                    "${parsed.attachment.name} 会使总附件超出 7MB 上限",
                                )
                            } else {
                                usedBytes += itemBytes
                                parsed
                            }
                        } else {
                            parsed
                        }
                    }
                }.getOrElse {
                    AttachmentResult.Rejected("读取附件 ${displayName.ifBlank { "未命名文件" }} 失败")
                }
            }
        }
    val ready = results.filterIsInstance<AttachmentResult.Ready>().map { it.attachment }
    val summary = summarizeAttachmentResults(results)
    val notice =
        when {
            overflow > 0 && summary != null -> "$summary；另有 $overflow 个超出上限未加入"
            overflow > 0 -> "有 $overflow 个文件超出 ${AttachmentReader.MAX_ATTACHMENTS} 个上限，未加入"
            else -> summary
        }
    return ready to notice
}

internal data class ParsedTranscriptState(
    val baseConversation: List<TranscriptItem> = emptyList(),
    val baseBlocks: List<ConversationBlock> = emptyList(),
    val signals: SessionSignals = SessionSignals(emptyList(), emptyList(), null, null),
)

internal data class OptimisticPrompt(
    val id: String,
    val text: String,
    val neededUserRows: Int,
    val createdAt: Long = System.currentTimeMillis(),
)

internal const val OPTIMISTIC_PROMPT_TTL_MS = 90_000L

internal fun OptimisticPrompt.toTranscriptItem() =
    TranscriptItem(id = id, kind = TranscriptKind.USER, text = text)

internal data class RejectedCommandOutcome(
    val newIds: Set<String>,
    val errorMessage: String?,
    val prunedPromptText: String?,
)

internal fun evaluateRejectedCommands(
    commands: List<RemoteCommandRecord>,
    handledIds: Set<String>,
    announceErrors: Boolean,
): RejectedCommandOutcome {
    val rejected =
        commands.filter { cmd ->
            val status = cmd.receipt?.optString("status").orEmpty()
            status.isNotEmpty() && status != "accepted"
        }
    if (rejected.isEmpty()) return RejectedCommandOutcome(emptySet(), null, null)
    val unseen = mutableListOf<RemoteCommandRecord>()
    val newIds = mutableSetOf<String>()
    for ((index, cmd) in rejected.withIndex()) {
        val id = cmd.command.optString("client_cmd_id").ifBlank { "seq:${cmd.receipt?.optLong("seq", index.toLong())}" }
        if (id !in handledIds) {
            newIds += id
            unseen += cmd
        }
    }
    if (!announceErrors || unseen.isEmpty()) {
        return RejectedCommandOutcome(newIds, null, null)
    }
    val latest = unseen.last()
    val reason =
        latest.receipt?.optString("message")?.takeIf { it.isNotBlank() }
            ?: latest.receipt?.optString("error")?.takeIf { it.isNotBlank() }
            ?: latest.receipt?.optString("status").orEmpty()
    val message =
        when (val code = latest.receipt?.optString("code").orEmpty()) {
            "native_writer_busy", "explicit_control_required", "session_control_releasing", "session_not_started" ->
                describeGatewayError(GatewayException(409, code))
            else -> if (reason.isNotBlank()) "指令被拒绝: $reason" else "指令被拒绝"
        }
    val promptText =
        if (latest.action == "prompt") {
            formatCommandPromptText(latest.command.optJSONObject("command")).takeIf { it.isNotBlank() }
        } else {
            null
        }
    return RejectedCommandOutcome(newIds, message, promptText)
}
