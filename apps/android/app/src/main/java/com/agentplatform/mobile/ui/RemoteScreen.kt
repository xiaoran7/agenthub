package com.agentplatform.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.AgentGateway
import com.agentplatform.mobile.data.ComposerMode
import com.agentplatform.mobile.data.ConnectMode
import com.agentplatform.mobile.data.isCodexThreadId
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.EventWindow
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.ModelCatalog
import com.agentplatform.mobile.data.PermissionMode
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.RemoteBinding
import com.agentplatform.mobile.data.RemoteCommandRecord
import com.agentplatform.mobile.data.RemoteEvent
import com.agentplatform.mobile.data.RemoteSessionRow
import com.agentplatform.mobile.data.SandboxMode
import com.agentplatform.mobile.data.SessionPhase
import com.agentplatform.mobile.data.connectCommand
import com.agentplatform.mobile.data.describeGatewayError
import com.agentplatform.mobile.data.displayEngineLabel
import com.agentplatform.mobile.data.engineOfSession
import com.agentplatform.mobile.data.matchCatalogModel
import com.agentplatform.mobile.data.modelsForEngine
import com.agentplatform.mobile.data.optStringOrEmpty
import com.agentplatform.mobile.data.sendPromptCommand
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

// =========================================================================
// Remote session screen: live SSE + cursor stream, queueing, & reconnect
// =========================================================================

@Composable
internal fun RemoteScreen(
    gateway: AgentGateway,
    controller: AppController,
    session: RemoteSessionRow,
    hostGeneration: String,
    onBack: () -> Unit,
    onSwitchSession: (String) -> Unit = {},
    seedTurn: TurnSeed? = null,
    onSeedConsumed: () -> Unit = {},
) {
    BackHandler(enabled = true) { onBack() }

    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val context = LocalContext.current
    val binding = session.binding
    val hubId = binding.hubSessionId
    val runnerId = session.runnerId

    val window = remember(gateway, runnerId, hubId) { EventWindow() }
    val wakeChannel = remember(gateway, runnerId, hubId) { Channel<Unit>(Channel.CONFLATED) }
    var events by remember(gateway, runnerId, hubId) { mutableStateOf(emptyList<RemoteEvent>()) }
    var commands by remember(gateway, runnerId, hubId) {
        mutableStateOf(emptyList<RemoteCommandRecord>())
    }
    var seeded by remember(gateway, runnerId, hubId) { mutableStateOf(false) }
    var seedError by remember(gateway, runnerId, hubId) { mutableStateOf("") }
    var actionError by remember(gateway, runnerId, hubId) { mutableStateOf("") }
    var sending by remember(gateway, runnerId, hubId) { mutableStateOf(false) }
    var takeoverCommandId by remember(gateway, runnerId, hubId) { mutableStateOf<String?>(null) }
    var takeoverGeneration by remember(gateway, runnerId, hubId) { mutableStateOf<String?>(null) }
    var reconnecting by remember(gateway, runnerId, hubId) { mutableStateOf(false) }
    var attaching by remember(gateway, runnerId, hubId) { mutableStateOf(false) }
    var submittingQuestionIds by remember(gateway, runnerId, hubId) { mutableStateOf(emptySet<String>()) }
    var draft by rememberSaveable(runnerId, hubId) { mutableStateOf("") }
    val sessionKey = "$runnerId/$hubId"
    val queuedPrompt = controller.queuedPromptFor(sessionKey)
    var pendingSeedPrompt by remember(runnerId, hubId) { mutableStateOf(seedTurn?.firstPrompt) }
    var handledRejectedCommandIds by remember(runnerId, hubId) { mutableStateOf(emptySet<String>()) }
    var manualInterruptPending by remember(gateway, runnerId, hubId) { mutableStateOf(false) }
    var wasBusy by remember(gateway, runnerId, hubId) { mutableStateOf(false) }
    var optimisticPrompts by remember(runnerId, hubId) { mutableStateOf(emptyList<OptimisticPrompt>()) }
    var dismissedFailure by remember(gateway, runnerId, hubId) { mutableStateOf<String?>(null) }
    var showUsage by remember { mutableStateOf(false) }
    var showSettingsSheet by remember { mutableStateOf(false) }
    var showQuickKeys by remember { mutableStateOf(false) }

    var effort by rememberSaveable(runnerId, hubId) { mutableStateOf(seedTurn?.effort ?: EffortLevel.DEFAULT) }
    var model by rememberSaveable(runnerId, hubId) { mutableStateOf(seedTurn?.model.orEmpty()) }
    var modelExplicitlyCleared by remember(gateway, runnerId, hubId) { mutableStateOf(false) }
    var permissionMode by rememberSaveable(runnerId, hubId) { mutableStateOf(seedTurn?.permissionMode ?: PermissionMode.DEFAULT) }
    var permissionModeTouched by rememberSaveable(runnerId, hubId) { mutableStateOf(seedTurn?.permissionTouched == true) }
    var attachments by remember(gateway, runnerId, hubId) { mutableStateOf(emptyList<RemoteAttachment>()) }

    LaunchedEffect(seedTurn) {
        if (seedTurn != null) {
            if (seedTurn.firstPrompt != null && pendingSeedPrompt == null) {
                pendingSeedPrompt = seedTurn.firstPrompt
            }
            onSeedConsumed()
        }
    }
    var attachmentNotice by remember(gateway, runnerId, hubId) { mutableStateOf<String?>(null) }
    var detailBinding by remember(gateway, runnerId, hubId) { mutableStateOf<RemoteBinding?>(null) }
    val listState = rememberLazyListState()

    val current = controller.state.snapshot.session(runnerId, hubId) ?: session
    val generation = hostGeneration.ifBlank { current.binding.hostGeneration }
    val liveBinding = detailBinding?.takeIf { it.hostGeneration == generation } ?: current.binding
    val state = liveBinding.executionState
    val render = SessionPhase.of(current.online, liveBinding.controlConnected, state)
    val host = controller.state.snapshot.host(runnerId)
    val codexSession = host?.codexSessionControl == true && isCodexThreadId(liveBinding.providerSessionId)
    val releasingControl = current.online && state == ExecutionState.RELEASING_CONTROL
    val canTakeControl = current.online && codexSession && !render.connected && !releasingControl
    val mode = if (canTakeControl) ComposerMode.TAKE_CONTROL else render.composer
    val composerSending = sending || takeoverCommandId != null

    val candidate =
        remember(host?.hello?.candidates, liveBinding.providerSessionId) {
            host?.hello?.candidates?.firstOrNull {
                it.providerSessionId == liveBinding.providerSessionId
            }
        }
    val hostPlugins = mergeHostPlugins(host?.hello?.plugins, controller.state.plugins.orEmpty())
    val reportedProvider = current.provider ?: candidate?.provider
    val engine = engineOfSession(liveBinding.providerSessionId, reportedProvider, hostPlugins)
    val runtimeLabel = displayEngineLabel(engine, liveBinding.providerSessionId, reportedProvider, hostPlugins)
    val modelOptions =
        remember(host?.models, engine, reportedProvider, liveBinding.providerSessionId) {
            modelsForEngine(
                catalog = host?.models ?: ModelCatalog.Empty,
                engine = engine,
                reportedProvider = reportedProvider,
                providerSessionId = liveBinding.providerSessionId,
            )
        }
    val initialModel = matchCatalogModel(modelOptions, candidate?.model)
    LaunchedEffect(initialModel?.id) {
        if (model.isEmpty() && !modelExplicitlyCleared) {
            model = initialModel?.id.orEmpty()
        }
    }

    LaunchedEffect(runnerId, gateway) {
        controller.loadRunnerHello(runnerId, force = false)
    }

    // Cross-device attention alert for other sessions waiting on user approval/input
    val otherAttention =
        remember(controller.state.snapshot, controller.state.plugins, runnerId, hubId) {
            buildAttentionQueue(controller.state.snapshot, controller.state.plugins.orEmpty()).filterNot {
                it.entry.runnerId == runnerId && it.entry.hubSessionId == hubId
            }
        }

    // Seed + follow with SSE wakeup and cursor short-circuiting.
    LaunchedEffect(gateway, runnerId, hubId, lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            runRemoteSessionStream(
                gateway = gateway,
                runnerId = runnerId,
                hubId = hubId,
                window = window,
                wakeChannel = wakeChannel,
                initiallySeeded = seeded,
                currentFallbackBusy = { (detailBinding?.executionState ?: state).busy },
                currentCommands = { commands },
                onCommandsUpdated = { nextCommands ->
                    commands = nextCommands
                    val outcome =
                        evaluateRejectedCommands(nextCommands, handledRejectedCommandIds, announceErrors = seeded)
                    if (outcome.newIds.isNotEmpty()) handledRejectedCommandIds += outcome.newIds
                    if (outcome.errorMessage != null) actionError = outcome.errorMessage
                    if (outcome.prunedPrompts.isNotEmpty() && optimisticPrompts.isNotEmpty()) {
                        optimisticPrompts = pruneRejectedOptimisticPrompts(optimisticPrompts, outcome)
                    }
                    if (outcome.newIds.isNotEmpty() && submittingQuestionIds.isNotEmpty()) {
                        val failedQIds =
                            nextCommands
                                .filter { it.action == "answer_question" && it.receipt?.optStringOrEmpty("status").let { s -> !s.isNullOrEmpty() && s != "accepted" } }
                                .mapNotNull { it.command.optJSONObject("command")?.optStringOrEmpty("question_id")?.takeIf(String::isNotBlank) }
                                .toSet()
                        if (failedQIds.isNotEmpty()) submittingQuestionIds = submittingQuestionIds - failedQIds
                    }
                },
                onDetailBindingUpdated = { detailBinding = it },
                onEventsUpdated = { events = it },
                onSeeded = { seeded = true },
                onSeedError = { seedError = it },
                onAuthExpired = { controller.logout() },
            )
        }
    }

    LaunchedEffect(seeded, current.online, generation) {
        if (seeded && current.online && codexSession && events.isEmpty() &&
            commands.none { it.command.optJSONObject("command")?.optStringOrEmpty("mode") == "preview" }) {
            controller.dispatch(
                runnerId = runnerId,
                hubSessionId = hubId,
                hostGeneration = generation,
                action = connectCommand(ConnectMode.PREVIEW, liveBinding.workspacePath, liveBinding.providerSessionId),
            ) { wakeChannel.trySend(Unit) }
        }
    }

    val live = current.online && (state == ExecutionState.RUNNING || optimisticPrompts.isNotEmpty())
    val transcriptFold = remember(runnerId, hubId) { IncrementalTranscriptFold() }
    var parsed by remember(runnerId, hubId) { mutableStateOf(ParsedTranscriptState()) }

    LaunchedEffect(events, commands, hubId, live, candidate) {
        parsed =
            withContext(Dispatchers.Default) {
                val baseItems = transcriptFold.update(events, commands, hubId, live)
                val baseBlocks = groupTranscript(baseItems)
                val signals = analyzeSessionSignals(events, hubId, candidate)
                ParsedTranscriptState(
                    baseConversation = baseItems,
                    baseBlocks = baseBlocks,
                    signals = signals,
                )
            }
    }

    val baseConversation = parsed.baseConversation
    val userRowCount =
        remember(baseConversation) {
            baseConversation.count { it.kind == TranscriptKind.USER }
        }
    val pendingPrompts =
        remember(optimisticPrompts, userRowCount) {
            optimisticPrompts.filter { !optimisticPromptLanded(it.neededUserRows, userRowCount) }
        }

    LaunchedEffect(optimisticPrompts) {
        while (optimisticPrompts.isNotEmpty()) {
            delay(1_000L)
            val rows = parsed.baseConversation.count { it.kind == TranscriptKind.USER }
            val now = System.currentTimeMillis()
            val kept =
                optimisticPrompts.filter {
                    !optimisticPromptLanded(it.neededUserRows, rows) &&
                        now - it.createdAt <= OPTIMISTIC_PROMPT_TTL_MS
                }
            if (kept.size != optimisticPrompts.size) optimisticPrompts = kept
        }
    }

    val conversation =
        remember(baseConversation, pendingPrompts) {
            if (pendingPrompts.isEmpty()) baseConversation
            else baseConversation + pendingPrompts.map { it.toTranscriptItem() }
        }
    val blocks =
        remember(parsed.baseBlocks, pendingPrompts) {
            if (pendingPrompts.isEmpty()) parsed.baseBlocks
            else parsed.baseBlocks + pendingPrompts.map { ConversationBlock.Single(it.toTranscriptItem()) }
        }
    val approvals = parsed.signals.approvals
    val questions = parsed.signals.questions
    val turnFailure = parsed.signals.turnFailure
    val turnLive = state == ExecutionState.RUNNING || pendingPrompts.isNotEmpty()
    val lastUserIndex = conversation.indexOfLast { it.kind == TranscriptKind.USER }
    val answeredSincePrompt =
        lastUserIndex >= 0 &&
            conversation.drop(lastUserIndex + 1).any { it.kind == TranscriptKind.ASSISTANT }
    val isWaitingFirstAssistant =
        turnLive &&
            lastUserIndex >= 0 &&
            !answeredSincePrompt &&
            approvals.isEmpty() &&
            questions.isEmpty()
    val usage = parsed.signals.usage

    // Follow new streaming output all the way to the bottom of the tail item,
    // and show a floating "回到最新" pill whenever the user scrolled back up.
    val canScrollDown by remember { derivedStateOf { listState.canScrollForward } }
    val lastMessageLength = conversation.lastOrNull()?.text?.length ?: 0
    LaunchedEffect(blocks.size, lastMessageLength) {
        if (blocks.isEmpty()) return@LaunchedEffect
        if (!listState.canScrollForward || listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index == blocks.lastIndex) {
            listState.scrollToItem(blocks.lastIndex, scrollOffset = 100_000)
        }
    }

    fun send(
        action: JSONObject,
        targetGeneration: String = generation,
        clearComposerOnSuccess: Boolean = false,
        onFailure: (() -> Unit)? = null,
        onQueued: ((String) -> Unit)? = null,
    ) {
        if (sending) {
            onFailure?.invoke()
            return
        }
        sending = true
        actionError = ""
        controller.dispatch(
            runnerId = runnerId,
            hubSessionId = hubId,
            hostGeneration = targetGeneration.ifBlank { null },
            action = action,
        ) { result ->
            sending = false
            result
                .onSuccess {
                    onQueued?.invoke(it.clientCmdId)
                    if (clearComposerOnSuccess) {
                        draft = ""
                        attachments = emptyList()
                    }
                    wakeChannel.trySend(Unit)
                }
                .onFailure {
                    actionError = describeGatewayError(it)
                    onFailure?.invoke()
                }
        }
    }

    fun dispatchPromptPayload(
        text: String,
        promptAttachments: List<RemoteAttachment>,
        clearComposer: Boolean,
        acquireControl: Boolean = false,
    ) {
        val trimmed = text.trim()
        if ((trimmed.isEmpty() && promptAttachments.isEmpty()) ||
            (mode != ComposerMode.READY && !(acquireControl && canTakeControl)) || composerSending) return
        manualInterruptPending = false
        val promptToSend =
            trimmed.ifEmpty {
                "请查看附件：" + promptAttachments.joinToString("、") { it.name }
            }
        val optId = "optimistic-${UUID.randomUUID()}"
        val neededUserRows =
            nextOptimisticPromptNeeded(
                pending = pendingPrompts.map { it.neededUserRows },
                userRowCount = userRowCount,
            )
        val savedDraft = if (clearComposer) draft else null
        val savedAttachments = if (clearComposer) attachments else emptyList()
        optimisticPrompts =
            optimisticPrompts +
                OptimisticPrompt(id = optId, text = promptToSend, neededUserRows = neededUserRows)
        if (clearComposer && !acquireControl) {
            draft = ""
            attachments = emptyList()
        }
        send(
            action =
                sendPromptCommand(
                    text = promptToSend,
                    effort = effort,
                    model = model,
                    permissionMode = permissionMode,
                    attachments = promptAttachments,
                    includePermissionMode = permissionModeTouched,
                    acquireControl = acquireControl,
                ),
            clearComposerOnSuccess = false,
            onQueued = { id ->
                optimisticPrompts = optimisticPrompts.map { if (it.id == optId) it.copy(clientCmdId = id) else it }
                if (acquireControl) {
                    takeoverCommandId = id
                    takeoverGeneration = generation
                }
            },
            onFailure = {
                optimisticPrompts = optimisticPrompts.filterNot { it.id == optId }
                if (savedDraft != null) {
                    if (draft.isBlank()) draft = savedDraft
                    if (attachments.isEmpty() && savedAttachments.isNotEmpty()) {
                        attachments = savedAttachments
                    }
                }
            },
        )
    }

    LaunchedEffect(commands, takeoverCommandId) {
        val id = takeoverCommandId ?: return@LaunchedEffect
        val command = commands.firstOrNull { it.command.optStringOrEmpty("client_cmd_id") == id } ?: return@LaunchedEffect
        val receipt = command.receipt ?: return@LaunchedEffect
        takeoverCommandId = null
        takeoverGeneration = null
        if (receipt.optStringOrEmpty("status") == "accepted") {
            draft = ""
            attachments = emptyList()
        }
    }

    LaunchedEffect(takeoverCommandId, current.online, generation, render.connected) {
        val id = takeoverCommandId ?: return@LaunchedEffect
        val sameGeneration = takeoverGeneration == null || generation == takeoverGeneration
        if (current.online && render.connected && sameGeneration) {
            draft = ""
            attachments = emptyList()
            takeoverCommandId = null
            takeoverGeneration = null
            return@LaunchedEffect
        }
        if (!current.online || !sameGeneration) {
            takeoverCommandId = null
            takeoverGeneration = null
            return@LaunchedEffect
        }
        delay(30_000L)
        if (takeoverCommandId == id) {
            takeoverCommandId = null
            takeoverGeneration = null
            actionError = "接管会话超时，请重试。"
        }
    }

    LaunchedEffect(questions) {
        if (submittingQuestionIds.isNotEmpty()) {
            val activeIds = questions.mapTo(HashSet()) { it.questionId }
            submittingQuestionIds = submittingQuestionIds.filterTo(HashSet()) { it in activeIds }
        }
    }

    fun queueCurrentDraft() {
        val totalBefore = queuedPrompt?.attachments.orEmpty().size + attachments.size
        val next = mergeQueuedPrompt(queuedPrompt, draft, attachments) ?: return
        val dropped = totalBefore - next.attachments.size
        controller.setQueuedPromptFor(sessionKey, next)
        draft = ""
        attachments = emptyList()
        if (dropped > 0) attachmentNotice = "有 $dropped 个附件超出总大小或数量上限，未加入队列"
    }

    // Dispatch seeded initial prompt only after control channel is connected and composer is ready.
    LaunchedEffect(pendingSeedPrompt, render.connected, mode, sending) {
        val promptAction = pendingSeedPrompt ?: return@LaunchedEffect
        if (render.connected && mode == ComposerMode.READY && !sending) {
            pendingSeedPrompt = null
            val text = promptAction.optStringOrEmpty("text").trim()
            val optId = if (text.isNotEmpty()) "optimistic-${UUID.randomUUID()}" else null
            if (optId != null) {
                val needed =
                    nextOptimisticPromptNeeded(
                        pending = pendingPrompts.map { it.neededUserRows },
                        userRowCount = userRowCount,
                    )
                optimisticPrompts = optimisticPrompts + OptimisticPrompt(id = optId, text = text, neededUserRows = needed)
            }
            send(
                action = promptAction,
                onQueued = { id ->
                    if (optId != null) {
                        optimisticPrompts = optimisticPrompts.map { if (it.id == optId) it.copy(clientCmdId = id) else it }
                    }
                },
                onFailure = {
                    if (optId != null) optimisticPrompts = optimisticPrompts.filterNot { it.id == optId }
                },
            )
        }
    }

    // Auto-dispatch queued prompt once an active turn finishes normally (never after manual interrupt or error)
    LaunchedEffect(mode, sending, queuedPrompt, turnFailure, manualInterruptPending) {
        if (mode == ComposerMode.BUSY) {
            wasBusy = true
            return@LaunchedEffect
        }
        val pendingQueue = queuedPrompt
        if (
            pendingQueue != null &&
            shouldAutoDispatchQueue(
                wasBusy = wasBusy,
                mode = mode,
                sending = sending,
                manualInterruptPending = manualInterruptPending,
                hasTurnFailure = turnFailure != null && !turnFailure.retrying,
            )
        ) {
            controller.setQueuedPromptFor(sessionKey, null)
            wasBusy = false
            dispatchPromptPayload(pendingQueue.text, pendingQueue.attachments, clearComposer = false)
        } else if (mode == ComposerMode.READY && !sending) {
            wasBusy = false
            if (manualInterruptPending) manualInterruptPending = false
        }
    }

    val canReconnect =
        current.online &&
            !liveBinding.controlConnected &&
            !releasingControl &&
            liveBinding.providerSessionId.isNotBlank() &&
            liveBinding.workspacePath.isNotBlank()

    val reconnectNow: () -> Unit = {
        if (canReconnect && !reconnecting) {
            reconnecting = true
            actionError = ""
            controller.dispatch(
                runnerId = runnerId,
                hubSessionId = hubId,
                hostGeneration = generation.ifBlank { null },
                action =
                    connectCommand(
                        mode = if (codexSession) ConnectMode.TAKE_CONTROL else ConnectMode.RESUME,
                        workspacePath = liveBinding.workspacePath,
                        providerSessionId = liveBinding.providerSessionId,
                        model = model,
                        permissionMode = permissionMode,
                        sandbox = if (permissionMode == PermissionMode.PLAN) SandboxMode.READ_ONLY else SandboxMode.WORKSPACE_WRITE,
                    ),
            ) { res ->
                reconnecting = false
                res
                    .onSuccess {
                        scope.launch { controller.refresh() }
                        wakeChannel.trySend(Unit)
                    }
                    .onFailure { actionError = describeGatewayError(it) }
            }
        }
    }

    fun attachPicked(uris: List<android.net.Uri>) {
        if (uris.isEmpty() || attaching) return
        attaching = true
        scope.launch {
            try {
                val (ready, notice) =
                    readPickedAttachments(
                        context = context,
                        uris = uris,
                        existingCount = attachments.size,
                        existingAttachments = attachments,
                    )
                var dropped = 0
                if (ready.isNotEmpty()) {
                    val merged = mergeAttachmentsWithinBudget(attachments, ready)
                    attachments = merged.attachments
                    dropped = merged.droppedCount
                }
                attachmentNotice =
                    notice ?: if (dropped > 0) "有 $dropped 个附件超出总大小或数量上限，未加入" else null
            } finally {
                attaching = false
            }
        }
    }

    val sessionIdDisplay = liveBinding.providerSessionId.ifBlank { hubId }

    Box(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding()
                .windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars))
                .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
        contentAlignment = Alignment.TopCenter,
    ) {
        Column(Modifier.widthIn(max = 920.dp).fillMaxSize()) {
            RemoteSessionTopBar(
                titleText =
                    sanitizeSessionTitle(liveBinding.title)
                        .ifBlank { sanitizeSessionTitle(candidate?.title) }
                        .ifBlank { workspaceLabel(liveBinding.workspacePath) }
                        .ifBlank { "会话" },
                deviceTitle = deviceLabel(runnerId, host?.hello?.deviceName),
                runtimeLabel = runtimeLabel,
                engine = engine,
                hostOnline = current.online,
                controlConnected = liveBinding.controlConnected,
                state = state,
                actionsEnabled = render.connected && !composerSending,
                sessionIdDisplay = sessionIdDisplay,
                hasUsage = usage != null,
                showUsage = showUsage,
                showQuickKeys = showQuickKeys,
                onBack = onBack,
                onToggleUsage = { showUsage = !showUsage },
                onToggleQuickKeys = { showQuickKeys = !showQuickKeys },
                onOpenSettings = { showSettingsSheet = true },
            )

            CrossDeviceAttentionBanner(
                otherAttention = otherAttention,
                onSwitchSession = onSwitchSession,
            )

            SessionControlStrip(
                releasing = releasingControl,
                canAcquire = canReconnect,
                canRelease = codexSession && render.connected && !turnLive && !state.waiting && liveBinding.currentTurnId == null,
                acquiring = reconnecting,
                busy = composerSending,
                onAcquire = reconnectNow,
                onRelease = { send(JSONObject().put("action", "release_control")) },
            )

            val banner = actionError.ifBlank { seedError }
            if (banner.isNotBlank()) {
                OrcaInlineError(
                    banner,
                    Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
                )
            }

            if (showUsage && usage != null) {
                ContextCapacityStrip(
                    usage = usage,
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xxs),
                )
            }

            if (turnFailure != null && !turnFailure.retrying && turnFailure.message != dismissedFailure) {
                TurnFailureCard(
                    failure = turnFailure,
                    onDismiss = { dismissedFailure = turnFailure.message },
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
                )
            }

            RemoteInterventionRail(
                approvals = approvals,
                questions = questions,
                enabled = render.connected && !sending,
                submittingQuestionIds = submittingQuestionIds,
                onResolveApproval = { approval, approved ->
                    send(
                        resolveApprovalCommand(approval.approvalId, approved),
                        targetGeneration = approval.hostGeneration,
                    )
                },
                onAnswerQuestion = { question, answer ->
                    if (!sending && question.questionId !in submittingQuestionIds) {
                        submittingQuestionIds = submittingQuestionIds + question.questionId
                        send(
                            answerQuestionCommand(question.questionId, answer),
                            targetGeneration = question.hostGeneration,
                            onFailure = { submittingQuestionIds = submittingQuestionIds - question.questionId },
                        )
                    }
                },
            )

            // Transcript area + floating scroll-to-bottom button
            RemoteTranscriptArea(
                modifier = Modifier.weight(1f),
                seeded = seeded,
                seedError = seedError,
                listState = listState,
                blocks = blocks,
                connected = render.connected,
                canReconnect = canReconnect,
                onReconnect = reconnectNow,
                onQuickPrompt = { prompt ->
                    if (mode == ComposerMode.READY) {
                        dispatchPromptPayload(prompt, emptyList(), clearComposer = true)
                    } else {
                        draft = prompt
                    }
                },
                runtimeLabel = runtimeLabel,
                isWaitingFirstAssistant = isWaitingFirstAssistant,
                canScrollDown = canScrollDown,
            )

            RemoteBottomConsoleDock(
                render = render,
                mode = mode,
                sending = composerSending,
                canInterrupt = !sending,
                hostOnline = current.online,
                showQuickKeys = showQuickKeys,
                queuedPrompt = queuedPrompt,
                onEditQueue = { currentQueue ->
                    draft = if (draft.isBlank()) currentQueue.text else "${currentQueue.text}\n\n$draft"
                    val merged = mergeAttachmentsWithinBudget(currentQueue.attachments, attachments)
                    attachments = merged.attachments
                    if (merged.droppedCount > 0) {
                        attachmentNotice = "有 ${merged.droppedCount} 个附件超出总大小或数量上限，未合并"
                    }
                    controller.setQueuedPromptFor(sessionKey, null)
                },
                onCancelQueue = { controller.setQueuedPromptFor(sessionKey, null) },
                onSendQueueNow = { currentQueue ->
                    controller.setQueuedPromptFor(sessionKey, null)
                    dispatchPromptPayload(
                        currentQueue.text,
                        currentQueue.attachments,
                        clearComposer = false,
                    )
                },
                attachments = attachments,
                onRemoveAttachment = { attachments = attachments - it },
                attachmentNotice = attachmentNotice,
                draft = draft,
                onDraftChange = { draft = it },
                effort = effort,
                onEffortChange = { effort = it },
                onSend = { dispatchPromptPayload(draft, attachments, clearComposer = true, acquireControl = canTakeControl) },
                onQueue = { queueCurrentDraft() },
                onAttach = { attachPicked(it) },
                onAttachmentError = { attachmentNotice = it },
                onInterrupt = { manualInterruptPending = true; send(JSONObject().put("action", "interrupt")) },
                onKey = { key ->
                    when (key) {
                        "Esc" -> draft = ""
                        "Tab" -> draft = "$draft  "
                        "Ctrl+C" -> {
                            manualInterruptPending = true
                            send(JSONObject().put("action", "interrupt"))
                        }
                    }
                },
                onMacro = { macro ->
                    if (mode == ComposerMode.READY) {
                        dispatchPromptPayload(macro, emptyList(), clearComposer = false)
                    } else {
                        draft = macro
                    }
                },
            )

            if (showSettingsSheet) {
                SessionSettingsSheet(
                    modelOptions = modelOptions,
                    model = model,
                    onModelChange = { nextModel ->
                        model = nextModel
                        modelExplicitlyCleared = nextModel.isEmpty()
                    },
                    effort = effort,
                    onEffortChange = { effort = it },
                    permissionMode = permissionMode,
                    onPermissionModeChange = { nextPerm ->
                        permissionMode = nextPerm
                        permissionModeTouched = true
                    },
                    sessionIdText = sessionIdDisplay,
                    enabled = render.connected && !sending,
                    onRefreshModels = {
                        scope.launch { controller.loadRunnerHello(runnerId, force = true) }
                    },
                    onDismiss = { showSettingsSheet = false },
                )
            }
        }
    }
}
