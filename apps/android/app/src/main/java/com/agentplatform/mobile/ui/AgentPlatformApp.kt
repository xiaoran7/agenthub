package com.agentplatform.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.ConnectMode
import com.agentplatform.mobile.data.isCodexThreadId
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.PendingApproval
import com.agentplatform.mobile.data.SandboxMode
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.connectCommand
import com.agentplatform.mobile.data.describeGatewayError
import com.agentplatform.mobile.data.newHubSessionId
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.json.JSONObject

// =========================================================================
// AgentHub adaptive mobile & tablet shell
// =========================================================================

private const val DEST_SESSIONS = "sessions"
private const val DEST_HOSTS = "hosts"
private const val DEST_SETTINGS = "settings"

@Composable
fun AgentPlatformApp(controller: AppController) {
    val ui = controller.state
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val scope = rememberCoroutineScope()

    var destination by rememberSaveable { mutableStateOf(DEST_SESSIONS) }
    var openSessionKey by rememberSaveable { mutableStateOf<String?>(null) }
    var launchHostId by rememberSaveable { mutableStateOf<String?>(null) }
    var resumeDraft by rememberSaveable(stateSaver = ResumeDraftSaver) {
        mutableStateOf<ResumeDraft?>(null)
    }
    var providerFilterWire by rememberSaveable { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var launching by remember { mutableStateOf(false) }
    var resolvingApprovalId by remember { mutableStateOf<String?>(null) }
    var resumingCandidateKey by remember { mutableStateOf<String?>(null) }
    var seedTurn by remember { mutableStateOf<TurnSeed?>(null) }

    val refreshNow: () -> Unit = {
        if (!refreshing) {
            scope.launch {
                refreshing = true
                controller.refresh()
                refreshing = false
            }
        }
    }

    LaunchedEffect(controller, lifecycle, ui.signedIn, openSessionKey) {
        if (!ui.signedIn) return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                refreshing = true
                controller.refresh()
                refreshing = false
                val waiting =
                    controller.state.snapshot.sessions.any {
                        it.online && it.binding.executionState.waiting
                    }
                val watching = openSessionKey != null
                delay(
                    when {
                        watching && waiting -> 15_000L
                        watching -> 20_000L
                        waiting -> 4_000L
                        else -> 8_000L
                    },
                )
            }
        }
    }

    val selected =
        openSessionKey?.let { key ->
            ui.snapshot.sessions.firstOrNull { it.key == key }
        }

    // Also allow opening a freshly dispatched hubSessionId while the next snapshot poll is in flight.
    val pendingSelected =
        remember(openSessionKey, selected, ui.snapshot) {
            if (selected != null) {
                selected
            } else {
                val key = openSessionKey ?: return@remember null
                val sep = key.indexOf('/')
                if (sep <= 0) return@remember null
                val rId = key.substring(0, sep)
                val hId = key.substring(sep + 1)
                val host = ui.snapshot.host(rId) ?: return@remember null
                com.agentplatform.mobile.data.RemoteSessionRow(
                    runnerId = rId,
                    online = host.online,
                    binding =
                        com.agentplatform.mobile.data.RemoteBinding(
                            hubSessionId = hId,
                            providerSessionId = "",
                            workspacePath = host.workspaces.firstOrNull().orEmpty(),
                            hostGeneration = host.hostGeneration,
                            controlConnected = host.online,
                            executionState = com.agentplatform.mobile.data.ExecutionState.IDLE,
                            currentTurnId = null,
                        ),
                )
            }
        }

    BackHandler(enabled = openSessionKey != null || launchHostId != null || resumeDraft != null) {
        when {
            openSessionKey != null -> openSessionKey = null
            launchHostId != null -> launchHostId = null
            resumeDraft != null -> resumeDraft = null
        }
    }

    val destinations =
        remember(ui.snapshot.waitingCount) {
            listOf(
                BottomDestination(DEST_SESSIONS, "会话", badge = ui.snapshot.waitingCount),
                BottomDestination(DEST_HOSTS, "电脑"),
                BottomDestination(DEST_SETTINGS, "设置"),
            )
        }

    val quickResumeCandidate: (TreeEntry.Candidate) -> Unit = candidate@{ entry ->
        val host = ui.snapshot.host(entry.runnerId)
        val codex = isCodexThreadId(entry.providerSessionId)
        val canPreview = host?.codexSessionControl == true && codex
        val existing =
            ui.snapshot.sessions.firstOrNull {
                it.runnerId == entry.runnerId &&
                    it.binding.providerSessionId == entry.providerSessionId
            }
        if (codex && !canPreview) {
            if (existing != null) {
                openSessionKey = existing.key
            } else {
                controller.reportError("请先更新这台电脑上的 AgentHub Runner。")
            }
            return@candidate
        }
        if (existing != null && existing.binding.controlConnected) {
            controller.clearError()
            openSessionKey = existing.key
        } else if (host != null && host.online && (entry.resumable || canPreview) && resumingCandidateKey == null) {
            val hubId = existing?.binding?.hubSessionId
                ?: entry.providerSessionId.takeIf(::isCodexThreadId) ?: newHubSessionId()
            resumingCandidateKey = entry.key
            controller.clearError()
            controller.dispatch(
                runnerId = host.runnerId,
                hubSessionId = hubId,
                hostGeneration = host.hostGeneration.ifBlank { null },
                action =
                    connectCommand(
                        mode = if (canPreview) ConnectMode.PREVIEW else ConnectMode.RESUME,
                        workspacePath = entry.workspaceId,
                        providerSessionId = entry.providerSessionId,
                        sandbox = SandboxMode.READ_ONLY,
                    ),
            ) { result ->
                resumingCandidateKey = null
                result
                    .onSuccess {
                        openSessionKey = "${host.runnerId}/$hubId"
                        scope.launch { controller.refresh() }
                    }
                    .onFailure {
                        // Fall back to LaunchScreen prefilled if quick resume needs manual workspace adjustment
                        resumeDraft =
                            ResumeDraft(
                                runnerId = entry.runnerId,
                                workspaceId = entry.workspaceId,
                                providerSessionId = entry.providerSessionId,
                            )
                        controller.reportError(describeGatewayError(it))
                    }
            }
        } else if (entry.resumable) {
            resumeDraft =
                ResumeDraft(
                    runnerId = entry.runnerId,
                    workspaceId = entry.workspaceId,
                    providerSessionId = entry.providerSessionId,
                )
        }
    }

    val resolveInlineApproval: (TreeEntry.Active, PendingApproval, Boolean) -> Unit =
        { entry, approval, approved ->
            if (resolvingApprovalId == null) {
                resolvingApprovalId = approval.approvalId
                controller.dispatch(
                    runnerId = entry.runnerId,
                    hubSessionId = entry.hubSessionId,
                    hostGeneration = approval.hostGeneration.ifBlank { null },
                    action =
                        JSONObject()
                            .put("action", "resolve_approval")
                            .put("approval_id", approval.approvalId)
                            .put("approved", approved),
                ) { res ->
                    resolvingApprovalId = null
                    res
                        .onSuccess { scope.launch { controller.refresh() } }
                        .onFailure { controller.reportError(describeGatewayError(it)) }
                }
            }
        }

    val targetState =
        when {
            !ui.signedIn -> "gate"
            openSessionKey != null -> "session"
            launchHostId != null || resumeDraft != null -> "launch"
            else -> destination
        }

    BoxWithConstraints(
        modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background),
    ) {
        val useSideRail = maxWidth >= 720.dp
        val showPrimaryNav =
            ui.signedIn && (useSideRail || (openSessionKey == null && launchHostId == null && resumeDraft == null))

        Row(Modifier.fillMaxSize()) {
            if (useSideRail && showPrimaryNav) {
                SideNavigationRail(
                    destinations = destinations,
                    current = destination,
                    onSelect = {
                        destination = it
                        openSessionKey = null
                        launchHostId = null
                        resumeDraft = null
                        controller.clearError()
                    },
                )
            }

            Column(Modifier.weight(1f).fillMaxHeight()) {
                Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
                    AnimatedContent(
                        targetState = targetState,
                        transitionSpec = {
                            fadeIn(tween(MotionTokens.Standard))
                                .togetherWith(fadeOut(tween(MotionTokens.Quick)))
                                .using(SizeTransform(clip = false))
                        },
                        label = "screen",
                    ) { target ->
                        when (target) {
                            "gate" -> AuthGate(controller)
                            "session" -> {
                                val client = controller.gateway
                                val activeRow = pendingSelected
                                if (client != null && activeRow != null) {
                                    RemoteScreen(
                                        gateway = client,
                                        controller = controller,
                                        session = activeRow,
                                        hostGeneration =
                                            ui.snapshot
                                                .host(activeRow.runnerId)
                                                ?.hostGeneration
                                                .orEmpty(),
                                        onBack = { openSessionKey = null },
                                        onSwitchSession = { nextKey -> openSessionKey = nextKey },
                                        seedTurn = seedTurn,
                                        onSeedConsumed = { seedTurn = null },
                                    )
                                } else {
                                    LaunchedEffect(Unit) { openSessionKey = null }
                                }
                            }
                            "launch" -> {
                                val hostId = launchHostId ?: resumeDraft?.runnerId
                                val host = hostId?.let { ui.snapshot.host(it) }
                                if (host != null) {
                                    LaunchScreen(
                                        controller = controller,
                                        host = host,
                                        enabledPluginIds = ui.enabledPluginIds,
                                        submitting = launching,
                                        error = ui.error,
                                        resumeFrom = resumeDraft,
                                        onBack = {
                                            launchHostId = null
                                            resumeDraft = null
                                        },
                                        onLaunch = { command, hubId, firstPrompt, seed ->
                                            launching = true
                                            controller.dispatch(
                                                runnerId = host.runnerId,
                                                hubSessionId = hubId,
                                                hostGeneration = host.hostGeneration.ifBlank { null },
                                                action = command,
                                            ) { result ->
                                                launching = false
                                                result
                                                    .onSuccess {
                                                        launchHostId = null
                                                        resumeDraft = null
                                                        seedTurn = seed.copy(firstPrompt = firstPrompt)
                                                        openSessionKey = "${host.runnerId}/$hubId"
                                                        scope.launch { controller.refresh() }
                                                    }
                                                    .onFailure {
                                                        controller.reportError(describeGatewayError(it))
                                                    }
                                            }
                                        },
                                    )
                                } else {
                                    LaunchedEffect(Unit) {
                                        launchHostId = null
                                        resumeDraft = null
                                    }
                                }
                            }
                            DEST_SESSIONS ->
                                Box(
                                    modifier = Modifier.fillMaxSize(),
                                    contentAlignment = Alignment.TopCenter,
                                ) {
                                    Box(Modifier.widthIn(max = 920.dp).fillMaxSize()) {
                                        SessionsScreen(
                                            snapshot = ui.snapshot,
                                            enabledPluginIds = ui.enabledPluginIds,
                                            loading = ui.loading && ui.snapshot === Snapshot.Empty,
                                            refreshing = refreshing,
                                            resumingKey = resumingCandidateKey,
                                            resolvingApprovalId = resolvingApprovalId,
                                            error = ui.error,
                                            providerFilter =
                                                providerFilterWire?.let { EngineKind.fromWire(it) },
                                            onFilterChange = { providerFilterWire = it?.wire },
                                            onOpenEntry = { entry ->
                                                when (entry) {
                                                    is TreeEntry.Active -> openSessionKey = entryKeyOf(entry)
                                                    is TreeEntry.Candidate -> quickResumeCandidate(entry)
                                                }
                                                controller.clearError()
                                            },
                                            onConfigureResume = { entry ->
                                                resumeDraft =
                                                    ResumeDraft(
                                                        runnerId = entry.runnerId,
                                                        workspaceId = entry.workspaceId,
                                                        providerSessionId = entry.providerSessionId,
                                                    )
                                                controller.clearError()
                                            },
                                            onResolveApproval = resolveInlineApproval,
                                            onLaunchNew = { runnerId ->
                                                launchHostId = runnerId
                                                controller.clearError()
                                            },
                                            onOpenHost = { destination = DEST_HOSTS },
                                            onRefresh = refreshNow,
                                        )
                                    }
                                }
                            DEST_HOSTS ->
                                Box(
                                    modifier = Modifier.fillMaxSize(),
                                    contentAlignment = Alignment.TopCenter,
                                ) {
                                    Box(Modifier.widthIn(max = 920.dp).fillMaxSize()) {
                                        HostsScreen(
                                            controller = controller,
                                            snapshot = ui.snapshot,
                                            loading = ui.loading && ui.snapshot === Snapshot.Empty,
                                            error = ui.error,
                                            onOpenSession = { key -> openSessionKey = key },
                                            onLaunch = { runnerId -> launchHostId = runnerId },
                                            onRefresh = refreshNow,
                                        )
                                    }
                                }
                            else ->
                                Box(
                                    modifier = Modifier.fillMaxSize(),
                                    contentAlignment = Alignment.TopCenter,
                                ) {
                                    Box(Modifier.widthIn(max = 840.dp).fillMaxSize()) {
                                        SettingsScreen(
                                            controller = controller,
                                            account = ui.account,
                                            config = ui.config,
                                            appearance = ui.appearance,
                                        )
                                    }
                                }
                        }
                    }
                }
                if (!useSideRail && showPrimaryNav) {
                    BottomBar(
                        destinations = destinations,
                        current = destination,
                        onSelect = {
                            destination = it
                            controller.clearError()
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun AuthGate(controller: AppController) {
    val ui = controller.state
    var base by rememberSaveable { mutableStateOf(controller.savedGatewayBase) }
    var email by rememberSaveable { mutableStateOf(controller.savedEmail) }
    var password by remember { mutableStateOf("") }
    var token by remember { mutableStateOf(ui.config?.accessToken.orEmpty()) }
    var devUser by rememberSaveable { mutableStateOf(controller.savedDevUser) }
    var advanced by rememberSaveable { mutableStateOf(false) }

    Box(
        modifier =
            Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars))
                .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier =
                Modifier
                    .widthIn(max = 460.dp)
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = OrcaSpace.xl, vertical = OrcaSpace.xl),
            verticalArrangement = Arrangement.Center,
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                OrcaLogo(size = 26.dp, tint = MaterialTheme.colorScheme.onBackground)
                Text(
                    text = "AgentHub",
                    style = MaterialTheme.typography.headlineMedium,
                    color = MaterialTheme.colorScheme.onBackground,
                )
            }
            Text(
                text = "电脑执行，手机接着做。",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = OrcaSpace.xs, bottom = OrcaSpace.xl),
            )

            OrcaCard {
                Column(
                    modifier = Modifier.fillMaxWidth().padding(OrcaSpace.lg),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.md),
                ) {
                    GatewayField(
                        value = base,
                        onValueChange = { base = it },
                        label = "Gateway 地址",
                        placeholder = "https://gateway.example.com",
                    )
                    GatewayField(
                        value = email,
                        onValueChange = { email = it },
                        label = "邮箱",
                        placeholder = "you@example.com",
                    )
                    GatewayField(
                        value = password,
                        onValueChange = { password = it },
                        label = "密码",
                        placeholder = "账号密码",
                        secret = true,
                    )
                    if (ui.error != null) {
                        Text(
                            text = ui.error,
                            style = MaterialTheme.typography.bodySmall,
                            color = OrcaColors.accentRed,
                        )
                    }
                    OrcaPrimaryButton(
                        label = if (ui.authenticating) "正在登录…" else "登录",
                        enabled = !ui.authenticating && base.isNotBlank() && email.isNotBlank(),
                    ) {
                        controller.login(email = email, password = password, gatewayBase = base)
                    }
                    if (ui.config?.hasCredential == true || ui.config?.developmentUserId?.isNotBlank() == true) {
                        OrcaTextAction(
                            label = if (ui.authenticating) "正在重新连接…" else "重试已保存的登录",
                            enabled = !ui.authenticating,
                        ) {
                            controller.signInWithSavedCredential()
                        }
                    }
                    OrcaTextAction(
                        label = if (advanced) "收起高级连接" else "改用访问令牌 / 本机开发",
                    ) { advanced = !advanced }
                    if (advanced) {
                        GatewayField(
                            value = token,
                            onValueChange = { token = it },
                            label = "访问令牌（单机部署）",
                            placeholder = "Bearer token",
                            secret = true,
                        )
                        GatewayField(
                            value = devUser,
                            onValueChange = { devUser = it },
                            label = "开发用户（仅本机回环）",
                            placeholder = "local-user",
                        )
                        OrcaSecondaryButton("用此凭据连接") {
                            val error =
                                controller.saveConfig(
                                    AppConfig(
                                        gatewayBase = base,
                                        accessToken = token,
                                        developmentUserId = devUser,
                                    ),
                                )
                            if (error != null) {
                                controller.reportError(error)
                            } else {
                                controller.rememberServerAddress(
                                    gatewayBase = base,
                                    email = email,
                                    devUser = devUser,
                                )
                            }
                        }
                    }
                }
            }

            Text(
                text = "凭据由 Android Keystore 加密保存，不写入日志，不参与备份。",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = OrcaSpace.md),
            )
        }
    }
}

@Composable
internal fun GatewayField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    secret: Boolean = false,
    singleLine: Boolean = true,
) {
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
        singleLine = singleLine,
        textStyle =
            MaterialTheme.typography.bodyMedium.copy(
                fontFamily = if (secret) FontFamily.SansSerif else FontFamily.Default,
                color = MaterialTheme.colorScheme.onSurface,
            ),
        visualTransformation =
            if (secret) PasswordVisualTransformation() else VisualTransformation.None,
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

@Composable
internal fun ScreenHeader(
    title: String,
    subtitle: String,
    statusColor: Color,
    onBack: (() -> Unit)? = null,
    actions: @Composable () -> Unit = {},
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(horizontal = OrcaSpace.sm, vertical = OrcaSpace.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (onBack != null) {
            OrcaIconButton(onClick = onBack) {
                OrcaBackChevron(MaterialTheme.colorScheme.onSurface)
            }
        } else {
            Spacer(Modifier.width(OrcaSpace.sm))
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
                overflow = TextOverflow.Ellipsis,
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
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        actions()
    }
}
