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
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.AgentGateway
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.GatewayException
import java.net.URLEncoder
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

// =========================================================================
// AgentHub mobile companion — Orca-style stack shell.
//
// Structure mirrors stablyai/orca mobile: Home (fleet) -> Host (worktrees and
// sessions) -> Session (live stream). Data comes from AgentHub's own Gateway
// `/v3/remote` plane; no Orca backend is involved.
// =========================================================================

private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
private fun encoded(value: String): String = URLEncoder.encode(value, "UTF-8")

private const val ROUTE_HOME = "home"
private const val ROUTE_HOST = "host"
private const val ROUTE_SESSION = "session"
private const val ROUTE_SETTINGS = "settings"

@Composable
fun AgentPlatformApp(controller: AppController) {
    val ui = controller.state
    val config = ui.config
    val gateway = remember(config) { config?.let(::AgentGateway) }
    val scope = rememberCoroutineScope()

    var route by rememberSaveable { mutableStateOf(ROUTE_HOME) }
    var hostId by rememberSaveable { mutableStateOf<String?>(null) }
    var sessionRunner by rememberSaveable { mutableStateOf<String?>(null) }
    var sessionHub by rememberSaveable { mutableStateOf<String?>(null) }

    var snapshot by remember { mutableStateOf(JSONObject()) }
    var fleetError by remember { mutableStateOf("") }
    var fleetLoading by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }

    // Single fleet poll shared by Home, Host and Session chrome.
    LaunchedEffect(gateway) {
        if (gateway == null) {
            snapshot = JSONObject()
            return@LaunchedEffect
        }
        while (true) {
            fleetLoading = true
            try {
                snapshot =
                    withContext(Dispatchers.IO) { gateway.remoteRequest("/sessions") }
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
            delay(2_000)
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

    BackHandler(enabled = route != ROUTE_HOME) {
        when (route) {
            ROUTE_SESSION -> route = ROUTE_HOST
            ROUTE_HOST, ROUTE_SETTINGS -> route = ROUTE_HOME
        }
    }

    AnimatedContent(
        targetState = route,
        transitionSpec = {
            val forward = targetState != ROUTE_SETTINGS && initialState == ROUTE_HOME
            val direction = if (forward) 1 else -1
            (
                fadeIn(tween(MotionTokens.Standard)) +
                    slideInHorizontally(tween(MotionTokens.Emphasized)) { it / 10 * direction }
            ).togetherWith(
                fadeOut(tween(MotionTokens.Quick)) +
                    slideOutHorizontally(tween(MotionTokens.Standard)) { -it / 14 * direction },
            ).using(SizeTransform(clip = false))
        },
        label = "orca-route",
    ) { target ->
        when (target) {
            ROUTE_HOME ->
                OrcaHomeScreen(
                    config = config,
                    snapshot = snapshot,
                    loading = fleetLoading,
                    error = fleetError,
                    onOpenSettings = { route = ROUTE_SETTINGS },
                    onOpenHost = { id ->
                        hostId = id
                        route = ROUTE_HOST
                    },
                    onRefresh = { gateway?.let { fleetLoading = true; scope.launch { runCatching { snapshot = withContext(Dispatchers.IO) { it.remoteRequest("/sessions") } } } } },
                )

            ROUTE_HOST ->
                OrcaHostScreen(
                    snapshot = snapshot,
                    hostId = hostId,
                    busy = busy,
                    error = fleetError,
                    onBack = { route = ROUTE_HOME },
                    onRefresh = {
                        gateway?.let { g ->
                            scope.launch {
                                runCatching {
                                    snapshot = withContext(Dispatchers.IO) { g.remoteRequest("/sessions") }
                                }
                            }
                        }
                    },
                    onOpenSession = { runner, hub ->
                        sessionRunner = runner
                        sessionHub = hub
                        route = ROUTE_SESSION
                    },
                    onCreate = { runner, workspace, generation ->
                        dispatch(
                            runner = runner,
                            hub = UUID.randomUUID().toString(),
                            action =
                                JSONObject()
                                    .put("action", "connect")
                                    .put("mode", "create")
                                    .put("workspace_path", workspace)
                                    .put("sandbox", "read-only")
                                    .put("approval_policy", "untrusted"),
                            generation = generation,
                        )
                    },
                    onResume = { runner, workspace, providerSession, generation ->
                        dispatch(
                            runner = runner,
                            hub = UUID.randomUUID().toString(),
                            action =
                                JSONObject()
                                    .put("action", "connect")
                                    .put("mode", "resume")
                                    .put("workspace_path", workspace)
                                    .put("provider_session_id", providerSession)
                                    .put("sandbox", "read-only")
                                    .put("approval_policy", "untrusted"),
                            generation = generation,
                        )
                    },
                    onOpenSettings = { route = ROUTE_SETTINGS },
                )

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
                        onBack = { route = ROUTE_HOST },
                    )
                } else {
                    route = ROUTE_HOST
                }
            }

            ROUTE_SETTINGS ->
                OrcaSettingsScreen(
                    config = config,
                    onBack = { route = ROUTE_HOME },
                    onSave = { next ->
                        controller.saveConfig(next).also { error ->
                            if (error == null) route = ROUTE_HOME
                        }
                    },
                    onClear = {
                        controller.clearConfig()
                        route = ROUTE_HOME
                    },
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
                401, 403 -> "认证失效，请在设置中更新访问令牌。"
                null -> "无法连接 Gateway，请检查地址与网络。"
                else -> "Gateway 暂时不可用，请稍后重试。"
            }
    }

private fun bindingState(binding: JSONObject?): String =
    binding?.optString("execution_state").orEmpty().ifBlank { "unknown" }

private fun shortWorkspace(raw: String): String =
    if (raw.startsWith("ws-")) raw.removePrefix("ws-").replace(Regex("-[0-9a-f]{16}$"), "")
    else raw.substringAfterLast('/').substringAfterLast('\\')

// =========================================================================
// Home — Orca "Current" layout: brand, greeting, stats, desktops, settings
// =========================================================================

@Composable
private fun OrcaHomeScreen(
    config: AppConfig?,
    snapshot: JSONObject,
    loading: Boolean,
    error: String,
    onOpenSettings: () -> Unit,
    onOpenHost: (String) -> Unit,
    onRefresh: () -> Unit,
) {
    val runners = snapshot.optJSONArray("runners")?.objects().orEmpty()
    val sessions = snapshot.optJSONArray("sessions")?.objects().orEmpty()
    val online = runners.count { it.optBoolean("online") }
    val activeAgents = sessions.count { bindingState(it.optJSONObject("binding")) == "running" }
    val waiting =
        sessions.count {
            bindingState(it.optJSONObject("binding")) in setOf("awaiting_permission", "awaiting_input")
        }

    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 6.dp, bottom = 40.dp),
    ) {
        item {
            OrcaHomeTopBar(
                loading = loading,
                configured = config != null,
                onRefresh = onRefresh,
                onOpenSettings = onOpenSettings,
            )
        }

        if (config == null) {
            item { OrcaOnboarding(onOpenSettings) }
            return@LazyColumn
        }

        item {
            Text(
                text = "欢迎回来",
                style = MaterialTheme.typography.headlineLarge.copy(fontSize = 24.sp),
                fontWeight = FontWeight.ExtraBold,
                color = MaterialTheme.colorScheme.onBackground,
                modifier = Modifier.padding(top = 18.dp, bottom = 14.dp),
            )
        }

        item {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                OrcaStatCard(
                    value = "$online / ${runners.size}",
                    label = "在线主机",
                    modifier = Modifier.weight(1f),
                    highlight = if (online > 0) MaterialTheme.colorScheme.onSurface else null,
                )
                OrcaStatCard(
                    value = "$activeAgents",
                    label = "活跃智能体",
                    modifier = Modifier.weight(1f),
                    highlight = if (activeAgents > 0) OrcaColors.accentBlue else null,
                )
                OrcaStatCard(
                    value = "$waiting",
                    label = "待审批",
                    modifier = Modifier.weight(1f),
                    highlight = if (waiting > 0) OrcaColors.accentAmber else null,
                )
            }
        }

        if (error.isNotBlank()) {
            item {
                OrcaInlineError(error, Modifier.padding(top = 12.dp))
            }
        }

        item { OrcaSectionHeader("主机") }

        if (runners.isEmpty()) {
            item {
                OrcaCard {
                    OrcaNoticeCard(
                        title = if (loading) "正在发现主机…" else "还没有已接入的主机",
                        body = "在电脑上运行 agenthub-runner daemon，并确保它已连接同一个 Gateway，主机就会自动出现在这里。",
                    )
                }
            }
        } else {
            itemsIndexed(runners, key = { _, runner -> runner.optString("runner_id") }) { _, runner ->
                val runnerId = runner.optString("runner_id")
                val hostSessions = sessions.filter { it.optString("runner_id") == runnerId }
                OrcaHostRow(
                    runnerId = runnerId,
                    online = runner.optBoolean("online"),
                    workspaceCount = runner.optJSONObject("hello")?.optJSONArray("workspaces")?.length() ?: 0,
                    activeCount =
                        hostSessions.count {
                            bindingState(it.optJSONObject("binding")) == "running"
                        },
                    onClick = { onOpenHost(runnerId) },
                )
                Spacer(Modifier.height(8.dp))
            }
        }

        item {
            OrcaPairRow(onClick = onOpenSettings)
        }

        if (sessions.isNotEmpty()) {
            item { OrcaSectionHeader("最近会话") }
            items(sessions.take(4), key = { it.optString("runner_id") + it.optJSONObject("binding")?.optString("hub_session_id") }) { item ->
                val binding = item.optJSONObject("binding") ?: return@items
                val state = bindingState(binding)
                OrcaResumeRow(
                    title = shortWorkspace(binding.optString("workspace_path")),
                    meta = stateLabel(state),
                    stateColor = stateColor(state),
                    onClick = { onOpenHost(item.optString("runner_id")) },
                )
                Spacer(Modifier.height(8.dp))
            }
        }

        item {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 34.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Surface(
                    modifier = Modifier
                        .size(38.dp)
                        .clip(RoundedCornerShape(10.dp))
                        .clickable(onClick = onOpenSettings),
                    shape = RoundedCornerShape(10.dp),
                    color = Color.Transparent,
                ) {
                    Box(contentAlignment = Alignment.Center) {
                        OrcaSettingsIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                Text(
                    "设置",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp),
                )
            }
        }
    }
}

@Composable
private fun OrcaHomeTopBar(
    loading: Boolean,
    configured: Boolean,
    onRefresh: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OrcaLogo(size = 20, tint = MaterialTheme.colorScheme.onBackground)
            Text(
                "AgentHub",
                style = MaterialTheme.typography.titleLarge,
                fontWeight = FontWeight.Bold,
                color = MaterialTheme.colorScheme.onBackground,
            )
            if (configured) {
                OrcaStatusDot(
                    color = if (loading) OrcaColors.accentAmber else OrcaColors.accentGreen,
                    size = 6,
                )
            }
        }

        Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            OrcaIconButton(onClick = onRefresh) {
                if (loading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(15.dp),
                        strokeWidth = 1.7.dp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    OrcaRefreshIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            OrcaIconButton(onClick = onOpenSettings) {
                OrcaSettingsIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
    }
}

@Composable
private fun OrcaHostRow(
    runnerId: String,
    online: Boolean,
    workspaceCount: Int,
    activeCount: Int,
    onClick: () -> Unit,
) {
    OrcaCard(onClick = onClick) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 12.dp, top = 12.dp, bottom = 12.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            HostStatusTile(online = online)
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 14.dp, end = 8.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    hostDisplayName(runnerId),
                    style = MaterialTheme.typography.titleMedium.copy(fontSize = 15.sp),
                    fontWeight = FontWeight.SemiBold,
                    color =
                        if (online) MaterialTheme.colorScheme.onSurface
                        else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    "$workspaceCount 个工作区" + if (activeCount > 0) " · $activeCount 个运行中" else "",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    modifier = Modifier.padding(top = 3.dp),
                ) {
                    OrcaStatusDot(
                        color = if (online) OrcaColors.accentGreen else MaterialTheme.colorScheme.outline,
                        size = 7,
                    )
                    Text(
                        if (online) "已连接" else "已断开",
                        style = MaterialTheme.typography.bodySmall,
                        color =
                            if (online) OrcaColors.accentGreen
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        runnerId.takeLast(8),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }
            OrcaChevronRight(tint = OrcaColors.textMuted)
        }
    }
}

@Composable
private fun OrcaPairRow(onClick: () -> Unit) {
    OrcaCard(onClick = onClick) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 12.dp, top = 12.dp, bottom = 12.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OrcaIconTile {
                OrcaPlusIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 14.dp, end = 8.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    "接入另一台主机",
                    style = MaterialTheme.typography.titleMedium.copy(fontSize = 15.sp),
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    "在电脑上运行 agenthub-runner daemon",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            OrcaChevronRight(tint = OrcaColors.textMuted)
        }
    }
}

@Composable
private fun OrcaResumeRow(
    title: String,
    meta: String,
    stateColor: Color,
    onClick: () -> Unit,
) {
    OrcaCard(onClick = onClick) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 12.dp, top = 12.dp, bottom = 12.dp, end = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OrcaIconTile(size = 38) {
                OrcaTerminalIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant, size = 16)
            }
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 12.dp, end = 8.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    OrcaStatusDot(color = stateColor, size = 7)
                    Text(
                        meta,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            OrcaChevronRight(tint = OrcaColors.textMuted)
        }
    }
}

@Composable
private fun OrcaOnboarding(onOpenSettings: () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 44.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            "欢迎使用 AgentHub",
            style = MaterialTheme.typography.headlineLarge.copy(fontSize = 24.sp),
            fontWeight = FontWeight.ExtraBold,
        )
        Spacer(Modifier.height(26.dp))
        Box(
            modifier = Modifier
                .size(80.dp)
                .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(20.dp)),
            contentAlignment = Alignment.Center,
        ) {
            OrcaMonitorIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant, size = 32)
        }
        Spacer(Modifier.height(18.dp))
        Text(
            "连接你的电脑",
            style = MaterialTheme.typography.titleLarge,
            fontWeight = FontWeight.Bold,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            "在电脑上运行 AgentHub Runner，手机端即可实时查看并接管 Coding Assistant、Codex 的会话，无需开放任何公网入站端口。",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            lineHeight = 19.sp,
            modifier = Modifier.padding(horizontal = 10.dp),
        )
        Spacer(Modifier.height(20.dp))
        OrcaPrimaryButton("配置 Gateway 端点", onClick = onOpenSettings)
        Spacer(Modifier.height(34.dp))
        OrcaSectionHeader("如何接入")
        OrcaConnectStep(1, "启动 Runner", "在电脑终端执行 agenthub-runner daemon，让设备向 Gateway 主动出站注册。")
        OrcaDivider(insetStart = 0)
        OrcaConnectStep(2, "保持同一 Gateway", "手机与 Runner 必须指向同一个 Gateway 地址，会话与审批才会出现在主机列表里。")
        OrcaDivider(insetStart = 0)
        OrcaConnectStep(3, "自动接入", "回到首页下拉刷新，设备上线后即可进入主机查看工作区与活动会话。")
    }
}

@Composable
private fun OrcaConnectStep(index: Int, title: String, body: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            "$index",
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(18.dp),
        )
        Column(verticalArrangement = Arrangement.spacedBy(3.dp), modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
            Text(
                body,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                lineHeight = 18.sp,
            )
        }
    }
}

// =========================================================================
// Host — Orca host screen: header, live sessions, workspaces, discovery
// =========================================================================

@Composable
private fun OrcaHostScreen(
    snapshot: JSONObject,
    hostId: String?,
    busy: Boolean,
    error: String,
    onBack: () -> Unit,
    onRefresh: () -> Unit,
    onOpenSession: (String, String) -> Unit,
    onCreate: (String, String, String) -> Unit,
    onResume: (String, String, String, String) -> Unit,
    onOpenSettings: () -> Unit,
) {
    val runners = snapshot.optJSONArray("runners")?.objects().orEmpty()
    val runner = runners.firstOrNull { it.optString("runner_id") == hostId }
    val hello = runner?.optJSONObject("hello")
    val generation = hello?.optString("host_generation").orEmpty()
    val online = runner?.optBoolean("online") == true
    val workspaces = hello?.optJSONArray("workspaces")?.objects()?.map { it.toString() }.orEmpty()
    val allSessions = snapshot.optJSONArray("sessions")?.objects().orEmpty()
    val hostSessions = allSessions.filter { it.optString("runner_id") == hostId }
    val activeIds =
        hostSessions.mapNotNull {
            it.optJSONObject("binding")?.optString("provider_session_id")
        }.toSet()
    val candidates =
        hello?.optJSONObject("discovery")?.optJSONArray("candidates")?.objects().orEmpty()
            .filter { it.optString("provider_session_id") !in activeIds }

    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 40.dp),
    ) {
        item {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 4.dp, bottom = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OrcaIconButton(onClick = onBack) {
                    OrcaBackChevron(tint = MaterialTheme.colorScheme.onSurface)
                }
                OrcaStatusDot(
                    color = if (online) OrcaColors.accentGreen else MaterialTheme.colorScheme.outline,
                    size = 8,
                    modifier = Modifier.padding(horizontal = 6.dp),
                )
                Text(
                    hostDisplayName(hostId.orEmpty()),
                    style = MaterialTheme.typography.titleMedium.copy(fontSize = 16.sp),
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.weight(1f),
                )
                OrcaIconButton(onClick = onRefresh) {
                    OrcaRefreshIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                OrcaIconButton(onClick = onOpenSettings) {
                    OrcaSettingsIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }

        if (runner == null) {
            item {
                OrcaCard {
                    OrcaNoticeCard("主机已不在机队中", "它可能已被撤销或尚未连接 Gateway。返回首页刷新重试。")
                }
            }
            return@LazyColumn
        }

        item {
            Row(
                modifier = Modifier.padding(bottom = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OrcaChip(
                    text = if (online) "已连接" else "已断开",
                    color = if (online) OrcaColors.accentGreen else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                OrcaChip(text = "${workspaces.size} 个工作区")
                OrcaChip(text = "${hostSessions.size} 个会话")
                OrcaChip(text = hello?.optString("provider").orEmpty().ifBlank { "codex_app_server" })
            }
        }

        if (error.isNotBlank()) {
            item { OrcaInlineError(error, Modifier.padding(top = 10.dp)) }
        }

        item { OrcaSectionHeader("活动会话") }
        if (hostSessions.isEmpty()) {
            item {
                OrcaCard {
                    OrcaNoticeCard("还没有会话", "在下方选择一个工作区发起任务，或在电脑上继续已有 Agent 会话。")
                }
            }
        } else {
            items(hostSessions, key = { it.optJSONObject("binding")?.optString("hub_session_id").orEmpty() }) { item ->
                val binding = item.optJSONObject("binding") ?: return@items
                val state = bindingState(binding)
                val control = binding.optBoolean("control_connected") && item.optBoolean("online")
                OrcaSessionRow(
                    workspace = shortWorkspace(binding.optString("workspace_path")),
                    providerSession = binding.optString("provider_session_id"),
                    state = state,
                    control = control,
                    onClick = {
                        onOpenSession(item.optString("runner_id"), binding.optString("hub_session_id"))
                    },
                )
                Spacer(Modifier.height(8.dp))
            }
        }

        item { OrcaSectionHeader("工作区") }
        if (workspaces.isEmpty()) {
            item {
                OrcaCard {
                    OrcaNoticeCard("没有可用的工作区", "启动 Runner 时通过 --workspace 指定工作目录，或先在电脑上创建一次会话。")
                }
            }
        } else {
            itemsIndexed(workspaces, key = { _, workspace -> workspace }) { _, workspace ->
                OrcaWorkspaceRow(
                    workspace = workspace,
                    busy = busy,
                    enabled = online && generation.isNotBlank(),
                    onCreate = { onCreate(hostId.orEmpty(), workspace, generation) },
                )
                Spacer(Modifier.height(8.dp))
            }
        }

        if (candidates.isNotEmpty()) {
            item { OrcaSectionHeader("发现的已有会话") }
            items(candidates, key = { it.optString("provider_session_id") }) { candidate ->
                val providerId = candidate.optString("provider_session_id")
                OrcaCandidateRow(
                    title = candidate.optString("title").ifBlank { providerId.take(12) },
                    workspace = shortWorkspace(candidate.optString("workspace_path")),
                    busy = busy,
                    enabled = online && generation.isNotBlank(),
                    onResume = {
                        onResume(
                            hostId.orEmpty(),
                            candidate.optString("workspace_path"),
                            providerId,
                            generation,
                        )
                    },
                )
                Spacer(Modifier.height(8.dp))
            }
        }
    }
}

@Composable
private fun OrcaSessionRow(
    workspace: String,
    providerSession: String,
    state: String,
    control: Boolean,
    onClick: () -> Unit,
) {
    OrcaCard(onClick = onClick) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 12.dp, top = 12.dp, bottom = 12.dp, end = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OrcaIconTile(size = 38) {
                OrcaTerminalIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant, size = 16)
            }
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 12.dp, end = 8.dp),
                verticalArrangement = Arrangement.spacedBy(3.dp),
            ) {
                Text(
                    workspace,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    OrcaStatusDot(color = stateColor(state), size = 7)
                    Text(
                        stateLabel(state),
                        style = MaterialTheme.typography.bodySmall,
                        color = stateColor(state),
                    )
                    Text(
                        providerSession.take(12),
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (!control) {
                        OrcaChip(text = "离线", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
            OrcaChevronRight(tint = OrcaColors.textMuted)
        }
    }
}

@Composable
private fun OrcaWorkspaceRow(
    workspace: String,
    busy: Boolean,
    enabled: Boolean,
    onCreate: () -> Unit,
) {
    OrcaCard {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 12.dp, top = 10.dp, bottom = 10.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OrcaIconTile(size = 38) {
                OrcaTerminalIcon(tint = MaterialTheme.colorScheme.onSurfaceVariant, size = 16)
            }
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(start = 12.dp, end = 8.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    shortWorkspace(workspace),
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    workspace,
                    style = MaterialTheme.typography.labelSmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Surface(
                modifier = Modifier
                    .clip(RoundedCornerShape(6.dp))
                    .clickable(enabled = enabled && !busy, onClick = onCreate),
                shape = RoundedCornerShape(6.dp),
                color =
                    if (enabled && !busy) MaterialTheme.colorScheme.surfaceVariant
                    else MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
                border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
            ) {
                Text(
                    if (busy) "下发中…" else "发起任务",
                    style = MaterialTheme.typography.bodySmall,
                    fontWeight = FontWeight.SemiBold,
                    color =
                        if (enabled && !busy) MaterialTheme.colorScheme.onSurface
                        else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                    modifier = Modifier.padding(horizontal = 10.dp, vertical = 5.dp),
                )
            }
        }
    }
}

@Composable
private fun OrcaCandidateRow(
    title: String,
    workspace: String,
    busy: Boolean,
    enabled: Boolean,
    onResume: () -> Unit,
) {
    OrcaCard {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 12.dp, top = 10.dp, bottom = 10.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(
                modifier = Modifier
                    .weight(1f)
                    .padding(end = 8.dp),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    workspace,
                    style = MaterialTheme.typography.labelSmall,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                )
            }
            Text(
                if (busy) "下发中…" else "接管",
                style = MaterialTheme.typography.bodySmall,
                fontWeight = FontWeight.Bold,
                color =
                    if (enabled && !busy) OrcaColors.accentBlue
                    else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                modifier = Modifier
                    .clip(RoundedCornerShape(6.dp))
                    .clickable(enabled = enabled && !busy, onClick = onResume)
                    .padding(horizontal = 8.dp, vertical = 5.dp),
            )
        }
    }
}

// =========================================================================
// Settings — Orca-style connection form
// =========================================================================

@Composable
private fun OrcaSettingsScreen(
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
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding()
            .imePadding(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 40.dp),
    ) {
        item {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 4.dp, bottom = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                OrcaIconButton(onClick = onBack) {
                    OrcaBackChevron(tint = MaterialTheme.colorScheme.onSurface)
                }
                Text(
                    "设置",
                    style = MaterialTheme.typography.titleMedium.copy(fontSize = 16.sp),
                    fontWeight = FontWeight.Bold,
                    modifier = Modifier.weight(1f).padding(start = 4.dp),
                )
                if (config != null) {
                    OrcaStatusDot(color = OrcaColors.accentGreen, size = 7)
                    Text(
                        "已连接",
                        style = MaterialTheme.typography.bodySmall,
                        color = OrcaColors.accentGreen,
                        modifier = Modifier.padding(start = 6.dp),
                    )
                }
            }
        }

        item { OrcaSectionHeader("网关连接") }

        item {
            OrcaCard {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    OrcaField(
                        value = gatewayBase,
                        onValueChange = { gatewayBase = it },
                        label = "Gateway 端点",
                        placeholder = "http://127.0.0.1:3001",
                        imeAction = ImeAction.Next,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        OrcaChip(
                            text = "127.0.0.1:3001",
                            color = MaterialTheme.colorScheme.onSurface,
                            onClick = {
                                gatewayBase = "http://127.0.0.1:3001"
                                developmentUserId = "local-developer"
                                error = null
                            },
                        )
                        OrcaChip(
                            text = "10.0.2.2 (模拟器)",
                            onClick = {
                                gatewayBase = "http://10.0.2.2:3001"
                                developmentUserId = "local-developer"
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
                        label = "开发用户（仅回环地址有效）",
                        placeholder = "local-developer",
                        imeAction = ImeAction.Done,
                    )

                    error?.let {
                        Text(
                            it,
                            color = OrcaColors.accentRed,
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }

                    OrcaPrimaryButton(
                        label = if (config == null) "保存并连接" else "更新连接",
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
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 18.dp),
                horizontalArrangement = Arrangement.Center,
            ) {
                Text(
                    "凭据由 Android Keystore 加密保存，不写入日志，不参与备份。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        item { OrcaSectionHeader("接入步骤") }
        item {
            Column {
                OrcaConnectStep(1, "启动 Runner", "在电脑终端执行 agenthub-runner daemon，保持出站连接。")
                OrcaDivider(insetStart = 0)
                OrcaConnectStep(2, "填写同一 Gateway", "手机端填写的地址和令牌必须与 Runner 配置一致。")
                OrcaDivider(insetStart = 0)
                OrcaConnectStep(3, "返回首页", "设备上线后会自动出现在主机列表，点进去即可接管会话。")
            }
        }

        if (config != null) {
            item {
                TextButton(
                    onClick = onClear,
                    modifier = Modifier.fillMaxWidth().padding(top = 18.dp),
                ) {
                    Text("断开并清除本机配置", color = OrcaColors.accentRed)
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
                label,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        placeholder = {
            Text(
                placeholder,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
            )
        },
        singleLine = true,
        textStyle = MaterialTheme.typography.bodyMedium.copy(
            fontFamily = if (secret) FontFamily.SansSerif else FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurface,
        ),
        visualTransformation =
            if (secret) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
        keyboardOptions = KeyboardOptions(imeAction = imeAction),
        keyboardActions = KeyboardActions(onDone = { focus.clearFocus() }),
        shape = RoundedCornerShape(6.dp),
        colors =
            OutlinedTextFieldDefaults.colors(
                focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
                unfocusedContainerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.25f),
                focusedBorderColor = OrcaColors.accentBlue,
                unfocusedBorderColor = MaterialTheme.colorScheme.outline,
                cursorColor = OrcaColors.accentBlue,
            ),
    )
}
