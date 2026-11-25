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
import com.agentplatform.mobile.data.optNullableString
import com.agentplatform.mobile.data.optStringOrEmpty
import com.agentplatform.mobile.data.summarizeAttachmentResults
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.EmptyCoroutineContext
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

private sealed interface SessionSyncSignal {
    data class DirectEvent(val event: RemoteEvent) : SessionSyncSignal
    data object Reconcile : SessionSyncSignal
}

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
    val reconcilePending = AtomicBoolean(false)
    val gapDetected = AtomicBoolean(false)
    val gapVersion = AtomicLong(0L)
    val signals = Channel<SessionSyncSignal>(capacity = 256)
    var sseJob: Job? = null

    fun requestReconcile() {
        reconcilePending.set(true)
        signals.trySend(SessionSyncSignal.Reconcile)
    }

    fun markGapDetected() {
        gapDetected.set(true)
        gapVersion.incrementAndGet()
        requestReconcile()
    }

    val wakeBridgeJob =
        launch {
            for (ignored in wakeChannel) {
                requestReconcile()
            }
        }

    fun CoroutineScope.ensureSseStarted(startAfter: Long) {
        if (sseJob != null) return
        sseJob =
            launch {
                var sseCursor = startAfter
                var attempt = 0
                while (isActive) {
                    val cancelled = AtomicBoolean(false)
                    val closer = AtomicReference<(() -> Unit)?>(null)
                    val job = coroutineContext[Job]
                    val reg =
                        job?.invokeOnCompletion {
                            cancelled.set(true)
                            val closeFn = closer.getAndSet(null)
                            if (closeFn != null) {
                                Dispatchers.IO.dispatch(
                                    EmptyCoroutineContext,
                                    Runnable { runCatching { closeFn() } },
                                )
                            }
                        }
                    var connectedAtMs = 0L
                    var authExpired = false
                    var forbiddenMessage: String? = null
                    try {
                        withContext(Dispatchers.IO) {
                            gateway.streamEvents(
                                after = sseCursor,
                                shouldContinue = { !cancelled.get() && job?.isActive == true },
                                onOpen = { closeFn ->
                                    closer.set(closeFn)
                                    if (cancelled.get() || job?.isActive != true) {
                                        closer.getAndSet(null)?.invoke()
                                    }
                                },
                                onConnected = { streamUp ->
                                    sseLive.set(streamUp)
                                    if (streamUp) connectedAtMs = System.currentTimeMillis()
                                },
                                onFrame = { frame ->
                                    if (frame.cursor > sseCursor) sseCursor = frame.cursor
                                    if (frame.overflowed) {
                                        markGapDetected()
                                        return@streamEvents
                                    }
                                    if (frame.event != "remote") return@streamEvents
                                    if (frame.runnerId != null && frame.runnerId != runnerId) {
                                        return@streamEvents
                                    }
                                    val ev = frame.remoteEvent
                                    if (ev != null && ev.payloadType == "event") {
                                        val evHub = ev.event?.optNullableString("hub_session_id").orEmpty()
                                        if (evHub.isNotEmpty() && evHub != hubId) return@streamEvents
                                        if (gapDetected.get()) {
                                            requestReconcile()
                                            return@streamEvents
                                        }
                                        if (ev.cursor > 0L) {
                                            if (signals.trySend(SessionSyncSignal.DirectEvent(ev)).isSuccess) {
                                                if (ev.eventKind == "turn/completed") {
                                                    requestReconcile()
                                                }
                                                return@streamEvents
                                            }
                                            markGapDetected()
                                            return@streamEvents
                                        }
                                    } else if (ev != null && ev.payloadType == "receipt") {
                                        val rHub = ev.receipt?.optNullableString("hub_session_id").orEmpty()
                                        if (rHub.isNotEmpty() && rHub != hubId) return@streamEvents
                                    }
                                    requestReconcile()
                                },
                            )
                        }
                    } catch (cancel: CancellationException) {
                        throw cancel
                    } catch (failure: Exception) {
                        if (failure is GatewayException) {
                            when (failure.status) {
                                401 -> authExpired = true
                                403 -> forbiddenMessage = describeGatewayError(failure)
                            }
                        }
                    } finally {
                        sseLive.set(false)
                        reg?.dispose()
                        closer.getAndSet(null)?.let { closeFn ->
                            Dispatchers.IO.dispatch(
                                EmptyCoroutineContext,
                                Runnable { runCatching { closeFn() } },
                            )
                        }
                    }
                    if (authExpired) {
                        onAuthExpired()
                        break
                    }
                    if (forbiddenMessage != null) {
                        onSeedError(forbiddenMessage)
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

    var shouldReconcileHttp = true
    try {
        if (seededOnce) ensureSseStarted(window.cursor)
        while (true) {
            var activity = false
            if (shouldReconcileHttp || !seededOnce) {
                val gapTokenBeforeReconcile = gapVersion.get()
                val hadGapBeforeReconcile = gapDetected.get()
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
                        if (gapVersion.get() == gapTokenBeforeReconcile) {
                            gapDetected.set(false)
                        }
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
                        } else if (detail.cursor > lastDetailCursor || window.cursor == 0L || hadGapBeforeReconcile) {
                            var truncated = false
                            while (true) {
                                val before = window.cursor
                                val page =
                                    withContext(Dispatchers.IO) {
                                        gateway.fetchEventsPage(runnerId, hubId, before, limit = 1000)
                                    }
                                if (page.truncated) {
                                    truncated = true
                                    break
                                }
                                if (!window.append(page.events) || window.cursor <= before) break
                                activity = true
                            }
                            if (truncated) {
                                val freshDetail =
                                    withContext(Dispatchers.IO) { gateway.fetchSession(runnerId, hubId) }
                                if (freshDetail.commands != currentCommands()) {
                                    onCommandsUpdated(freshDetail.commands)
                                }
                                onDetailBindingUpdated(freshDetail.binding)
                                seedResume(gateway, runnerId, hubId, freshDetail, window) { progressive ->
                                    onEventsUpdated(progressive)
                                }
                                lastDetailCursor = freshDetail.cursor
                                activity = true
                            } else {
                                lastDetailCursor = detail.cursor
                            }
                        }
                        if (hadGapBeforeReconcile && gapVersion.get() == gapTokenBeforeReconcile) {
                            gapDetected.set(false)
                        }
                    }
                    failures = 0
                    onSeedError("")
                } catch (cancel: CancellationException) {
                    throw cancel
                } catch (failure: Exception) {
                    if (failure is GatewayException && failure.status == 401) {
                        onAuthExpired()
                        break
                    }
                    if (failure is GatewayException && failure.status == 403) {
                        failures += 1
                        onSeedError(describeGatewayError(failure))
                    } else if (failure is GatewayException && failure.code == "invalid_cursor") {
                        window.reset(initialCursor = 0L, batch = emptyList())
                        lastDetailCursor = -1L
                        seededOnce = false
                        continue
                    } else {
                        failures += 1
                        if (failures >= 3) onSeedError(describeGatewayError(failure))
                    }
                }
                if (activity) onEventsUpdated(window.snapshot())
            }

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
            if (reconcilePending.getAndSet(false)) {
                shouldReconcileHttp = true
                continue
            }
            val firstSignal = withTimeoutOrNull(waitMs) { signals.receive() }
            if (firstSignal == null) {
                shouldReconcileHttp = true
                continue
            }
            var reconcileNeeded =
                firstSignal is SessionSyncSignal.Reconcile || reconcilePending.getAndSet(false)
            val directBatch = ArrayList<RemoteEvent>()
            if (firstSignal is SessionSyncSignal.DirectEvent) {
                directBatch.add(firstSignal.event)
            }
            while (true) {
                val next = signals.tryReceive().getOrNull() ?: break
                when (next) {
                    is SessionSyncSignal.DirectEvent -> directBatch.add(next.event)
                    SessionSyncSignal.Reconcile -> reconcileNeeded = true
                }
            }
            if (reconcilePending.getAndSet(false)) {
                reconcileNeeded = true
            }
            if (seededOnce && directBatch.isNotEmpty()) {
                if (gapDetected.get()) {
                    reconcileNeeded = true
                } else {
                    if (directBatch.any { it.cursor <= window.cursor }) {
                        reconcileNeeded = true
                    }
                    if (window.append(directBatch)) {
                        if (window.cursor > lastDetailCursor) {
                            lastDetailCursor = window.cursor
                        }
                        onEventsUpdated(window.snapshot())
                    }
                    val latestBindingJson =
                        directBatch.asReversed().firstNotNullOfOrNull { it.event?.optJSONObject("binding") }
                    if (latestBindingJson != null) {
                        onDetailBindingUpdated(RemoteBinding.parse(latestBindingJson))
                    }
                }
            } else if (!seededOnce && directBatch.isNotEmpty()) {
                reconcileNeeded = true
            }
            shouldReconcileHttp = reconcileNeeded
        }
    } finally {
        sseJob?.cancel()
        wakeBridgeJob.cancel()
        signals.close()
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
        var before = window.cursor
        var page = gateway.fetchEventsPage(runnerId, hubId, before, limit = 1000)
        if (page.truncated && before > 0L) {
            window.reset(initialCursor = 0L, batch = emptyList())
            before = 0L
            page = gateway.fetchEventsPage(runnerId, hubId, 0L, limit = 1000)
        }
        if (!window.append(page.events) || window.cursor <= before) break
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
    var usedWireBytes = existingAttachments.sumOf { it.estimatedWireBytes() }
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
                            val itemWireBytes = parsed.attachment.estimatedWireBytes()
                            if (
                                usedBytes + itemBytes > AttachmentReader.MAX_TOTAL_BYTES ||
                                usedWireBytes + itemWireBytes > MAX_ATTACHMENT_WIRE_BYTES
                            ) {
                                AttachmentResult.Rejected(
                                    "${parsed.attachment.name} 会使总附件超出 7MB 上限",
                                )
                            } else {
                                usedBytes += itemBytes
                                usedWireBytes += itemWireBytes
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
    val clientCmdId: String? = null,
)

internal const val OPTIMISTIC_PROMPT_TTL_MS = 90_000L

internal fun OptimisticPrompt.toTranscriptItem() =
    TranscriptItem(id = id, kind = TranscriptKind.USER, text = text)

internal data class RejectedPromptRef(
    val clientCmdId: String?,
    val text: String,
)

internal data class RejectedCommandOutcome(
    val newIds: Set<String>,
    val errorMessage: String?,
    val prunedPromptText: String?,
    val prunedPrompts: List<RejectedPromptRef> = emptyList(),
)

internal fun pruneRejectedOptimisticPrompts(
    prompts: List<OptimisticPrompt>,
    outcome: RejectedCommandOutcome,
): List<OptimisticPrompt> {
    if (prompts.isEmpty() || outcome.prunedPrompts.isEmpty()) return prompts
    val remaining = prompts.toMutableList()
    for (ref in outcome.prunedPrompts) {
        val idxById =
            ref.clientCmdId?.takeIf { it.isNotBlank() }?.let { cid ->
                remaining.indexOfFirst { it.clientCmdId == cid }
            } ?: -1
        val idx =
            if (idxById >= 0) {
                idxById
            } else if (ref.text.isNotBlank()) {
                remaining.indexOfLast {
                    it.text == ref.text || ref.text.startsWith("${it.text}\n[附件: ")
                }
            } else {
                -1
            }
        if (idx >= 0) remaining.removeAt(idx)
    }
    return remaining
}

internal fun evaluateRejectedCommands(
    commands: List<RemoteCommandRecord>,
    handledIds: Set<String>,
    announceErrors: Boolean,
): RejectedCommandOutcome {
    val rejected =
        commands.filter { cmd ->
            val status = cmd.receipt?.optStringOrEmpty("status").orEmpty()
            status.isNotEmpty() && status != "accepted"
        }
    if (rejected.isEmpty()) return RejectedCommandOutcome(emptySet(), null, null)
    val unseen = mutableListOf<RemoteCommandRecord>()
    val newIds = mutableSetOf<String>()
    for ((index, cmd) in rejected.withIndex()) {
        val id = cmd.command.optStringOrEmpty("client_cmd_id").ifBlank { "seq:${cmd.receipt?.optLong("seq", index.toLong())}" }
        if (id !in handledIds) {
            newIds += id
            unseen += cmd
        }
    }
    if (!announceErrors || unseen.isEmpty()) {
        return RejectedCommandOutcome(newIds, null, null)
    }
    val latest = unseen.last()
    val code = latest.receipt?.optNullableString("code")
    val reason =
        latest.receipt?.optNullableString("message")
            ?: latest.receipt?.optNullableString("error")
            ?: latest.receipt?.optNullableString("status").orEmpty()
    val message =
        if (code != null) {
            describeGatewayError(GatewayException(409, code))
        } else if (reason.isNotBlank()) {
            "指令被拒绝: $reason"
        } else {
            "指令被拒绝"
        }
    val prunedPrompts =
        unseen.mapNotNull { cmd ->
            if (cmd.action != "send_prompt") return@mapNotNull null
            val cid = cmd.command.optStringOrEmpty("client_cmd_id").takeIf { it.isNotBlank() }
            val text = formatCommandPromptText(cmd.command.optJSONObject("command"))
            if (cid == null && text.isBlank()) null else RejectedPromptRef(cid, text)
        }
    val promptText =
        if (latest.action == "send_prompt") {
            formatCommandPromptText(latest.command.optJSONObject("command")).takeIf { it.isNotBlank() }
        } else {
            prunedPrompts.lastOrNull()?.text?.takeIf { it.isNotBlank() }
        }
    return RejectedCommandOutcome(newIds, message, promptText, prunedPrompts)
}
