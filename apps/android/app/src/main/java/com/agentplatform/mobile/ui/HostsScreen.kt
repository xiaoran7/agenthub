package com.agentplatform.mobile.ui

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.os.Build
import android.os.PersistableBundle
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
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.AgentPlugin
import com.agentplatform.mobile.data.DeviceBinding
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.RemoteSessionRow
import com.agentplatform.mobile.data.Snapshot
import com.agentplatform.mobile.data.describeGatewayError
import com.agentplatform.mobile.data.engineOfSession
import kotlinx.coroutines.delay

// =========================================================================
// Hosts fleet management screen: Runner status, pairing codes, & revocation
// =========================================================================

@Composable
fun HostsScreen(
    controller: AppController,
    snapshot: Snapshot,
    loading: Boolean,
    error: String?,
    onOpenSession: (String) -> Unit,
    onLaunch: (String) -> Unit,
    onRefresh: () -> Unit,
    onBack: (() -> Unit)? = null,
) {
    val context = LocalContext.current
    var binding by remember { mutableStateOf<DeviceBinding?>(null) }
    var bindingBusy by remember { mutableStateOf(false) }
    var bindingCopied by remember { mutableStateOf(false) }
    var confirmRevokeId by remember { mutableStateOf<String?>(null) }
    var revokingId by remember { mutableStateOf<String?>(null) }
    val activeHosts = remember(snapshot.hosts) { snapshot.hosts.filterNot { it.revoked }.distinctBy { it.runnerId } }
    val sessionsByRunner = remember(snapshot.sessions) { snapshot.sessions.groupBy { it.runnerId } }

    LaunchedEffect(bindingCopied) {
        if (bindingCopied) {
            delay(2000)
            bindingCopied = false
        }
    }

    LazyColumn(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding()
                .navigationBarsPadding()
                .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal)),
        contentPadding = PaddingValues(bottom = OrcaSpace.xl),
    ) {
        item {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (onBack != null) {
                    MobileRoundButton(
                        label = "返回",
                        onClick = onBack,
                        modifier = Modifier.padding(end = OrcaSpace.md),
                    ) {
                        OrcaBackChevron(MaterialTheme.colorScheme.onSurface)
                    }
                }
                Column(Modifier.weight(1f)) {
                    Text(
                        text = "电脑",
                        style = MaterialTheme.typography.headlineMedium,
                        color = MaterialTheme.colorScheme.onBackground,
                    )
                    if (onBack == null) {
                        Text(
                            text = "管理已绑定的电脑 Runner，发起任务或生成一次性配对码。",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = OrcaSpace.xxs),
                        )
                    }
                }
                OrcaSecondaryButton(
                    label = if (bindingBusy) "生成中…" else "生成绑定码",
                    enabled = !bindingBusy,
                    fillWidth = false,
                    modifier = Modifier.padding(start = OrcaSpace.sm),
                ) {
                    bindingBusy = true
                    controller.createBindingCode { res ->
                        bindingBusy = false
                        res
                            .onSuccess { binding = it }
                            .onFailure { controller.reportError(describeGatewayError(it)) }
                    }
                }
            }
        }

        if (binding != null) {
            item {
                OrcaCard(Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs)) {
                    Column(
                        Modifier.padding(OrcaSpace.md),
                        verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                    ) {
                        Row(
                            Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = "一次性设备绑定码（10 分钟内有效）",
                                style = OrcaType.section,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            OrcaTextAction(if (bindingCopied) "已复制" else "复制") {
                                val code = binding?.bindingCode.orEmpty()
                                val clipboardManager = context.getSystemService(ClipboardManager::class.java)
                                val clip = ClipData.newPlainText("device-binding-code", code)
                                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                                    clip.description.extras =
                                        PersistableBundle().apply {
                                            putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
                                        }
                                }
                                clipboardManager?.setPrimaryClip(clip)
                                bindingCopied = true
                            }
                        }
                        Text(
                            text = binding?.bindingCode.orEmpty(),
                            style = OrcaType.code,
                            color = OrcaColors.accentBlue,
                        )
                    }
                }
            }
        }

        if (error != null) {
            item {
                OrcaInlineError(
                    message = error,
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
                )
            }
        }

        if (loading) {
            item {
                Column(
                    Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.md),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    SkeletonRow()
                    SkeletonRow()
                }
            }
            return@LazyColumn
        }

        if (activeHosts.isEmpty()) {
            item {
                OrcaCard(Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.md)) {
                    EmptyPanel(
                        title = "还没有已接入的电脑",
                        body = "点上方“生成绑定码”，在电脑端 AgentHub Runner 中输入配对码即可上线。",
                        icon = { OrcaMonitorIcon(MaterialTheme.colorScheme.onSurfaceVariant, 22.dp) },
                    )
                }
            }
            return@LazyColumn
        }

        items(activeHosts, key = { "host-" + it.runnerId }) { host ->
            Column(Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs)) {
                HostCard(
                    host = host,
                    sessions = sessionsByRunner[host.runnerId].orEmpty(),
                    plugins = mergeHostPlugins(host.hello?.plugins, controller.state.plugins.orEmpty()),
                    confirmingRevoke = confirmRevokeId == host.runnerId,
                    revoking = revokingId == host.runnerId,
                    onLaunch = { onLaunch(host.runnerId) },
                    onOpenSession = { hub -> onOpenSession("${host.runnerId}/$hub") },
                    onRefresh = onRefresh,
                    onRevoke = {
                        if (confirmRevokeId == host.runnerId) {
                            confirmRevokeId = null
                            revokingId = host.runnerId
                            controller.revokeHostDevice(host.runnerId) { res ->
                                revokingId = null
                                res.onFailure { controller.reportError(describeGatewayError(it)) }
                            }
                        } else {
                            confirmRevokeId = host.runnerId
                        }
                    },
                )
            }
        }
    }
}

@Composable
private fun HostCard(
    host: HostDevice,
    sessions: List<RemoteSessionRow>,
    plugins: List<AgentPlugin>,
    confirmingRevoke: Boolean,
    revoking: Boolean,
    onLaunch: () -> Unit,
    onOpenSession: (String) -> Unit,
    onRefresh: () -> Unit,
    onRevoke: () -> Unit,
) {
    OrcaCard {
        Column {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrcaSpace.rowH, vertical = OrcaSpace.md),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.md),
            ) {
                HostStatusTile(online = host.online)
                Column(Modifier.weight(1f)) {
                    Text(
                        text = deviceLabel(host.runnerId, host.hello?.deviceName),
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text =
                            if (host.online) {
                                "${host.workspaces.size} 个工作区 · ${sessions.size} 个活跃会话"
                            } else {
                                "离线 · 等待电脑端恢复连线"
                            },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = host.runnerId.takeLast(12),
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.65f),
                        maxLines = 1,
                    )
                }
                StatusPill(
                    label = if (host.online) "已连接" else "已断开",
                    color = if (host.online) OrcaColors.accentGreen else OrcaColors.textMuted,
                )
            }

            OrcaRowDivider()

            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrcaSpace.xs, vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    OrcaTextAction(
                        label = "发起任务",
                        enabled = host.online,
                        onClick = onLaunch,
                    )
                    OrcaTextAction(label = "刷新", onClick = onRefresh)
                }
                OrcaTextAction(
                    label =
                        when {
                            revoking -> "撤销中…"
                            confirmingRevoke -> "再点确认撤销"
                            else -> "撤销设备"
                        },
                    enabled = !revoking,
                    onClick = onRevoke,
                )
            }

            if (sessions.isNotEmpty()) {
                OrcaRowDivider()
                sessions.take(4).forEachIndexed { index, session ->
                    if (index > 0) OrcaRowDivider(startInset = OrcaSpace.rowH + OrcaSize.tile + OrcaSpace.md)
                    val connected = session.online && session.binding.controlConnected
                    val provider =
                        engineOfSession(
                            session.binding.providerSessionId,
                            session.provider
                                ?: host.hello?.candidates
                                    ?.firstOrNull { it.providerSessionId == session.binding.providerSessionId }
                                    ?.provider,
                            plugins,
                        )
                    val cleanTitle =
                        sanitizeSessionTitle(session.binding.title)
                            .ifBlank { workspaceLabel(session.binding.workspacePath) }
                    OrcaRow(
                        title = cleanTitle,
                        subtitle =
                            if (connected) workspaceLabel(session.binding.workspacePath)
                            else "控制已断开，点进会话可一键重连",
                        statusColor = stateColorOf(session.binding.executionState),
                        statusText = session.binding.executionState.label,
                        meta = session.binding.providerSessionId.take(10),
                        leading = { SessionEntryTile(engine = provider, resumable = false) },
                        trailing = {
                            OrcaChevronRight(
                                MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                            )
                        },
                        onClick = { onOpenSession(session.binding.hubSessionId) },
                    )
                }
            }
        }
    }
}
