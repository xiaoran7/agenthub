package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.ui.Alignment
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.data.AgentPlugin
import com.agentplatform.mobile.data.EngineKind
import com.agentplatform.mobile.data.PendingApproval
import com.agentplatform.mobile.data.Snapshot
import java.time.LocalDate
import java.time.ZoneId

@Composable
fun SessionsScreen(
    snapshot: Snapshot,
    attention: List<AttentionItem>,
    enabledPluginIds: Set<String>,
    loading: Boolean,
    refreshing: Boolean,
    resumingKey: String?,
    resolvingApprovalId: String?,
    error: String?,
    providerFilter: EngineKind?,
    onFilterChange: (EngineKind?) -> Unit,
    onOpenEntry: (TreeEntry) -> Unit,
    onConfigureResume: (TreeEntry.Candidate) -> Unit,
    onResolveApproval: (TreeEntry.Active, PendingApproval, Boolean) -> Unit,
    onLaunchNew: (String) -> Unit,
    onOpenHost: () -> Unit,
    onRefresh: () -> Unit,
    onOpenNavigation: (() -> Unit)? = null,
    plugins: List<AgentPlugin> = emptyList(),
) {
    val tree = remember(snapshot, providerFilter, plugins) { buildSessionTree(snapshot, providerFilter, plugins) }
    val hosts = remember(snapshot.hosts) { snapshot.hosts.filterNot { it.revoked }.distinctBy { it.runnerId } }
    val filterEngines = remember(enabledPluginIds, hosts, providerFilter) {
        EngineKind.entries.filter { engine ->
            engine != EngineKind.PLUGIN ||
                enabledPluginIds.isNotEmpty() ||
                providerFilter == EngineKind.PLUGIN ||
                hosts.any { host -> host.hello?.plugins?.any { it.enabled } == true }
        }
    }

    var selectedRunnerId by rememberSaveable { mutableStateOf<String?>(null) }
    var searchOpen by rememberSaveable { mutableStateOf(false) }
    var searchQuery by rememberSaveable { mutableStateOf("") }
    var grouping by rememberSaveable { mutableStateOf(SessionFeedGrouping.DATE) }
    var onlineOnly by rememberSaveable { mutableStateOf(false) }

    val selectedHost = hosts.firstOrNull { it.runnerId == selectedRunnerId }
    val runnerId = selectedHost?.runnerId
    val launchHost = (selectedHost ?: hosts.firstOrNull { it.online })?.takeIf { it.online }
    val zone = ZoneId.systemDefault()
    val today = LocalDate.now(zone)
    val groups = remember(tree, runnerId, searchQuery, onlineOnly, grouping, today, zone) {
        buildSessionFeed(tree, runnerId, searchQuery, onlineOnly, grouping, today, zone)
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .statusBarsPadding()
            .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal))
            .windowInsetsPadding(WindowInsets.navigationBars.union(WindowInsets.ime).only(WindowInsetsSides.Bottom)),
    ) {
        SessionFeedHeader(
            title = if (runnerId == null && providerFilter == null) "全部会话" else "会话",
            grouping = grouping,
            onGroupingChange = { grouping = it },
            onlineOnly = onlineOnly,
            onToggleOnlineOnly = { onlineOnly = !onlineOnly },
            providerFilter = providerFilter,
            filterEngines = filterEngines,
            onFilterChange = onFilterChange,
            refreshing = refreshing,
            searchOpen = searchOpen,
            onToggleSearch = {
                searchOpen = !searchOpen
                if (!searchOpen) searchQuery = ""
            },
            onRefresh = onRefresh,
            onOpenNavigation = onOpenNavigation,
        )

        AnimatedVisibility(
            visible = searchOpen,
            enter =
                expandVertically(
                    animationSpec = tween(MotionTokens.Standard),
                    expandFrom = Alignment.Top,
                ) + fadeIn(tween(MotionTokens.Quick)),
            exit =
                shrinkVertically(
                    animationSpec = tween(MotionTokens.Quick),
                    shrinkTowards = Alignment.Top,
                ) + fadeOut(tween(MotionTokens.Quick)),
        ) {
            SessionFeedSearch(query = searchQuery, onQueryChange = { searchQuery = it })
        }

        if (hosts.isNotEmpty()) {
            LazyRow(
                modifier = Modifier.fillMaxWidth().selectableGroup(),
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                item(key = "all") {
                    SessionDeviceChip(
                        label = "全部电脑",
                        selected = runnerId == null,
                        online = null,
                        onClick = { selectedRunnerId = null },
                    )
                }
                items(hosts, key = { "host:${it.runnerId}" }) { host ->
                    SessionDeviceChip(
                        label = deviceLabel(host.runnerId, host.hello?.deviceName),
                        selected = runnerId == host.runnerId,
                        online = host.online,
                        onClick = { selectedRunnerId = host.runnerId },
                    )
                }
            }
        }

        LazyColumn(
            modifier = Modifier.weight(1f),
            contentPadding = PaddingValues(top = 8.dp, bottom = 12.dp),
        ) {
            if (error != null) {
                item(key = "error", contentType = "error") {
                    OrcaInlineError(message = error, modifier = Modifier.padding(horizontal = 20.dp, vertical = 4.dp))
                }
            }

            if (loading) {
                item(key = "skeleton", contentType = "skeleton") {
                    Column(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        repeat(4) { SkeletonRow() }
                    }
                }
                return@LazyColumn
            }

            if (hosts.isEmpty() && groups.isEmpty()) {
                item(key = "pair", contentType = "empty") {
                    OrcaCard(Modifier.padding(horizontal = 20.dp, vertical = 12.dp)) {
                        EmptyPanel(
                            title = "还没有接入电脑",
                            body = "在电脑页生成绑定码，并在 AgentHub Runner 中完成配对。",
                            icon = { OrcaMonitorIcon(MaterialTheme.colorScheme.onSurfaceVariant, 22.dp) },
                            action = { OrcaSecondaryButton("绑定电脑", onClick = onOpenHost) },
                        )
                    }
                }
                return@LazyColumn
            }

            if (attention.isNotEmpty()) {
                item(key = "attention", contentType = "heading") {
                    SessionFeedHeading("待处理 · ${attention.size}")
                }
                items(
                    attention,
                    key = { "attention:${it.entry.runnerId}/${it.entry.hubSessionId}" },
                    contentType = { "approval" },
                ) { item ->
                    AttentionQueueCard(
                        item = item,
                        resolving = resolvingApprovalId != null && resolvingApprovalId == item.approval?.approvalId,
                        onOpen = { onOpenEntry(item.entry) },
                        onApprove = item.approval?.takeIf { it.canApprove }
                            ?.let { approval -> { onResolveApproval(item.entry, approval, true) } },
                        onDecline = item.approval?.takeIf { it.canDecline }
                            ?.let { approval -> { onResolveApproval(item.entry, approval, false) } },
                        modifier = Modifier.padding(horizontal = 20.dp, vertical = 4.dp),
                    )
                }
            }

            if (groups.isEmpty()) {
                item(key = "no_sessions", contentType = "empty") {
                    val (title, detail) = when {
                        searchQuery.isNotBlank() -> "没有匹配的会话" to "试试其他关键词。"
                        selectedHost?.online == false -> "电脑离线" to "电脑重新连接后会同步会话。"
                        onlineOnly -> "没有在线会话" to "在更多选项中可查看全部会话。"
                        providerFilter != null -> "当前 Agent 暂无会话" to "在更多选项中可切换 Agent。"
                        else -> "暂无会话" to "新建会话，或等待电脑同步已有会话。"
                    }
                    Column(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 18.dp),
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(title, style = MaterialTheme.typography.titleMedium)
                        Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }

            groups.forEach { group ->
                item(key = "heading:${group.key}", contentType = "heading") {
                    SessionFeedHeading(group.label)
                }
                items(
                    group.entries,
                    key = { "session:${it.entry.key}" },
                    contentType = { "session" },
                ) { row ->
                    val entry = row.entry
                    SessionFeedRow(
                        row = row,
                        showWorkspace = grouping != SessionFeedGrouping.WORKSPACE,
                        showDevice = runnerId == null && hosts.size > 1,
                        resuming = resumingKey == entry.key,
                        onOpen = { onOpenEntry(entry) },
                        onConfigureResume =
                            if (entry is TreeEntry.Candidate && entry.resumable && row.online) {
                                { onConfigureResume(entry) }
                            } else {
                                null
                            },
                    )
                }
            }
        }

        if (!loading && hosts.isNotEmpty()) {
            SessionLaunchBar(
                hostName = launchHost?.let { deviceLabel(it.runnerId, it.hello?.deviceName) },
                onClick = if (launchHost == null) onOpenHost else { { onLaunchNew(launchHost.runnerId) } },
            )
        }
    }
}

@Composable
private fun SessionFeedHeading(label: String) {
    Text(
        text = label,
        modifier = Modifier.semantics { heading() }.padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 4.dp),
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
