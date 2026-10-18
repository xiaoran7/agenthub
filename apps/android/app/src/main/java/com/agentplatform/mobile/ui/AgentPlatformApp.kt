package com.agentplatform.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.AppUiState
import com.agentplatform.mobile.data.ActivityStatus
import com.agentplatform.mobile.data.AgentType
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.DeviceRecord
import com.agentplatform.mobile.data.Presence
import com.agentplatform.mobile.data.SessionFilter
import com.agentplatform.mobile.data.SessionRecord
import com.agentplatform.mobile.data.countSessions
import com.agentplatform.mobile.data.filterSessions
import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.delay

private enum class AppSection(val label: String) {
    REMOTE("工作台"),
    SESSIONS("会话机队"),
    WAITING("待审批"),
    DEVICES("主机拓扑"),
    SETTINGS("网关设置"),
}

@Composable
fun AgentPlatformApp(controller: AppController) {
    val state = controller.state
    var section by rememberSaveable {
        mutableStateOf(if (state.config == null) AppSection.SETTINGS else AppSection.REMOTE)
    }
    var selectedDevice by rememberSaveable { mutableStateOf<String?>(null) }

    LaunchedEffect(state.config) {
        if (state.config == null) {
            section = AppSection.SETTINGS
            return@LaunchedEffect
        }
        while (true) {
            delay(5_000)
            controller.refresh()
        }
    }

    val activeFilter =
        if (section == AppSection.WAITING) SessionFilter.WAITING else SessionFilter.ALL
    val detailList =
        remember(state.sessions, activeFilter, selectedDevice) {
            filterSessions(state.sessions, activeFilter, selectedDevice)
        }
    val selected = state.selectedSession
    BackHandler(enabled = selected != null) { controller.selectSession(null) }

    if (selected != null) {
        val index = detailList.indexOfFirst { it.key == selected.key }
        MotionEntrance(
            motionKey = "${selected.key}:${selected.state.activityStatus}",
            modifier = Modifier.fillMaxSize(),
        ) {
            SessionDetailScreen(
                state = state,
                position = index.takeIf { it >= 0 }?.plus(1),
                total = detailList.size,
                onBack = { controller.selectSession(null) },
                onPrevious = detailList.getOrNull(index - 1)?.let { previous ->
                    { controller.selectSession(previous) }
                },
                onNext = detailList.getOrNull(index + 1)?.let { next ->
                    { controller.selectSession(next) }
                },
            )
        }
        return
    }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        contentWindowInsets = WindowInsets.safeDrawing,
        bottomBar = {
            AgentBottomNavBar(
                selected = section,
                waitingCount = countSessions(state.sessions).waiting,
                onSelect = {
                    section = it
                    if (it == AppSection.SESSIONS || it == AppSection.WAITING) {
                        controller.selectSession(null)
                    }
                },
            )
        },
    ) { innerPadding ->
        AnimatedContent(
            targetState = section,
            modifier = Modifier.fillMaxSize(),
            transitionSpec = {
                val direction = if (targetState.ordinal >= initialState.ordinal) 1 else -1
                (
                    fadeIn(tween(MotionTokens.Standard)) +
                        slideInHorizontally(tween(MotionTokens.Emphasized)) { width ->
                            direction * (width / 9)
                        }
                ).togetherWith(
                    fadeOut(tween(MotionTokens.Quick)) +
                        slideOutHorizontally(tween(MotionTokens.Standard)) { width ->
                            -direction * (width / 12)
                        },
                ).using(SizeTransform(clip = false))
            },
            label = "app-section",
        ) { targetSection ->
            when (targetSection) {
                AppSection.REMOTE -> {
                    val cfg = state.config
                    if (cfg != null) {
                        RemoteScreen(cfg, Modifier.padding(innerPadding))
                    } else {
                        UnconfiguredGatewayPrompt(
                            onGoToSettings = { section = AppSection.SETTINGS },
                            modifier = Modifier.padding(innerPadding),
                        )
                    }
                }
                AppSection.SESSIONS,
                AppSection.WAITING ->
                    SessionsScreen(
                        state = state,
                        forcedFilter =
                            if (targetSection == AppSection.WAITING) {
                                SessionFilter.WAITING
                            } else {
                                null
                            },
                        selectedDevice = selectedDevice,
                        onSelectedDevice = { selectedDevice = it },
                        onRefresh = controller::refresh,
                        onSelectSession = controller::selectSession,
                        onNavigateToRemote = { section = AppSection.REMOTE },
                        modifier = Modifier.padding(innerPadding),
                    )
                AppSection.DEVICES ->
                    DevicesScreen(
                        state = state,
                        onRefresh = controller::refresh,
                        onSelectDevice = {
                            selectedDevice = it
                            section = AppSection.SESSIONS
                        },
                        modifier = Modifier.padding(innerPadding),
                    )
                AppSection.SETTINGS ->
                    SettingsScreen(
                        config = state.config,
                        onSave = { config ->
                            controller.saveConfig(config).also { error ->
                                if (error == null) section = AppSection.REMOTE
                            }
                        },
                        onClear = controller::clearConfig,
                        modifier = Modifier.padding(innerPadding),
                    )
            }
        }
    }
}

// =========================================================================
// Unconfigured Gateway Prompt (Orca Cold-Start Guide)
// =========================================================================

@Composable
private fun UnconfiguredGatewayPrompt(
    onGoToSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxSize()
            .padding(20.dp),
        contentAlignment = Alignment.Center,
    ) {
        Surface(
            shape = RoundedCornerShape(16.dp),
            color = MaterialTheme.colorScheme.surface,
            border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.8f)),
            shadowElevation = 2.dp,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(
                modifier = Modifier.padding(24.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    modifier = Modifier
                        .size(48.dp)
                        .background(MaterialTheme.colorScheme.primaryContainer, CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    NavIconRemote(tint = MaterialTheme.colorScheme.primary)
                }

                Text(
                    "进入 Agent 移动控制舱",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                )

                Text(
                    "连接你的私有 Gateway 端点，即可在手机端实时流式观测正在运行的 Claude Code / OpenAI Codex 任务并随时进行权限放行。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    lineHeight = 20.sp,
                )

                Button(
                    onClick = onGoToSettings,
                    modifier = Modifier.fillMaxWidth().height(44.dp),
                    shape = RoundedCornerShape(10.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                ) {
                    Text("前往设置 Gateway 端点 →", fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}

// =========================================================================
// Sessions Screen (Orca High-Density Fleet Overview)
// =========================================================================

@Composable
private fun SessionsScreen(
    state: AppUiState,
    forcedFilter: SessionFilter?,
    selectedDevice: String?,
    onSelectedDevice: (String?) -> Unit,
    onRefresh: () -> Unit,
    onSelectSession: (SessionRecord) -> Unit,
    onNavigateToRemote: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var localFilter by rememberSaveable { mutableStateOf(SessionFilter.ALL) }
    val filter = forcedFilter ?: localFilter
    val sessions =
        remember(state.sessions, filter, selectedDevice) {
            filterSessions(state.sessions, filter, selectedDevice)
        }
    val counts = remember(state.sessions) { countSessions(state.sessions) }
    val hero =
        state.sessions.firstOrNull { it.state.activityStatus.isWaiting }
            ?: state.sessions.firstOrNull { it.state.presence == Presence.LIVE }

    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 14.dp, top = 8.dp, end = 14.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item {
            AgentTopBar(
                title = if (forcedFilter == SessionFilter.WAITING) "待审批拦截" else "智能体会话机队",
                subtitle = state.config?.gatewayBase ?: "未连接",
                loading = state.loading,
                onRefresh = onRefresh,
            )
        }

        state.error?.let { error ->
            item { ErrorBanner(message = error, onRetry = onRefresh) }
        }

        // Active Session Spotlight Card (High Attention)
        if (hero != null) {
            item {
                ActiveSpotlightCard(
                    session = hero,
                    waitingCount = counts.waiting,
                    onClick = { onSelectSession(hero) },
                )
            }
        }

        // Fleet Metric Strip (3-Cell Telemetry)
        item {
            WorkspaceSummaryStrip(state = state)
        }

        // Filter Pills Row
        if (forcedFilter == null) {
            item {
                SessionFilterRow(
                    selected = localFilter,
                    counts = counts,
                    onSelect = { localFilter = it },
                )
            }
        }

        if (selectedDevice != null) {
            item {
                ActiveDeviceFilter(
                    deviceId = selectedDevice,
                    onClear = { onSelectedDevice(null) },
                )
            }
        }

        item {
            Row(
                modifier = Modifier.fillMaxWidth().padding(top = 4.dp, bottom = 2.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "${filter.label} (${sessions.size})",
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontFamily = FontFamily.Monospace,
                )
            }
        }

        if (state.config == null) {
            item {
                EmptyStateCard(
                    title = "未连接 Gateway 网关",
                    body = "请在“网关设置”中配置部署端点与安全令牌，即可实时接管与观测会话。",
                )
            }
        } else if (state.loading && state.sessions.isEmpty()) {
            item { LoadingCard("正在同步智能体机队元数据…") }
        } else if (sessions.isEmpty()) {
            item {
                EmptyStateCard(
                    title = "暂无${filter.label}会话",
                    body = "宿主机运行 Agent 时将实时呈现，后台每 5 秒自动同步状态。",
                )
            }
        } else {
            itemsIndexed(sessions, key = { _, session -> session.key }) { index, session ->
                MotionEntrance(
                    motionKey = "${filter.name}:${session.key}:${session.state.activityStatus}",
                    index = index,
                ) {
                    SessionCard(session = session, onClick = { onSelectSession(session) })
                }
            }
        }
    }
}

// =========================================================================
// Top Bar with Host Pulse Indicator
// =========================================================================

@Composable
private fun AgentTopBar(
    title: String,
    subtitle: String = "",
    loading: Boolean,
    onRefresh: (() -> Unit)?,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    text = "AgentHub",
                    style = MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.onBackground,
                )
                LiveStatusPulse(isActive = !loading)
                if (subtitle.isNotBlank()) {
                    Surface(
                        shape = RoundedCornerShape(4.dp),
                        color = MaterialTheme.colorScheme.surfaceVariant,
                    ) {
                        Text(
                            text = subtitle.removePrefix("http://").removePrefix("https://"),
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 5.dp, vertical = 2.dp),
                            fontSize = 10.sp,
                        )
                    }
                }
            }
            Text(
                text = title,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        if (onRefresh != null) {
            Surface(
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.7f),
                border = androidx.compose.foundation.BorderStroke(
                    0.5.dp,
                    MaterialTheme.colorScheme.outline.copy(alpha = 0.5f),
                ),
                modifier = Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .clickable(enabled = !loading, onClick = onRefresh),
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    if (loading) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(12.dp),
                            strokeWidth = 1.6.dp,
                            color = MaterialTheme.colorScheme.primary,
                        )
                        Text(
                            "同步",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.primary,
                        )
                    } else {
                        RefreshIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text(
                            "刷新",
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun LiveStatusPulse(isActive: Boolean) {
    val transition = rememberInfiniteTransition(label = "pulse")
    val alpha by transition.animateFloat(
        initialValue = 0.35f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1100),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "dot-alpha",
    )
    Box(
        Modifier
            .size(7.dp)
            .graphicsLayer { this.alpha = if (isActive) alpha else 0.4f }
            .background(if (isActive) StatusGreenBright else StatusAmber, CircleShape),
    )
}

// =========================================================================
// Active Spotlight Card (Orca Attention Hero)
// =========================================================================

@Composable
private fun ActiveSpotlightCard(
    session: SessionRecord,
    waitingCount: Int,
    onClick: () -> Unit,
) {
    val isWaiting = session.state.activityStatus.isWaiting
    val isWorking = session.state.activityStatus == ActivityStatus.WORKING
    val accentColor = if (isWaiting) StatusAmber else if (isWorking) StatusBlue else StatusGreenBright

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .clickable(onClick = onClick)
            .border(
                1.dp,
                accentColor.copy(alpha = 0.4f),
                RoundedCornerShape(12.dp),
            ),
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = 1.5.dp,
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
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
                    Box(
                        Modifier
                            .size(7.dp)
                            .background(accentColor, CircleShape),
                    )
                    Text(
                        text = if (isWaiting) "需要人类介入审批" else if (isWorking) "Agent 正在工作流中" else "活跃任务",
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = FontWeight.Bold,
                        color = accentColor,
                    )
                }
                Text(
                    text = relativeTime(session.observedAt),
                    style = MaterialTheme.typography.bodySmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            Text(
                text = session.project.displayName,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )

            HorizontalDivider(
                color = MaterialTheme.colorScheme.outline.copy(alpha = 0.4f),
                thickness = 0.5.dp,
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "${session.agentType.label} · ${session.state.activityStatus.label}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = if (isWaiting && waitingCount > 1) "查看 ($waitingCount 条审批) →" else "进入控制台 →",
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }
    }
}

// =========================================================================
// Summary Telemetry Strip
// =========================================================================

@Composable
private fun WorkspaceSummaryStrip(state: AppUiState) {
    val activeDevices = state.devices.count { !it.revoked }
    val onlineDevices = state.devices.count { !it.revoked && it.online }
    val counts = countSessions(state.sessions)

    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        CompactStatBadge(
            label = "在线主机",
            value = "$onlineDevices / $activeDevices",
            active = onlineDevices > 0,
            modifier = Modifier.weight(1f),
        )
        CompactStatBadge(
            label = "活跃智能体",
            value = "${counts.live}",
            active = counts.live > 0,
            modifier = Modifier.weight(1f),
        )
        CompactStatBadge(
            label = "待处理审批",
            value = "${counts.waiting}",
            active = counts.waiting > 0,
            highlight = counts.waiting > 0,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun CompactStatBadge(
    label: String,
    value: String,
    active: Boolean,
    modifier: Modifier = Modifier,
    highlight: Boolean = false,
) {
    val borderColor = if (highlight) StatusAmber.copy(alpha = 0.5f) else MaterialTheme.colorScheme.outline.copy(alpha = 0.5f)
    val bgColor = if (highlight) StatusAmber.copy(alpha = 0.08f) else MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.45f)

    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(8.dp),
        color = bgColor,
        border = androidx.compose.foundation.BorderStroke(0.5.dp, borderColor),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 9.dp, vertical = 6.dp),
            verticalArrangement = Arrangement.spacedBy(1.dp),
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelSmall,
                color = if (highlight) StatusAmber else MaterialTheme.colorScheme.onSurfaceVariant,
                fontSize = 11.sp,
            )
            Text(
                text = value,
                style = MaterialTheme.typography.titleSmall,
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Bold,
                color = if (highlight) StatusAmber else if (active) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// =========================================================================
// Filter Pills Row
// =========================================================================

@Composable
private fun SessionFilterRow(
    selected: SessionFilter,
    counts: com.agentplatform.mobile.data.SessionCounts,
    onSelect: (SessionFilter) -> Unit,
) {
    LazyRow(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        items(SessionFilter.entries) { filter ->
            val count = when (filter) {
                SessionFilter.ALL -> counts.all
                SessionFilter.WAITING -> counts.waiting
                SessionFilter.LIVE -> counts.live
                SessionFilter.FINISHED -> counts.finished
                SessionFilter.HISTORY -> counts.history
            }
            val active = selected == filter
            Surface(
                shape = RoundedCornerShape(8.dp),
                color = if (active) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surface,
                border = androidx.compose.foundation.BorderStroke(
                    0.5.dp,
                    if (active) MaterialTheme.colorScheme.primary.copy(alpha = 0.4f) else MaterialTheme.colorScheme.outline.copy(alpha = 0.5f),
                ),
                modifier = Modifier
                    .clip(RoundedCornerShape(8.dp))
                    .clickable { onSelect(filter) },
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp),
                ) {
                    Text(
                        text = filter.label,
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = if (active) FontWeight.Bold else FontWeight.Normal,
                        color = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        text = "$count",
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                        color = if (active) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                    )
                }
            }
        }
    }
}

// =========================================================================
// Session Card (Industrial High-Precision Card with Left Color Indicator)
// =========================================================================

@Composable
private fun SessionCard(session: SessionRecord, onClick: () -> Unit) {
    val status = session.state.activityStatus
    val color = statusColor(status)

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick)
            .border(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.7f), RoundedCornerShape(10.dp)),
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = 0.5.dp,
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(IntrinsicSize.Min),
        ) {
            // Left Status Indicator Bar (Orca style)
            Box(
                modifier = Modifier
                    .width(3.5.dp)
                    .fillMaxHeight()
                    .background(color),
            )

            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    AgentBadge(session.agentType)
                    Text(
                        text = relativeTime(session.observedAt),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.sp,
                    )
                }

                Text(
                    text = session.project.displayName,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.Bold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(
                            Modifier
                                .size(6.dp)
                                .background(color, CircleShape),
                        )
                        Spacer(Modifier.width(5.dp))
                        Text(
                            text = status.label,
                            color = color,
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                    Text(
                        text = "主机 ${shortId(session.deviceId)}",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }
        }
    }
}

@Composable
private fun AgentBadge(type: AgentType) {
    val dotColor = when (type) {
        AgentType.CLAUDE_CODE -> Color(0xFFC86646)
        AgentType.CODEX -> Color(0xFF0284C7)
        AgentType.ANTIGRAVITY -> Color(0xFF10B981)
        AgentType.GROK_BUILD -> Color(0xFF8B5CF6)
        AgentType.UNKNOWN -> Color(0xFF64748B)
    }

    Surface(
        shape = RoundedCornerShape(4.dp),
        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Box(Modifier.size(5.dp).background(dotColor, CircleShape))
            Text(
                type.label,
                style = MaterialTheme.typography.labelSmall,
                fontWeight = FontWeight.Medium,
                fontSize = 11.sp,
            )
        }
    }
}

// =========================================================================
// Modern Bottom Navigation Bar (Vector Icons, Minimal Pill)
// =========================================================================

@Composable
private fun AgentBottomNavBar(
    selected: AppSection,
    waitingCount: Int,
    onSelect: (AppSection) -> Unit,
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .navigationBarsPadding(),
        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.98f),
        border = androidx.compose.foundation.BorderStroke(
            0.5.dp,
            MaterialTheme.colorScheme.outline.copy(alpha = 0.4f),
        ),
        shadowElevation = 8.dp,
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 4.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.SpaceAround,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            AppSection.entries.forEach { section ->
                val active = section == selected
                val tint = if (active) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant

                Column(
                    modifier = Modifier
                        .weight(1f)
                        .clip(RoundedCornerShape(8.dp))
                        .clickable { onSelect(section) }
                        .padding(vertical = 4.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        when (section) {
                            AppSection.REMOTE -> NavIconRemote(tint = tint)
                            AppSection.SESSIONS -> NavIconSessions(tint = tint)
                            AppSection.WAITING -> NavIconWaiting(tint = tint)
                            AppSection.DEVICES -> NavIconDevices(tint = tint)
                            AppSection.SETTINGS -> NavIconSettings(tint = tint)
                        }

                        if (section == AppSection.WAITING && waitingCount > 0) {
                            Box(
                                modifier = Modifier
                                    .align(Alignment.TopEnd)
                                    .padding(start = 12.dp, bottom = 8.dp)
                                    .size(7.dp)
                                    .background(StatusAmber, CircleShape),
                            )
                        }
                    }

                    Text(
                        text = section.label,
                        style = MaterialTheme.typography.labelSmall,
                        fontWeight = if (active) FontWeight.Bold else FontWeight.Normal,
                        color = tint,
                        fontSize = 10.5.sp,
                    )
                }
            }
        }
    }
}

// Vector Navigation Icons
@Composable
private fun NavIconRemote(tint: Color) {
    Canvas(modifier = Modifier.size(19.dp)) {
        val stroke = 1.8.dp.toPx()
        val path = Path().apply {
            moveTo(size.width * 0.22f, size.height * 0.30f)
            lineTo(size.width * 0.46f, size.height * 0.50f)
            lineTo(size.width * 0.22f, size.height * 0.70f)
        }
        drawPath(path, color = tint, style = Stroke(width = stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
        drawLine(
            color = tint,
            start = Offset(size.width * 0.54f, size.height * 0.70f),
            end = Offset(size.width * 0.78f, size.height * 0.70f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
private fun NavIconSessions(tint: Color) {
    Canvas(modifier = Modifier.size(19.dp)) {
        val stroke = 1.8.dp.toPx()
        val w = size.width
        val h = size.height
        drawRoundRect(
            color = tint,
            topLeft = Offset(w * 0.15f, h * 0.18f),
            size = Size(w * 0.70f, h * 0.56f),
            cornerRadius = CornerRadius(3.dp.toPx(), 3.dp.toPx()),
            style = Stroke(width = stroke),
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.30f, h * 0.38f),
            end = Offset(w * 0.70f, h * 0.38f),
            strokeWidth = 1.4.dp.toPx(),
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.30f, h * 0.52f),
            end = Offset(w * 0.54f, h * 0.52f),
            strokeWidth = 1.4.dp.toPx(),
            cap = StrokeCap.Round,
        )
    }
}

@Composable
private fun NavIconWaiting(tint: Color) {
    Canvas(modifier = Modifier.size(19.dp)) {
        val stroke = 1.8.dp.toPx()
        val center = Offset(size.width * 0.5f, size.height * 0.5f)
        val radius = size.width * 0.38f
        drawCircle(
            color = tint,
            radius = radius,
            center = center,
            style = Stroke(width = stroke),
        )
        drawLine(
            color = tint,
            start = center,
            end = Offset(center.x, center.y - radius * 0.54f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = center,
            end = Offset(center.x + radius * 0.44f, center.y),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
private fun NavIconDevices(tint: Color) {
    Canvas(modifier = Modifier.size(19.dp)) {
        val stroke = 1.8.dp.toPx()
        val w = size.width
        val h = size.height
        drawRoundRect(
            color = tint,
            topLeft = Offset(w * 0.20f, h * 0.22f),
            size = Size(w * 0.60f, h * 0.44f),
            cornerRadius = CornerRadius(2.dp.toPx(), 2.dp.toPx()),
            style = Stroke(width = stroke),
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.12f, h * 0.74f),
            end = Offset(w * 0.88f, h * 0.74f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
private fun NavIconSettings(tint: Color) {
    Canvas(modifier = Modifier.size(19.dp)) {
        val stroke = 1.8.dp.toPx()
        val w = size.width
        val h = size.height
        drawLine(
            color = tint,
            start = Offset(w * 0.18f, h * 0.36f),
            end = Offset(w * 0.82f, h * 0.36f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawCircle(
            color = tint,
            radius = 2.4.dp.toPx(),
            center = Offset(w * 0.38f, h * 0.36f),
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.18f, h * 0.64f),
            end = Offset(w * 0.82f, h * 0.64f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawCircle(
            color = tint,
            radius = 2.4.dp.toPx(),
            center = Offset(w * 0.62f, h * 0.64f),
        )
    }
}

@Composable
private fun RefreshIcon(tint: Color) {
    Canvas(modifier = Modifier.size(12.dp)) {
        val stroke = 1.5.dp.toPx()
        drawArc(
            color = tint,
            startAngle = 30f,
            sweepAngle = 280f,
            useCenter = false,
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
    }
}

// =========================================================================
// Devices Screen
// =========================================================================

@Composable
private fun DevicesScreen(
    state: AppUiState,
    onRefresh: () -> Unit,
    onSelectDevice: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(14.dp, 8.dp, 14.dp, 24.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item {
            AgentTopBar(
                title = "已接入宿主机与 Runner 拓扑",
                subtitle = state.config?.gatewayBase ?: "",
                loading = state.loading,
                onRefresh = onRefresh,
            )
        }
        state.error?.let { item { ErrorBanner(it, onRefresh) } }

        val devices = state.devices.filter { !it.revoked }
        if (devices.isEmpty()) {
            item {
                EmptyStateCard(
                    "暂未发现活动主机",
                    "在你的电脑上运行 agenthub-runner daemon，设备将自动接入拓扑网格。",
                )
            }
        } else {
            itemsIndexed(devices, key = { _, device -> device.deviceId }) { index, device ->
                val deviceSessions = state.sessions.filter { it.deviceId == device.deviceId }
                MotionEntrance(
                    motionKey = "${device.deviceId}:${device.online}",
                    index = index,
                ) {
                    DeviceCard(
                        device = device,
                        sessions = deviceSessions,
                        onClick = { onSelectDevice(device.deviceId) },
                    )
                }
            }
        }
    }
}

@Composable
private fun DeviceCard(
    device: DeviceRecord,
    sessions: List<SessionRecord>,
    onClick: () -> Unit,
) {
    val agents = sessions.map(SessionRecord::agentType).distinct().joinToString(" · ") { it.label }
    val isOnline = device.online

    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(10.dp))
            .clickable(onClick = onClick)
            .border(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.7f), RoundedCornerShape(10.dp)),
        color = MaterialTheme.colorScheme.surface,
        shadowElevation = 0.5.dp,
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Box(
                        Modifier
                            .size(7.dp)
                            .background(if (isOnline) StatusGreenBright else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f), CircleShape),
                    )
                    Text(
                        text = "宿主机 ${shortId(device.deviceId)}",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                    )
                }
                Surface(
                    shape = RoundedCornerShape(4.dp),
                    color = if (isOnline) StatusGreenBright.copy(alpha = 0.12f) else MaterialTheme.colorScheme.surfaceVariant,
                ) {
                    Text(
                        text = if (isOnline) "在线接入" else "离线",
                        color = if (isOnline) StatusGreen else MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                    )
                }
            }

            Text(
                text = "ID: ${shortId(device.deviceId)} · ${if (agents.isNotBlank()) agents else "未启动 Agent"}",
                style = MaterialTheme.typography.bodySmall,
                fontFamily = FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            HorizontalDivider(
                color = MaterialTheme.colorScheme.outline.copy(alpha = 0.3f),
                thickness = 0.5.dp,
            )

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "${sessions.size} 个活动会话",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = "查看此机队 →",
                    style = MaterialTheme.typography.labelSmall,
                    fontWeight = FontWeight.Bold,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
        }
    }
}

// =========================================================================
// Settings Screen
// =========================================================================

@Composable
private fun SettingsScreen(
    config: AppConfig?,
    onSave: (AppConfig) -> String?,
    onClear: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var gatewayBase by rememberSaveable {
        mutableStateOf(config?.gatewayBase ?: "")
    }
    var accessToken by rememberSaveable {
        mutableStateOf(config?.accessToken ?: "")
    }
    var developmentUserId by rememberSaveable {
        mutableStateOf(config?.developmentUserId ?: "")
    }
    var error by remember { mutableStateOf<String?>(null) }

    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(14.dp, 8.dp, 14.dp, 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            AgentTopBar(
                title = "网关安全连接与端点配置",
                loading = false,
                onRefresh = null,
            )
        }

        item {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                color = MaterialTheme.colorScheme.surface,
                border = androidx.compose.foundation.BorderStroke(
                    0.5.dp,
                    MaterialTheme.colorScheme.outline.copy(alpha = 0.7f),
                ),
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(
                        "连接凭证与网关",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                    )

                    OutlinedTextField(
                        value = gatewayBase,
                        onValueChange = { gatewayBase = it },
                        modifier = Modifier.fillMaxWidth(),
                        label = { Text("Gateway 端点") },
                        placeholder = { Text("http://127.0.0.1:3001") },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                        shape = RoundedCornerShape(8.dp),
                    )

                    // Quick-fill address pills for faster developer workflow
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Surface(
                            onClick = {
                                gatewayBase = "http://127.0.0.1:3001"
                                developmentUserId = "local-developer"
                                error = null
                            },
                            shape = RoundedCornerShape(4.dp),
                            color = MaterialTheme.colorScheme.surfaceVariant,
                        ) {
                            Text(
                                "127.0.0.1:3001",
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = FontFamily.Monospace,
                                color = MaterialTheme.colorScheme.primary,
                                modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                            )
                        }
                        Surface(
                            onClick = {
                                gatewayBase = "http://10.0.2.2:3001"
                                developmentUserId = "local-developer"
                                error = null
                            },
                            shape = RoundedCornerShape(4.dp),
                            color = MaterialTheme.colorScheme.surfaceVariant,
                        ) {
                            Text(
                                "10.0.2.2 (模拟器)",
                                style = MaterialTheme.typography.labelSmall,
                                fontFamily = FontFamily.Monospace,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
                            )
                        }
                    }

                    OutlinedTextField(
                        value = accessToken,
                        onValueChange = { accessToken = it },
                        modifier = Modifier.fillMaxWidth(),
                        label = { Text("访问令牌 (Access Token)") },
                        singleLine = true,
                        visualTransformation = PasswordVisualTransformation(),
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                        shape = RoundedCornerShape(8.dp),
                    )

                    OutlinedTextField(
                        value = developmentUserId,
                        onValueChange = { developmentUserId = it },
                        modifier = Modifier.fillMaxWidth(),
                        label = { Text("开发用户 ID（仅回环地址有效，选填）") },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                        shape = RoundedCornerShape(8.dp),
                    )

                    error?.let {
                        Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                    }

                    Button(
                        onClick = {
                            error = onSave(
                                AppConfig(
                                    gatewayBase = gatewayBase,
                                    accessToken = accessToken,
                                    developmentUserId = developmentUserId,
                                ),
                            )
                        },
                        modifier = Modifier.fillMaxWidth().height(44.dp),
                        shape = RoundedCornerShape(8.dp),
                        colors = ButtonDefaults.buttonColors(containerColor = MaterialTheme.colorScheme.primary),
                    ) {
                        Text(
                            text = if (config == null) "保存并连网关" else "更新连接配置",
                            fontWeight = FontWeight.SemiBold,
                        )
                    }

                    if (config != null) {
                        TextButton(
                            onClick = onClear,
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("断开并清除本机配置", color = MaterialTheme.colorScheme.error)
                        }
                    }
                }
            }
        }

        item {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(10.dp),
                color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
            ) {
                Text(
                    text = "提示：手机端通过安全的 Gateway 只读连接或交互式远程代理接入电脑，凭据存储受 Android Keystore 安全硬件保护，不写日志，不参与备份。",
                    modifier = Modifier.padding(14.dp),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    lineHeight = 18.sp,
                )
            }
        }
    }
}

// =========================================================================
// Session Detail Screen
// =========================================================================

@Composable
private fun SessionDetailScreen(
    state: AppUiState,
    position: Int?,
    total: Int,
    onBack: () -> Unit,
    onPrevious: (() -> Unit)?,
    onNext: (() -> Unit)?,
) {
    val session = state.selectedSession ?: return

    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding(),
        contentPadding = PaddingValues(14.dp, 8.dp, 14.dp, 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextButton(onClick = onBack) {
                    Text("← 返回列表", fontWeight = FontWeight.SemiBold)
                }
                if (position != null && total > 1) {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        TextButton(onClick = { onPrevious?.invoke() }, enabled = onPrevious != null) {
                            Text("上一条")
                        }
                        TextButton(onClick = { onNext?.invoke() }, enabled = onNext != null) {
                            Text("下一条")
                        }
                    }
                }
            }
        }

        item {
            Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Text(
                    session.project.displayName,
                    style = MaterialTheme.typography.headlineMedium,
                    fontWeight = FontWeight.Bold,
                )
                Text(
                    "${session.agentType.label} · 宿主机 ${shortId(session.deviceId)}",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }

        item {
            Surface(
                modifier = Modifier
                    .fillMaxWidth()
                    .border(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.7f), RoundedCornerShape(12.dp)),
                shape = RoundedCornerShape(12.dp),
                color = MaterialTheme.colorScheme.surface,
            ) {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("会话状态", style = MaterialTheme.typography.labelSmall)
                        val color = statusColor(session.state.activityStatus)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.size(7.dp).background(color, CircleShape))
                            Spacer(Modifier.width(5.dp))
                            Text(
                                session.state.activityStatus.label,
                                color = color,
                                style = MaterialTheme.typography.labelSmall,
                                fontWeight = FontWeight.Bold,
                            )
                        }
                    }

                    HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.3f), thickness = 0.5.dp)
                    DetailFactRow("Presence", presenceLabel(session.state.presence))
                    DetailFactRow("最近观测", absoluteTime(session.observedAt))
                    DetailFactRow("Session ID", session.agentSessionId)
                    DetailFactRow("工作目录", session.project.displayName)
                }
            }
        }
    }
}

@Composable
private fun DetailFactRow(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(
            value,
            style = MaterialTheme.typography.bodySmall,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Medium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(start = 12.dp),
        )
    }
}

// =========================================================================
// Helpers and Common Cards
// =========================================================================

@Composable
private fun ErrorBanner(message: String, onRetry: () -> Unit) {
    Surface(
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.error.copy(alpha = 0.10f),
        border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.error.copy(alpha = 0.4f)),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                message,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = onRetry) {
                Text("重试", color = MaterialTheme.colorScheme.error, fontWeight = FontWeight.Bold)
            }
        }
    }
}

@Composable
private fun EmptyStateCard(title: String, body: String) {
    Surface(
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surface,
        border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.6f)),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                title,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.Bold,
            )
            Text(
                body,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 18.sp,
            )
        }
    }
}

@Composable
private fun LoadingCard(message: String) {
    Surface(
        shape = RoundedCornerShape(10.dp),
        color = MaterialTheme.colorScheme.surface,
        border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f)),
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            CircularProgressIndicator(
                modifier = Modifier.size(16.dp),
                strokeWidth = 2.dp,
                color = MaterialTheme.colorScheme.primary,
            )
            Text(
                message,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun ActiveDeviceFilter(deviceId: String, onClear: () -> Unit) {
    Surface(
        shape = RoundedCornerShape(6.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = "过滤设备: ${shortId(deviceId)}",
                style = MaterialTheme.typography.labelSmall,
                fontFamily = FontFamily.Monospace,
            )
            Text(
                text = "清除 ✕",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.clickable(onClick = onClear),
            )
        }
    }
}

private fun shortId(id: String): String = id.takeLast(8)

private fun statusColor(status: ActivityStatus): Color = when (status) {
    ActivityStatus.WAITING_PERMISSION,
    ActivityStatus.WAITING_INPUT -> StatusAmber
    ActivityStatus.WORKING -> StatusBlue
    ActivityStatus.COMPLETED -> StatusGreen
    ActivityStatus.FAILED -> StatusRed
    ActivityStatus.STOPPED,
    ActivityStatus.IDLE,
    ActivityStatus.UNKNOWN -> Color(0xFF64748B)
}

private fun presenceLabel(presence: Presence): String = when (presence) {
    Presence.LIVE -> "实时在线"
    Presence.PERSISTED -> "离线历史"
    Presence.UNKNOWN -> "未知状态"
}

private fun relativeTime(instant: Instant): String {
    val duration = Duration.between(instant, Instant.now())
    return when {
        duration.isNegative -> "刚刚"
        duration.toMinutes() < 1 -> "刚刚"
        duration.toHours() < 1 -> "${duration.toMinutes()} 分钟前"
        duration.toDays() < 1 -> "${duration.toHours()} 小时前"
        else -> "${duration.toDays()} 天前"
    }
}

private fun absoluteTime(instant: Instant): String =
    DateTimeFormatter.ofPattern("yyyy-MM-dd HH:mm:ss")
        .withZone(ZoneId.systemDefault())
        .format(instant)
