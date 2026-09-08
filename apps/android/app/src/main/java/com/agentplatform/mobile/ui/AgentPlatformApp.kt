package com.agentplatform.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.LinearEasing
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
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
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

private enum class AppSection(val label: String, val mark: String) {
    REMOTE("远程", "↗"),
    SESSIONS("全部", "□"),
    WAITING("待处理", "✓"),
    DEVICES("设备", "▣"),
    SETTINGS("设置", "○"),
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
            FloatingDock(
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
                AppSection.REMOTE -> state.config?.let { config ->
                    RemoteScreen(config, Modifier.padding(innerPadding))
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

@Composable
private fun SessionsScreen(
    state: AppUiState,
    forcedFilter: SessionFilter?,
    selectedDevice: String?,
    onSelectedDevice: (String?) -> Unit,
    onRefresh: () -> Unit,
    onSelectSession: (SessionRecord) -> Unit,
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
        contentPadding = PaddingValues(start = 20.dp, top = 18.dp, end = 20.dp, bottom = 28.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item {
            ProductHeader(
                title = if (forcedFilter == SessionFilter.WAITING) "待处理" else "会话",
                subtitle =
                    if (forcedFilter == SessionFilter.WAITING) {
                        "需要你回到电脑处理的 Coding Agent 会话。"
                    } else {
                        "跨设备查看 Claude Code 与 Codex 的实时状态。"
                    },
                loading = state.loading,
                onRefresh = onRefresh,
            )
        }
        state.error?.let { error ->
            item { ErrorBanner(message = error, onRetry = onRefresh) }
        }
        item {
            CurrentSessionHero(
                session = hero,
                waitingCount = counts.waiting,
                onClick = hero?.let { { onSelectSession(it) } },
            )
        }
        item {
            MetricsGrid(state = state)
        }
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
            SectionTitle(
                title = "${filter.label}会话",
                count = sessions.size,
            )
        }
        if (state.config == null) {
            item {
                EmptyCard(
                    title = "先连接 Gateway",
                    body = "在设置中填写部署地址与访问令牌，原生客户端就会读取同一份只读会话数据。",
                )
            }
        } else if (state.loading && state.sessions.isEmpty()) {
            item { LoadingCard("正在读取 Session…") }
        } else if (sessions.isEmpty()) {
            item {
                EmptyCard(
                    title = "当前没有${filter.label}会话",
                    body = "数据会每 5 秒自动刷新，你也可以点击右上角立即同步。",
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

@Composable
private fun ProductHeader(
    title: String,
    subtitle: String,
    loading: Boolean,
    onRefresh: (() -> Unit)?,
) {
    Column(verticalArrangement = Arrangement.spacedBy(26.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                "AgentHub",
                style = MaterialTheme.typography.titleLarge,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (onRefresh != null) {
                    TextButton(onClick = onRefresh, enabled = !loading) {
                        Text(if (loading) "同步中" else "刷新")
                    }
                }
                BrandMark(54.dp)
            }
        }
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(title, style = MaterialTheme.typography.displayLarge)
            Text(
                subtitle,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodyLarge,
            )
        }
    }
}

@Composable
private fun BrandMark(size: androidx.compose.ui.unit.Dp) {
    val ink = MaterialTheme.colorScheme.onBackground
    val transition = rememberInfiniteTransition(label = "brand-motion")
    val rotation by
        transition.animateFloat(
            initialValue = 0f,
            targetValue = 360f,
            animationSpec =
                infiniteRepeatable(
                    animation = tween(22_000, easing = LinearEasing),
                    repeatMode = RepeatMode.Restart,
                ),
            label = "brand-rotation",
        )
    val signalScale by
        transition.animateFloat(
            initialValue = 1f,
            targetValue = 1.45f,
            animationSpec =
                infiniteRepeatable(
                    animation = tween(1_200),
                    repeatMode = RepeatMode.Reverse,
                ),
            label = "brand-signal",
        )
    Canvas(
        Modifier
            .size(size)
            .graphicsLayer { rotationZ = rotation },
    ) {
        val stroke = this.size.minDimension * 0.075f
        drawArc(
            color = ink,
            startAngle = 48f,
            sweepAngle = 276f,
            useCenter = false,
            topLeft = Offset(stroke, stroke),
            size = androidx.compose.ui.geometry.Size(
                width = this.size.width - stroke * 2,
                height = this.size.height - stroke * 2,
            ),
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
        drawCircle(
            color = PinkSignal,
            radius = this.size.minDimension * 0.075f * signalScale,
            center = Offset(this.size.width * 0.79f, this.size.height * 0.29f),
        )
        drawCircle(
            color = ink,
            radius = this.size.minDimension * 0.045f,
            center = Offset(this.size.width * 0.83f, this.size.height * 0.69f),
        )
    }
}

@Composable
private fun CurrentSessionHero(
    session: SessionRecord?,
    waitingCount: Int,
    onClick: (() -> Unit)?,
) {
    Surface(
        modifier =
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(32.dp))
                .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(32.dp)),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    "CURRENT STATUS",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.labelSmall,
                )
                PinkDot()
            }
            if (session == null) {
                Text("等待第一条会话", style = MaterialTheme.typography.headlineMedium)
                Text(
                    "Runner 上报后，这里会优先显示正在工作或等待你的会话。",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
                    Text(
                        if (session.state.activityStatus.isWaiting) {
                            "当前需要处理"
                        } else {
                            "当前正在继续"
                        },
                        style = MaterialTheme.typography.labelLarge,
                    )
                    Text(
                        session.project.displayName,
                        style = MaterialTheme.typography.headlineMedium,
                    )
                    Text(
                        "${session.agentType.label} · ${session.state.activityStatus.label}",
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        if (waitingCount > 0) "$waitingCount 条等待处理" else "暂时没有阻塞",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text("查看  →", fontWeight = FontWeight.Bold)
                }
            }
        }
    }
}

@Composable
private fun MetricsGrid(state: AppUiState) {
    val activeDevices = state.devices.count { !it.revoked }
    val onlineDevices = state.devices.count { !it.revoked && it.online }
    val counts = countSessions(state.sessions)
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            MetricCard(
                label = "在线设备",
                value = "$onlineDevices / $activeDevices",
                mark = "▣",
                modifier = Modifier.weight(1f),
            )
            MetricCard(
                label = "活动会话",
                value = counts.live.toString(),
                mark = "∿",
                modifier = Modifier.weight(1f),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            MetricCard(
                label = "等待处理",
                value = counts.waiting.toString(),
                mark = "•",
                signal = counts.waiting > 0,
                modifier = Modifier.weight(1f),
            )
            MetricCard(
                label = "最近完成",
                value = counts.finished.toString(),
                mark = "✓",
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun MetricCard(
    label: String,
    value: String,
    mark: String,
    modifier: Modifier = Modifier,
    signal: Boolean = false,
) {
    Surface(
        modifier =
            modifier
                .height(132.dp)
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(26.dp)),
        shape = RoundedCornerShape(26.dp),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            modifier = Modifier.padding(18.dp),
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(label, style = MaterialTheme.typography.titleMedium)
                Text(mark, fontSize = 24.sp, fontWeight = FontWeight.Black)
            }
            Row(verticalAlignment = Alignment.Bottom) {
                AnimatedMetricText(value, style = MaterialTheme.typography.headlineLarge)
                if (signal) {
                    Spacer(Modifier.width(7.dp))
                    PinkDot()
                }
            }
        }
    }
}

@Composable
private fun SessionFilterRow(
    selected: SessionFilter,
    counts: com.agentplatform.mobile.data.SessionCounts,
    onSelect: (SessionFilter) -> Unit,
) {
    LazyRow(horizontalArrangement = Arrangement.spacedBy(9.dp)) {
        items(SessionFilter.entries) { filter ->
            val active = filter == selected
            MotionSelection(active = active) {
                Surface(
                    modifier = Modifier.clickable { onSelect(filter) },
                    shape = RoundedCornerShape(999.dp),
                    color =
                        if (active) {
                            MaterialTheme.colorScheme.primary
                        } else {
                            MaterialTheme.colorScheme.surface
                        },
                    contentColor =
                        if (active) {
                            MaterialTheme.colorScheme.onPrimary
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        },
                    border =
                        if (active) null
                        else androidx.compose.foundation.BorderStroke(
                            1.dp,
                            MaterialTheme.colorScheme.outline,
                        ),
                ) {
                    Text(
                        "${filter.label} ${counts[filter]}",
                        modifier = Modifier.padding(horizontal = 17.dp, vertical = 11.dp),
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
            }
        }
    }
}

@Composable
private fun ActiveDeviceFilter(deviceId: String, onClear: () -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        color = SoftPink.copy(alpha = 0.62f),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("设备 ${shortId(deviceId)}", fontWeight = FontWeight.Bold)
            TextButton(onClick = onClear) { Text("清除") }
        }
    }
}

@Composable
private fun SectionTitle(title: String, count: Int) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(title, style = MaterialTheme.typography.headlineMedium)
            Spacer(Modifier.width(8.dp))
            PinkDot()
        }
        Text(
            "$count 条",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyLarge,
        )
    }
}

@Composable
private fun SessionCard(session: SessionRecord, onClick: () -> Unit) {
    Surface(
        modifier =
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(26.dp))
                .clickable(onClick = onClick)
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(26.dp)),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(15.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                AgentPill(session.agentType)
                Text(
                    relativeTime(session.observedAt),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            Text(
                session.project.displayName,
                style = MaterialTheme.typography.titleLarge,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StatusPill(session)
                Text(
                    "设备 ${shortId(session.deviceId)}  →",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}

@Composable
private fun AgentPill(type: AgentType) {
    Surface(
        shape = RoundedCornerShape(999.dp),
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Text(
            type.label,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
            style = MaterialTheme.typography.labelLarge,
        )
    }
}

@Composable
private fun StatusPill(session: SessionRecord) {
    val color = statusColor(session.state.activityStatus)
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(
            Modifier
                .size(8.dp)
                .background(color, CircleShape),
        )
        Spacer(Modifier.width(7.dp))
        Text(
            session.state.activityStatus.label,
            color = color,
            fontWeight = FontWeight.Bold,
        )
    }
}

@Composable
private fun DevicesScreen(
    state: AppUiState,
    onRefresh: () -> Unit,
    onSelectDevice: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(20.dp, 18.dp, 20.dp, 28.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        item {
            ProductHeader(
                title = "设备",
                subtitle = "查看每台电脑最近上报的状态与已观测 Agent。",
                loading = state.loading,
                onRefresh = onRefresh,
            )
        }
        state.error?.let { item { ErrorBanner(it, onRefresh) } }
        item { SectionTitle("设备列表", state.devices.count { !it.revoked }) }
        val devices = state.devices.filter { !it.revoked }
        if (devices.isEmpty()) {
            item {
                EmptyCard(
                    "还没有设备",
                    "在电脑上运行 Runner 并完成绑定后，设备会出现在这里。",
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
    Surface(
        modifier =
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(26.dp))
                .clickable(onClick = onClick)
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(26.dp)),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            modifier = Modifier.padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("设备 ${shortId(device.deviceId)}", style = MaterialTheme.typography.titleLarge)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(9.dp)
                            .background(
                                if (device.online) SuccessGreen else MaterialTheme.colorScheme.outline,
                                CircleShape,
                            ),
                    )
                    Spacer(Modifier.width(7.dp))
                    Text(if (device.online) "在线" else "离线")
                }
            }
            Text(
                agents.ifBlank { "尚未观测到 Agent" },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            HorizontalDivider(color = MaterialTheme.colorScheme.outline)
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text("${sessions.size} 条会话")
                Text("${sessions.count { it.state.activityStatus.isWaiting }} 条待处理  →")
            }
        }
    }
}

@Composable
private fun SettingsScreen(
    config: AppConfig?,
    onSave: (AppConfig) -> String?,
    onClear: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val initial = config ?: AppConfig.LocalDevelopment
    var gatewayBase by remember(config) { mutableStateOf(initial.gatewayBase) }
    var accessToken by remember(config) { mutableStateOf(initial.accessToken) }
    var developmentUserId by remember(config) { mutableStateOf(initial.developmentUserId) }
    var error by remember { mutableStateOf<String?>(null) }

    Column(
        modifier =
            modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(20.dp, 18.dp, 20.dp, 32.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        ProductHeader(
            title = "设置",
            subtitle = "连接你的 AgentHub Gateway，凭据只保存在本机。",
            loading = false,
            onRefresh = null,
        )
        Surface(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(30.dp)),
            shape = RoundedCornerShape(30.dp),
            color = MaterialTheme.colorScheme.surface,
        ) {
            Column(
                modifier = Modifier.padding(22.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text("GATEWAY CONNECTION", style = MaterialTheme.typography.labelSmall)
                Text("连接同一份只读数据", style = MaterialTheme.typography.headlineMedium)
                Text(
                    "生产部署必须使用 HTTPS。访问令牌使用 Android Keystore 加密，不写日志、不参与备份。",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OutlinedTextField(
                    value = gatewayBase,
                    onValueChange = { gatewayBase = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("Gateway 地址") },
                    placeholder = { Text("https://agent.example.com") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                    shape = RoundedCornerShape(16.dp),
                )
                OutlinedTextField(
                    value = accessToken,
                    onValueChange = { accessToken = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("访问令牌") },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                    shape = RoundedCornerShape(16.dp),
                )
                OutlinedTextField(
                    value = developmentUserId,
                    onValueChange = { developmentUserId = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text("本机开发用户（可选）") },
                    supportingText = {
                        Text("仅用于回环地址的开发认证，生产环境请留空。")
                    },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    shape = RoundedCornerShape(16.dp),
                )
                error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
                Button(
                    onClick = {
                        error =
                            onSave(
                                AppConfig(
                                    gatewayBase = gatewayBase,
                                    accessToken = accessToken,
                                    developmentUserId = developmentUserId,
                                ),
                            )
                    },
                    modifier = Modifier.fillMaxWidth().height(54.dp),
                    shape = RoundedCornerShape(18.dp),
                    colors =
                        ButtonDefaults.buttonColors(
                            containerColor = MaterialTheme.colorScheme.primary,
                        ),
                ) {
                    Text(if (config == null) "保存并连接" else "保存并重新连接")
                }
                if (config != null) {
                    TextButton(
                        onClick = onClear,
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("清除本机连接", color = MaterialTheme.colorScheme.error)
                    }
                }
            }
        }
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(24.dp),
            color = MaterialTheme.colorScheme.surfaceVariant,
        ) {
            Text(
                "模拟器本地调试：先执行 adb reverse tcp:3001 tcp:3001，再使用 http://127.0.0.1:3001。明文 HTTP 只对白名单回环地址开放。",
                modifier = Modifier.padding(18.dp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

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
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding(),
        contentPadding = PaddingValues(20.dp, 14.dp, 20.dp, 38.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextButton(onClick = onBack) { Text("← 返回", fontWeight = FontWeight.Bold) }
                BrandMark(50.dp)
            }
        }
        item {
            Text("SESSION DETAIL", style = MaterialTheme.typography.labelSmall)
            Spacer(Modifier.height(8.dp))
            Text(
                session.project.displayName,
                style = MaterialTheme.typography.headlineLarge,
            )
            Spacer(Modifier.height(8.dp))
            Text(
                "${session.agentType.label} · 设备 ${shortId(session.deviceId)}",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        item {
            Surface(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(30.dp)),
                shape = RoundedCornerShape(30.dp),
                color = MaterialTheme.colorScheme.surface,
            ) {
                Column(
                    modifier = Modifier.padding(22.dp),
                    verticalArrangement = Arrangement.spacedBy(18.dp),
                ) {
                    Text("当前状态", style = MaterialTheme.typography.labelLarge)
                    Text(
                        session.state.activityStatus.label,
                        style = MaterialTheme.typography.headlineLarge,
                    )
                    StatusPill(session)
                    HorizontalDivider(color = MaterialTheme.colorScheme.outline)
                    DetailFact("Presence", presenceLabel(session.state.presence))
                    DetailFact("最近观察", absoluteTime(session.observedAt))
                    DetailFact("Session", session.agentSessionId)
                }
            }
        }
        if (position != null && total > 1) {
            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Button(
                        onClick = { onPrevious?.invoke() },
                        enabled = onPrevious != null,
                        modifier = Modifier.weight(1f),
                        shape = RoundedCornerShape(18.dp),
                    ) {
                        Text("← 上一条")
                    }
                    Text("$position / $total")
                    Button(
                        onClick = { onNext?.invoke() },
                        enabled = onNext != null,
                        modifier = Modifier.weight(1f),
                        shape = RoundedCornerShape(18.dp),
                    ) {
                        Text("下一条 →")
                    }
                }
            }
        }
        item { SectionTitle("状态时间线", state.timeline.size) }
        when {
            state.timelineLoading && state.timeline.isEmpty() ->
                item { LoadingCard("正在读取时间线…") }
            state.timelineError != null ->
                item { EmptyCard("时间线暂不可用", state.timelineError) }
            state.timeline.isEmpty() ->
                item { EmptyCard("暂无状态事件", "Gateway 还没有保存这条 Session 的历史变化。") }
            else ->
                itemsIndexed(
                    state.timeline,
                    key = { _, event -> event.eventId },
                ) { index, event ->
                    MotionEntrance(motionKey = event.eventId, index = index) {
                        TimelineCard(event)
                    }
                }
        }
    }
}

@Composable
private fun DetailFact(label: String, value: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(
            value,
            modifier = Modifier.padding(start = 18.dp),
            fontWeight = FontWeight.Bold,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun TimelineCard(event: SessionRecord) {
    Row(modifier = Modifier.fillMaxWidth()) {
        Box(
            Modifier
                .padding(top = 7.dp)
                .size(10.dp)
                .background(statusColor(event.state.activityStatus), CircleShape),
        )
        Spacer(Modifier.width(13.dp))
        Surface(
            modifier =
                Modifier
                    .weight(1f)
                    .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(22.dp)),
            shape = RoundedCornerShape(22.dp),
            color = MaterialTheme.colorScheme.surface,
        ) {
            Column(
                modifier = Modifier.padding(17.dp),
                verticalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Text(event.state.activityStatus.label, fontWeight = FontWeight.Bold)
                Text(
                    absoluteTime(event.observedAt),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun FloatingDock(
    selected: AppSection,
    waitingCount: Int,
    onSelect: (AppSection) -> Unit,
) {
    Surface(
        modifier =
            Modifier
                .padding(horizontal = 14.dp, vertical = 9.dp)
                .navigationBarsPadding()
                .fillMaxWidth()
                .shadow(22.dp, RoundedCornerShape(28.dp)),
        shape = RoundedCornerShape(28.dp),
        color = MaterialTheme.colorScheme.surface.copy(alpha = 0.97f),
        border =
            androidx.compose.foundation.BorderStroke(
                1.dp,
                MaterialTheme.colorScheme.outline,
            ),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 5.dp, vertical = 7.dp),
            horizontalArrangement = Arrangement.SpaceEvenly,
        ) {
            AppSection.entries.forEach { item ->
                val active = item == selected
                MotionSelection(
                    active = active,
                    modifier = Modifier.weight(1f),
                ) {
                    Column(
                        modifier =
                            Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(20.dp))
                                .clickable { onSelect(item) }
                                .background(
                                    if (active) MaterialTheme.colorScheme.primary
                                    else Color.Transparent,
                                )
                                .padding(vertical = 7.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Box {
                            Text(
                                item.mark,
                                color =
                                    if (active) MaterialTheme.colorScheme.onPrimary
                                    else MaterialTheme.colorScheme.onSurfaceVariant,
                                fontSize = 21.sp,
                                fontWeight = FontWeight.Black,
                            )
                            if (item == AppSection.WAITING && waitingCount > 0) {
                                Box(
                                    Modifier
                                        .align(Alignment.TopEnd)
                                        .size(7.dp)
                                        .background(PinkSignal, CircleShape),
                                )
                            }
                        }
                        Text(
                            item.label,
                            color =
                                if (active) MaterialTheme.colorScheme.onPrimary
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                            style = MaterialTheme.typography.labelLarge,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun ErrorBanner(message: String, onRetry: () -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(22.dp),
        color = MaterialTheme.colorScheme.error.copy(alpha = 0.08f),
    ) {
        Row(
            modifier = Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(message, modifier = Modifier.weight(1f), color = MaterialTheme.colorScheme.error)
            TextButton(onClick = onRetry) { Text("重试") }
        }
    }
}

@Composable
private fun LoadingCard(message: String) {
    Surface(
        modifier =
            Modifier
                .fillMaxWidth()
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(24.dp)),
        shape = RoundedCornerShape(24.dp),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Row(
            modifier = Modifier.padding(20.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(13.dp))
            Text(message)
        }
    }
}

@Composable
private fun EmptyCard(title: String, body: String) {
    Surface(
        modifier =
            Modifier
                .fillMaxWidth()
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(26.dp)),
        shape = RoundedCornerShape(26.dp),
        color = MaterialTheme.colorScheme.surface,
    ) {
        Column(
            modifier = Modifier.padding(22.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(title, style = MaterialTheme.typography.titleLarge)
            Text(body, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun PinkDot() {
    Box(
        Modifier
            .size(8.dp)
            .background(PinkSignal, CircleShape),
    )
}

@Composable
private fun statusColor(status: ActivityStatus): Color =
    when (status) {
        ActivityStatus.WORKING -> WorkingBlue
        ActivityStatus.WAITING_INPUT,
        ActivityStatus.WAITING_PERMISSION -> WaitingAmber
        ActivityStatus.COMPLETED -> SuccessGreen
        ActivityStatus.FAILED -> MaterialTheme.colorScheme.error
        ActivityStatus.IDLE,
        ActivityStatus.STOPPED,
        ActivityStatus.UNKNOWN -> MaterialTheme.colorScheme.onSurfaceVariant
    }

private fun shortId(deviceId: String): String =
    if (deviceId.length <= 12) deviceId else "${deviceId.take(6)}…${deviceId.takeLast(4)}"

private fun relativeTime(instant: Instant): String {
    val seconds = Duration.between(instant, Instant.now()).seconds.coerceAtLeast(0)
    return when {
        seconds < 10 -> "刚刚"
        seconds < 60 -> "${seconds}秒前"
        seconds < 3_600 -> "${seconds / 60}分钟前"
        seconds < 86_400 -> "${seconds / 3_600}小时前"
        else -> "${seconds / 86_400}天前"
    }
}

private val absoluteFormatter =
    DateTimeFormatter.ofPattern("M月d日 HH:mm").withZone(ZoneId.systemDefault())

private fun absoluteTime(instant: Instant): String = absoluteFormatter.format(instant)

private fun presenceLabel(presence: Presence): String =
    when (presence) {
        Presence.LIVE -> "实时"
        Presence.PERSISTED -> "历史记录"
        Presence.UNKNOWN -> "未知"
    }
