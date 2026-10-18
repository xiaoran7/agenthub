package com.agentplatform.mobile.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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

private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
private fun encoded(value: String): String = URLEncoder.encode(value, "UTF-8")
private fun workspaceLabel(workspaceId: String): String =
    if (workspaceId.startsWith("ws-")) workspaceId.removePrefix("ws-").replace(Regex("-[0-9a-f]{16}$"), "")
    else workspaceId.substringAfterLast('/').substringAfterLast('\\')

private fun remoteMessage(error: Exception): String = when ((error as? GatewayException)?.code) {
    "runner_offline" -> "电脑 Runner 处于离线状态，连接恢复后可继续。"
    "session_control_lost", "host_generation_mismatch" -> "会话控制已断开，请重新接入。"
    "approval_not_pending" -> "该审批请求已处理或已过期。"
    "session_busy_or_unknown" -> "Agent 正在执行任务中，请稍候。"
    else -> if ((error as? GatewayException)?.status == 401) "认证失效，请在设置中重新配置令牌。" else "暂时无法连接远程服务，请检查网络和电脑连接。"
}

data class UiMessage(
    val id: String,
    val role: MessageRole,
    val text: String,
    val isStreaming: Boolean = false,
    val toolTitle: String? = null,
)

// =========================================================================
// Orca Mobile Companion: Live Mission Control & Terminal Accessory Cockpit
// =========================================================================

@Composable
fun RemoteScreen(config: AppConfig, modifier: Modifier = Modifier) {
    val gateway = remember(config) { AgentGateway(config) }
    val scope = rememberCoroutineScope()
    var snapshot by remember { mutableStateOf(JSONObject()) }
    var selected by remember { mutableStateOf<Pair<String, String>?>(null) }
    var events by remember { mutableStateOf(listOf<JSONObject>()) }
    var commands by remember { mutableStateOf(listOf<JSONObject>()) }
    var draft by remember { mutableStateOf("") }
    var history by remember { mutableStateOf(listOf<String>()) }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var create by remember { mutableStateOf(false) }
    val attempts = remember(gateway) { mutableMapOf<String, JSONObject>() }
    val listState = rememberLazyListState()
    val isDark = isSystemInDarkTheme()

    // 轮询会话机队快照
    LaunchedEffect(gateway) {
        while (true) {
            try {
                snapshot = withContext(Dispatchers.IO) { gateway.remoteRequest("/sessions") }
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (failure: Exception) {
                snapshot = JSONObject(snapshot.toString()).also { stale ->
                    stale.optJSONArray("sessions")?.objects()?.forEach { it.put("online", false) }
                    stale.optJSONArray("runners")?.objects()?.forEach { it.put("online", false) }
                }
                error = remoteMessage(failure)
            }
            delay(1500)
        }
    }

    // 轮询当前选定会话事件流与命令状态
    LaunchedEffect(gateway, selected) {
        events = emptyList()
        commands = emptyList()
        draft = ""
        val target = selected ?: return@LaunchedEffect
        var cursor = 0L
        while (true) {
            try {
                val batch = withContext(Dispatchers.IO) {
                    gateway.remoteRequest("/events?runner=${encoded(target.first)}&hub=${encoded(target.second)}&after=$cursor")
                        .getJSONArray("events").objects()
                }
                if (batch.isNotEmpty()) {
                    cursor = batch.last().getLong("cursor")
                    events = events + batch
                }
                commands = withContext(Dispatchers.IO) {
                    gateway.remoteRequest("/sessions/${encoded(target.first)}/${encoded(target.second)}")
                        .getJSONArray("commands").objects()
                }
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (failure: Exception) {
                error = remoteMessage(failure)
            }
            delay(500)
        }
    }

    val sessions = snapshot.optJSONArray("sessions")?.objects().orEmpty()
    val session = sessions.find {
        it.optString("runner_id") == selected?.first &&
            it.getJSONObject("binding").optString("hub_session_id") == selected?.second
    }
    val binding = session?.optJSONObject("binding")
    val connected = session?.optBoolean("online") == true && binding?.optBoolean("control_connected") == true
    val executionState = binding?.optString("execution_state") ?: "idle"
    val idle = connected && executionState in listOf("idle", "completed")

    fun submit(runner: String, hub: String, action: JSONObject, generation: String? = null) {
        if (busy) return
        busy = true
        error = ""
        val key = "$runner:${if (action.optString("mode") == "create") "new" else hub}:$action"
        scope.launch {
            try {
                val command = attempts.getOrPut(key) {
                    JSONObject().put("client_cmd_id", UUID.randomUUID().toString())
                        .put("hub_session_id", hub)
                        .put("host_generation", generation ?: JSONObject.NULL)
                        .put("command", action)
                }
                withContext(Dispatchers.IO) { gateway.remoteRequest("/runners/${encoded(runner)}/commands", command) }
                attempts.remove(key)
                if (action.optString("action") == "send_prompt") {
                    val promptText = action.optString("text")
                    if (promptText.isNotBlank()) {
                        history = (history + promptText).takeLast(20)
                    }
                    draft = ""
                }
                selected = runner to command.getString("hub_session_id")
                create = false
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (failure: Exception) {
                if (failure is GatewayException && failure.status in 400..499) attempts.remove(key)
                error = remoteMessage(failure)
            } finally {
                busy = false
            }
        }
    }

    fun control(action: JSONObject) {
        selected?.let { submit(it.first, it.second, action, binding?.optString("host_generation")) }
    }

    BackHandler(selected != null) { selected = null }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .padding(horizontal = 14.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        // =====================================================================
        // Top Mission Header: Host status, Active fleet count & Navigation
        // =====================================================================
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = 4.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (selected != null) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Surface(
                        shape = RoundedCornerShape(8.dp),
                        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.7f),
                        border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f)),
                        modifier = Modifier
                            .clip(RoundedCornerShape(8.dp))
                            .clickable { selected = null },
                    ) {
                        Text(
                            "← 列表",
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.SemiBold,
                            color = MaterialTheme.colorScheme.onSurface,
                            modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                        )
                    }

                    Column {
                        Text(
                            workspaceLabel(binding?.optString("workspace_path").orEmpty()),
                            style = MaterialTheme.typography.titleSmall,
                            fontWeight = FontWeight.Bold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            "Runner: ${selected?.first?.takeLast(8)} · ${binding?.optString("provider_session_id")?.take(10)}",
                            style = MaterialTheme.typography.labelSmall,
                            fontFamily = FontFamily.Monospace,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            fontSize = 11.sp,
                        )
                    }
                }

                // Interrupt Action / Emergency Stop
                if (connected && !idle && !busy) {
                    Surface(
                        shape = RoundedCornerShape(8.dp),
                        color = MaterialTheme.colorScheme.error.copy(alpha = 0.12f),
                        border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.error.copy(alpha = 0.5f)),
                        modifier = Modifier
                            .clip(RoundedCornerShape(8.dp))
                            .clickable { control(JSONObject().put("action", "interrupt")) },
                    ) {
                        Row(
                            modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(4.dp),
                        ) {
                            Box(Modifier.size(6.dp).background(MaterialTheme.colorScheme.error, CircleShape))
                            Text(
                                "终止任务 (Ctrl+C)",
                                color = MaterialTheme.colorScheme.error,
                                style = MaterialTheme.typography.labelSmall,
                                fontWeight = FontWeight.Bold,
                            )
                        }
                    }
                }
            } else {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(
                        "工作台 · 任务机队",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.Bold,
                    )
                    Surface(
                        shape = RoundedCornerShape(12.dp),
                        color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.6f),
                    ) {
                        Text(
                            "${sessions.count { it.optBoolean("online") }} 活跃",
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onPrimaryContainer,
                            modifier = Modifier.padding(horizontal = 8.dp, vertical = 2.dp),
                        )
                    }
                }

                Surface(
                    shape = RoundedCornerShape(8.dp),
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .clickable(enabled = !busy) { create = true },
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = 10.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(
                            "+ 发起新任务",
                            style = MaterialTheme.typography.labelSmall,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                    }
                }
            }
        }

        // Error Banner
        if (error.isNotEmpty()) {
            Surface(
                shape = RoundedCornerShape(8.dp),
                color = MaterialTheme.colorScheme.error.copy(alpha = 0.08f),
                border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.error.copy(alpha = 0.4f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(
                    text = error,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(10.dp),
                )
            }
        }

        // =====================================================================
        // Mode 1: Fleet Overview / Sessions & Worktrees Selector (When unselected)
        // =====================================================================
        if (selected == null) {
            val activeProviderIds = remember(sessions) {
                sessions.mapNotNull { it.optJSONObject("binding")?.optString("provider_session_id") }.toSet()
            }
            val candidates = remember(snapshot, activeProviderIds) {
                val list = mutableListOf<Triple<String, String, JSONObject>>()
                snapshot.optJSONArray("runners")?.objects().orEmpty().filter { it.optBoolean("online") }.forEach { runner ->
                    val runnerId = runner.optString("runner_id")
                    val generation = runner.optJSONObject("hello")?.optString("host_generation").orEmpty()
                    val cands = runner.optJSONObject("hello")?.optJSONObject("discovery")?.optJSONArray("candidates")?.objects().orEmpty()
                    cands.filter { !activeProviderIds.contains(it.optString("provider_session_id")) }.forEach { cand ->
                        list.add(Triple(runnerId, generation, cand))
                    }
                }
                list
            }

            if (sessions.isEmpty() && candidates.isEmpty()) {
                Surface(
                    shape = RoundedCornerShape(12.dp),
                    color = MaterialTheme.colorScheme.surface,
                    border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.6f)),
                    modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
                ) {
                    Column(
                        modifier = Modifier.padding(20.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        Text(
                            "等待宿主机接入",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                        )
                        Text(
                            "在电脑终端运行 agenthub-runner daemon，手机端将自动发现并接管 Claude Code 与 Codex 会话流。",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            lineHeight = 18.sp,
                        )
                    }
                }
            } else {
                LazyColumn(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    if (sessions.isNotEmpty()) {
                        item {
                            Text(
                                "正在运行中的智能体会话",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                fontWeight = FontWeight.SemiBold,
                            )
                        }
                        items(sessions) { item ->
                            val b = item.getJSONObject("binding")
                            val isOnline = item.optBoolean("online")
                            val isControlConnected = b.optBoolean("control_connected")
                            val pId = b.getString("provider_session_id")
                            val provName = if (pId.startsWith("sess_")) "ZCode" else if (pId.startsWith("claude_")) "Claude" else "Codex"
                            val execState = b.optString("execution_state")

                            Surface(
                                shape = RoundedCornerShape(12.dp),
                                color = MaterialTheme.colorScheme.surface,
                                border = androidx.compose.foundation.BorderStroke(
                                    0.5.dp,
                                    if (isOnline && isControlConnected) MaterialTheme.colorScheme.primary.copy(alpha = 0.35f)
                                    else MaterialTheme.colorScheme.outline.copy(alpha = 0.6f),
                                ),
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(12.dp))
                                    .clickable { selected = item.getString("runner_id") to b.getString("hub_session_id") },
                            ) {
                                Row(
                                    modifier = Modifier.padding(14.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                            Box(
                                                Modifier
                                                    .size(7.dp)
                                                    .background(
                                                        if (isOnline && isControlConnected) StatusGreenBright else StatusAmber,
                                                        CircleShape,
                                                    ),
                                            )
                                            Text(
                                                workspaceLabel(b.getString("workspace_path")),
                                                style = MaterialTheme.typography.titleMedium,
                                                fontWeight = FontWeight.Bold,
                                            )
                                        }
                                        Text(
                                            "$provName · 会话 ${pId.take(12)} · 状态 $execState",
                                            style = MaterialTheme.typography.bodySmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            fontFamily = FontFamily.Monospace,
                                        )
                                    }

                                    Surface(
                                        shape = RoundedCornerShape(8.dp),
                                        color = MaterialTheme.colorScheme.primaryContainer,
                                    ) {
                                        Text(
                                            "接管控制 →",
                                            color = MaterialTheme.colorScheme.onPrimaryContainer,
                                            style = MaterialTheme.typography.labelSmall,
                                            fontWeight = FontWeight.Bold,
                                            modifier = Modifier.padding(horizontal = 9.dp, vertical = 4.dp),
                                        )
                                    }
                                }
                            }
                        }
                    }

                    if (candidates.isNotEmpty()) {
                        item {
                            Spacer(Modifier.height(6.dp))
                            Text(
                                "发现已存在的 Worktree 候选（点击一键挂接）",
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                fontWeight = FontWeight.SemiBold,
                            )
                        }
                        items(candidates) { (runnerId, generation, cand) ->
                            val provId = cand.optString("provider_session_id")
                            val ws = cand.optString("workspace_path")
                            val title = cand.optString("title").ifBlank { "会话 · ${provId.take(10)}" }

                            Surface(
                                shape = RoundedCornerShape(10.dp),
                                color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
                                border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.4f)),
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(10.dp))
                                    .clickable(enabled = !busy) {
                                        val hubId = UUID.randomUUID().toString()
                                        submit(
                                            runner = runnerId,
                                            hub = hubId,
                                            action = JSONObject()
                                                .put("action", "connect")
                                                .put("mode", "resume")
                                                .put("workspace_path", ws)
                                                .put("provider_session_id", provId)
                                                .put("sandbox", "read-only")
                                                .put("approval_policy", "untrusted"),
                                            generation = generation,
                                        )
                                    },
                            ) {
                                Row(
                                    modifier = Modifier.padding(12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.SpaceBetween,
                                ) {
                                    Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                        Text(
                                            workspaceLabel(ws),
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            fontWeight = FontWeight.SemiBold,
                                        )
                                        Text(
                                            title,
                                            style = MaterialTheme.typography.bodyMedium,
                                            fontWeight = FontWeight.Medium,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                    Text(
                                        "挂接 →",
                                        style = MaterialTheme.typography.labelSmall,
                                        color = MaterialTheme.colorScheme.primary,
                                        fontWeight = FontWeight.Bold,
                                        modifier = Modifier.padding(start = 8.dp),
                                    )
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // =================================================================
            // Mode 2: Live Mission Cockpit & Conversation Feed (Active Session)
            // =================================================================
            val conversation = remember(events, commands) { parseStructuredConversation(events, commands) }
            val approvals = remoteApprovals(events, selected?.second.orEmpty())

            LaunchedEffect(conversation.size) {
                if (conversation.isNotEmpty()) {
                    listState.animateScrollToItem(conversation.size - 1)
                }
            }

            // Connection mini-strip
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Box(
                        Modifier
                            .size(7.dp)
                            .clip(CircleShape)
                            .background(if (connected) StatusGreenBright else StatusAmber),
                    )
                    Text(
                        if (connected) "控制连线已建立 · ${executionState.uppercase()}" else "尝试恢复会话控制中…",
                        style = MaterialTheme.typography.labelSmall,
                        fontFamily = FontFamily.Monospace,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        fontSize = 11.5.sp,
                    )
                }
            }

            // High-Contrast Interactive Approval Card (Orca style)
            if (approvals.isNotEmpty()) {
                approvals.forEach { (id, approval) ->
                    Surface(
                        shape = RoundedCornerShape(12.dp),
                        color = StatusAmber.copy(alpha = 0.10f),
                        border = androidx.compose.foundation.BorderStroke(1.dp, StatusAmber),
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(vertical = 4.dp),
                        shadowElevation = 2.dp,
                    ) {
                        Column(
                            modifier = Modifier.padding(14.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                Box(Modifier.size(8.dp).background(StatusAmber, CircleShape))
                                Text(
                                    "拦截到受控权限请求",
                                    style = MaterialTheme.typography.titleSmall,
                                    fontWeight = FontWeight.Bold,
                                    color = StatusAmber,
                                )
                            }
                            Text(
                                approval.optString("message").ifBlank { "Agent 请求执行敏感或受保护的操作，请指示是否允许放行。" },
                                style = MaterialTheme.typography.bodySmall,
                                fontFamily = FontFamily.Monospace,
                                color = MaterialTheme.colorScheme.onSurface,
                            )
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                OutlinedButton(
                                    onClick = { control(JSONObject().put("action", "resolve_approval").put("approval_id", id).put("approved", false)) },
                                    enabled = connected && !busy && approval.optBoolean("can_decline", true),
                                    modifier = Modifier.weight(1f),
                                    shape = RoundedCornerShape(8.dp),
                                ) {
                                    Text("拒绝 [N]")
                                }
                                Button(
                                    onClick = { control(JSONObject().put("action", "resolve_approval").put("approval_id", id).put("approved", true)) },
                                    enabled = connected && !busy && approval.optBoolean("can_approve", true),
                                    modifier = Modifier.weight(1f),
                                    shape = RoundedCornerShape(8.dp),
                                    colors = ButtonDefaults.buttonColors(containerColor = StatusGreen),
                                ) {
                                    Text("批准执行 [Y]")
                                }
                            }
                        }
                    }
                }
            }

            // Stream Conversation Feed
            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(conversation, key = { it.id }) { msg ->
                    when (msg.role) {
                        MessageRole.USER -> {
                            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) {
                                Surface(
                                    shape = RoundedCornerShape(14.dp, 14.dp, 3.dp, 14.dp),
                                    color = MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.85f),
                                    border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.primary.copy(alpha = 0.3f)),
                                    modifier = Modifier.widthIn(max = 300.dp),
                                ) {
                                    Text(
                                        text = msg.text,
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                                    )
                                }
                            }
                        }
                        MessageRole.ASSISTANT -> {
                            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterStart) {
                                Column(Modifier.fillMaxWidth()) {
                                    if (msg.text.contains("<think>") && msg.text.contains("</think>")) {
                                        val start = msg.text.indexOf("<think>") + 7
                                        val end = msg.text.indexOf("</think>")
                                        val thought = msg.text.substring(start, end)
                                        val mainContent = msg.text.substring(end + 8)
                                        ThinkingAccordion(thought = thought, isGenerating = msg.isStreaming)
                                        RenderMarkdownContent(content = mainContent, isDark = isDark)
                                    } else {
                                        RenderMarkdownContent(content = msg.text, isDark = isDark)
                                    }
                                }
                            }
                        }
                        MessageRole.TOOL -> {
                            Surface(
                                shape = RoundedCornerShape(6.dp),
                                color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
                                border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.4f)),
                                modifier = Modifier.fillMaxWidth().padding(vertical = 1.dp),
                            ) {
                                Row(
                                    modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                                ) {
                                    Box(Modifier.size(5.dp).background(StatusBlue, CircleShape))
                                    Text(
                                        msg.toolTitle ?: "执行完成",
                                        style = MaterialTheme.typography.labelSmall,
                                        fontFamily = FontFamily.Monospace,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                            }
                        }
                        else -> {}
                    }
                }
            }

            // =================================================================
            // Orca Terminal Accessory Row: Essential keys for mobile hacker UX
            // =================================================================
            TerminalAccessoryBar(
                onKeyCtrlC = { control(JSONObject().put("action", "interrupt")) },
                onKeyEsc = { draft = "" },
                onKeyTab = { draft = "$draft  " },
                onMacro = { macro ->
                    if (idle && !busy) {
                        control(JSONObject().put("action", "send_prompt").put("text", macro))
                    } else {
                        draft = macro
                    }
                },
                isBusy = busy,
                isIdle = idle,
            )

            // Modern Bottom Composer Capsule
            Surface(
                shape = RoundedCornerShape(14.dp),
                color = MaterialTheme.colorScheme.surface,
                border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.7f)),
                modifier = Modifier.fillMaxWidth(),
                shadowElevation = 1.dp,
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 10.dp, vertical = 3.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    TextField(
                        value = draft,
                        onValueChange = { draft = it },
                        modifier = Modifier.weight(1f),
                        placeholder = {
                            Text(
                                if (connected) (if (idle) "输入指令以继续此任务…" else "Agent 正在执行中…") else "等待电脑连线…",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        },
                        colors = TextFieldDefaults.colors(
                            focusedContainerColor = Color.Transparent,
                            unfocusedContainerColor = Color.Transparent,
                            disabledContainerColor = Color.Transparent,
                            focusedIndicatorColor = Color.Transparent,
                            unfocusedIndicatorColor = Color.Transparent,
                            disabledIndicatorColor = Color.Transparent,
                        ),
                        enabled = idle && !busy,
                        maxLines = 4,
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                        keyboardActions = KeyboardActions(
                            onSend = {
                                if (draft.isNotBlank() && idle && !busy) {
                                    control(JSONObject().put("action", "send_prompt").put("text", draft.trim()))
                                }
                            },
                        ),
                    )

                    IconButton(
                        onClick = {
                            if (draft.isNotBlank() && idle && !busy) {
                                control(JSONObject().put("action", "send_prompt").put("text", draft.trim()))
                            }
                        },
                        enabled = idle && !busy && draft.isNotBlank(),
                        modifier = Modifier
                            .size(32.dp)
                            .clip(CircleShape)
                            .background(
                                if (idle && !busy && draft.isNotBlank()) MaterialTheme.colorScheme.primary
                                else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.2f),
                            ),
                    ) {
                        SendArrowUpIcon(
                            tint = if (idle && !busy && draft.isNotBlank()) MaterialTheme.colorScheme.onPrimary
                            else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
                        )
                    }
                }
            }
        }
    }

    // Modal Dialog: Create new task on workspace
    if (create) {
        AlertDialog(
            onDismissRequest = { create = false },
            title = { Text("选择宿主机工作目录发起新任务") },
            text = {
                LazyColumn(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    snapshot.optJSONArray("runners")?.objects().orEmpty().filter { it.optBoolean("online") }.forEach { runner ->
                        val paths = runner.optJSONObject("hello")?.optJSONArray("workspaces") ?: JSONArray()
                        items(paths.length()) { index ->
                            val path = paths.getString(index)
                            Surface(
                                shape = RoundedCornerShape(8.dp),
                                color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable(enabled = !busy) {
                                        val hello = runner.optJSONObject("hello") ?: return@clickable
                                        val generation = hello.optString("host_generation")
                                        if (generation.isEmpty()) return@clickable
                                        submit(
                                            runner.getString("runner_id"),
                                            UUID.randomUUID().toString(),
                                            JSONObject()
                                                .put("action", "connect")
                                                .put("mode", "create")
                                                .put("workspace_path", path)
                                                .put("sandbox", "read-only")
                                                .put("approval_policy", "untrusted"),
                                            generation,
                                        )
                                    },
                            ) {
                                Text(
                                    "${runner.getString("runner_id").takeLast(8)} · $path",
                                    style = MaterialTheme.typography.bodySmall,
                                    fontFamily = FontFamily.Monospace,
                                    modifier = Modifier.padding(10.dp),
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { create = false }) { Text("取消") }
            },
        )
    }
}

// =========================================================================
// Terminal Accessory Row (Orca Inspired Developer Keyboard Extension)
// =========================================================================

@Composable
private fun TerminalAccessoryBar(
    onKeyCtrlC: () -> Unit,
    onKeyEsc: () -> Unit,
    onKeyTab: () -> Unit,
    onMacro: (String) -> Unit,
    isBusy: Boolean,
    isIdle: Boolean,
) {
    LazyRow(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(5.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        item {
            AccessoryKeyPill(label = "Ctrl+C", isDanger = true, onClick = onKeyCtrlC)
        }
        item {
            AccessoryKeyPill(label = "Esc", onClick = onKeyEsc)
        }
        item {
            AccessoryKeyPill(label = "Tab", onClick = onKeyTab)
        }
        item {
            AccessoryMacroPill(label = "继续", macro = "继续", onClick = onMacro)
        }
        item {
            AccessoryMacroPill(label = "git status", macro = "git status", onClick = onMacro)
        }
        item {
            AccessoryMacroPill(label = "运行测试", macro = "运行测试", onClick = onMacro)
        }
        item {
            AccessoryMacroPill(label = "查看变更", macro = "git diff", onClick = onMacro)
        }
    }
}

@Composable
private fun AccessoryKeyPill(
    label: String,
    isDanger: Boolean = false,
    onClick: () -> Unit,
) {
    Surface(
        shape = RoundedCornerShape(6.dp),
        color = if (isDanger) MaterialTheme.colorScheme.error.copy(alpha = 0.12f) else MaterialTheme.colorScheme.surfaceVariant,
        border = androidx.compose.foundation.BorderStroke(
            0.5.dp,
            if (isDanger) MaterialTheme.colorScheme.error.copy(alpha = 0.4f) else MaterialTheme.colorScheme.outline.copy(alpha = 0.5f),
        ),
        modifier = Modifier
            .clip(RoundedCornerShape(6.dp))
            .clickable(onClick = onClick),
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Bold,
            color = if (isDanger) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
        )
    }
}

@Composable
private fun AccessoryMacroPill(
    label: String,
    macro: String,
    onClick: (String) -> Unit,
) {
    Surface(
        shape = RoundedCornerShape(6.dp),
        color = MaterialTheme.colorScheme.surface,
        border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f)),
        modifier = Modifier
            .clip(RoundedCornerShape(6.dp))
            .clickable { onClick(macro) },
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 4.dp),
        )
    }
}

@Composable
private fun SendArrowUpIcon(tint: Color) {
    Canvas(modifier = Modifier.size(12.dp)) {
        val stroke = 1.6.dp.toPx()
        val cx = size.width * 0.5f
        drawLine(
            color = tint,
            start = Offset(cx, size.height * 0.85f),
            end = Offset(cx, size.height * 0.15f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.20f, size.height * 0.45f),
            end = Offset(cx, size.height * 0.15f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(size.width * 0.80f, size.height * 0.45f),
            end = Offset(cx, size.height * 0.15f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun RenderMarkdownContent(content: String, isDark: Boolean) {
    val segments = remember(content) { parseMarkdownSegments(content) }
    Column(Modifier.fillMaxWidth()) {
        segments.forEach { seg ->
            if (seg.isCodeBlock) {
                CodeBlockSurface(code = seg.text, language = seg.language)
            } else {
                Text(
                    text = buildInlineMarkdown(seg.text, isDark),
                    style = MaterialTheme.typography.bodyMedium.copy(lineHeight = 22.sp),
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(vertical = 2.dp),
                )
            }
        }
    }
}

private fun parseStructuredConversation(events: List<JSONObject>, commands: List<JSONObject>): List<UiMessage> {
    val result = linkedMapOf<String, UiMessage>()
    val prompts = commands.mapNotNull { command ->
        command.optJSONObject("receipt")?.optJSONObject("result")?.optJSONObject("turn")?.optString("id")?.let {
            it to command.getJSONObject("command").getJSONObject("command").optString("text")
        }
    }.toMap()

    for (envelope in events) {
        val event = envelope.optJSONObject("payload")?.optJSONObject("event") ?: continue
        val p = event.optJSONObject("raw")?.optJSONObject("params") ?: continue
        val item = p.optJSONObject("item")
        val id = p.optString("itemId").ifBlank { item?.optString("id").orEmpty() }

        when (event.optString("kind")) {
            "turn/started" -> {
                p.optJSONObject("turn")?.optString("id")?.let { turn ->
                    prompts[turn]?.let {
                        result["user:$turn"] = UiMessage(id = "user:$turn", role = MessageRole.USER, text = it)
                    }
                }
            }
            "item/agentMessage/delta" -> {
                if (id.isNotEmpty()) {
                    val prev = result[id]?.text.orEmpty()
                    result[id] = UiMessage(id = id, role = MessageRole.ASSISTANT, text = prev + p.optString("delta"), isStreaming = true)
                }
            }
            "item/completed" -> {
                if (id.isNotEmpty()) {
                    when (item?.optString("type")) {
                        "userMessage" -> {
                            val contentArr = item.optJSONArray("content")
                            val userText = if (contentArr != null) {
                                (0 until contentArr.length())
                                    .mapNotNull { contentArr.optJSONObject(it) }
                                    .filter { it.optString("type") == "text" }
                                    .map { it.optString("text") }
                                    .joinToString("\n")
                            } else {
                                item.optString("text")
                            }
                            if (userText.isNotEmpty()) {
                                result[id] = UiMessage(id = id, role = MessageRole.USER, text = userText)
                            }
                        }
                        "agentMessage" -> {
                            result[id] = UiMessage(id = id, role = MessageRole.ASSISTANT, text = item.optString("text"), isStreaming = false)
                        }
                        "commandExecution" -> {
                            result[id] = UiMessage(id = id, role = MessageRole.TOOL, text = "", toolTitle = "执行命令: ${item.optString("command").take(24)}…")
                        }
                    }
                }
            }
        }
    }
    return result.values.toList()
}

internal fun remoteApprovals(events: List<JSONObject>, hubSessionId: String): Map<String, JSONObject> {
    val pending = linkedMapOf<String, JSONObject>()
    for (envelope in events) {
        val payload = envelope.optJSONObject("payload") ?: continue
        when (payload.optString("type")) {
            "ready" -> payload.optJSONArray("pending_approvals")?.let { approvals ->
                pending.clear()
                approvals.objects()
                    .filter { it.optString("hub_session_id") == hubSessionId }
                    .forEach { pending[it.getString("approval_id")] = it }
            }
            "host_lost" -> pending.clear()
            "receipt" -> {
                val receipt = payload.optJSONObject("receipt") ?: continue
                val result = receipt.optJSONObject("result")
                if (receipt.optString("status") == "accepted" && result?.optBoolean("response_sent") == true) {
                    result.optString("approval_id").takeIf { it.isNotEmpty() }?.let(pending::remove)
                }
            }
            "event" -> {
                val event = payload.optJSONObject("event") ?: continue
                when (event.optString("kind")) {
                    "approval_requested" -> event.optJSONObject("approval")
                        ?.takeIf { it.optString("hub_session_id") == hubSessionId }
                        ?.let { pending[it.getString("approval_id")] = it }
                    "approval_resolved" -> event.optString("approval_id")
                        .takeIf { it.isNotEmpty() }
                        ?.let(pending::remove)
                    "turn/completed" -> if (event.optJSONObject("binding")?.optString("hub_session_id") == hubSessionId) pending.clear()
                }
            }
        }
    }
    return pending
}
