package com.agentplatform.mobile.ui

import androidx.compose.runtime.Composable
import com.agentplatform.mobile.data.EffortLevel
import com.agentplatform.mobile.data.HostDevice
import com.agentplatform.mobile.data.PermissionMode
import com.agentplatform.mobile.data.RemoteSessionRow

internal enum class ActiveLaunchSheet {
    HOST,
    WORKSPACE,
    MODEL,
    EFFORT,
    PERMISSION,
}

@Composable
internal fun LaunchActiveSheets(
    activeSheet: ActiveLaunchSheet?,
    allHosts: List<HostDevice>,
    host: HostDevice,
    hostDisplayName: String,
    isResumeIntent: Boolean,
    onlineHostCount: Int,
    workspaceChoices: List<LaunchSheetChoice>,
    workspace: String,
    selectedEngineLabel: String,
    modelChoices: List<LaunchSheetChoice>,
    model: String,
    effort: EffortLevel,
    permissionMode: PermissionMode,
    onSelectHost: (String) -> Unit,
    onSwitchToHostSheet: () -> Unit,
    onSelectWorkspace: (String) -> Unit,
    onSelectModel: (String) -> Unit,
    onSelectEffort: (EffortLevel) -> Unit,
    onSelectPermission: (PermissionMode) -> Unit,
    onDismiss: () -> Unit,
) {
    when (activeSheet) {
        ActiveLaunchSheet.HOST ->
            LaunchOptionSheet(
                title = "选择执行电脑",
                choices =
                    allHosts.map { item ->
                        LaunchSheetChoice(
                            id = item.runnerId,
                            title = deviceLabel(item.runnerId, item.hello?.deviceName),
                            subtitle = "${item.workspaces.size} 个授权工作区",
                            badge = if (item.online) "在线" else "离线",
                            enabled = item.online,
                        )
                    },
                selectedId = host.runnerId,
                onSelect = onSelectHost,
                onDismiss = onDismiss,
            )
        ActiveLaunchSheet.WORKSPACE ->
            LaunchOptionSheet(
                title = "选择工作区目录",
                headerSubtitle = "电脑：$hostDisplayName · ${if (host.online) "在线" else "离线"}",
                headerActionLabel = if (!isResumeIntent && onlineHostCount > 1) "切换电脑" else null,
                onHeaderAction =
                    if (!isResumeIntent && onlineHostCount > 1) {
                        onSwitchToHostSheet
                    } else {
                        null
                    },
                choices = workspaceChoices,
                selectedId = workspace,
                onSelect = onSelectWorkspace,
                onDismiss = onDismiss,
            )
        ActiveLaunchSheet.MODEL ->
            LaunchOptionSheet(
                title = "选择模型（$selectedEngineLabel）",
                choices = modelChoices,
                selectedId = model,
                onSelect = onSelectModel,
                onDismiss = onDismiss,
            )
        ActiveLaunchSheet.EFFORT ->
            LaunchOptionSheet(
                title = "选择思考强度",
                choices =
                    EffortLevel.entries.map { item ->
                        LaunchSheetChoice(
                            id = item.wire,
                            title = item.label,
                            subtitle = effortHint(item),
                        )
                    },
                selectedId = effort.wire,
                onSelect = { wire ->
                    EffortLevel.fromWire(wire)?.let(onSelectEffort)
                },
                onDismiss = onDismiss,
            )
        ActiveLaunchSheet.PERMISSION ->
            LaunchOptionSheet(
                title = "选择执行权限模式",
                choices =
                    PermissionMode.entries.map { item ->
                        LaunchSheetChoice(
                            id = item.wire,
                            title = item.label,
                            subtitle = item.hint,
                        )
                    },
                selectedId = permissionMode.wire,
                onSelect = { wire ->
                    PermissionMode.fromWire(wire)?.let(onSelectPermission)
                },
                onDismiss = onDismiss,
            )
        null -> Unit
    }
}

internal fun buildWorkspaceSheetChoices(
    host: HostDevice,
    activeSessions: List<RemoteSessionRow>,
): List<LaunchSheetChoice> {
    val devName = deviceLabel(host.runnerId, host.hello?.deviceName)
    val counts = HashMap<String, Int>()
    val lastActive = HashMap<String, Long>()
    host.hello?.candidates.orEmpty().forEach { c ->
        counts[c.workspacePath] = (counts[c.workspacePath] ?: 0) + 1
        val stamp = c.lastActivityAtMs ?: 0L
        if (stamp > (lastActive[c.workspacePath] ?: 0L)) {
            lastActive[c.workspacePath] = stamp
        }
    }
    activeSessions.filter { it.runnerId == host.runnerId }.forEach { s ->
        val ws = s.binding.workspacePath
        if (ws.isNotBlank()) {
            counts[ws] = (counts[ws] ?: 0) + 1
            lastActive[ws] = maxOf(lastActive[ws] ?: 0L, Long.MAX_VALUE / 2)
        }
    }
    val labelFreq = host.workspaces.groupingBy { workspaceLabel(it) }.eachCount()
    val sorted =
        host.workspaces.sortedWith(
            compareByDescending<String> { lastActive[it] ?: 0L }
                .thenByDescending { counts[it] ?: 0 }
                .thenBy { workspaceLabel(it).lowercase() },
        )
    return sorted.map { id ->
        val title = workspaceLabel(id)
        val count = counts[id] ?: 0
        val duplicated = (labelFreq[title] ?: 0) > 1
        LaunchSheetChoice(
            id = id,
            title = title,
            subtitle =
                workspaceChoiceSubtitle(
                    workspaceId = id,
                    workspaceLabels = host.workspaceLabels,
                    deviceName = devName,
                    sessionCount = count,
                    duplicated = duplicated,
                ),
            badge = if (count > 0) "$count 会话" else null,
        )
    }
}
