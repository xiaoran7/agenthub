package com.agentplatform.mobile.ui

import android.net.Uri
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.ConnectMode
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.PermissionMode
import com.agentplatform.mobile.data.RemoteAttachment
import com.agentplatform.mobile.data.SandboxMode
import com.agentplatform.mobile.data.connectCommand
import com.agentplatform.mobile.data.engineOfSession
import com.agentplatform.mobile.data.isCodexThreadId
import com.agentplatform.mobile.data.matchCatalogModel
import com.agentplatform.mobile.data.modelsForEngine
import com.agentplatform.mobile.data.newHubSessionId
import com.agentplatform.mobile.data.sendPromptCommand
import kotlinx.coroutines.launch
import org.json.JSONObject

// =========================================================================
// LaunchScreen: branded welcome + unified floating composer with attachments
// =========================================================================

@Immutable
data class ResumeDraft(
    val runnerId: String,
    val workspaceId: String,
    val providerSessionId: String,
)

internal val ResumeDraftSaver: Saver<ResumeDraft?, Any> =
    listSaver(
        save = { draft ->
            draft?.let { listOf(it.runnerId, it.workspaceId, it.providerSessionId) }.orEmpty()
        },
        restore = { parts ->
            if (parts.size == 3) ResumeDraft(parts[0], parts[1], parts[2]) else null
        },
    )

internal data class TurnSeed(
    val effort: EffortLevel,
    val model: String,
    val permissionMode: PermissionMode,
    val permissionTouched: Boolean = false,
    val firstPrompt: JSONObject? = null,
)

@Composable
internal fun LaunchScreen(
    controller: AppController,
    initialHost: HostDevice,
    allHosts: List<HostDevice>,
    initialEngineWire: String?,
    submitting: Boolean,
    error: String?,
    resumeFrom: ResumeDraft?,
    onBack: () -> Unit,
    onLaunch: (String, JSONObject, String, JSONObject?, TurnSeed) -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val draftKey = resumeFrom?.providerSessionId
    var selectedRunnerId by rememberSaveable(initialHost.runnerId, draftKey) {
        mutableStateOf(resumeFrom?.runnerId ?: initialHost.runnerId)
    }
    val host = allHosts.firstOrNull { it.runnerId == selectedRunnerId } ?: initialHost
    val resumedHere = resumeFrom?.takeIf { it.runnerId == host.runnerId }
    val hostDisplayName = deviceLabel(host.runnerId, host.hello?.deviceName)

    var workspace by rememberSaveable(host.runnerId, draftKey) {
        mutableStateOf(resumedHere?.workspaceId ?: host.workspaces.firstOrNull().orEmpty())
    }
    var prompt by rememberSaveable(draftKey) { mutableStateOf("") }
    var effort by rememberSaveable(draftKey) { mutableStateOf(EffortLevel.DEFAULT) }
    var permissionMode by rememberSaveable(draftKey) { mutableStateOf(PermissionMode.DEFAULT) }
    var sandboxMode by rememberSaveable(draftKey) { mutableStateOf(SandboxMode.WORKSPACE_WRITE) }
    var showAdvanced by rememberSaveable(draftKey) { mutableStateOf(resumedHere != null) }
    var activeSheet by remember { mutableStateOf<ActiveLaunchSheet?>(null) }
    var showAttachmentSheet by remember { mutableStateOf(false) }
    var attachments by remember(draftKey) { mutableStateOf(emptyList<RemoteAttachment>()) }
    var attaching by remember(draftKey) { mutableStateOf(false) }
    var attachmentNotice by remember(draftKey) { mutableStateOf<String?>(null) }

    val hostPlugins =
        host.hello?.plugins?.takeIf { it.isNotEmpty() }
            ?: controller.state.plugins.orEmpty()
    val launchOptions = EngineKind.launchableAgents(hostPlugins)
    var engineManuallyPicked by rememberSaveable(host.runnerId, draftKey) {
        mutableStateOf(false)
    }
    var engineWire by rememberSaveable(host.runnerId, draftKey) {
        mutableStateOf(
            initialEngineWire
                ?.takeIf { wire -> launchOptions.any { it.wire == wire } }
                ?: EngineKind.CODEX.wire,
        )
    }
    LaunchedEffect(initialEngineWire, launchOptions) {
        if (
            !engineManuallyPicked &&
            !initialEngineWire.isNullOrBlank() &&
            launchOptions.any { it.wire == initialEngineWire }
        ) {
            engineWire = initialEngineWire
        }
    }
    val selectedOption =
        launchOptions.firstOrNull { it.wire == engineWire } ?: launchOptions.first()
    val engine = selectedOption.engine
    var model by rememberSaveable(host.runnerId, engineWire, draftKey) { mutableStateOf("") }
    var nativeId by rememberSaveable(host.runnerId, draftKey) {
        mutableStateOf(resumedHere?.providerSessionId.orEmpty())
    }

    LaunchedEffect(host.runnerId) {
        controller.loadRunnerHello(host.runnerId, force = false)
    }

    LaunchedEffect(host.runnerId, host.workspaces) {
        if (
            host.workspaces.isNotEmpty() &&
            (workspace.isBlank() || (resumedHere == null && workspace !in host.workspaces))
        ) {
            workspace = host.workspaces.first()
        }
    }

    fun attachPicked(uris: List<Uri>) {
        if (uris.isEmpty() || attaching) return
        attaching = true
        scope.launch {
            try {
                val (ready, notice) =
                    readPickedAttachments(
                        context = context,
                        uris = uris,
                        existingCount = attachments.size,
                        existingAttachments = attachments,
                    )
                var dropped = 0
                if (ready.isNotEmpty()) {
                    val merged = mergeAttachmentsWithinBudget(attachments, ready)
                    attachments = merged.attachments
                    dropped = merged.droppedCount
                }
                attachmentNotice =
                    notice ?: if (dropped > 0) "有 $dropped 个附件超出总大小或数量上限，未加入" else null
            } finally {
                attaching = false
            }
        }
    }

    val attachmentPicker =
        rememberAttachmentPicker(
            onAttach = ::attachPicked,
            onError = { attachmentNotice = it },
        )

    val candidateRecord =
        resumedHere?.let { draft ->
            host.hello?.candidates?.firstOrNull { it.providerSessionId == draft.providerSessionId }
        }
    val resumedEngine: EngineKind? =
        if (nativeId.isNotBlank()) {
            engineOfSession(nativeId.trim(), candidateRecord?.provider, hostPlugins)
        } else {
            null
        }
    val modelOptions =
        modelsForEngine(
            catalog = host.models,
            engine = resumedEngine ?: engine,
            reportedProvider = candidateRecord?.provider ?: selectedOption.wire,
            providerSessionId = nativeId.trim(),
        )
    val isResumeIntent = nativeId.isNotBlank()
    val canSubmit =
        !submitting &&
            !attaching &&
            workspace.isNotBlank() &&
            host.online &&
            (!isResumeIntent || nativeId.isNotBlank())

    fun dispatchLaunch() {
        if (!canSubmit) return
        val effectiveSandbox =
            if (permissionMode == PermissionMode.PLAN) SandboxMode.READ_ONLY
            else if (!isResumeIntent && engine == EngineKind.CODEX) sandboxMode
            else SandboxMode.WORKSPACE_WRITE
        val seed =
            TurnSeed(
                effort = effort,
                model = model,
                permissionMode = permissionMode,
                permissionTouched = permissionMode != PermissionMode.DEFAULT,
            )
        val firstPrompt = firstPromptOf(prompt, effort, model, permissionMode, attachments)
        if (isResumeIntent) {
            val trimmedNativeId = nativeId.trim()
            val existing =
                controller.state.snapshot.sessions.firstOrNull {
                    it.runnerId == host.runnerId &&
                        it.binding.providerSessionId == trimmedNativeId
                }
            val hubId = existing?.binding?.hubSessionId ?: newHubSessionId()
            onLaunch(
                host.runnerId,
                connectCommand(
                    mode =
                        if (host.codexSessionControl && isCodexThreadId(trimmedNativeId)) {
                            ConnectMode.TAKE_CONTROL
                        } else {
                            ConnectMode.RESUME
                        },
                    workspacePath = workspace,
                    providerSessionId = trimmedNativeId,
                    model = model,
                    permissionMode = permissionMode,
                    sandbox =
                        if (permissionMode == PermissionMode.PLAN) SandboxMode.READ_ONLY
                        else SandboxMode.WORKSPACE_WRITE,
                ),
                hubId,
                firstPrompt,
                seed,
            )
        } else {
            val hubId = newHubSessionId()
            onLaunch(
                host.runnerId,
                connectCommand(
                    mode = ConnectMode.CREATE,
                    workspacePath = workspace,
                    agentType = engine,
                    model = model,
                    permissionMode = permissionMode,
                    sandbox = effectiveSandbox,
                    agentTypeWire = selectedOption.wire,
                ),
                hubId,
                firstPrompt,
                seed,
            )
        }
    }

    val workspaceChoices =
        remember(host, controller.state.snapshot.sessions) {
            buildWorkspaceSheetChoices(host, controller.state.snapshot.sessions)
        }
    val onlineHostCount = remember(allHosts) { allHosts.count { it.online } }

    Box(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding()
                .windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars))
                .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
        contentAlignment = Alignment.TopCenter,
    ) {
        Column(Modifier.widthIn(max = 760.dp).fillMaxSize()) {
            // Top bar with clickable Host pill and Advanced toggle
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrcaSpace.sm, vertical = OrcaSpace.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                OrcaIconButton(onClick = onBack) {
                    OrcaBackChevron(MaterialTheme.colorScheme.onSurface)
                }
                Surface(
                    onClick = { activeSheet = ActiveLaunchSheet.HOST },
                    enabled = !isResumeIntent && allHosts.isNotEmpty(),
                    modifier = Modifier.minimumInteractiveComponentSize(),
                    shape = RoundedCornerShape(OrcaRadius.pill),
                    color = OrcaColors.bgPanel,
                    border = BorderStroke(1.dp, OrcaColors.borderSubtle),
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        OrcaStatusDot(
                            if (host.online) OrcaColors.accentGreen else OrcaColors.accentAmber,
                            6.dp,
                        )
                        Text(
                            text = hostDisplayName,
                            style = MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.SemiBold),
                            color = MaterialTheme.colorScheme.onSurface,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.widthIn(max = 160.dp),
                        )
                        if (!isResumeIntent && allHosts.size > 1) {
                            OrcaChevronDown(MaterialTheme.colorScheme.onSurfaceVariant, 10.dp)
                        }
                    }
                }
                Surface(
                    onClick = { showAdvanced = !showAdvanced },
                    modifier = Modifier.minimumInteractiveComponentSize(),
                    shape = RoundedCornerShape(OrcaRadius.pill),
                    color =
                        if (showAdvanced) OrcaColors.accentBlue.copy(alpha = 0.14f)
                        else OrcaColors.bgPanel,
                    border =
                        BorderStroke(
                            1.dp,
                            if (showAdvanced) OrcaColors.accentBlue.copy(alpha = 0.5f)
                            else OrcaColors.borderSubtle,
                        ),
                ) {
                    Text(
                        text = if (showAdvanced) "收起" else "高级",
                        style = MaterialTheme.typography.labelMedium,
                        color =
                            if (showAdvanced) OrcaColors.accentBlue
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 7.dp),
                    )
                }
            }

            // Stable top-anchored scrollable area (never snaps when toggling Advanced or IME)
            LazyColumn(
                modifier = Modifier.weight(1f).fillMaxWidth(),
                contentPadding =
                    PaddingValues(
                        horizontal = OrcaSpace.lg,
                        vertical = OrcaSpace.md,
                    ),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.lg),
            ) {
                if (error != null) {
                    item { OrcaInlineError(error) }
                }

                if (!host.online) {
                    item {
                        OrcaNoticeCard(
                            title = "当前电脑离线",
                            body = "点顶部电脑名称可切换到其他在线设备。",
                            icon = { OrcaTerminalIcon(OrcaColors.accentAmber, 20.dp) },
                        )
                    }
                } else if (host.workspaces.isEmpty()) {
                    item {
                        OrcaNoticeCard(
                            title = "暂无已登记工作区",
                            body = "请在电脑端 Runner 中授权目录，或切换其他在线电脑。",
                            icon = { OrcaTerminalIcon(OrcaColors.accentBlue, 20.dp) },
                        )
                    }
                }

                if (resumedHere != null) {
                    item {
                        OrcaNoticeCard(
                            title = "已选中电脑上的历史会话",
                            body = "确认下方目录后，直接输入追问或点右下角箭头即可接管，上下文保持不变。",
                            icon = { OrcaTerminalIcon(OrcaColors.accentBlue, 20.dp) },
                        )
                    }
                }

                if (!isResumeIntent) {
                    item {
                        Column(
                            modifier = Modifier.fillMaxWidth().padding(top = OrcaSpace.lg),
                            horizontalAlignment = Alignment.CenterHorizontally,
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Surface(
                                shape = RoundedCornerShape(16.dp),
                                color = OrcaColors.bgPanel,
                                border = BorderStroke(1.dp, OrcaColors.borderSubtle),
                                shadowElevation = 2.dp,
                            ) {
                                AgentHubBrandMark(size = 52.dp)
                            }
                            Spacer(Modifier.height(2.dp))
                            Text(
                                text = "今天想交给哪位 Agent？",
                                style = MaterialTheme.typography.titleLarge.copy(fontWeight = FontWeight.SemiBold),
                                color = MaterialTheme.colorScheme.onBackground,
                            )
                            Text(
                                text = "选好引擎与目录，任务将在 $hostDisplayName 原生执行",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }

                    item {
                        AgentEngineCardGrid(
                            options = launchOptions,
                            selectedWire = selectedOption.wire,
                            onSelect = { picked ->
                                engineManuallyPicked = true
                                engineWire = picked.wire
                                model = ""
                            },
                        )
                    }

                    item {
                        LaunchQuickPromptChips(onPick = { prompt = it })
                    }
                }

                item {
                    AnimatedVisibility(
                        visible = showAdvanced,
                        enter =
                            expandVertically(
                                animationSpec = tween(MotionTokens.Standard),
                                expandFrom = Alignment.Top,
                            ) + fadeIn(tween(MotionTokens.Standard)),
                        exit =
                            shrinkVertically(
                                animationSpec = tween(MotionTokens.Quick),
                                shrinkTowards = Alignment.Top,
                            ) + fadeOut(tween(MotionTokens.Quick)),
                    ) {
                        Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.md)) {
                            if (!isResumeIntent && engine == EngineKind.CODEX) {
                                Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                                    Text(
                                        text = "文件沙箱范围（Codex）",
                                        style = OrcaType.section,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                    FilterRow(
                                        options =
                                            listOf(
                                                SandboxMode.READ_ONLY.wire to SandboxMode.READ_ONLY.label,
                                                SandboxMode.WORKSPACE_WRITE.wire to SandboxMode.WORKSPACE_WRITE.label,
                                            ),
                                        selected = sandboxMode.wire,
                                        onSelect = { value ->
                                            SandboxMode.fromWire(value)?.let { sandboxMode = it }
                                        },
                                    )
                                }
                            }

                            OrcaCard {
                                Column(
                                    Modifier.padding(OrcaSpace.md),
                                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                                ) {
                                    GatewayField(
                                        value = nativeId,
                                        onValueChange = { nativeId = it },
                                        label = "恢复已有原生会话 ID（可选）",
                                        placeholder = "留空则新建空白会话",
                                    )
                                    if (isResumeIntent) {
                                        OrcaTextAction(
                                            label = "清除 ID，改为新建会话",
                                            enabled = !submitting,
                                            onClick = { nativeId = "" },
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Attachment previews above composer when files/images are selected
            AttachmentChips(
                attachments = attachments,
                onRemove = { attachments = attachments - it },
                modifier = Modifier.padding(bottom = OrcaSpace.xs),
            )
            attachmentNotice?.let { notice ->
                AttachmentNotice(
                    message = notice,
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xxs),
                )
            }

            // Unified floating composer with direct attach button, workspace, model, effort, permission
            val modelDisplay =
                if (model.isBlank()) "默认"
                else matchCatalogModel(modelOptions, model)?.label ?: model
            Surface(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
                shape = RoundedCornerShape(24.dp),
                color = OrcaColors.bgPanel,
                border =
                    BorderStroke(
                        1.dp,
                        if (canSubmit) OrcaColors.accentBlue.copy(alpha = 0.5f)
                        else OrcaColors.borderStrong,
                    ),
                shadowElevation = 4.dp,
            ) {
                Column(
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .padding(start = 14.dp, end = 10.dp, top = 12.dp, bottom = 10.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Box(
                        modifier = Modifier.fillMaxWidth().defaultMinSize(minHeight = 40.dp),
                        contentAlignment = Alignment.CenterStart,
                    ) {
                        if (prompt.isEmpty()) {
                            Text(
                                text =
                                    when {
                                        attachments.isNotEmpty() -> "补充说明，或直接发送所选附件…"
                                        isResumeIntent -> "输入追问，或直接点右下角接管会话…"
                                        else -> "给 ${selectedOption.label} 下达任务，留空直接进入…"
                                    },
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.68f),
                            )
                        }
                        BasicTextField(
                            value = prompt,
                            onValueChange = { prompt = it },
                            enabled = !submitting,
                            modifier = Modifier.fillMaxWidth(),
                            textStyle =
                                MaterialTheme.typography.bodyMedium.copy(
                                    color = MaterialTheme.colorScheme.onSurface,
                                    fontSize = 15.sp,
                                    lineHeight = 22.sp,
                                ),
                            cursorBrush = SolidColor(OrcaColors.accentBlue),
                            maxLines = 5,
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                            keyboardActions = KeyboardActions(onSend = { dispatchLaunch() }),
                        )
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Surface(
                            onClick = { showAttachmentSheet = true },
                            enabled = !submitting && !attaching,
                            modifier =
                                Modifier
                                    .size(38.dp)
                                    .semantics { contentDescription = "添加附件" },
                            shape = CircleShape,
                            color =
                                if (attachments.isNotEmpty()) OrcaColors.accentBlue.copy(alpha = 0.14f)
                                else OrcaColors.bgRaised,
                            border =
                                BorderStroke(
                                    1.dp,
                                    if (attachments.isNotEmpty()) OrcaColors.accentBlue.copy(alpha = 0.5f)
                                    else OrcaColors.borderSubtle,
                                ),
                        ) {
                            Box(contentAlignment = Alignment.Center) {
                                OrcaPlusIcon(
                                    tint =
                                        if (attachments.isNotEmpty()) OrcaColors.accentBlue
                                        else MaterialTheme.colorScheme.onSurface,
                                    size = 17.dp,
                                )
                            }
                        }
                        LazyRow(
                            modifier = Modifier.weight(1f),
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            item(key = "chip_ws") {
                                LaunchDockChip(
                                    label = "目录",
                                    value =
                                        workspace.ifBlank { "选择" }.let {
                                            if (workspace.isBlank()) it else workspaceLabel(it)
                                        },
                                    highlighted = workspace.isBlank(),
                                    onClick = { activeSheet = ActiveLaunchSheet.WORKSPACE },
                                )
                            }
                            item(key = "chip_model") {
                                LaunchDockChip(
                                    label = "模型",
                                    value = modelDisplay,
                                    highlighted = model.isNotBlank(),
                                    onClick = { activeSheet = ActiveLaunchSheet.MODEL },
                                )
                            }
                            item(key = "chip_effort") {
                                LaunchDockChip(
                                    label = "思考",
                                    value = effort.label,
                                    highlighted = effort != EffortLevel.DEFAULT,
                                    onClick = { activeSheet = ActiveLaunchSheet.EFFORT },
                                )
                            }
                            item(key = "chip_perm") {
                                LaunchDockChip(
                                    label = "权限",
                                    value = permissionMode.label,
                                    highlighted = permissionMode != PermissionMode.DEFAULT,
                                    onClick = { activeSheet = ActiveLaunchSheet.PERMISSION },
                                )
                            }
                        }
                        Box(
                            modifier =
                                Modifier
                                    .size(40.dp)
                                    .background(
                                        if (canSubmit) OrcaColors.surfaceBright
                                        else MaterialTheme.colorScheme.surfaceVariant,
                                        CircleShape,
                                    )
                                    .clip(CircleShape)
                                    .clickable(enabled = canSubmit) { dispatchLaunch() },
                            contentAlignment = Alignment.Center,
                        ) {
                            if (submitting || attaching) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(16.dp),
                                    strokeWidth = 1.6.dp,
                                    color = OrcaColors.onBright,
                                )
                            } else {
                                OrcaSendIcon(
                                    tint =
                                        if (canSubmit) OrcaColors.onBright
                                        else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    if (showAttachmentSheet) {
        AttachmentPickerSheet(
            controller = attachmentPicker,
            onDismiss = { showAttachmentSheet = false },
        )
    }

    LaunchActiveSheets(
        activeSheet = activeSheet,
        allHosts = allHosts,
        host = host,
        hostDisplayName = hostDisplayName,
        isResumeIntent = isResumeIntent,
        onlineHostCount = onlineHostCount,
        workspaceChoices = workspaceChoices,
        workspace = workspace,
        selectedEngineLabel = selectedOption.label,
        modelChoices =
            remember(modelOptions) {
                listOf(LaunchSheetChoice(id = "", title = "使用引擎默认模型")) +
                    modelOptions.map { item ->
                        LaunchSheetChoice(
                            id = item.id,
                            title = item.label,
                            subtitle = item.id.takeIf { it != item.label },
                        )
                    }
            },
        model = model,
        effort = effort,
        permissionMode = permissionMode,
        onSelectHost = { nextId ->
            selectedRunnerId = nextId
            model = ""
        },
        onSwitchToHostSheet = { activeSheet = ActiveLaunchSheet.HOST },
        onSelectWorkspace = { workspace = it },
        onSelectModel = { model = it },
        onSelectEffort = { effort = it },
        onSelectPermission = { nextPerm ->
            permissionMode = nextPerm
            if (nextPerm == PermissionMode.PLAN) {
                sandboxMode = SandboxMode.READ_ONLY
            } else if (sandboxMode == SandboxMode.READ_ONLY) {
                sandboxMode = SandboxMode.WORKSPACE_WRITE
            }
        },
        onDismiss = { activeSheet = null },
    )
}

private fun firstPromptOf(
    prompt: String,
    effort: EffortLevel,
    model: String,
    permissionMode: PermissionMode,
    attachments: List<RemoteAttachment>,
): JSONObject? {
    val text = prompt.trim()
    if (text.isEmpty() && attachments.isEmpty()) return null
    return sendPromptCommand(
        text = text,
        effort = effort,
        model = model,
        permissionMode = permissionMode,
        attachments = attachments,
    )
}
