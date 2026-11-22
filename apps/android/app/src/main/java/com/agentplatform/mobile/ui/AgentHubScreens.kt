package com.agentplatform.mobile.ui

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.DeviceBinding
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.ExecutionState
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.PendingApproval
import com.agentplatform.mobile.data.RemoteSessionRow
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.describeGatewayError
import com.agentplatform.mobile.data.isCodexThreadId
import com.agentplatform.mobile.data.engineOfSession
import kotlinx.coroutines.delay

// =========================================================================
// Sessions & Hosts fleet screens
// =========================================================================

@Composable
fun SessionsScreen(
    snapshot: Snapshot,
    enabledPluginIds: Set<String>,
    loading: Boolean,
    refreshing: Boolean,
    resumingKey: String?,
    resolvingApprovalId: String?,
    error: String?,
    providerFilter: EngineKind?,
    onFilterChange: (EngineKind?) -> Unit,
    onOpenEntry: (TreeEntry) -> Unit,
    onConfigureResume: (TreeEntry.Candidate) -> Unit,
    onResolveApproval: (TreeEntry.Active, PendingApproval, Boolean) -> Unit,
    onLaunchNew: (String) -> Unit,
    onOpenHost: () -> Unit,
    onRefresh: () -> Unit,
) {
    val tree = remember(snapshot, providerFilter) { buildSessionTree(snapshot, providerFilter) }
    // Global cross-device attention queue — never hidden by providerFilter.
    val attention = remember(snapshot) { buildAttentionQueue(snapshot) }
    val devices = remember(tree) { tree.filter { it.online } }
    val offline = remember(tree) { tree.filterNot { it.online } }
    val firstOnlineRunnerId =
        remember(snapshot.hosts) { snapshot.hosts.firstOrNull { it.online && !it.revoked }?.runnerId }
    val filterEngines =
        remember(enabledPluginIds, snapshot, providerFilter) {
            EngineKind.entries.filter { kind ->
                kind != EngineKind.PLUGIN ||
                    enabledPluginIds.isNotEmpty() ||
                    providerFilter == EngineKind.PLUGIN
            }
        }

    LazyColumn(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding()
                .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
        contentPadding = PaddingValues(bottom = OrcaSpace.xl),
    ) {
        item {
            BrandBar(
                onlineHosts = snapshot.onlineHostCount,
                totalHosts = snapshot.hosts.count { !it.revoked },
                refreshing = refreshing,
                onNewSession = firstOnlineRunnerId?.let { id -> { onLaunchNew(id) } },
                onRefresh = onRefresh,
            )
        }

        if (error != null) {
            item {
                OrcaInlineError(
                    message = error,
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
                )
            }
        }

        item {
            MetricStrip(
                metrics =
                    listOf(
                        Triple(
                            "${snapshot.onlineHostCount}/${snapshot.hosts.count { !it.revoked }}",
                            "电脑在线",
                            null,
                        ),
                        Triple(
                            "${snapshot.runningCount}",
                            "运行中",
                            if (snapshot.runningCount > 0) OrcaColors.accentBlue else null,
                        ),
                        Triple(
                            "${attention.size.coerceAtLeast(snapshot.waitingCount)}",
                            "待处理",
                            if (attention.isNotEmpty() || snapshot.waitingCount > 0) OrcaColors.accentAmber else null,
                        ),
                    ),
                modifier = Modifier.padding(horizontal = OrcaSpace.lg),
            )
        }

        if (loading) {
            item {
                Column(
                    modifier =
                        Modifier
                            .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.lg)
                            .fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    SkeletonBlock(96.dp)
                    SkeletonRow()
                    SkeletonRow()
                    SkeletonRow()
                }
            }
            return@LazyColumn
        }

        if (snapshot.hosts.none { !it.revoked }) {
            item {
                OrcaCard(Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.lg)) {
                    EmptyPanel(
                        title = "还没有接入任何电脑",
                        body = "在“电脑”页生成绑定码，并在电脑端 AgentHub Runner 中完成配对，这台电脑就会出现在这里。",
                        icon = {
                            OrcaMonitorIcon(
                                MaterialTheme.colorScheme.onSurfaceVariant,
                                22.dp,
                            )
                        },
                        action = {
                            OrcaSecondaryButton("前往绑定电脑", onClick = onOpenHost)
                        },
                    )
                }
            }
            return@LazyColumn
        }

        if (attention.isNotEmpty()) {
            item {
                OrcaSectionHeader(
                    title = "要你处理 · ${attention.size}",
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg),
                )
            }
            items(attention, key = { "attention-${it.entry.runnerId}/${it.entry.hubSessionId}" }) { item ->
                AttentionQueueCard(
                    item = item,
                    resolving = resolvingApprovalId != null && resolvingApprovalId == item.approval?.approvalId,
                    onOpen = { onOpenEntry(item.entry) },
                    onApprove =
                        item.approval
                            ?.takeIf { it.canApprove }
                            ?.let { appr -> { onResolveApproval(item.entry, appr, true) } },
                    onDecline =
                        item.approval
                            ?.takeIf { it.canDecline }
                            ?.let { appr -> { onResolveApproval(item.entry, appr, false) } },
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xxs),
                )
            }
        }

        item {
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = OrcaSpace.lg),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OrcaSectionHeader(
                    title = "全部会话",
                    modifier = Modifier.weight(1f),
                )
                OrcaTextAction(if (refreshing) "刷新中…" else "刷新", enabled = !refreshing) {
                    onRefresh()
                }
            }
        }

        item {
            LazyRow(
                contentPadding = PaddingValues(horizontal = OrcaSpace.lg),
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                item {
                    OrcaFilterPill(
                        label = "全部",
                        active = providerFilter == null,
                        onClick = { onFilterChange(null) },
                    )
                }
                items(filterEngines, key = { it.wire }) { engine ->
                    OrcaFilterPill(
                        label = engine.label,
                        active = providerFilter == engine,
                        onClick = { onFilterChange(engine) },
                    )
                }
            }
        }

        items(devices, key = { "device-" + it.runnerId }) { device ->
            DeviceGroupSection(
                device = device,
                resumingKey = resumingKey,
                onOpenEntry = onOpenEntry,
                onConfigureResume = onConfigureResume,
                onLaunchNew = { onLaunchNew(device.runnerId) },
            )
        }

        if (offline.isNotEmpty()) {
            item { OrcaSectionHeader("离线电脑", Modifier.padding(horizontal = OrcaSpace.lg)) }
            items(offline, key = { "offline-" + it.runnerId }) { device ->
                BoxedRow {
                    OrcaRow(
                        title = device.label,
                        subtitle = "离线 · 等待电脑端恢复连线",
                        statusColor = OrcaColors.textMuted,
                        statusText = "离线",
                        leading = { HostStatusTile(online = false) },
                        onClick = null,
                    )
                }
            }
        }
    }
}

@Composable
private fun AttentionQueueCard(
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
                style = OrcaType.mono,
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
private fun BoxedRow(content: @Composable () -> Unit) {
    Column(Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xxs)) {
        OrcaCard { content() }
    }
}

@Composable
private fun DeviceGroupSection(
    device: DeviceGroup,
    resumingKey: String?,
    onOpenEntry: (TreeEntry) -> Unit,
    onConfigureResume: (TreeEntry.Candidate) -> Unit,
    onLaunchNew: () -> Unit,
) {
    Column(Modifier.padding(top = OrcaSpace.xs)) {
        OrcaSectionHeader(
            title = device.label,
            modifier = Modifier.padding(horizontal = OrcaSpace.lg),
            trailing = {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    Text(
                        text = "${device.count} 个会话",
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    OrcaTextAction("新建", onClick = onLaunchNew)
                }
            },
        )
        if (device.agents.isEmpty()) {
            Column(Modifier.padding(horizontal = OrcaSpace.lg)) {
                OrcaCard {
                    OrcaRow(
                        title = "暂无会话",
                        subtitle = "点这里或右上角“新建”立即在这台电脑上开启任务",
                        leading = {
                            OrcaIconTile { OrcaPlusIcon(MaterialTheme.colorScheme.onSurfaceVariant) }
                        },
                        onClick = onLaunchNew,
                    )
                }
            }
        }
        device.agents.forEach { agent ->
            Column(Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xxs)) {
                OrcaCard {
                    Column {
                        Row(
                            modifier =
                                Modifier
                                    .fillMaxWidth()
                                    .padding(
                                        start = OrcaSpace.rowH,
                                        end = OrcaSpace.rowH,
                                        top = OrcaSpace.sm,
                                        bottom = OrcaSpace.xxs,
                                    ),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                        ) {
                            ProviderGlyphIcon(
                                engine = agent.provider,
                                tone = SessionGlyphTone.ACTIVE,
                                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                                size = 14.dp,
                            )
                            Text(
                                text = agent.label,
                                style = OrcaType.section,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            Text(
                                text = "${agent.count}",
                                style = OrcaType.mono,
                                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.65f),
                            )
                        }
                        agent.workspaces.forEach { workspace ->
                            WorkspaceBlock(
                                workspace = workspace,
                                resumingKey = resumingKey,
                                onOpenEntry = onOpenEntry,
                                onConfigureResume = onConfigureResume,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun WorkspaceBlock(
    workspace: WorkspaceGroup,
    resumingKey: String?,
    onOpenEntry: (TreeEntry) -> Unit,
    onConfigureResume: (TreeEntry.Candidate) -> Unit,
) {
    Column(Modifier.fillMaxWidth()) {
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(
                        start = OrcaSpace.rowH,
                        end = OrcaSpace.rowH,
                        top = OrcaSpace.sm,
                        bottom = OrcaSpace.xxs,
                    ),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        ) {
            OrcaTerminalIcon(
                MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.75f),
                13.dp,
            )
            Text(
                text = workspace.label,
                style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
        }
        workspace.entries.forEach { entry ->
            EntryRow(
                entry = entry,
                resuming = resumingKey == entry.key,
                onOpen = { onOpenEntry(entry) },
                onConfigureResume =
                    if (entry is TreeEntry.Candidate && entry.resumable) {
                        { onConfigureResume(entry) }
                    } else {
                        null
                    },
            )
        }
    }
}

@Composable
private fun EntryRow(
    entry: TreeEntry,
    resuming: Boolean,
    onOpen: () -> Unit,
    onConfigureResume: (() -> Unit)? = null,
) {
    val statusText: String
    val statusColor: Color
    val subtitle: String
    val metaText: String?
    val clickable: Boolean
    when (entry) {
        is TreeEntry.Active -> {
            statusText =
                when {
                    !entry.online -> "电脑离线"
                    !entry.controlConnected -> "控制断开"
                    else -> entry.executionState.label
                }
            statusColor =
                when {
                    !entry.online -> OrcaColors.textMuted
                    !entry.controlConnected -> OrcaColors.accentAmber
                    else -> stateColorOf(entry.executionState)
                }
            subtitle =
                if (entry.controlConnected && entry.online) {
                    entry.agentLabel.takeIf { it != entry.provider.label }
                        ?.let { "$it · ${workspaceLabel(entry.workspaceId)}" }
                        ?: workspaceLabel(entry.workspaceId)
                } else {
                    "点进会话可一键重新连接"
                }
            metaText = relativeTime(entry.lastActivityAtMs)
            clickable = true
        }
        is TreeEntry.Candidate -> {
            statusText =
                when {
                    resuming -> "打开中…"
                    isCodexThreadId(entry.providerSessionId) -> "可查看"
                    entry.resumable -> "可接管"
                    else -> "仅本地"
                }
            statusColor =
                when {
                    resuming -> OrcaColors.accentBlue
                    entry.resumable -> MaterialTheme.colorScheme.onSurfaceVariant
                    else -> OrcaColors.textMuted
                }
            subtitle =
                listOfNotNull(
                    entry.model?.takeIf { it.isNotBlank() && it != "<synthetic>" },
                    entry.totalTokens?.takeIf { it > 0 }?.let { "${compactTokens(it)} tok" },
                ).joinToString(" · ").ifBlank {
                    when {
                        isCodexThreadId(entry.providerSessionId) -> "查看历史会话"
                        !entry.resumable -> "该外部会话暂不支持远程接管"
                        else -> "点此接管历史会话"
                    }
                }
            metaText = relativeTime(entry.lastActivityAtMs)
            clickable = (entry.resumable || isCodexThreadId(entry.providerSessionId)) && !resuming
        }
    }
    OrcaRow(
        title = entry.title,
        subtitle = subtitle,
        statusColor = statusColor,
        statusText = statusText,
        meta = metaText,
        enabled = clickable,
        leading = {
            SessionEntryTile(
                engine = entry.provider,
                resumable = entry is TreeEntry.Candidate,
            )
        },
        trailing = {
            if (resuming) {
                CircularProgressIndicator(
                    modifier = Modifier.size(16.dp),
                    strokeWidth = 1.6.dp,
                    color = OrcaColors.accentBlue,
                )
            } else if (onConfigureResume != null) {
                OrcaTextAction("选项", onClick = onConfigureResume)
            } else if (clickable) {
                OrcaChevronRight(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f))
            }
        },
        onClick = if (clickable) onOpen else null,
    )
}

@Composable
private fun BrandBar(
    onlineHosts: Int,
    totalHosts: Int,
    refreshing: Boolean,
    onNewSession: (() -> Unit)?,
    onRefresh: () -> Unit,
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OrcaLogo(size = 22.dp, tint = MaterialTheme.colorScheme.onBackground)
        Column(Modifier.padding(start = OrcaSpace.sm).weight(1f)) {
            Text(
                text = "AgentHub",
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.onBackground,
            )
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                OrcaStatusDot(
                    if (onlineHosts > 0) OrcaColors.accentGreen else OrcaColors.textMuted,
                    5.dp,
                )
                Text(
                    text =
                        when {
                            totalHosts == 0 -> "尚未接入电脑"
                            onlineHosts == 0 -> "$totalHosts 台电脑全部离线"
                            else -> "$onlineHosts / $totalHosts 台电脑在线"
                        },
                    style = OrcaType.mono,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (onNewSession != null) {
            OrcaIconButton(onClick = onNewSession) {
                OrcaPlusIcon(OrcaColors.accentBlue)
            }
        }
        OrcaIconButton(onClick = onRefresh, enabled = !refreshing) {
            if (refreshing) {
                CircularProgressIndicator(
                    modifier = Modifier.size(16.dp),
                    strokeWidth = 1.6.dp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                OrcaRefreshIcon(MaterialTheme.colorScheme.onSurfaceVariant)
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

internal fun relativeTime(atMs: Long?): String? {
    if (atMs == null || atMs <= 0) return null
    val delta = System.currentTimeMillis() - atMs
    if (delta < 0) return null
    val minutes = delta / 60_000
    return when {
        minutes < 1 -> "刚刚"
        minutes < 60 -> "$minutes 分钟前"
        minutes < 60 * 24 -> "${minutes / 60} 小时前"
        minutes < 60 * 24 * 30 -> "${minutes / (60 * 24)} 天前"
        else -> "${minutes / (60 * 24 * 30)} 个月前"
    }
}
