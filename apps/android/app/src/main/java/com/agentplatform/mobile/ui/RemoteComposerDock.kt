package com.agentplatform.mobile.ui

import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.data.AttachmentReader
import com.agentplatform.mobile.data.CatalogModel
import com.agentplatform.mobile.data.ComposerMode
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.PermissionMode
import com.agentplatform.mobile.data.RemoteAttachment
import kotlinx.coroutines.delay

// =========================================================================
// Composer dock, QueuedPromptBar, SessionSettingsSheet & Accessory keys
// =========================================================================

internal typealias QueuedPromptDraft = com.agentplatform.mobile.data.QueuedPromptDraft

internal const val MAX_ATTACHMENT_WIRE_BYTES = 9L * 1024 * 1024

internal fun RemoteAttachment.estimatedWireBytes(): Long {
    val envelope =
        name.toByteArray(Charsets.UTF_8).size.toLong() +
            mediaType.toByteArray(Charsets.UTF_8).size.toLong() +
            encoding.length.toLong() +
            96L
    if (encoding == "base64") {
        var extra = 0L
        for (ch in data) if (ch == '/') extra += 1L
        return data.length.toLong() + extra + envelope
    }
    var bytes = envelope
    for (ch in data) {
        val code = ch.code
        bytes +=
            when {
                ch == '"' || ch == '\\' || ch == '/' || ch == '\b' || ch == '\u000C' || ch == '\n' || ch == '\r' || ch == '\t' -> 2L
                code < 0x20 -> 6L
                code < 0x80 -> 1L
                code < 0x800 -> 2L
                ch.isHighSurrogate() || ch.isLowSurrogate() -> 2L
                else -> 3L
            }
    }
    return bytes
}

internal data class AttachmentMergeResult(
    val attachments: List<RemoteAttachment>,
    val droppedCount: Int,
)

internal fun mergeAttachmentsWithinBudget(
    existing: List<RemoteAttachment>,
    incoming: List<RemoteAttachment>,
    maxCount: Int = AttachmentReader.MAX_ATTACHMENTS,
    maxDecodedBytes: Long = AttachmentReader.MAX_TOTAL_BYTES,
    maxWireBytes: Long = MAX_ATTACHMENT_WIRE_BYTES,
): AttachmentMergeResult {
    val kept = ArrayList<RemoteAttachment>(minOf(existing.size + incoming.size, maxCount))
    var decodedBytes = 0L
    var wireBytes = 0L
    var dropped = 0
    fun tryAdd(att: RemoteAttachment) {
        if (kept.size >= maxCount) {
            dropped += 1
            return
        }
        val dec = att.estimatedDecodedBytes()
        val wire = att.estimatedWireBytes()
        if (decodedBytes + dec > maxDecodedBytes || wireBytes + wire > maxWireBytes) {
            dropped += 1
            return
        }
        kept.add(att)
        decodedBytes += dec
        wireBytes += wire
    }
    existing.forEach(::tryAdd)
    incoming.forEach(::tryAdd)
    return AttachmentMergeResult(kept, dropped)
}

internal fun mergeQueuedPrompt(
    existing: QueuedPromptDraft?,
    nextText: String,
    nextAttachments: List<RemoteAttachment>,
): QueuedPromptDraft? {
    val trimmed = nextText.trim()
    if (trimmed.isEmpty() && nextAttachments.isEmpty()) return existing
    if (existing == null) {
        val merged = mergeAttachmentsWithinBudget(emptyList(), nextAttachments)
        if (trimmed.isEmpty() && merged.attachments.isEmpty()) return null
        return QueuedPromptDraft(
            text = trimmed,
            attachments = merged.attachments,
            count = 1,
        )
    }
    val mergedText =
        when {
            existing.text.isBlank() -> trimmed
            trimmed.isBlank() -> existing.text
            else -> "${existing.text}\n\n$trimmed"
        }
    val merged = mergeAttachmentsWithinBudget(existing.attachments, nextAttachments)
    val addedAnything = trimmed.isNotEmpty() || merged.attachments.size > existing.attachments.size
    if (!addedAnything) return existing
    return QueuedPromptDraft(
        text = mergedText,
        attachments = merged.attachments,
        count = existing.count + 1,
    )
}

internal fun shouldAutoDispatchQueue(
    wasBusy: Boolean,
    mode: ComposerMode,
    sending: Boolean,
    manualInterruptPending: Boolean,
    hasTurnFailure: Boolean,
): Boolean =
    wasBusy &&
        mode == ComposerMode.READY &&
        !sending &&
        !manualInterruptPending &&
        !hasTurnFailure

@Composable
internal fun QueuedPromptBar(
    queued: QueuedPromptDraft,
    canSendNow: Boolean,
    onEdit: () -> Unit,
    onCancel: () -> Unit,
    onSendNow: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth().padding(horizontal = OrcaSpace.md, vertical = 4.dp),
        shape = RoundedCornerShape(12.dp),
        color = OrcaColors.accentBlue.copy(alpha = 0.10f),
        border = BorderStroke(1.dp, OrcaColors.accentBlue.copy(alpha = 0.45f)),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    OrcaStatusDot(OrcaColors.accentBlue, 5.dp)
                    Text(
                        text =
                            if (queued.count > 1) "待发队列（已合并 ${queued.count} 条）· 当前轮完成后自动发出"
                            else "待发队列 · 当前轮完成后自动发出",
                        style = OrcaType.mono.copy(fontWeight = FontWeight.SemiBold),
                        color = OrcaColors.accentBlue,
                    )
                }
            }
            val preview =
                queued.text.ifBlank {
                    "附件：${queued.attachments.joinToString("、") { it.name }}"
                } +
                    if (queued.text.isNotBlank() && queued.attachments.isNotEmpty()) {
                        "（含 ${queued.attachments.size} 个附件）"
                    } else {
                        ""
                    }
            Text(
                text = preview,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OrcaTextAction(label = "编辑", onClick = onEdit)
                OrcaTextAction(label = "取消", onClick = onCancel)
                if (canSendNow) {
                    OrcaTextAction(label = "立即发送", onClick = onSendNow)
                }
            }
        }
    }
}

@Composable
internal fun Composer(
    draft: String,
    onDraftChange: (String) -> Unit,
    mode: ComposerMode,
    hint: String,
    sending: Boolean,
    hasAttachments: Boolean,
    effort: EffortLevel = EffortLevel.DEFAULT,
    onEffortChange: ((EffortLevel) -> Unit)? = null,
    onSend: () -> Unit,
    onQueue: () -> Unit,
    onAttach: (List<android.net.Uri>) -> Unit,
    onAttachmentError: (String) -> Unit = {},
    stoppable: Boolean,
    canInterrupt: Boolean,
    onInterrupt: () -> Unit,
) {
    val canInput = mode != ComposerMode.BLOCKED && !sending
    val isQueueMode = mode == ComposerMode.BUSY
    val canSubmit = canInput && (draft.isNotBlank() || hasAttachments)
    val submit = if (isQueueMode) onQueue else onSend
    val colors = MaterialTheme.colorScheme
    var focused by remember { mutableStateOf(false) }
    var showAttachSheet by remember { mutableStateOf(false) }
    var showEffortSheet by remember { mutableStateOf(false) }
    val attachmentPicker =
        rememberAttachmentPicker(
            onAttach = onAttach,
            onError = onAttachmentError,
        )

    if (showAttachSheet) {
        AttachmentPickerSheet(
            controller = attachmentPicker,
            onDismiss = { showAttachSheet = false },
        )
    }

    if (showEffortSheet && onEffortChange != null) {
        LaunchOptionSheet(
            title = "思考强度",
            headerSubtitle = "下一轮发送立即生效",
            choices =
                EffortLevel.entries.map { item ->
                    LaunchSheetChoice(
                        id = item.wire,
                        title = item.label,
                        subtitle = effortHint(item),
                    )
                },
            selectedId = effort.wire,
            onSelect = { wire ->
                EffortLevel.fromWire(wire)?.let(onEffortChange)
            },
            onDismiss = { showEffortSheet = false },
        )
    }

    Surface(
        modifier = Modifier.fillMaxWidth().padding(horizontal = OrcaSpace.md, vertical = 6.dp),
        shape = RoundedCornerShape(28.dp),
        color = if (mode == ComposerMode.BLOCKED) colors.surfaceVariant else colors.surface,
        border = BorderStroke(1.dp, if (focused && canInput) colors.primary else colors.outlineVariant),
        shadowElevation = if (mode == ComposerMode.BLOCKED) 0.dp else 2.dp,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 4.dp, vertical = 3.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            IconButton(
                onClick = { showAttachSheet = true },
                enabled = canInput,
                modifier = Modifier.semantics { contentDescription = "添加附件" },
            ) {
                OrcaPlusIcon(
                    tint = if (canInput) colors.onSurface else colors.onSurfaceVariant.copy(alpha = 0.45f),
                    size = 19.dp,
                )
            }
            BasicTextField(
                value = draft,
                onValueChange = onDraftChange,
                modifier =
                    Modifier
                        .weight(1f)
                        .onFocusChanged { focused = it.isFocused },
                enabled = canInput,
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.onSurface),
                cursorBrush = SolidColor(colors.primary),
                maxLines = 6,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                keyboardActions = KeyboardActions(onSend = { if (canSubmit) submit() }),
                decorationBox = { innerTextField ->
                    Box(
                        modifier = Modifier.fillMaxWidth().defaultMinSize(minHeight = 48.dp)
                            .padding(vertical = 11.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        if (draft.isEmpty()) {
                            Text(
                                text = hint.ifBlank { "发消息…" },
                                style = MaterialTheme.typography.bodyMedium,
                                color = colors.onSurfaceVariant,
                            )
                        }
                        innerTextField()
                    }
                },
            )
            if (onEffortChange != null && mode != ComposerMode.BLOCKED) {
                val activeEffort = effort != EffortLevel.DEFAULT
                Surface(
                    onClick = { showEffortSheet = true },
                    enabled = canInput,
                    modifier =
                        Modifier
                            .padding(end = 2.dp, bottom = 7.dp)
                            .semantics { contentDescription = "思考强度：${effort.label}" },
                    shape = RoundedCornerShape(OrcaRadius.pill),
                    color =
                        if (activeEffort) OrcaColors.accentBlue.copy(alpha = 0.13f)
                        else OrcaColors.bgRaised,
                    border =
                        BorderStroke(
                            1.dp,
                            if (activeEffort) OrcaColors.accentBlue.copy(alpha = 0.5f)
                            else OrcaColors.borderSubtle,
                        ),
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(3.dp),
                    ) {
                        Text(
                            text = if (activeEffort) effort.label else "思考",
                            style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Medium),
                            color =
                                if (activeEffort) OrcaColors.accentBlue
                                else colors.onSurfaceVariant,
                            maxLines = 1,
                        )
                        OrcaChevronDown(
                            tint = if (activeEffort) OrcaColors.accentBlue else colors.onSurfaceVariant,
                            size = 8.dp,
                        )
                    }
                }
            }
            if (stoppable) {
                IconButton(
                    onClick = onInterrupt,
                    enabled = canInterrupt,
                    modifier = Modifier.semantics { contentDescription = "停止执行" },
                ) {
                    OrcaStopIcon(OrcaColors.accentRed, 14.dp)
                }
            }
            if (mode == ComposerMode.TAKE_CONTROL) {
                OrcaTextAction(label = "接管并发送", enabled = canSubmit, onClick = submit)
            } else {
                val actionLabel = when {
                    sending -> "发送中"
                    isQueueMode -> "加入待发队列"
                    else -> "发送消息"
                }
                IconButton(
                    onClick = submit,
                    enabled = canSubmit,
                    modifier = Modifier.semantics { contentDescription = actionLabel },
                ) {
                    val tint = if (canSubmit) colors.onPrimary else colors.onSurfaceVariant
                    Box(
                        modifier =
                            Modifier
                                .size(38.dp)
                                .background(if (canSubmit) colors.primary else colors.surfaceVariant, CircleShape),
                        contentAlignment = Alignment.Center,
                    ) {
                        when {
                            sending ->
                                CircularProgressIndicator(
                                    modifier = Modifier.size(16.dp),
                                    strokeWidth = 1.6.dp,
                                    color = tint,
                                )
                            isQueueMode -> OrcaQueueIcon(tint = tint, size = 17.dp)
                            else -> OrcaSendIcon(tint = tint, size = 17.dp)
                        }
                    }
                }
            }
        }
    }
}

@Composable
internal fun SessionHeroCard(
    connected: Boolean,
    canReconnect: Boolean,
    onReconnect: () -> Unit,
    onQuickPrompt: (String) -> Unit,
) {
    Column(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(vertical = OrcaSpace.xxl, horizontal = OrcaSpace.sm),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(OrcaSpace.md),
    ) {
        Surface(
            modifier = Modifier.size(54.dp),
            shape = RoundedCornerShape(16.dp),
            color =
                if (connected) OrcaColors.accentBlue.copy(alpha = 0.12f)
                else MaterialTheme.colorScheme.surfaceVariant,
            border =
                BorderStroke(
                    1.dp,
                    if (connected) OrcaColors.accentBlue.copy(alpha = 0.35f)
                    else MaterialTheme.colorScheme.outline,
                ),
        ) {
            Box(contentAlignment = Alignment.Center) {
                OrcaTerminalIcon(
                    tint =
                        if (connected) OrcaColors.accentBlue
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    size = 26.dp,
                )
            }
        }
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
        ) {
            Text(
                text = if (connected) "工作台已就绪" else "会话控制未连接",
                style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.Bold),
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text =
                    if (connected) "向电脑端 Agent 发送指令，执行输出将实时同步在此"
                    else "电脑在线时可一键重新接管当前会话，或等待电脑端 Runner 恢复连线",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.widthIn(max = 280.dp),
                textAlign = TextAlign.Center,
            )
        }
        if (connected) {
            Column(
                modifier = Modifier.padding(top = OrcaSpace.sm),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                Text(
                    text = "快速操作",
                    style = OrcaType.section,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                FilterRow(
                    options =
                        listOf(
                            "查看当前工作区 git status 与改动" to "查看项目状态",
                            "git log -n 5 查看最近提交记录" to "查看最近提交",
                        ),
                    selected = "",
                    onSelect = onQuickPrompt,
                )
            }
        } else if (canReconnect) {
            OrcaPrimaryButton(
                label = "重新连接并接管会话",
                modifier = Modifier.widthIn(max = 240.dp),
                onClick = onReconnect,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SessionSettingsSheet(
    modelOptions: List<CatalogModel>,
    model: String,
    onModelChange: (String) -> Unit,
    effort: EffortLevel,
    onEffortChange: (EffortLevel) -> Unit,
    permissionMode: PermissionMode,
    onPermissionModeChange: (PermissionMode) -> Unit,
    sessionIdText: String,
    enabled: Boolean,
    onRefreshModels: () -> Unit,
    onDismiss: () -> Unit,
) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboardManager.current
    var copiedId by remember { mutableStateOf(false) }

    LaunchedEffect(copiedId) {
        if (copiedId) {
            delay(1800)
            copiedId = false
        }
    }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = MaterialTheme.colorScheme.surface,
        shape = RoundedCornerShape(topStart = OrcaRadius.card, topEnd = OrcaRadius.card),
        dragHandle = {
            Box(
                modifier =
                    Modifier
                        .padding(vertical = OrcaSpace.sm)
                        .width(36.dp)
                        .height(4.dp)
                        .background(
                            MaterialTheme.colorScheme.outlineVariant,
                            RoundedCornerShape(2.dp),
                        ),
            )
        },
    ) {
        Column(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = OrcaSpace.xl)
                    .padding(bottom = OrcaSpace.xl),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.lg),
        ) {
            Column {
                Text(
                    text = "会话配置",
                    style = MaterialTheme.typography.titleLarge,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    text = "下一轮生效，随时可切回默认",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            // 1. Model choice (includes "默认" via ModelChoiceRow!)
            Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = "模型 (Model)",
                        style = OrcaType.section,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    OrcaTextAction(label = "刷新列表", onClick = onRefreshModels)
                }
                ModelChoiceRow(
                    options = modelOptions,
                    selected = model,
                    onSelect = onModelChange,
                    enabled = enabled,
                )
            }

            // 2. Effort
            Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                Text(
                    text = "思考强度 (Effort)",
                    style = OrcaType.section,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                FilterRow(
                    options = EffortLevel.entries.map { it to it.label },
                    selected = effort,
                    onSelect = onEffortChange,
                    enabled = enabled,
                )
            }

            // 3. Permission Mode
            Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                Text(
                    text = "权限模式 (Permission)",
                    style = OrcaType.section,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                FilterRow(
                    options = PermissionMode.entries.map { it to it.label },
                    selected = permissionMode,
                    onSelect = onPermissionModeChange,
                    enabled = enabled,
                )
            }

            // 4. Session ID
            OrcaListGroup {
                OrcaRow(
                    title = "会话 ID",
                    subtitle = sessionIdText,
                    trailing = {
                        OrcaTextAction(if (copiedId) "已复制" else "复制") {
                            clipboard.setText(AnnotatedString(sessionIdText))
                            copiedId = true
                        }
                    },
                )
            }

            OrcaPrimaryButton(
                label = "完成配置",
                onClick = {
                    scope.launch {
                        runCatching { sheetState.hide() }
                        onDismiss()
                    }
                },
            )
        }
    }
}

private data class AccessoryKey(val label: String, val danger: Boolean = false)

private val ACCESSORY_KEYS =
    listOf(
        AccessoryKey("Esc"),
        AccessoryKey("Tab"),
        AccessoryKey("Ctrl+C", danger = true),
    )

private val ACCESSORY_MACROS =
    listOf(
        "继续",
        "git status",
        "运行测试",
        "git diff",
    )

@Composable
internal fun TerminalAccessoryBar(
    enabled: Boolean,
    onKey: (String) -> Unit,
    onMacro: (String) -> Unit,
) {
    Column {
        LazyRow(
            modifier = Modifier.fillMaxWidth(),
            contentPadding = PaddingValues(horizontal = OrcaSpace.lg),
            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            items(ACCESSORY_KEYS) { key ->
                KeyCap(label = key.label, enabled = enabled, danger = key.danger) {
                    onKey(key.label)
                }
            }
            item {
                Box(
                    Modifier
                        .width(1.dp)
                        .height(18.dp)
                        .background(MaterialTheme.colorScheme.outline),
                )
            }
            items(ACCESSORY_MACROS) { macro ->
                Surface(
                    onClick = { onMacro(macro) },
                    enabled = enabled,
                    modifier = Modifier.minimumInteractiveComponentSize(),
                    shape = RoundedCornerShape(OrcaRadius.button),
                    color = Color.Transparent,
                    border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                ) {
                    Text(
                        text = macro,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = 7.dp),
                    )
                }
            }
        }
        Spacer(Modifier.height(OrcaSpace.xs))
    }
}
