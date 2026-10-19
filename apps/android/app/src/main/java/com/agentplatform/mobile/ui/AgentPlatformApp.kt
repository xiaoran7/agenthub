package com.agentplatform.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.AgentGateway
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.GatewayException
import java.net.URLEncoder
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONArray
import org.json.JSONObject

// =========================================================================
// AgentHub mobile companion.
//
// Navigation is a three-level stack (Home -> Host -> Session) plus two
// utilities that each own exactly one job:
//   Settings  — where this phone points (gateway endpoint, token)
//   Connect   — how to add another computer (runner install guide)
// Settings is reachable from one place only: the Home header.
// =========================================================================

private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
private fun JSONArray.strings(): List<String> = (0 until length()).map { optString(it) }
private fun encoded(value: String): String = URLEncoder.encode(value, "UTF-8")

private const val ROUTE_HOME = "home"
private const val ROUTE_HOST = "host"
private const val ROUTE_SESSION = "session"
private const val ROUTE_SETTINGS = "settings"
private const val ROUTE_CONNECT = "connect"

private const val GUTTER = 16

/** Parsed once per snapshot; every screen reads plain Kotlin, not JSON. */
private data class Fleet(
    val runners: List<HostEntry>,
    val sessions: List<SessionEntry>,
    val activeCount: Int,
    val waitingCount: Int,
    val onlineCount: Int,
)

private data class HostEntry(
    val id: String,
    val online: Boolean,
    val provider: String,
    val generation: String,
    val workspaces: List<String>,
    val sessions: List<SessionEntry>,
    val candidates: List<CandidateEntry>,
)

private data class SessionEntry(
    val runnerId: String,
    val hubId: String,
    val providerSessionId: String,
    val workspace: String,
    val state: String,
    val controlConnected: Boolean,
    val hostOnline: Boolean,
    val generation: String,
) {
    val controllable: Boolean get() = controlConnected && hostOnline
}

private data class CandidateEntry(val providerSessionId: String, val title: String, val workspace: String)

private fun parseFleet(snapshot: JSONObject): Fleet {
    val sessionItems = snapshot.optJSONArray("sessions")?.objects().orEmpty()
    val allSessions =
        sessionItems.mapNotNull { item ->
            val binding = item.optJSONObject("binding") ?: return@mapNotNull null
            SessionEntry(
                runnerId = item.optString("runner_id"),
                hubId = binding.optString("hub_session_id"),
                providerSessionId = binding.optString("provider_session_id"),
                workspace = binding.optString("workspace_path"),
                state = binding.optString("execution_state").ifBlank { "unknown" },
                controlConnected = binding.optBoolean("control_connected"),
                hostOnline = item.optBoolean("online"),
                generation = binding.optString("host_generation"),
            )
        }
    val byRunner = allSessions.groupBy { it.runnerId }
    val orderedSessions = allSessions.sortedBy { statePriority(it.state) }

    val runners =
        snapshot.optJSONArray("runners")?.objects().orEmpty().map { runner ->
            val hello = runner.optJSONObject("hello")
            val id = runner.optString("runner_id")
            val own = byRunner[id].orEmpty()
            val activeProviderIds = own.map { it.providerSessionId }.toSet()
            HostEntry(
                id = id,
                online = runner.optBoolean("online"),
                provider = hello?.optString("provider").orEmpty().ifBlank { "codex" },
                generation = hello?.optString("host_generation").orEmpty(),
                workspaces = hello?.optJSONArray("workspaces")?.strings().orEmpty(),
                sessions = own.sortedBy { statePriority(it.state) },
                candidates =
                    hello?.optJSONObject("discovery")?.optJSONArray("candidates")?.objects().orEmpty()
                        .filter { it.optString("provider_session_id") !in activeProviderIds }
                        .map {
                            CandidateEntry(
                                providerSessionId = it.optString("provider_session_id"),
                                title = it.optString("title"),
                                workspace = it.optString("workspace_path"),
                            )
                        },
            )
        }

    return Fleet(
        runners = runners,
        sessions = orderedSessions,
        activeCount = allSessions.count { it.state == "running" },
        waitingCount = allSessions.count { it.state == "awaiting_permission" || it.state == "awaiting_input" },
        onlineCount = runners.count { it.online },
    )
}

/** Actionable sessions surface first; finished or unknown ones sink. */
private fun statePriority(state: String): Int =
    when (state) {
        "awaiting_permission", "awaiting_input" -> 0
        "running" -> 1
        "idle" -> 2
        "error" -> 3
        "completed", "terminated" -> 4
        else -> 5
    }

@Composable
fun AgentPlatformApp(controller: AppController) {
    val ui = controller.state
    val config = ui.config
    val gateway = remember(config) { config?.let(::AgentGateway) }
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current.lifecycle

    var route by rememberSaveable { mutableStateOf(ROUTE_HOME) }
    var hostId by rememberSaveable { mutableStateOf<String?>(null) }
    var sessionRunner by rememberSaveable { mutableStateOf<String?>(null) }
    var sessionHub by rememberSaveable { mutableStateOf<String?>(null) }
    var sessionFromHome by rememberSaveable { mutableStateOf(false) }

    var snapshot by remember { mutableStateOf(JSONObject()) }
    var fleetError by remember { mutableStateOf("") }
    var fleetLoading by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val refreshSignal = remember { Channel<Unit>(Channel.CONFLATED) }

    val fleet = remember(snapshot) { parseFleet(snapshot) }

    // One fleet poll shared by every screen. It only runs while the app is in
    // the foreground and slows down when nothing is executing.
    LaunchedEffect(gateway, lifecycle) {
        if (gateway == null) {
            snapshot = JSONObject()
            return@LaunchedEffect
        }
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                try {
                    val next = withContext(Dispatchers.IO) { gateway.remoteRequest("/sessions") }
                    // Only publish when something actually changed; an idle
                    // poll then costs zero recomposition.
                    if (next.toString() != snapshot.toString()) snapshot = next
                    fleetError = ""
                } catch (cancel: CancellationException) {
                    throw cancel
                } catch (failure: Exception) {
                    fleetError = fleetMessage(failure)
                    snapshot =
                        JSONObject(snapshot.toString()).also { stale ->
                            stale.optJSONArray("runners")?.objects()?.forEach { it.put("online", false) }
                            stale.optJSONArray("sessions")?.objects()?.forEach { it.put("online", false) }
                        }
                }
                fleetLoading = false
                val active = snapshot.optJSONArray("sessions")?.objects().orEmpty().any {
                    val state = it.optJSONObject("binding")?.optString("execution_state")
                    state == "running" || state == "awaiting_permission" || state == "awaiting_input"
                }
                val wait = if (active) 1_500L else 3_000L
                // A manual refresh wakes the loop immediately; the timeout is
                // just the next scheduled poll.
                withTimeoutOrNull(wait) { refreshSignal.receive() }
            }
        }
    }

    fun dispatch(
        runner: String,
        hub: String,
        action: JSONObject,
        generation: String?,
        onDone: () -> Unit = {},
    ) {
        val client = gateway ?: return
        if (busy) return
        busy = true
        fleetError = ""
        scope.launch {
            try {
                val body =
                    JSONObject()
                        .put("client_cmd_id", UUID.randomUUID().toString())
                        .put("hub_session_id", hub)
                        .put("host_generation", generation ?: JSONObject.NULL)
                        .put("command", action)
                withContext(Dispatchers.IO) {
                    client.remoteRequest("/runners/${encoded(runner)}/commands", body)
                }
                onDone()
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (failure: Exception) {
                fleetError = fleetMessage(failure)
            } finally {
                busy = false
            }
        }
    }

    fun openSession(runner: String, hub: String, fromHome: Boolean) {
        sessionRunner = runner
        sessionHub = hub
        sessionFromHome = fromHome
        route = ROUTE_SESSION
    }

    BackHandler(enabled = route != ROUTE_HOME) {
        route =
            when (route) {
                ROUTE_SESSION -> if (sessionFromHome) ROUTE_HOME else ROUTE_HOST
                else -> ROUTE_HOME
            }
    }

    // Route stack: Home is the root. Only Settings and the session screen get
    // their own header affordance; the rest relies on the system back gesture.
    AnimatedContent(
        targetState = route,
        transitionSpec = {
            val forward = targetState != ROUTE_HOME && initialState == ROUTE_HOME
            val direction = if (forward) 1 else -1
            (
                fadeIn(tween(MotionTokens.Quick)) +
                    slideInHorizontally(tween(MotionTokens.Standard)) { it / 12 * direction }
            ).togetherWith(
                fadeOut(tween(MotionTokens.Quick)) +
                    slideOutHorizontally(tween(MotionTokens.Standard)) { -it / 16 * direction },
            ).using(SizeTransform(clip = false))
        },
        label = "route",
    ) { target ->
        when (target) {
            ROUTE_HOME ->
                HomeScreen(
                    config = config,
                    fleet = fleet,
                    loading = fleetLoading,
                    error = fleetError,
                    onRefresh = {
                        fleetLoading = true
                        refreshSignal.trySend(Unit)
                    },
                    onOpenSettings = { route = ROUTE_SETTINGS },
                    onOpenConnect = { route = ROUTE_CONNECT },
                    onOpenHost = { id ->
                        hostId = id
                        route = ROUTE_HOST
                    },
                    onOpenSession = { runner, hub -> openSession(runner, hub, fromHome = true) },
                )

            ROUTE_HOST -> {
                val host = fleet.runners.firstOrNull { it.id == hostId }
                HostScreen(
                    host = host,
                    busy = busy,
                    error = fleetError,
                    onBack = { route = ROUTE_HOME },
                    onRefresh = {
                        fleetLoading = true
                        refreshSignal.trySend(Unit)
                    },
                    onOpenSession = { hub -> openSession(hostId.orEmpty(), hub, fromHome = false) },
                    onCreate = { workspace ->
                        dispatch(
                            runner = hostId.orEmpty(),
                            hub = UUID.randomUUID().toString(),
                            action =
                                JSONObject()
                                    .put("action", "connect")
                                    .put("mode", "create")
                                    .put("workspace_path", workspace)
                                    .put("sandbox", "read-only")
                                    .put("approval_policy", "untrusted"),
                            generation = host?.generation,
                        )
                    },
                    onResume = { workspace, providerSession ->
                        dispatch(
                            runner = hostId.orEmpty(),
                            hub = UUID.randomUUID().toString(),
                            action =
                                JSONObject()
                                    .put("action", "connect")
                                    .put("mode", "resume")
                                    .put("workspace_path", workspace)
                                    .put("provider_session_id", providerSession)
                                    .put("sandbox", "read-only")
                                    .put("approval_policy", "untrusted"),
                            generation = host?.generation,
                        )
                    },
                )
            }

            ROUTE_SESSION -> {
                val g = gateway
                val runner = sessionRunner
                val hub = sessionHub
                if (g != null && runner != null && hub != null) {
                    RemoteScreen(
                        gateway = g,
                        runnerId = runner,
                        hubId = hub,
                        snapshot = snapshot,
                        onBack = { route = if (sessionFromHome) ROUTE_HOME else ROUTE_HOST },
                    )
                } else {
                    route = ROUTE_HOME
                }
            }

            ROUTE_SETTINGS ->
                SettingsScreen(
                    config = config,
                    onBack = { route = ROUTE_HOME },
                    onSave = { next ->
                        controller.saveConfig(next).also { error ->
                            if (error == null) {
                                refreshSignal.trySend(Unit)
                                route = ROUTE_HOME
                            }
                        }
                    },
                    onClear = {
                        controller.clearConfig()
                        route = ROUTE_HOME
                    },
                )

            ROUTE_CONNECT ->
                ConnectScreen(
                    configured = config != null,
                    onBack = { route = ROUTE_HOME },
                    onOpenSettings = { route = ROUTE_SETTINGS },
                )
        }
    }
}

private fun fleetMessage(failure: Exception): String =
    when ((failure as? GatewayException)?.code) {
        "runner_offline" -> "电脑 Runner 离线，恢复连线后可继续操作。"
        "session_control_lost", "host_generation_mismatch" -> "会话控制已断开，请重新接入。"
        "approval_not_pending" -> "该审批已处理或已过期。"
        "runner_not_found", "session_not_found" -> "目标会话已不存在。"
        else ->
            when ((failure as? GatewayException)?.status) {
                401, 403 -> "认证失效，请到设置更新访问令牌。"
                null -> "无法连接 Gateway，请检查地址与网络。"
                else -> "Gateway 暂时不可用，请稍后重试。"
            }
    }

private fun shortWorkspace(raw: String): String =
    if (raw.startsWith("ws-")) raw.removePrefix("ws-").replace(Regex("-[0-9a-f]{16}$"), "")
    else raw.substringAfterLast('/').substringAfterLast('\\').ifBlank { raw }

// =========================================================================
// Home
// =========================================================================

@Composable
private fun HomeScreen(
    config: AppConfig?,
    fleet: Fleet,
    loading: Boolean,
    error: String,
    onRefresh: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenConnect: () -> Unit,
    onOpenHost: (String) -> Unit,
    onOpenSession: (String, String) -> Unit,
) {
    LazyColumn(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding(),
        contentPadding = PaddingValues(start = GUTTER.dp, end = GUTTER.dp, bottom = OrcaSpace.xxl),
    ) {
        item {
            BrandBar(
                loading = loading,
                configured = config != null,
                onRefresh = onRefresh,
                onOpenSettings = onOpenSettings,
            )
        }

        if (config == null) {
            item { Onboarding(onOpenSettings, onOpenConnect) }
            return@LazyColumn
        }

        item {
            Column(
                modifier = Modifier.padding(top = OrcaSpace.xl, bottom = OrcaSpace.lg),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                Text(
                    text = "欢迎回来",
                    style = MaterialTheme.typography.headlineLarge,
                    color = MaterialTheme.colorScheme.onBackground,
                )
                Text(
                    text =
                        when {
                            fleet.runners.isEmpty() -> "还没有接入任何主机"
                            fleet.onlineCount == 0 -> "${fleet.runners.size} 台主机全部离线"
                            else -> "${fleet.onlineCount} / ${fleet.runners.size} 台主机在线 · ${fleet.activeCount} 个会话运行中"
                        },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        item {
            OrcaStatStrip(
                stats =
                    listOf(
                        OrcaStat("${fleet.onlineCount}/${fleet.runners.size}", "在线主机"),
                        OrcaStat("${fleet.activeCount}", "运行中", if (fleet.activeCount > 0) OrcaColors.accentBlue else null),
                        OrcaStat(
                            "${fleet.waitingCount}",
                            "待处理",
                            if (fleet.waitingCount > 0) OrcaColors.accentAmber else null,
                        ),
                    ),
            )
        }

        if (error.isNotBlank()) {
            item { OrcaInlineError(error, Modifier.padding(top = OrcaSpace.md)) }
        }

        item {
            OrcaSectionHeader("主机") {
                OrcaTextAction("接入主机", onClick = onOpenConnect)
            }
        }

        if (fleet.runners.isEmpty()) {
            item {
                OrcaCard {
                    OrcaNoticeCard(
                        title = if (loading) "正在发现主机…" else "还没有已接入的主机",
                        body = "在电脑上运行 AgentHub Runner 并连接同一个 Gateway，主机就会自动出现在这里。",
                        icon = { OrcaMonitorIcon(MaterialTheme.colorScheme.onSurfaceVariant, 22.dp) },
                    )
                }
            }
        } else {
            item {
                OrcaListGroup {
                    fleet.runners.forEachIndexed { index, host ->
                        if (index > 0) OrcaRowDivider()
                        OrcaRow(
                            title = hostDisplayName(host.id),
                            subtitle = hostSubtitle(host),
                            statusColor = if (host.online) OrcaColors.accentGreen else MaterialTheme.colorScheme.outlineVariant,
                            statusText = if (host.online) "已连接" else "已断开",
                            meta = host.id.takeLast(8),
                            leading = { HostStatusTile(online = host.online) },
                            onClick = { onOpenHost(host.id) },
                            trailing = {
                                OrcaChevronRight(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f))
                            },
                        )
                    }
                }
            }
        }

        if (fleet.sessions.isNotEmpty()) {
            item {
                OrcaSectionHeader("最近会话")
            }
            item {
                OrcaListGroup {
                    fleet.sessions.take(5).forEachIndexed { index, session ->
                        if (index > 0) OrcaRowDivider()
                        OrcaRow(
                            title = shortWorkspace(session.workspace),
                            subtitle = sessionSubtitle(session),
                            statusColor = stateColor(session.state),
                            statusText = stateLabel(session.state),
                            meta = session.providerSessionId.take(12),
                            leading = { OrcaIconTile { OrcaTerminalIcon(MaterialTheme.colorScheme.onSurfaceVariant) } },
                            trailing = {
                                OrcaChevronRight(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f))
                            },
                            onClick = { onOpenSession(session.runnerId, session.hubId) },
                        )
                    }
                }
            }
        }
    }
}

private fun hostSubtitle(host: HostEntry): String {
    val parts = mutableListOf<String>()
    if (host.workspaces.isNotEmpty()) parts += "${host.workspaces.size} 个工作区"
    if (host.sessions.isNotEmpty()) parts += "${host.sessions.size} 个会话"
    if (parts.isEmpty()) parts += host.provider
    return parts.joinToString(" · ")
}

private fun sessionSubtitle(session: SessionEntry): String {
    val workspace = shortWorkspace(session.workspace)
    val host = hostDisplayName(session.runnerId)
    return if (workspace.isBlank()) host else "$host · $workspace"
}

@Composable
private fun BrandBar(
    loading: Boolean,
    configured: Boolean,
    onRefresh: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = OrcaSpace.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OrcaLogo(size = 20.dp, tint = MaterialTheme.colorScheme.onBackground)
        Text(
            text = "AgentHub",
            style = MaterialTheme.typography.titleLarge,
            color = MaterialTheme.colorScheme.onBackground,
            modifier = Modifier.padding(start = OrcaSpace.sm),
        )
        if (configured) {
            OrcaStatusDot(
                color = if (loading) OrcaColors.accentAmber else OrcaColors.accentGreen,
                size = 5.dp,
                modifier = Modifier.padding(start = OrcaSpace.xs),
            )
        }
        Spacer(Modifier.weight(1f))
        OrcaIconButton(onClick = onRefresh) {
            if (loading) {
                CircularProgressIndicator(
                    modifier = Modifier.size(16.dp),
                    strokeWidth = 1.6.dp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                OrcaRefreshIcon(MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        OrcaIconButton(onClick = onOpenSettings) {
            OrcaSettingsIcon(MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun Onboarding(onOpenSettings: () -> Unit, onOpenConnect: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxWidth().padding(top = OrcaSpace.xxl),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = "欢迎使用 AgentHub",
            style = MaterialTheme.typography.headlineLarge,
            color = MaterialTheme.colorScheme.onBackground,
        )
        Text(
            text = "在手机上查看并接管电脑上的 Agent 会话",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = OrcaSpace.sm),
        )
        Box(
            modifier =
                Modifier
                    .padding(top = OrcaSpace.xl)
                    .size(64.dp)
                    .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(OrcaRadius.card)),
            contentAlignment = Alignment.Center,
        ) {
            OrcaMonitorIcon(MaterialTheme.colorScheme.onSurfaceVariant, 26.dp)
        }
        Row(
            modifier = Modifier.fillMaxWidth().padding(top = OrcaSpace.xl),
            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            OrcaSecondaryButton("接入主机", Modifier.weight(1f), onClick = onOpenConnect)
            OrcaPrimaryButton("配置连接", Modifier.weight(1f), onClick = onOpenSettings)
        }
    }
}

// =========================================================================
// Host
// =========================================================================

@Composable
private fun HostScreen(
    host: HostEntry?,
    busy: Boolean,
    error: String,
    onBack: () -> Unit,
    onRefresh: () -> Unit,
    onOpenSession: (String) -> Unit,
    onCreate: (String) -> Unit,
    onResume: (String, String) -> Unit,
) {
    LazyColumn(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding(),
        contentPadding = PaddingValues(start = GUTTER.dp, end = GUTTER.dp, bottom = OrcaSpace.xxl),
    ) {
        item {
            ScreenHeader(
                title = hostDisplayName(host?.id.orEmpty()),
                subtitle =
                    when {
                        host == null -> "主机已不在机队中"
                        host.online -> "已连接 · ${host.workspaces.size} 个工作区 · ${host.sessions.size} 个会话"
                        else -> "已断开 · 等待电脑端恢复连线"
                    },
                statusColor = if (host?.online == true) OrcaColors.accentGreen else MaterialTheme.colorScheme.outlineVariant,
                onBack = onBack,
                actions = {
                    OrcaIconButton(onClick = onRefresh) {
                        OrcaRefreshIcon(MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                },
            )
        }

        if (error.isNotBlank()) {
            item { OrcaInlineError(error, Modifier.padding(top = OrcaSpace.sm)) }
        }

        if (host == null) {
            item {
                OrcaCard(Modifier.padding(top = OrcaSpace.lg)) {
                    OrcaNoticeCard(
                        title = "主机已不在机队中",
                        body = "它可能已被撤销，或还没有连上 Gateway。回到首页刷新后重试。",
                        icon = { OrcaMonitorIcon(MaterialTheme.colorScheme.onSurfaceVariant, 22.dp) },
                    )
                }
            }
            return@LazyColumn
        }

        item { OrcaSectionHeader("活动会话") }
        if (host.sessions.isEmpty()) {
            item {
                OrcaCard {
                    OrcaNoticeCard(
                        title = "还没有会话",
                        body = "在下方选一个工作区发起任务，或接管电脑上已有的 Agent 会话。",
                    )
                }
            }
        } else {
            item {
                OrcaListGroup {
                    host.sessions.forEachIndexed { index, session ->
                        if (index > 0) OrcaRowDivider()
                        OrcaRow(
                            title = shortWorkspace(session.workspace),
                            subtitle = if (session.controllable) null else "控制已断开，需要重新接入",
                            statusColor = stateColor(session.state),
                            statusText = stateLabel(session.state),
                            meta = session.providerSessionId.take(12),
                            leading = { OrcaIconTile { OrcaTerminalIcon(MaterialTheme.colorScheme.onSurfaceVariant) } },
                            trailing = {
                                OrcaChevronRight(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f))
                            },
                            onClick = { onOpenSession(session.hubId) },
                        )
                    }
                }
            }
        }

        item { OrcaSectionHeader("工作区") }
        if (host.workspaces.isEmpty()) {
            item {
                OrcaCard {
                    OrcaNoticeCard(
                        title = "没有可用的工作区",
                        body = "启动 Runner 时用 --workspace 指定工作目录，这里就会列出可发起任务的项目。",
                    )
                }
            }
        } else {
            item {
                OrcaListGroup {
                    host.workspaces.forEachIndexed { index, workspace ->
                        if (index > 0) OrcaRowDivider()
                        val ready = host.online && host.generation.isNotBlank() && !busy
                        OrcaRow(
                            title = shortWorkspace(workspace),
                            subtitle = workspace,
                            leading = { OrcaIconTile { OrcaTerminalIcon(MaterialTheme.colorScheme.onSurfaceVariant) } },
                            trailing = {
                                OrcaTextAction(if (busy) "下发中…" else "发起任务", enabled = ready) {
                                    onCreate(workspace)
                                }
                            },
                        )
                    }
                }
            }
        }

        if (host.candidates.isNotEmpty()) {
            item { OrcaSectionHeader("发现的已有会话") }
            item {
                OrcaListGroup {
                    host.candidates.forEachIndexed { index, candidate ->
                        if (index > 0) OrcaRowDivider()
                        val ready = host.online && host.generation.isNotBlank() && !busy
                        OrcaRow(
                            title = candidate.title.ifBlank { candidate.providerSessionId.take(12) },
                            subtitle = shortWorkspace(candidate.workspace),
                            leading = { OrcaIconTile { OrcaTerminalIcon(MaterialTheme.colorScheme.onSurfaceVariant) } },
                            trailing = {
                                OrcaTextAction(if (busy) "下发中…" else "接管", enabled = ready) {
                                    onResume(candidate.workspace, candidate.providerSessionId)
                                }
                            },
                        )
                    }
                }
            }
        }
    }
}

// =========================================================================
// Shared header
// =========================================================================

@Composable
private fun ScreenHeader(
    title: String,
    subtitle: String,
    statusColor: Color,
    onBack: () -> Unit,
    actions: @Composable () -> Unit = {},
) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(top = OrcaSpace.sm, bottom = OrcaSpace.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OrcaIconButton(onClick = onBack) {
            OrcaBackChevron(MaterialTheme.colorScheme.onSurface)
        }
        Column(
            modifier = Modifier.weight(1f).padding(start = OrcaSpace.xs),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
        ) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
            )
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                OrcaStatusDot(statusColor, 5.dp)
                Text(
                    text = subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                )
            }
        }
        actions()
    }
}

// =========================================================================
// Connect guide — how to add another computer (distinct from settings)
// =========================================================================

@Composable
private fun ConnectScreen(
    configured: Boolean,
    onBack: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    val command = "agenthub-runner remote-run --server \$GATEWAY_URL --workspace . --db agenthub.db"

    LazyColumn(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding(),
        contentPadding = PaddingValues(start = GUTTER.dp, end = GUTTER.dp, bottom = OrcaSpace.xxl),
    ) {
        item {
            ScreenHeader(
                title = "接入主机",
                subtitle = "手机只出站，无需公网端口",
                statusColor = if (configured) OrcaColors.accentGreen else OrcaColors.accentAmber,
                onBack = onBack,
            )
        }

        item { OrcaSectionHeader("三步接入") }
        item {
            OrcaListGroup {
                OrcaStepRow(1, "启动 Runner", "在电脑终端执行 agenthub-runner daemon 或 remote-run，让它主动出站连接 Gateway。")
                OrcaRowDivider(OrcaSpace.rowH)
                OrcaStepRow(2, "指向同一 Gateway", "手机端填写的地址与令牌必须和 Runner 的 --server 参数一致。")
                OrcaRowDivider(OrcaSpace.rowH)
                OrcaStepRow(3, "回到首页", "设备上线后会自动出现在主机列表，点进去即可发起任务或接管会话。")
            }
        }

        item { OrcaSectionHeader("启动命令") }
        item {
            OrcaCard {
                Column(
                    modifier = Modifier.fillMaxWidth().padding(OrcaSpace.rowH),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    Text(
                        text = command,
                        style = OrcaType.code,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.End,
                    ) {
                        OrcaTextAction(if (copied) "已复制" else "复制命令") {
                            clipboard.setText(AnnotatedString(command))
                            copied = true
                        }
                    }
                }
            }
        }

        item {
            Column(
                modifier = Modifier.padding(top = OrcaSpace.xl),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                if (!configured) {
                    OrcaPrimaryButton("先配置 Gateway 连接", onClick = onOpenSettings)
                }
                OrcaSecondaryButton("返回首页", onClick = onBack)
            }
        }
    }
}

// =========================================================================
// Settings — where this phone points. Single entry point: Home header.
// =========================================================================

@Composable
private fun SettingsScreen(
    config: AppConfig?,
    onBack: () -> Unit,
    onSave: (AppConfig) -> String?,
    onClear: () -> Unit,
) {
    var gatewayBase by rememberSaveable { mutableStateOf(config?.gatewayBase ?: "") }
    var accessToken by rememberSaveable { mutableStateOf(config?.accessToken ?: "") }
    var developmentUserId by rememberSaveable { mutableStateOf(config?.developmentUserId ?: "") }
    var error by remember { mutableStateOf<String?>(null) }
    val focus = LocalFocusManager.current

    LazyColumn(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding()
                .imePadding(),
        contentPadding = PaddingValues(start = GUTTER.dp, end = GUTTER.dp, bottom = OrcaSpace.xxl),
    ) {
        item {
            ScreenHeader(
                title = "设置",
                subtitle = if (config != null) "已保存连接" else "尚未连接",
                statusColor = if (config != null) OrcaColors.accentGreen else OrcaColors.accentAmber,
                onBack = onBack,
            )
        }

        item { OrcaSectionHeader("Gateway") }
        item {
            OrcaCard {
                Column(
                    modifier = Modifier.fillMaxWidth().padding(OrcaSpace.rowH),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.md),
                ) {
                    OrcaField(
                        value = gatewayBase,
                        onValueChange = { gatewayBase = it },
                        label = "端点地址",
                        placeholder = "https://gateway.example.com",
                        imeAction = ImeAction.Next,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                        OrcaChip(
                            text = "127.0.0.1:3001",
                            onClick = {
                                gatewayBase = "http://127.0.0.1:3001"
                                if (developmentUserId.isBlank()) developmentUserId = "local-developer"
                                error = null
                            },
                        )
                        OrcaChip(
                            text = "10.0.2.2（模拟器）",
                            onClick = {
                                gatewayBase = "http://10.0.2.2:3001"
                                if (developmentUserId.isBlank()) developmentUserId = "local-developer"
                                error = null
                            },
                        )
                    }
                    OrcaField(
                        value = accessToken,
                        onValueChange = { accessToken = it },
                        label = "访问令牌",
                        placeholder = "Bearer token",
                        imeAction = ImeAction.Next,
                        secret = true,
                    )
                    OrcaField(
                        value = developmentUserId,
                        onValueChange = { developmentUserId = it },
                        label = "开发用户（仅回环地址）",
                        placeholder = "local-developer",
                        imeAction = ImeAction.Done,
                    )

                    error?.let {
                        Text(
                            text = it,
                            style = MaterialTheme.typography.bodySmall,
                            color = OrcaColors.accentRed,
                        )
                    }

                    OrcaPrimaryButton(
                        label = if (config == null) "保存并连接" else "保存连接",
                        onClick = {
                            focus.clearFocus()
                            error = onSave(
                                AppConfig(
                                    gatewayBase = gatewayBase,
                                    accessToken = accessToken,
                                    developmentUserId = developmentUserId,
                                ),
                            )
                        },
                    )
                }
            }
        }

        item {
            Text(
                text = "凭据由 Android Keystore 加密保存，不写入日志，不参与备份。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = OrcaSpace.xs, top = OrcaSpace.sm),
            )
        }

        if (config != null) {
            item {
                OrcaSectionHeader("维护")
            }
            item {
                OrcaListGroup {
                    OrcaRow(
                        title = "断开并清除本机配置",
                        subtitle = "清除后可重新填写 Gateway 地址",
                        titleColor = OrcaColors.accentRed,
                        onClick = onClear,
                    )
                }
            }
        }
    }
}

@Composable
private fun OrcaField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    imeAction: ImeAction,
    secret: Boolean = false,
) {
    val focus = LocalFocusManager.current
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = Modifier.fillMaxWidth(),
        label = {
            Text(
                text = label,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        placeholder = {
            Text(
                text = placeholder,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
            )
        },
        singleLine = true,
        textStyle =
            MaterialTheme.typography.bodyMedium.copy(
                fontFamily = if (secret) FontFamily.SansSerif else FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurface,
            ),
        visualTransformation = if (secret) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions = KeyboardOptions(imeAction = imeAction),
        keyboardActions = KeyboardActions(onDone = { focus.clearFocus() }),
        shape = RoundedCornerShape(OrcaRadius.input),
        colors =
            OutlinedTextFieldDefaults.colors(
                focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.35f),
                unfocusedContainerColor = Color.Transparent,
                focusedBorderColor = OrcaColors.accentBlue,
                unfocusedBorderColor = MaterialTheme.colorScheme.outline,
                cursorColor = OrcaColors.accentBlue,
            ),
    )
}
