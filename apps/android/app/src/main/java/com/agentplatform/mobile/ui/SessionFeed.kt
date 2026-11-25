package com.agentplatform.mobile.ui

import com.agentplatform.mobile.data.isCodexThreadId
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId

internal enum class SessionFeedGrouping(val label: String) {
    DATE("日期"),
    WORKSPACE("工作区"),
    STATUS("状态"),
}

internal data class SessionFeedEntry(
    val entry: TreeEntry,
    val workspaceName: String,
    val online: Boolean,
) {
    val status: String
        get() = when {
            !online -> "电脑离线"
            entry is TreeEntry.Active && !entry.controlConnected -> "控制断开"
            entry is TreeEntry.Active -> entry.executionState.label
            entry is TreeEntry.Candidate && isCodexThreadId(entry.providerSessionId) -> "历史可看"
            entry is TreeEntry.Candidate && entry.resumable -> "可接管"
            else -> "仅本地"
        }
}

internal data class SessionFeedGroup(
    val key: String,
    val label: String,
    val entries: List<SessionFeedEntry>,
)

internal fun buildSessionFeed(
    devices: List<DeviceGroup>,
    selectedRunnerId: String?,
    query: String,
    onlineOnly: Boolean,
    grouping: SessionFeedGrouping,
    today: LocalDate,
    zone: ZoneId,
): List<SessionFeedGroup> {
    val search = query.trim()
    val entries = buildList {
        for (device in devices) {
            if (selectedRunnerId != null && device.runnerId != selectedRunnerId) continue
            if (onlineOnly && !device.online) continue
            for (agent in device.agents) {
                for (workspace in agent.workspaces) {
                    for (entry in workspace.entries) {
                        val online = device.online && (entry !is TreeEntry.Active || entry.online)
                        if (onlineOnly && !online) continue
                        if (
                            search.isNotEmpty() &&
                            !entry.title.contains(search, ignoreCase = true) &&
                            !workspace.label.contains(search, ignoreCase = true) &&
                            !entry.agentLabel.contains(search, ignoreCase = true) &&
                            !device.label.contains(search, ignoreCase = true)
                        ) continue
                        add(SessionFeedEntry(entry, workspace.label, online))
                    }
                }
            }
        }
    }
    val ordered = entries.sortedWith(
        compareByDescending<SessionFeedEntry> { it.entry.lastActivityAtMs ?: 0L }
            .thenBy { it.entry.title }
            .thenBy { it.entry.key },
    )
    return ordered.groupBy { row ->
        when (grouping) {
            SessionFeedGrouping.DATE -> {
                val date = row.entry.lastActivityAtMs?.takeIf { it > 0 }
                    ?.let { Instant.ofEpochMilli(it).atZone(zone).toLocalDate() }
                val label = when (date) {
                    null -> "时间未知"
                    today -> "今天"
                    today.minusDays(1) -> "昨天"
                    else -> if (date.year == today.year) {
                        "${date.monthValue}月${date.dayOfMonth}日"
                    } else {
                        "${date.year}年${date.monthValue}月${date.dayOfMonth}日"
                    }
                }
                (date?.toString() ?: "unknown") to label
            }
            SessionFeedGrouping.WORKSPACE -> row.entry.workspaceId to row.workspaceName.ifBlank { "未指定工作区" }
            SessionFeedGrouping.STATUS -> row.status to row.status
        }
    }.map { (group, rows) ->
        SessionFeedGroup("${grouping.name}:${group.first}", group.second, rows)
    }
}
