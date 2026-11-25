package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.data.ComposerMode
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.SessionPhase
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

// =========================================================================
// Remote session chrome: Top header bar, cross-device banners, & bottom dock
// =========================================================================

@Composable
internal fun RemoteSessionTopBar(
    titleText: String,
    deviceTitle: String,
    runtimeLabel: String,
    engine: EngineKind = EngineKind.CODEX,
    hostOnline: Boolean,
    controlConnected: Boolean,
    state: ExecutionState,
    actionsEnabled: Boolean,
    sessionIdDisplay: String,
    hasUsage: Boolean,
    showUsage: Boolean,
    showQuickKeys: Boolean,
    onBack: () -> Unit,
    onToggleUsage: () -> Unit,
    onToggleQuickKeys: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    val clipboard = LocalClipboardManager.current
    var menuOpen by remember { mutableStateOf(false) }
    var copiedHeaderId by remember(sessionIdDisplay) { mutableStateOf(false) }
    val colors = MaterialTheme.colorScheme
    val stateLabel =
        when {
            !hostOnline -> "设备离线"
            !controlConnected -> "控制未连接"
            else -> state.label
        }
    val indicatorColor =
        when {
            !hostOnline -> colors.onSurfaceVariant
            !controlConnected -> OrcaColors.accentAmber
            else -> stateColorOf(state)
        }

    LaunchedEffect(copiedHeaderId) {
        if (copiedHeaderId) {
            delay(1800)
            copiedHeaderId = false
        }
    }

    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
    ) {
        ChatRoundAction(label = "返回会话列表", onClick = onBack) {
            OrcaBackChevron(colors.onSurface, 20.dp)
        }
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(3.dp),
        ) {
            Text(
                text = titleText,
                style = MaterialTheme.typography.titleLarge,
                color = colors.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Row(
                modifier = Modifier.semantics { contentDescription = "$deviceTitle · $runtimeLabel · $stateLabel" },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                AgentBrandIcon(
                    engine = engine,
                    reportedProvider = runtimeLabel,
                    size = 14.dp,
                )
                Text(
                    text = "$deviceTitle · $runtimeLabel",
                    modifier = Modifier.weight(1f, fill = false),
                    style = MaterialTheme.typography.bodySmall,
                    color = colors.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                OrcaStatusDot(indicatorColor, 6.dp)
            }
        }
        Box {
            ChatRoundAction(label = "会话操作", onClick = { menuOpen = true }) {
                Canvas(Modifier.size(20.dp)) {
                    for (position in listOf(0.2f, 0.5f, 0.8f)) {
                        drawCircle(colors.onSurface, radius = 1.8.dp.toPx(), center = Offset(size.width * position, center.y))
                    }
                }
            }
            DropdownMenu(
                expanded = menuOpen,
                onDismissRequest = { menuOpen = false },
            ) {
                DropdownMenuItem(
                    text = { Text(if (copiedHeaderId) "已复制会话 ID" else "复制会话 ID") },
                    onClick = {
                        clipboard.setText(AnnotatedString(sessionIdDisplay))
                        copiedHeaderId = true
                        menuOpen = false
                    },
                )
                if (hasUsage) {
                    DropdownMenuItem(
                        text = { Text(if (showUsage) "隐藏上下文用量" else "显示上下文用量") },
                        onClick = {
                            menuOpen = false
                            onToggleUsage()
                        },
                    )
                }
                DropdownMenuItem(
                    text = { Text(if (showQuickKeys) "隐藏快捷键" else "显示快捷键") },
                    enabled = actionsEnabled || showQuickKeys,
                    onClick = {
                        menuOpen = false
                        onToggleQuickKeys()
                    },
                )
                DropdownMenuItem(
                    text = { Text("会话设置") },
                    onClick = {
                        menuOpen = false
                        onOpenSettings()
                    },
                )
            }
        }
    }
}

@Composable
internal fun ChatRoundAction(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.size(48.dp).semantics { contentDescription = label },
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outlineVariant),
        shadowElevation = 1.dp,
    ) {
        Box(contentAlignment = Alignment.Center) { content() }
    }
}

@Composable
internal fun CrossDeviceAttentionBanner(
    otherAttention: List<AttentionItem>,
    onSwitchSession: (String) -> Unit,
) {
    if (otherAttention.isEmpty()) return
    val firstOther = otherAttention.first()
    Surface(
        onClick = {
            onSwitchSession("${firstOther.entry.runnerId}/${firstOther.entry.hubSessionId}")
        },
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(horizontal = OrcaSpace.lg, vertical = 4.dp),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = OrcaColors.accentAmber.copy(alpha = 0.12f),
        border = BorderStroke(1.dp, OrcaColors.accentAmber.copy(alpha = 0.45f)),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Row(
                modifier = Modifier.weight(1f),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                OrcaStatusDot(OrcaColors.accentAmber, 6.dp)
                Text(
                    text =
                        if (otherAttention.size == 1) {
                            "另一会话待处理：${firstOther.entry.title}"
                        } else {
                            "另有 ${otherAttention.size} 个会话等你处理：${firstOther.entry.title}"
                        },
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Text(
                text = "切换查看",
                style = OrcaType.mono.copy(fontWeight = FontWeight.Bold),
                color = OrcaColors.accentAmber,
            )
        }
    }
}

@Composable
internal fun SessionControlStrip(
    releasing: Boolean,
    canAcquire: Boolean,
    canRelease: Boolean,
    acquiring: Boolean,
    busy: Boolean,
    onAcquire: () -> Unit,
    onRelease: () -> Unit,
) {
    if (!releasing && !canAcquire && !canRelease) return
    val status = when {
        releasing -> "正在归还控制…"
        canRelease -> "AgentHub 已接管会话"
        else -> "正在只读查看会话"
    }
    Surface(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(horizontal = OrcaSpace.lg, vertical = 4.dp),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = OrcaColors.accentAmber.copy(alpha = 0.10f),
        border = BorderStroke(1.dp, OrcaColors.accentAmber.copy(alpha = 0.45f)),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = 6.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = status,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.weight(1f),
            )
            if (!releasing) {
                OrcaTextAction(
                    label = when {
                        canRelease -> "归还控制"
                        acquiring -> "接管中…"
                        else -> "接管会话"
                    },
                    enabled = !acquiring && !busy,
                    onClick = if (canRelease) onRelease else onAcquire,
                )
            }
        }
    }
}

@Composable
internal fun RemoteBottomConsoleDock(
    render: SessionPhase,
    mode: ComposerMode,
    sending: Boolean,
    canInterrupt: Boolean,
    hostOnline: Boolean,
    showQuickKeys: Boolean,
    queuedPrompt: QueuedPromptDraft?,
    onEditQueue: (QueuedPromptDraft) -> Unit,
    onCancelQueue: () -> Unit,
    onSendQueueNow: (QueuedPromptDraft) -> Unit,
    attachments: List<RemoteAttachment>,
    onRemoveAttachment: (RemoteAttachment) -> Unit,
    attachmentNotice: String?,
    draft: String,
    onDraftChange: (String) -> Unit,
    effort: EffortLevel = EffortLevel.DEFAULT,
    onEffortChange: ((EffortLevel) -> Unit)? = null,
    onSend: () -> Unit,
    onQueue: () -> Unit,
    onAttach: (List<android.net.Uri>) -> Unit,
    onAttachmentError: (String) -> Unit = {},
    onInterrupt: () -> Unit,
    onKey: (String) -> Unit,
    onMacro: (String) -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth().padding(bottom = OrcaSpace.xs)) {
        if (!hostOnline) {
            Row(
                modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                val color = MaterialTheme.colorScheme.onSurfaceVariant
                OrcaMonitorIcon(color, 16.dp)
                Text(
                    text = "设备离线，连接恢复后继续同步。",
                    style = MaterialTheme.typography.bodySmall,
                    color = color,
                )
            }
        }

        queuedPrompt?.let { currentQueue ->
            QueuedPromptBar(
                queued = currentQueue,
                canSendNow = mode == ComposerMode.READY && !sending,
                onEdit = { onEditQueue(currentQueue) },
                onCancel = onCancelQueue,
                onSendNow = { onSendQueueNow(currentQueue) },
            )
        }

        AttachmentChips(
            attachments = attachments,
            onRemove = onRemoveAttachment,
        )
        attachmentNotice?.let { notice ->
            AttachmentNotice(
                message = notice,
                modifier =
                    Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.xxs),
            )
        }

        AnimatedVisibility(
            visible = showQuickKeys,
            enter =
                expandVertically(
                    animationSpec = tween(MotionTokens.Quick),
                    expandFrom = Alignment.Bottom,
                ) + fadeIn(tween(MotionTokens.Quick)),
            exit =
                shrinkVertically(
                    animationSpec = tween(MotionTokens.Quick),
                    shrinkTowards = Alignment.Bottom,
                ) + fadeOut(tween(MotionTokens.Quick)),
        ) {
            TerminalAccessoryBar(
                enabled = render.connected && !sending,
                onKey = onKey,
                onMacro = onMacro,
            )
        }

        Composer(
            draft = draft,
            onDraftChange = onDraftChange,
            mode = mode,
            hint =
                when (mode) {
                    ComposerMode.BLOCKED -> if (hostOnline) "控制未连接" else "设备离线"
                    ComposerMode.BUSY -> "输入下一条消息…"
                    ComposerMode.READY, ComposerMode.TAKE_CONTROL -> "输入消息…"
                },
            sending = sending,
            hasAttachments = attachments.isNotEmpty(),
            effort = effort,
            onEffortChange = onEffortChange,
            onSend = onSend,
            onQueue = onQueue,
            onAttach = onAttach,
            onAttachmentError = onAttachmentError,
            stoppable = render.stoppable,
            canInterrupt = canInterrupt,
            onInterrupt = onInterrupt,
        )
    }
}

@Composable
internal fun RemoteTranscriptArea(
    modifier: Modifier = Modifier,
    seeded: Boolean,
    seedError: String,
    listState: LazyListState,
    blocks: List<ConversationBlock>,
    connected: Boolean,
    canReconnect: Boolean,
    onReconnect: () -> Unit,
    onQuickPrompt: (String) -> Unit,
    runtimeLabel: String,
    isWaitingFirstAssistant: Boolean,
    canScrollDown: Boolean,
) {
    val scope = rememberCoroutineScope()
    var activeToolGroup by remember { mutableStateOf<String?>(null) }
    val activeTools = remember(blocks, activeToolGroup) {
        val selected = activeToolGroup
        if (selected == null) null
        else blocks.firstNotNullOfOrNull { block ->
            when (block) {
                is ConversationBlock.Single ->
                    block.item.takeIf { it.displayKey == selected && it.kind == TranscriptKind.TOOL }?.let { listOf(it) }
                is ConversationBlock.Tools -> block.items.takeIf { tools -> tools.any { it.displayKey == selected } }
            }
        }
    }
    if (activeTools != null) {
        ToolActivitySheet(items = activeTools, onDismiss = { activeToolGroup = null })
    }
    Box(modifier = modifier.fillMaxWidth()) {
        if (!seeded && blocks.isEmpty() && seedError.isBlank()) {
            Column(
                modifier =
                    Modifier
                        .fillMaxSize()
                        .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                SkeletonBlock(64.dp)
                SkeletonRow()
                SkeletonBlock(96.dp)
            }
        } else {
            LazyColumn(
                state = listState,
                modifier = Modifier.fillMaxSize(),
                contentPadding =
                    PaddingValues(horizontal = OrcaSpace.lg, vertical = OrcaSpace.md),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.md),
            ) {
                if (blocks.isEmpty()) {
                    item {
                        SessionHeroCard(
                            connected = connected,
                            canReconnect = canReconnect,
                            onReconnect = onReconnect,
                            onQuickPrompt = onQuickPrompt,
                        )
                    }
                }
                items(
                    blocks,
                    key = { it.displayKey },
                    contentType = { block ->
                        when (block) {
                            is ConversationBlock.Single -> block.item.kind.name
                            is ConversationBlock.Tools -> TranscriptKind.TOOL.name
                        }
                    },
                ) { block ->
                    when (block) {
                        is ConversationBlock.Single ->
                            when (block.item.kind) {
                                TranscriptKind.USER ->
                                    UserBubble(
                                        text = block.item.text,
                                        sending =
                                            block.item.id.startsWith("optimistic-") ||
                                                block.item.id.startsWith("pending:"),
                                    )
                                TranscriptKind.ASSISTANT ->
                                    AssistantMessage(block.item)
                                TranscriptKind.TOOL ->
                                    ToolActivitySummary(items = listOf(block.item)) { activeToolGroup = block.displayKey }
                                TranscriptKind.SYSTEM -> SystemLine(block.item)
                            }
                        is ConversationBlock.Tools ->
                            ToolActivitySummary(items = block.items) { activeToolGroup = block.displayKey }
                    }
                }
                if (isWaitingFirstAssistant) {
                    item(key = "agent_thinking_skeleton") {
                        AgentThinkingSkeleton(agentLabel = runtimeLabel)
                    }
                }
            }
        }

        AnimatedVisibility(
            visible = canScrollDown && blocks.isNotEmpty(),
            modifier =
                Modifier
                    .align(Alignment.BottomEnd)
                    .padding(end = OrcaSpace.lg, bottom = OrcaSpace.sm),
            enter =
                fadeIn(tween(MotionTokens.Quick)) +
                    slideInVertically(tween(MotionTokens.Standard)) { it / 2 },
            exit =
                fadeOut(tween(MotionTokens.Quick)) +
                    slideOutVertically(tween(MotionTokens.Quick)) { it / 2 },
        ) {
            Surface(
                onClick = {
                    scope.launch {
                        listState.animateScrollToItem(blocks.lastIndex, scrollOffset = 100_000)
                    }
                },
                modifier = Modifier.minimumInteractiveComponentSize(),
                shape = RoundedCornerShape(OrcaRadius.pill),
                color = OrcaColors.surfaceBright,
                shadowElevation = 3.dp,
            ) {
                Text(
                    text = "回到最新 ↓",
                    style = MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.SemiBold),
                    color = OrcaColors.onBright,
                    modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = 8.dp),
                )
            }
        }
    }
}
