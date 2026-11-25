package com.agentplatform.mobile.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.AppController
import com.agentplatform.mobile.data.AgentPlugin
import com.agentplatform.mobile.data.describeGatewayError

internal class SettingsPluginActions(private val controller: AppController) {
    var pendingAction by mutableStateOf<String?>(null)
        private set
    var error by mutableStateOf<String?>(null)
        private set

    fun run(
        action: String,
        runnerId: String?,
        plugin: AgentPlugin? = null,
        enabled: Boolean? = null,
    ) {
        pendingAction = plugin?.id ?: action
        error = null
        val onResult: (Result<List<AgentPlugin>>) -> Unit = { result ->
            pendingAction = null
            result.onFailure { error = describeGatewayError(it) }
        }
        if (runnerId != null) {
            controller.performPluginAction(
                runnerId = runnerId,
                action = action,
                pluginId = plugin?.id,
                enabled = enabled,
                onResult = onResult,
            )
        } else if (plugin != null && enabled != null) {
            controller.togglePlugin(plugin.id, enabled, onResult)
        } else {
            pendingAction = null
            error = "当前没有在线的电脑 Runner，无法执行插件操作。"
        }
    }
}

@Composable
internal fun SettingsPlugins(controller: AppController, actions: SettingsPluginActions) {
    val plugins = controller.state.plugins
    val onlineRunnerId = controller.state.snapshot.hosts.firstOrNull { it.online && !it.revoked }?.runnerId
    val pendingAction = actions.pendingAction
    var expanded by rememberSaveable { mutableStateOf(false) }

    LaunchedEffect(controller, expanded) {
        if (expanded) controller.loadPlugins(force = false)
    }

    OrcaSectionHeader(
        title = "扩展",
        trailing = if (expanded && onlineRunnerId != null) {
            {
                OrcaTextAction(
                    label = if (pendingAction == "refresh_catalog") "刷新中…" else "刷新目录",
                    enabled = pendingAction == null,
                ) { actions.run("refresh_catalog", onlineRunnerId) }
            }
        } else {
            null
        },
    )

    OrcaListGroup {
        OrcaRow(
            title = "插件扩展",
            modifier = Modifier.heightIn(min = 56.dp).semantics {
                stateDescription = if (expanded) "已展开" else "已收起"
            },
            leading = { OrcaSettingsIcon(MaterialTheme.colorScheme.onSurface, 20.dp) },
            trailing = {
                OrcaChevronDown(MaterialTheme.colorScheme.onSurfaceVariant, expanded = expanded)
            },
            onClick = { expanded = !expanded },
        )

        if (expanded) {
            when {
                plugins == null -> {
                    OrcaRowDivider()
                    OrcaRow(title = "正在读取插件清单…")
                }
                plugins.isEmpty() -> {
                    OrcaRowDivider()
                    OrcaRow(title = "暂无扩展插件")
                }
                else -> plugins.forEach { plugin ->
                    OrcaRowDivider()
                    val runnerId = plugin.runnerId ?: onlineRunnerId
                    val updating = pendingAction == plugin.id

                    OrcaRow(
                        title = plugin.label,
                        subtitle = plugin.description,
                        meta = plugin.error?.takeIf { it.isNotBlank() } ?: "v${plugin.installedVersion ?: plugin.version}",
                        statusColor = when {
                            !plugin.installed || !plugin.enabled -> OrcaColors.textMuted
                            plugin.runtimeStatus == "downloading" -> OrcaColors.accentBlue
                            plugin.runtimeStatus == "error" ||
                                !plugin.available || !plugin.error.isNullOrBlank() -> OrcaColors.accentAmber
                            else -> OrcaColors.accentGreen
                        },
                        statusText = when {
                            !plugin.installed -> "未安装"
                            !plugin.enabled -> "已停用"
                            plugin.runtimeStatus == "downloading" -> "正在下载"
                            plugin.runtimeStatus == "error" || !plugin.error.isNullOrBlank() -> "异常"
                            !plugin.available -> "缺少底层 CLI"
                            else -> "已启用"
                        },
                    )

                    Row(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = OrcaSpace.rowH),
                        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs, Alignment.End),
                    ) {
                        if (!plugin.installed) {
                            OrcaTextAction(
                                label = if (updating) "处理中…" else "安装",
                                enabled = pendingAction == null && runnerId != null,
                            ) { actions.run("install", runnerId, plugin) }
                        } else {
                            OrcaTextAction(
                                label = when {
                                    updating -> "处理中…"
                                    plugin.enabled -> "停用"
                                    else -> "开启"
                                },
                                enabled = pendingAction == null,
                            ) { actions.run("toggle", runnerId, plugin, enabled = !plugin.enabled) }
                            if (runnerId != null) {
                                OrcaTextAction(label = "卸载", enabled = pendingAction == null) {
                                    actions.run("uninstall", runnerId, plugin)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if (expanded) {
        actions.error?.let { OrcaInlineError(it, Modifier.padding(vertical = OrcaSpace.xs)) }
    }
}
