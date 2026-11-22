package com.agentplatform.mobile.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.ConnectMode
import com.agentplatform.mobile.data.isCodexThreadId
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.PermissionMode
import com.agentplatform.mobile.data.SandboxMode
import com.agentplatform.mobile.data.connectCommand
import com.agentplatform.mobile.data.displayEngineLabel
import com.agentplatform.mobile.data.engineOfSession
import com.agentplatform.mobile.data.modelsForEngine
import com.agentplatform.mobile.data.newHubSessionId
import com.agentplatform.mobile.data.sendPromptCommand
import kotlinx.coroutines.launch
import org.json.JSONObject

// =========================================================================
// Launch screen: create a new session in a workspace, or resume a native ID
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
    host: HostDevice,
    enabledPluginIds: Set<String>,
    submitting: Boolean,
    error: String?,
    resumeFrom: ResumeDraft?,
    onBack: () -> Unit,
    onLaunch: (JSONObject, String, JSONObject?, TurnSeed) -> Unit,
) {
    val scope = rememberCoroutineScope()
    val resumedHere = resumeFrom?.takeIf { it.runnerId == host.runnerId }
    val draftKey = resumedHere?.providerSessionId
    var workspace by rememberSaveable(host.runnerId, draftKey) {
        mutableStateOf(resumedHere?.workspaceId ?: host.workspaces.firstOrNull().orEmpty())
    }
    var prompt by rememberSaveable(host.runnerId, draftKey) { mutableStateOf("") }
    var effort by rememberSaveable(host.runnerId, draftKey) { mutableStateOf(EffortLevel.DEFAULT) }
    var permissionMode by rememberSaveable(host.runnerId, draftKey) { mutableStateOf(PermissionMode.DEFAULT) }
    var sandboxMode by rememberSaveable(host.runnerId, draftKey) { mutableStateOf(SandboxMode.WORKSPACE_WRITE) }
    val hostPlugins =
        host.hello?.plugins?.takeIf { it.isNotEmpty() }
            ?: controller.state.plugins.orEmpty()
    val launchOptions = EngineKind.launchableAgents(hostPlugins)
    var engineWire by rememberSaveable(host.runnerId, draftKey) { mutableStateOf(EngineKind.CODEX.wire) }
    val selectedOption =
        launchOptions.firstOrNull { it.wire == engineWire } ?: launchOptions.first()
    val engine = selectedOption.engine
    var model by rememberSaveable(host.runnerId, draftKey) { mutableStateOf("") }
    var nativeId by rememberSaveable(host.runnerId, draftKey) { mutableStateOf(resumedHere?.providerSessionId.orEmpty()) }

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

    Box(
        modifier =
            Modifier
                .fillMaxSize()
                .statusBarsPadding()
                .windowInsetsPadding(WindowInsets.ime.union(WindowInsets.navigationBars))
                .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
        contentAlignment = Alignment.TopCenter,
    ) {
        Column(Modifier.widthIn(max = 760.dp).fillMaxSize()) {
            ScreenHeader(
                title = if (isResumeIntent) "恢复已有会话" else "发起任务",
                subtitle = deviceLabel(host.runnerId, host.hello?.deviceName),
                statusColor = if (host.online) OrcaColors.accentGreen else OrcaColors.accentAmber,
                onBack = onBack,
            )
            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding =
                    PaddingValues(
                        start = OrcaSpace.lg,
                        end = OrcaSpace.lg,
                        bottom = OrcaSpace.xxl,
                    ),
            ) {
                if (error != null) {
                    item {
                        OrcaInlineError(
                            error,
                            Modifier.padding(vertical = OrcaSpace.xs),
                        )
                    }
                }
                if (resumedHere != null) {
                    item {
                        OrcaNoticeCard(
                            title = "已选中电脑上的历史会话",
                            body = "下面已填好这条会话的原生 ID；确认工作区后点下方“恢复这条会话”即可接管，上下文保持不变。",
                            icon = {
                                OrcaTerminalIcon(MaterialTheme.colorScheme.onSurfaceVariant, 20.dp)
                            },
                            modifier = Modifier.padding(vertical = OrcaSpace.xs),
                        )
                    }
                }
                item { OrcaSectionHeader("工作区") }
                item {
                    if (host.workspaces.isEmpty()) {
                        OrcaCard {
                            OrcaRow(
                                title = "暂无已登记工作区",
                                subtitle = "请在电脑端 Runner 中打开目录或刷新设备信息",
                            )
                        }
                    } else {
                        OrcaListGroup {
                            host.workspaces.forEachIndexed { index, id ->
                                if (index > 0) OrcaRowDivider()
                                OrcaRow(
                                    title = workspaceLabel(id),
                                    subtitle = id,
                                    statusColor = if (id == workspace) OrcaColors.accentBlue else null,
                                    statusText = if (id == workspace) "已选择" else null,
                                    onClick = { workspace = id },
                                )
                            }
                        }
                    }
                }

                if (!isResumeIntent) {
                    item { OrcaSectionHeader("引擎") }
                    item {
                        ChoiceRow(
                            options = launchOptions.map { it.wire to it.label },
                            selected = selectedOption.wire,
                            onSelect = {
                                engineWire = it
                                model = ""
                            },
                        )
                    }
                } else if (resumedEngine != null) {
                    item { OrcaSectionHeader("引擎") }
                    item {
                        val runtimeName =
                            displayEngineLabel(resumedEngine, nativeId.trim(), candidateRecord?.provider, hostPlugins)
                        Text(
                            text = "$runtimeName · 恢复会话沿用原引擎",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                item {
                    OrcaSectionHeader(
                        title = "模型",
                        trailing = {
                            OrcaTextAction("刷新列表") {
                                scope.launch { controller.loadRunnerHello(host.runnerId, force = true) }
                            }
                        },
                    )
                }
                item {
                    ModelChoiceRow(
                        options = modelOptions,
                        selected = model,
                        onSelect = { model = it },
                    )
                }

                item { OrcaSectionHeader("推理强度") }
                item {
                    FilterRow(
                        options = EffortLevel.entries.map { it.wire to it.label },
                        selected = effort.wire,
                        onSelect = { value ->
                            EffortLevel.fromWire(value)?.let { effort = it }
                        },
                    )
                }

                item { OrcaSectionHeader("权限模式") }
                item {
                    FilterRow(
                        options = PermissionMode.entries.map { it.wire to it.label },
                        selected = permissionMode.wire,
                        onSelect = { value ->
                            PermissionMode.fromWire(value)?.let { nextPerm ->
                                permissionMode = nextPerm
                                if (nextPerm == PermissionMode.PLAN) {
                                    sandboxMode = SandboxMode.READ_ONLY
                                } else if (sandboxMode == SandboxMode.READ_ONLY) {
                                    sandboxMode = SandboxMode.WORKSPACE_WRITE
                                }
                            }
                        },
                    )
                }

                // Codex-only sandbox scope (matches Web RemoteLaunchModal: shown strictly for new Codex sessions)
                if (!isResumeIntent && engine == EngineKind.CODEX) {
                    item { OrcaSectionHeader("文件沙箱范围（Codex）") }
                    item {
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

                item { OrcaSectionHeader("首条指令") }
                item {
                    OrcaCard {
                        Column(
                            Modifier.padding(OrcaSpace.lg),
                            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                        ) {
                            GatewayField(
                                value = prompt,
                                onValueChange = { prompt = it },
                                label = "指令（可留空）",
                                placeholder = "例如：先跑一遍测试，再修复失败项",
                                singleLine = false,
                            )
                        }
                    }
                }

                item { OrcaSectionHeader("恢复已有会话") }
                item {
                    OrcaCard {
                        Column(
                            Modifier.padding(OrcaSpace.lg),
                            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                        ) {
                            GatewayField(
                                value = nativeId,
                                onValueChange = { nativeId = it },
                                label = "Provider 会话 ID",
                                placeholder = "从“可接管”列表点选，或粘贴原生 ID",
                            )
                            Text(
                                text = "恢复会接管电脑上已有的原生会话，上下文保持不变。",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                }

                item {
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
                    Column(
                        Modifier.padding(top = OrcaSpace.xl),
                        verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                    ) {
                        if (isResumeIntent) {
                            OrcaPrimaryButton(
                                label = if (submitting) "接管中…" else "恢复这条会话",
                                enabled = !submitting && workspace.isNotBlank() && nativeId.isNotBlank() && host.online,
                                onClick = {
                                    val trimmedNativeId = nativeId.trim()
                                    val existing =
                                        controller.state.snapshot.sessions.firstOrNull {
                                            it.runnerId == host.runnerId &&
                                                it.binding.providerSessionId == trimmedNativeId
                                        }
                                    val hubId = existing?.binding?.hubSessionId ?: newHubSessionId()
                                    onLaunch(
                                        connectCommand(
                                            mode = if (host.codexSessionControl && isCodexThreadId(trimmedNativeId)) ConnectMode.TAKE_CONTROL else ConnectMode.RESUME,
                                            workspacePath = workspace,
                                            providerSessionId = trimmedNativeId,
                                            model = model,
                                            permissionMode = permissionMode,
                                            sandbox =
                                                if (permissionMode == PermissionMode.PLAN) SandboxMode.READ_ONLY
                                                else SandboxMode.WORKSPACE_WRITE,
                                        ),
                                        hubId,
                                        firstPromptOf(prompt, effort, model, permissionMode),
                                        seed,
                                    )
                                },
                            )
                            OrcaSecondaryButton(
                                label = "清除 ID，改为新建空白会话",
                                enabled = !submitting,
                                onClick = { nativeId = "" },
                            )
                        } else {
                            OrcaPrimaryButton(
                                label = if (submitting) "下发中…" else "新建并进入",
                                enabled = !submitting && workspace.isNotBlank() && host.online,
                                onClick = {
                                    val hubId = newHubSessionId()
                                    onLaunch(
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
                                        firstPromptOf(prompt, effort, model, permissionMode),
                                        seed,
                                    )
                                },
                            )
                        }
                        Text(
                            text =
                                if (prompt.isBlank()) "首条指令留空，进入会话后再发"
                                else "将随连接一起下发首条指令",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 2.dp),
                        )
                    }
                }
            }
        }
    }
}

private fun firstPromptOf(
    prompt: String,
    effort: EffortLevel,
    model: String,
    permissionMode: PermissionMode,
): JSONObject? {
    val text = prompt.trim()
    if (text.isEmpty()) return null
    return sendPromptCommand(
        text = text,
        effort = effort,
        model = model,
        permissionMode = permissionMode,
    )
}
