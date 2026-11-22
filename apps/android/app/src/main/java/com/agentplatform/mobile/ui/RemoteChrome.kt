package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.data.AttachmentReader
import com.agentplatform.mobile.data.CatalogModel
import com.agentplatform.mobile.data.ComposerMode
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.PermissionMode
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.SessionPhase
import com.agentplatform.mobile.data.matchCatalogModel
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
    hostOnline: Boolean,
    controlConnected: Boolean,
    state: ExecutionState,
    stoppable: Boolean,
    sending: Boolean,
    sessionIdDisplay: String,
    usage: ContextUsage?,
    onBack: () -> Unit,
    onToggleUsage: () -> Unit,
    onInterrupt: () -> Unit,
) {
    val clipboard = LocalClipboardManager.current
    var copiedHeaderId by remember { mutableStateOf(false) }

    LaunchedEffect(copiedHeaderId) {
        if (copiedHeaderId) {
            delay(1800)
            copiedHeaderId = false
        }
    }

    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f)),
    ) {
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrcaSpace.sm, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OrcaIconButton(onClick = onBack) {
                OrcaBackChevron(MaterialTheme.colorScheme.onSurface, 18.dp)
            }
            Column(
                modifier = Modifier.weight(1f).padding(horizontal = OrcaSpace.xs),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    text = titleText,
                    style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.Bold),
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    Text(
                        text = "$deviceTitle · $runtimeLabel",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    val pillColor =
                        when {
                            !hostOnline -> OrcaColors.textMuted
                            !controlConnected -> OrcaColors.accentAmber
                            else -> stateColorOf(state)
                        }
                    Surface(
                        shape = RoundedCornerShape(OrcaRadius.pill),
                        color = pillColor.copy(alpha = 0.14f),
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(4.dp),
                        ) {
                            OrcaStatusDot(color = pillColor, size = 5.dp)
                            Text(
                                text =
                                    when {
                                        !hostOnline -> "电脑离线"
                                        !controlConnected -> "控制断开"
                                        else -> state.label
                                    },
                                style = OrcaType.mono.copy(fontSize = 10.sp),
                                color = pillColor,
                                maxLines = 1,
                            )
                        }
                    }
                }
            }
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                OrcaTextAction(label = if (copiedHeaderId) "已复制" else "会话ID") {
                    clipboard.setText(AnnotatedString(sessionIdDisplay))
                    copiedHeaderId = true
                }
                if (usage != null) {
                    ContextCapacityRing(usage = usage, onToggle = onToggleUsage)
                }
                if (stoppable) {
                    Surface(
                        onClick = onInterrupt,
                        modifier = Modifier.minimumInteractiveComponentSize().heightIn(min = 34.dp),
                        enabled = !sending,
                        shape = RoundedCornerShape(OrcaRadius.pill),
                        color = OrcaColors.accentRed.copy(alpha = 0.12f),
                        border = BorderStroke(1.dp, OrcaColors.accentRed.copy(alpha = 0.4f)),
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = OrcaSpace.sm, vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
                        ) {
                            OrcaStopIcon(OrcaColors.accentRed, 8.dp)
                            Text(
                                text = "停止",
                                style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Bold),
                                color = OrcaColors.accentRed,
                            )
                        }
                    }
                }
            }
        }
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
    modelOptions: List<CatalogModel>,
    model: String,
    initialModel: CatalogModel?,
    modelExplicitlyCleared: Boolean,
    effort: EffortLevel,
    permissionMode: PermissionMode,
    showQuickKeys: Boolean,
    onToggleQuickKeys: () -> Unit,
    onOpenSettingsSheet: () -> Unit,
    queuedPrompt: QueuedPromptDraft?,
    onEditQueue: (QueuedPromptDraft) -> Unit,
    onCancelQueue: () -> Unit,
    onSendQueueNow: (QueuedPromptDraft) -> Unit,
    attachments: List<RemoteAttachment>,
    onRemoveAttachment: (RemoteAttachment) -> Unit,
    attachmentNotice: String?,
    draft: String,
    onDraftChange: (String) -> Unit,
    onSend: () -> Unit,
    onQueue: () -> Unit,
    onAttach: (List<android.net.Uri>) -> Unit,
    onKey: (String) -> Unit,
    onMacro: (String) -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f)),
    ) {
        Column(
            modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.xs),
        ) {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrcaSpace.md, vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                val displayModel =
                    if (model.isNotBlank()) {
                        matchCatalogModel(modelOptions, model)?.label ?: model
                    } else if (!modelExplicitlyCleared) {
                        initialModel?.label.orEmpty()
                    } else {
                        ""
                    }
                val parameterLabel =
                    listOfNotNull(
                        displayModel.takeIf { it.isNotBlank() },
                        effort.label.takeIf { effort != EffortLevel.DEFAULT },
                        permissionMode.label.takeIf { permissionMode != PermissionMode.DEFAULT },
                    ).joinToString(" · ").ifBlank { "默认配置" }
                Surface(
                    onClick = onOpenSettingsSheet,
                    enabled = render.connected && !sending,
                    modifier = Modifier.minimumInteractiveComponentSize(),
                    shape = RoundedCornerShape(OrcaRadius.pill),
                    color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f),
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.8f)),
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        OrcaSettingsIcon(OrcaColors.accentBlue, 13.dp)
                        Text(
                            text = parameterLabel,
                            style = OrcaType.mono.copy(fontSize = 11.sp),
                            color = MaterialTheme.colorScheme.onSurface,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.widthIn(max = 220.dp),
                        )
                        OrcaChevronDown(MaterialTheme.colorScheme.onSurfaceVariant, 10.dp)
                    }
                }

                Surface(
                    onClick = onToggleQuickKeys,
                    enabled = render.connected && !sending,
                    modifier = Modifier.minimumInteractiveComponentSize(),
                    shape = RoundedCornerShape(OrcaRadius.pill),
                    color =
                        if (showQuickKeys) OrcaColors.accentBlue.copy(alpha = 0.14f)
                        else Color.Transparent,
                    border =
                        BorderStroke(
                            1.dp,
                            if (showQuickKeys) OrcaColors.accentBlue.copy(alpha = 0.6f)
                            else MaterialTheme.colorScheme.outline,
                        ),
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        OrcaTerminalIcon(
                            if (showQuickKeys) OrcaColors.accentBlue
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                            12.dp,
                        )
                        Text(
                            text = "快捷键",
                            style = OrcaType.mono.copy(fontSize = 11.sp),
                            color =
                                if (showQuickKeys) OrcaColors.accentBlue
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
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

            AnimatedVisibility(visible = showQuickKeys) {
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
                hint = if (mode == ComposerMode.TAKE_CONTROL) "输入消息，发送时接管会话" else render.hint,
                sending = sending,
                hasAttachments = attachments.isNotEmpty(),
                onSend = onSend,
                onQueue = onQueue,
                onAttach = onAttach,
            )
        }
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
    Box(modifier = modifier.fillMaxWidth()) {
        if (!seeded && seedError.isBlank()) {
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
                    PaddingValues(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
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
                    key = { block ->
                        when (block) {
                            is ConversationBlock.Single -> block.item.id
                            is ConversationBlock.Tools -> "group:" + block.items.first().id
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
                                    AssistantMessage(block.item, agentLabel = runtimeLabel)
                                TranscriptKind.TOOL -> ToolCard(block.item)
                                TranscriptKind.SYSTEM -> SystemLine(block.item)
                            }
                        is ConversationBlock.Tools -> ToolGroup(block.items)
                    }
                }
                if (isWaitingFirstAssistant) {
                    item(key = "agent_thinking_skeleton") {
                        AgentThinkingSkeleton(agentLabel = runtimeLabel)
                    }
                }
            }
        }

        if (canScrollDown && blocks.isNotEmpty()) {
            Surface(
                onClick = {
                    scope.launch {
                        listState.animateScrollToItem(blocks.lastIndex, scrollOffset = 100_000)
                    }
                },
                modifier =
                    Modifier
                        .align(Alignment.BottomEnd)
                        .padding(end = OrcaSpace.lg, bottom = OrcaSpace.sm)
                        .minimumInteractiveComponentSize(),
                shape = RoundedCornerShape(OrcaRadius.pill),
                color = OrcaColors.surfaceBright,
                shadowElevation = 3.dp,
            ) {
                Text(
                    text = "回到最新 ↓",
                    style = OrcaType.mono.copy(fontWeight = FontWeight.Bold),
                    color = OrcaColors.onBright,
                    modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = 8.dp),
                )
            }
        }
    }
}
