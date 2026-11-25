package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.isCodexThreadId

@Composable
internal fun SessionFeedHeader(
    title: String,
    grouping: SessionFeedGrouping,
    onGroupingChange: (SessionFeedGrouping) -> Unit,
    onlineOnly: Boolean,
    onToggleOnlineOnly: () -> Unit,
    providerFilter: EngineKind?,
    filterEngines: List<EngineKind>,
    onFilterChange: (EngineKind?) -> Unit,
    refreshing: Boolean,
    searchOpen: Boolean,
    onToggleSearch: () -> Unit,
    onRefresh: () -> Unit,
    onOpenNavigation: (() -> Unit)?,
) {
    var menuOpen by remember { mutableStateOf(false) }
    var choosingProvider by remember { mutableStateOf(false) }
    val ink = MaterialTheme.colorScheme.onSurface
    val dismissMenu = {
        menuOpen = false
        choosingProvider = false
    }

    Box(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Text(
            text = title,
            modifier = Modifier.align(Alignment.Center).padding(horizontal = 56.dp).semantics { heading() },
            style = MaterialTheme.typography.titleLarge.copy(fontWeight = FontWeight.SemiBold),
            color = ink,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (onOpenNavigation != null) {
            SessionHeaderButton(
                label = "打开导航",
                onClick = onOpenNavigation,
                modifier = Modifier.align(Alignment.CenterStart),
            ) {
                Canvas(Modifier.size(20.dp)) {
                    listOf(0.25f, 0.50f, 0.75f).forEach { y ->
                        drawLine(
                            color = ink,
                            start = Offset(size.width * 0.12f, size.height * y),
                            end = Offset(size.width * if (y == 0.50f) 0.68f else 0.88f, size.height * y),
                            strokeWidth = 2.dp.toPx(),
                            cap = StrokeCap.Round,
                        )
                    }
                }
            }
        }
        Box(Modifier.align(Alignment.CenterEnd)) {
            SessionHeaderButton(label = "会话筛选与选项", onClick = { menuOpen = true }) {
                if (refreshing) {
                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                } else {
                    Canvas(Modifier.size(20.dp)) {
                        listOf(0.20f, 0.50f, 0.80f).forEach { x ->
                            drawCircle(ink, radius = 1.7.dp.toPx(), center = Offset(size.width * x, center.y))
                        }
                    }
                }
            }
            MaterialTheme(shapes = MaterialTheme.shapes.copy(extraSmall = RoundedCornerShape(18.dp))) {
                DropdownMenu(
                    expanded = menuOpen,
                    onDismissRequest = dismissMenu,
                    modifier = Modifier.widthIn(min = 214.dp, max = 280.dp),
                ) {
                    AnimatedContent(
                        targetState = choosingProvider,
                        transitionSpec = {
                            if (targetState) {
                                (slideInHorizontally(tween(MotionTokens.Quick)) { it / 3 } + fadeIn(tween(MotionTokens.Quick)))
                                    .togetherWith(slideOutHorizontally(tween(MotionTokens.Quick)) { -it / 4 } + fadeOut(tween(MotionTokens.Quick)))
                            } else {
                                (slideInHorizontally(tween(MotionTokens.Quick)) { -it / 3 } + fadeIn(tween(MotionTokens.Quick)))
                                    .togetherWith(slideOutHorizontally(tween(MotionTokens.Quick)) { it / 4 } + fadeOut(tween(MotionTokens.Quick)))
                            }
                        },
                        label = "session-menu-page",
                    ) { inProviderSubmenu ->
                        Column {
                            if (inProviderSubmenu) {
                                DropdownMenuItem(
                                    text = { Text("Agent 筛选") },
                                    leadingIcon = { OrcaBackChevron(ink, 16.dp) },
                                    onClick = { choosingProvider = false },
                                )
                                SessionMenuOption("全部 Agent", providerFilter == null) {
                                    onFilterChange(null)
                                    dismissMenu()
                                }
                                filterEngines.forEach { engine ->
                                    SessionMenuOption(engine.label, providerFilter == engine) {
                                        onFilterChange(engine)
                                        dismissMenu()
                                    }
                                }
                            } else {
                                SessionMenuHeading("分组方式")
                                SessionFeedGrouping.entries.forEach { group ->
                                    SessionMenuOption(group.label, grouping == group) {
                                        onGroupingChange(group)
                                        dismissMenu()
                                    }
                                }
                                HorizontalDivider(Modifier.padding(horizontal = 12.dp, vertical = 4.dp))
                                SessionMenuHeading("筛选")
                                SessionMenuOption("仅看在线会话", onlineOnly) {
                                    onToggleOnlineOnly()
                                    dismissMenu()
                                }
                                DropdownMenuItem(
                                    text = { Text(providerFilter?.let { "Agent · ${it.label}" } ?: "筛选 Agent") },
                                    trailingIcon = { OrcaChevronRight(ink) },
                                    onClick = { choosingProvider = true },
                                )
                                HorizontalDivider(Modifier.padding(horizontal = 12.dp, vertical = 4.dp))
                                DropdownMenuItem(
                                    text = { Text(if (searchOpen) "关闭搜索" else "搜索会话") },
                                    onClick = {
                                        onToggleSearch()
                                        dismissMenu()
                                    },
                                )
                                DropdownMenuItem(
                                    text = { Text(if (refreshing) "正在刷新" else "刷新会话") },
                                    enabled = !refreshing,
                                    onClick = {
                                        onRefresh()
                                        dismissMenu()
                                    },
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SessionHeaderButton(
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
        border = BorderStroke(1.dp, OrcaColors.borderSubtle),
        shadowElevation = 1.dp,
    ) {
        Box(contentAlignment = Alignment.Center) { content() }
    }
}

@Composable
private fun SessionMenuHeading(label: String) {
    Text(
        text = label,
        modifier = Modifier.padding(start = 12.dp, top = 6.dp, bottom = 2.dp).semantics { heading() },
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        style = MaterialTheme.typography.labelMedium,
    )
}

@Composable
private fun SessionMenuOption(label: String, isSelected: Boolean, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(label) },
        modifier = Modifier.semantics { selected = isSelected },
        trailingIcon = {
            Box(Modifier.size(18.dp), contentAlignment = Alignment.Center) {
                if (isSelected) OrcaCheckIcon(MaterialTheme.colorScheme.primary, 16.dp)
            }
        },
        onClick = onClick,
    )
}

@Composable
internal fun SessionFeedSearch(query: String, onQueryChange: (String) -> Unit) {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    OutlinedTextField(
        value = query,
        onValueChange = onQueryChange,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 6.dp).focusRequester(focus),
        singleLine = true,
        textStyle = MaterialTheme.typography.bodyLarge,
        shape = RoundedCornerShape(20.dp),
        placeholder = { Text("搜索标题、工作区、Agent 或电脑") },
        leadingIcon = { SessionSearchIcon(MaterialTheme.colorScheme.onSurfaceVariant) },
        trailingIcon = if (query.isEmpty()) null else {
            { TextButton(onClick = { onQueryChange("") }) { Text("清除") } }
        },
    )
}

@Composable
internal fun SessionDeviceChip(
    label: String,
    selected: Boolean,
    online: Boolean?,
    onClick: () -> Unit,
) {
    val colors = MaterialTheme.colorScheme
    val status = when (online) {
        true -> "在线"
        false -> "离线"
        null -> null
    }
    Surface(
        shape = CircleShape,
        color = if (selected) colors.primary else colors.surface,
        contentColor = if (selected) colors.onPrimary else colors.onSurface,
        border = if (selected) null else BorderStroke(1.dp, OrcaColors.borderSubtle),
    ) {
        Row(
            modifier = Modifier
                .heightIn(min = 40.dp)
                .selectable(selected, role = Role.RadioButton, onClick = onClick)
                .semantics { if (status != null) stateDescription = status }
                .padding(horizontal = 14.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(7.dp),
        ) {
            if (online != null) {
                val dotColor = when {
                    selected -> colors.onPrimary
                    online -> OrcaColors.accentGreen
                    else -> colors.onSurfaceVariant
                }
                OrcaStatusDot(dotColor, 6.dp)
            }
            Text(
                text = label,
                modifier = Modifier.widthIn(max = 160.dp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.labelLarge,
            )
        }
    }
}

@Composable
internal fun SessionFeedRow(
    row: SessionFeedEntry,
    showWorkspace: Boolean,
    showDevice: Boolean,
    resuming: Boolean,
    onOpen: () -> Unit,
    onConfigureResume: (() -> Unit)?,
) {
    val entry = row.entry
    val colors = MaterialTheme.colorScheme
    val canOpen = when (entry) {
        is TreeEntry.Active -> true
        is TreeEntry.Candidate -> row.online && (entry.resumable || isCodexThreadId(entry.providerSessionId))
    }
    val status = if (resuming) "打开中…" else row.status
    val statusColor = when {
        resuming -> colors.primary
        !row.online -> colors.onSurfaceVariant
        entry is TreeEntry.Active && !entry.controlConnected -> OrcaColors.accentAmber
        entry is TreeEntry.Active -> stateColorOf(entry.executionState)
        else -> colors.onSurfaceVariant
    }
    val location = listOfNotNull(
        row.workspaceName.takeIf { showWorkspace && it.isNotBlank() },
        entry.deviceNameLabel.takeIf { showDevice && it.isNotBlank() },
    ).joinToString(" · ")

    Row(
        modifier = Modifier.fillMaxWidth().heightIn(min = 64.dp)
            .clickable(enabled = canOpen && !resuming, role = Role.Button, onClick = onOpen)
            .padding(horizontal = 20.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        AgentBrandBadge(
            engine = entry.provider,
            reportedProvider = entry.agentLabel,
            tileSize = 36.dp,
            iconSize = 21.dp,
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(
                text = entry.title,
                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
                color = colors.onSurface,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                OrcaStatusDot(statusColor, 5.dp)
                Text(status, style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant, maxLines = 1)
                if (location.isNotBlank()) {
                    Text(
                        text = "· $location",
                        style = MaterialTheme.typography.bodySmall,
                        color = colors.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        if (resuming) {
            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
        } else if (onConfigureResume != null) {
            TextButton(onClick = onConfigureResume) { Text("选项") }
        } else if (canOpen) {
            OrcaChevronRight(colors.onSurfaceVariant, 14.dp)
        }
    }
}

@Composable
internal fun SessionLaunchBar(hostName: String?, onClick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    Surface(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 12.dp),
        shape = RoundedCornerShape(28.dp),
        color = colors.surface,
        border = BorderStroke(1.dp, OrcaColors.borderSubtle),
        shadowElevation = 3.dp,
    ) {
        Row(
            modifier = Modifier.heightIn(min = 54.dp).padding(horizontal = 8.dp, vertical = 5.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Surface(
                modifier = Modifier.size(42.dp),
                shape = CircleShape,
                color = if (hostName != null) colors.primary else colors.surfaceVariant,
            ) {
                Box(contentAlignment = Alignment.Center) {
                    if (hostName != null) OrcaPlusIcon(colors.onPrimary, 20.dp)
                    else OrcaMonitorIcon(colors.onSurfaceVariant, 19.dp)
                }
            }
            Text(
                text = if (hostName != null) "新建会话" else "查看电脑连接",
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.titleMedium,
                color = colors.onSurface,
            )
            if (hostName != null) {
                Text(
                    text = hostName,
                    modifier = Modifier.widthIn(max = 120.dp),
                    style = MaterialTheme.typography.labelMedium,
                    color = colors.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
                OrcaChevronRight(colors.onSurfaceVariant)
            }
        }
    }
}

@Composable
private fun SessionSearchIcon(tint: Color, size: Dp = 18.dp) {
    Canvas(Modifier.size(size)) {
        val radius = this.size.minDimension * 0.30f
        val lens = Offset(this.size.width * 0.43f, this.size.height * 0.43f)
        val stroke = 1.8.dp.toPx()
        drawCircle(tint, radius = radius, center = lens, style = Stroke(width = stroke))
        drawLine(
            color = tint,
            start = lens + Offset(radius * 0.72f, radius * 0.72f),
            end = Offset(this.size.width * 0.88f, this.size.height * 0.88f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
internal fun AttentionQueueCard(
    item: AttentionItem,
    resolving: Boolean,
    onOpen: () -> Unit,
    onApprove: (() -> Unit)?,
    onDecline: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val entry = item.entry
    val summary =
        item.approval?.summary?.let(::readableText)
            ?: item.question?.prompt
            ?: "${workspaceLabel(entry.workspaceId)} · 等待你在会话中确认"
    val badgeText =
        when {
            item.approval?.riskLevel == "high" -> "高风险授权"
            item.approval != null -> item.approval.toolName ?: "等待授权"
            item.question != null -> "等待回答"
            else -> entry.executionState.label
        }
    val accent =
        if (item.approval?.riskLevel == "high") OrcaColors.accentRed else OrcaColors.accentAmber

    Surface(
        onClick = onOpen,
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(OrcaRadius.card),
        color = accent.copy(alpha = 0.08f),
        border = BorderStroke(1.dp, accent.copy(alpha = 0.45f)),
    ) {
        Column(
            modifier = Modifier.padding(OrcaSpace.md),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f).padding(end = OrcaSpace.sm)) {
                    Text(
                        text = entry.title,
                        style = OrcaType.rowTitle,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text =
                            listOfNotNull(
                                entry.deviceNameLabel.takeIf { it.isNotBlank() },
                                entry.agentLabel.takeIf { it.isNotBlank() },
                                workspaceLabel(entry.workspaceId),
                            ).joinToString(" · "),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                StatusPill(label = badgeText, color = accent)
            }
            Text(
                text = summary,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            if (onApprove != null || onDecline != null) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(top = 2.dp),
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    if (onDecline != null) {
                        OrcaSecondaryButton(
                            label = "拒绝",
                            modifier = Modifier.weight(1f),
                            enabled = !resolving,
                            onClick = onDecline,
                        )
                    }
                    if (onApprove != null) {
                        OrcaPrimaryButton(
                            label = if (resolving) "处理中…" else "批准放行",
                            modifier = Modifier.weight(1f),
                            enabled = !resolving,
                            onClick = onApprove,
                        )
                    }
                }
            }
        }
    }
}

@Composable
internal fun stateColorOf(state: ExecutionState): Color =
    when (state) {
        ExecutionState.RUNNING -> OrcaColors.accentBlue
        ExecutionState.AWAITING_PERMISSION, ExecutionState.AWAITING_INPUT,
        ExecutionState.RELEASING_CONTROL -> OrcaColors.accentAmber
        ExecutionState.IDLE, ExecutionState.COMPLETED -> OrcaColors.accentGreen
        ExecutionState.ERROR -> OrcaColors.accentRed
        ExecutionState.TERMINATED -> OrcaColors.accentPurple
        ExecutionState.UNKNOWN -> OrcaColors.textMuted
    }
