package com.agentplatform.mobile.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.BuildConfig
import com.agentplatform.mobile.data.AccountView
import com.agentplatform.mobile.data.AppConfig
import com.agentplatform.mobile.data.AppearanceTheme
import com.agentplatform.mobile.data.describeGatewayError

// =========================================================================
// Settings screen: Account, Gateway, Appearance (with System), & Plugins
// =========================================================================

@Composable
private fun AppearanceSwatch(
    theme: AppearanceTheme,
    selected: Boolean,
    modifier: Modifier = Modifier,
) {
    val palette = theme.palette
    val borderColor =
        if (selected) OrcaColors.accentBlue else MaterialTheme.colorScheme.outline
    Box(
        modifier =
            modifier
                .size(28.dp)
                .clip(RoundedCornerShape(OrcaRadius.tile))
                .then(
                    if (theme == AppearanceTheme.SYSTEM) {
                        Modifier.background(
                            Brush.linearGradient(
                                listOf(
                                    AppearanceTheme.PAPER.palette.bgBase,
                                    AppearanceTheme.GRAPHITE.palette.bgBase,
                                ),
                            ),
                        )
                    } else {
                        Modifier.background(palette.bgBase)
                    },
                )
                .border(
                    width = if (selected) 1.5.dp else 1.dp,
                    color = borderColor,
                    shape = RoundedCornerShape(OrcaRadius.tile),
                ),
        contentAlignment = Alignment.Center,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            listOf(palette.accentBlue, palette.accentGreen, palette.accentAmber).forEach { tint ->
                Box(Modifier.size(5.dp).background(tint, CircleShape))
            }
        }
    }
}

private fun credentialLabel(config: AppConfig): String =
    when {
        config.sessionCookie.isNotBlank() -> "账号会话"
        config.accessToken.isNotBlank() -> "访问令牌"
        config.developmentUserId.isNotBlank() -> "本机开发身份"
        else -> "未配置"
    }

@Composable
fun SettingsScreen(
    controller: AppController,
    account: AccountView?,
    config: AppConfig?,
    appearance: AppearanceTheme,
) {
    val plugins = controller.state.plugins
    var editingGateway by rememberSaveable { mutableStateOf(false) }
    var gatewayDraft by rememberSaveable { mutableStateOf(config?.gatewayBase.orEmpty()) }
    var confirmLogout by rememberSaveable { mutableStateOf(false) }
    var saveError by remember { mutableStateOf<String?>(null) }
    var pluginTogglingId by remember { mutableStateOf<String?>(null) }
    var pluginError by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(controller) {
        controller.loadPlugins(force = false)
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
            Column(
                modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.lg),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
            ) {
                Text(
                    text = "设置",
                    style = MaterialTheme.typography.headlineMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    text = "账号、外观主题、插件扩展与连接安全",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        // ------------------------------------------------------------ account
        item { OrcaSectionHeader("账号", Modifier.padding(horizontal = OrcaSpace.lg)) }
        item {
            OrcaListGroup(Modifier.padding(horizontal = OrcaSpace.lg)) {
                if (account != null) {
                    OrcaRow(
                        title = account.displayName.ifBlank { account.email },
                        subtitle = account.email.takeIf { it.isNotBlank() },
                        meta =
                            listOfNotNull(
                                account.workspaceName,
                                account.plan?.let { "计划 ${it.uppercase()}" },
                            ).joinToString(" · ").takeIf { it.isNotBlank() },
                        leading = {
                            OrcaIconTile { OrcaLogo(18.dp) }
                        },
                        statusColor = OrcaColors.accentGreen,
                        statusText = "已登录",
                    )
                } else {
                    OrcaRow(
                        title = "未登录",
                        subtitle = "重新登录后才能读取电脑上的会话",
                        statusColor = OrcaColors.accentAmber,
                        statusText = "离线",
                    )
                }
                OrcaRowDivider()
                OrcaRow(
                    title = "退出登录",
                    subtitle =
                        if (confirmLogout) "再点一次确认退出，本机凭据会被清除"
                        else "清除保存在本机的登录凭据",
                    titleColor = if (confirmLogout) OrcaColors.accentRed else Color.Unspecified,
                    onClick = {
                        if (confirmLogout) {
                            confirmLogout = false
                            controller.logout()
                        } else {
                            confirmLogout = true
                        }
                    },
                )
            }
        }
        if (confirmLogout) {
            item {
                Text(
                    text = "退出后可重新用账号密码登录；已下发的任务不受影响。",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
                )
            }
        }

        // ---------------------------------------------------------- appearance
        item { OrcaSectionHeader("外观主题", Modifier.padding(horizontal = OrcaSpace.lg)) }
        item {
            OrcaListGroup(Modifier.padding(horizontal = OrcaSpace.lg)) {
                AppearanceTheme.entries.forEachIndexed { index, theme ->
                    if (index > 0) OrcaRowDivider()
                    OrcaRow(
                        title = theme.label,
                        subtitle = theme.blurb,
                        statusColor = if (theme == appearance) OrcaColors.accentBlue else null,
                        statusText = if (theme == appearance) "使用中" else null,
                        leading = { AppearanceSwatch(theme, selected = theme == appearance) },
                        onClick = { controller.setAppearance(theme) },
                    )
                }
            }
        }

        // ------------------------------------------------------------ plugins
        val onlineRunnerId =
            controller.state.snapshot.hosts.firstOrNull { it.online && !it.revoked }?.runnerId
        item {
            OrcaSectionHeader(
                title = "插件扩展 (Runner 权威源)",
                modifier = Modifier.padding(horizontal = OrcaSpace.lg),
                trailing =
                    if (onlineRunnerId != null) {
                        {
                            OrcaTextAction(
                                label = if (pluginTogglingId == "__refresh__") "刷新中…" else "刷新目录",
                                enabled = pluginTogglingId == null,
                            ) {
                                pluginTogglingId = "__refresh__"
                                pluginError = null
                                controller.performPluginAction(
                                    runnerId = onlineRunnerId,
                                    action = "refresh_catalog",
                                ) { res ->
                                    pluginTogglingId = null
                                    res.onFailure { pluginError = describeGatewayError(it) }
                                }
                            }
                        }
                    } else {
                        null
                    },
            )
        }
        item {
            OrcaListGroup(Modifier.padding(horizontal = OrcaSpace.lg)) {
                when {
                    plugins == null ->
                        OrcaRow(
                            title = "正在读取插件清单…",
                            subtitle = "从 Runner / Gateway 同步可用 ACP 扩展智能体",
                        )
                    plugins.isEmpty() ->
                        OrcaRow(
                            title = "暂无已登记扩展插件",
                            subtitle = "5 大内置核心引擎（Antigravity / Codex / Claude Desktop / Claude Code / DeepSeek Harness）默认可用",
                        )
                    else ->
                        plugins.forEachIndexed { index, plugin ->
                            if (index > 0) OrcaRowDivider()
                            val toggling = pluginTogglingId == plugin.id
                            val targetRunnerId = plugin.runnerId ?: onlineRunnerId
                            OrcaRow(
                                title = "${plugin.label} · v${plugin.installedVersion ?: plugin.version}",
                                subtitle = plugin.description,
                                meta = plugin.error?.takeIf { it.isNotBlank() },
                                statusColor =
                                    when {
                                        !plugin.installed -> OrcaColors.textMuted
                                        !plugin.enabled -> OrcaColors.textMuted
                                        plugin.runtimeStatus == "downloading" -> OrcaColors.accentBlue
                                        plugin.runtimeStatus == "error" ||
                                            !plugin.available ||
                                            !plugin.error.isNullOrBlank() -> OrcaColors.accentAmber
                                        else -> OrcaColors.accentGreen
                                    },
                                statusText =
                                    when {
                                        !plugin.installed -> "未安装"
                                        plugin.enabled ->
                                            when {
                                                plugin.runtimeStatus == "downloading" -> "下载就绪中…"
                                                plugin.runtimeStatus == "error" -> "异常"
                                                !plugin.available -> "缺少底层 CLI"
                                                !plugin.error.isNullOrBlank() -> "异常"
                                                else -> "已启用"
                                            }
                                        else -> "已停用"
                                    },
                                trailing = {
                                    if (!plugin.installed && targetRunnerId != null) {
                                        OrcaTextAction(
                                            label = if (toggling) "安装中…" else "安装",
                                            enabled = !toggling,
                                        ) {
                                            pluginTogglingId = plugin.id
                                            pluginError = null
                                            controller.performPluginAction(
                                                runnerId = targetRunnerId,
                                                action = "install",
                                                pluginId = plugin.id,
                                            ) { res ->
                                                pluginTogglingId = null
                                                res.onFailure { pluginError = describeGatewayError(it) }
                                            }
                                        }
                                    } else {
                                        Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                                            OrcaTextAction(
                                                label =
                                                    when {
                                                        toggling -> "处理中…"
                                                        plugin.enabled -> "停用"
                                                        else -> "开启"
                                                    },
                                                enabled = !toggling,
                                            ) {
                                                pluginTogglingId = plugin.id
                                                pluginError = null
                                                if (targetRunnerId != null) {
                                                    controller.performPluginAction(
                                                        runnerId = targetRunnerId,
                                                        action = "toggle",
                                                        pluginId = plugin.id,
                                                        enabled = !plugin.enabled,
                                                    ) { res ->
                                                        pluginTogglingId = null
                                                        res.onFailure { pluginError = describeGatewayError(it) }
                                                    }
                                                } else {
                                                    controller.togglePlugin(plugin.id, !plugin.enabled) { res ->
                                                        pluginTogglingId = null
                                                        res.onFailure { pluginError = describeGatewayError(it) }
                                                    }
                                                }
                                            }
                                            if (targetRunnerId != null) {
                                                OrcaTextAction(
                                                    label = "卸载",
                                                    enabled = !toggling,
                                                ) {
                                                    pluginTogglingId = plugin.id
                                                    pluginError = null
                                                    controller.performPluginAction(
                                                        runnerId = targetRunnerId,
                                                        action = "uninstall",
                                                        pluginId = plugin.id,
                                                    ) { res ->
                                                        pluginTogglingId = null
                                                        res.onFailure { pluginError = describeGatewayError(it) }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                            )
                        }
                }
            }
        }
        if (pluginError != null) {
            item {
                OrcaInlineError(
                    message = pluginError.orEmpty(),
                    modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
                )
            }
        }

        // ------------------------------------------------------------ gateway
        item { OrcaSectionHeader("Gateway", Modifier.padding(horizontal = OrcaSpace.lg)) }
        item {
            OrcaListGroup(Modifier.padding(horizontal = OrcaSpace.lg)) {
                OrcaRow(
                    title = config?.gatewayBase ?: "未配置",
                    subtitle = if (editingGateway) "切换不同服务器时会自动清除旧凭据重新登录" else "服务端地址",
                    meta = config?.let { "凭据：${credentialLabel(it)}" },
                    trailing = {
                        OrcaTextAction(
                            label = if (editingGateway) "取消" else "修改",
                            onClick = {
                                editingGateway = !editingGateway
                                if (!editingGateway) {
                                    gatewayDraft = config?.gatewayBase.orEmpty()
                                    saveError = null
                                }
                            },
                        )
                    },
                )
                if (editingGateway) {
                    OrcaRowDivider()
                    Column(
                        modifier = Modifier.fillMaxWidth().padding(OrcaSpace.rowH),
                        verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                    ) {
                        GatewayField(
                            value = gatewayDraft,
                            onValueChange = { gatewayDraft = it },
                            label = "Gateway 地址",
                            placeholder = "https://gateway.example.com",
                        )
                        saveError?.let {
                            Text(
                                text = it,
                                style = MaterialTheme.typography.bodySmall,
                                color = OrcaColors.accentRed,
                            )
                        }
                        Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm)) {
                            OrcaSecondaryButton("重置", Modifier.weight(1f)) {
                                gatewayDraft = config?.gatewayBase.orEmpty()
                                saveError = null
                            }
                            OrcaPrimaryButton(
                                label = "保存",
                                modifier = Modifier.weight(1f),
                                enabled = gatewayDraft.isNotBlank(),
                            ) {
                                val next =
                                    (config ?: AppConfig(gatewayBase = gatewayDraft))
                                        .copy(gatewayBase = gatewayDraft)
                                saveError = controller.saveConfig(next)
                                if (saveError == null) editingGateway = false
                            }
                        }
                    }
                }
            }
        }

        // ------------------------------------------------------------ storage
        item { OrcaSectionHeader("本机存储与安全", Modifier.padding(horizontal = OrcaSpace.lg)) }
        item {
            OrcaListGroup(Modifier.padding(horizontal = OrcaSpace.lg)) {
                OrcaStepRow(
                    index = 1,
                    title = "凭据加密存放",
                    body = "账号会话与访问令牌写入 Android Keystore 加密的私有文件，不进入日志，也不参与系统备份。",
                )
                OrcaRowDivider()
                OrcaStepRow(
                    index = 2,
                    title = "会话不落库",
                    body = "对话内容只在打开会话期间驻留内存，退出页面即释放；手机不留历史副本。",
                )
                OrcaRowDivider()
                OrcaStepRow(
                    index = 3,
                    title = "任务在电脑上执行",
                    body = "手机只下发指令与审批，代码、文件和命令始终在 Host 本机运行。",
                )
            }
        }

        item {
            Text(
                text = "AgentHub Android v${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})",
                style = OrcaType.mono,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xl),
            )
        }
    }
}
