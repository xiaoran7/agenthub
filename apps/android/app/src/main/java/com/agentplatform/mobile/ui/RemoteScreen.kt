package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.agentplatform.mobile.data.AgentGateway
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

private fun JSONArray.objects(): List<JSONObject> = (0 until length()).mapNotNull { optJSONObject(it) }
private fun encoded(value: String): String = URLEncoder.encode(value, "UTF-8")

private fun workspaceLabel(workspaceId: String): String =
    if (workspaceId.startsWith("ws-")) workspaceId.removePrefix("ws-").replace(Regex("-[0-9a-f]{16}$"), "")
    else workspaceId.substringAfterLast('/').substringAfterLast('\\')

private fun sessionError(error: Exception): String =
    when ((error as? GatewayException)?.code) {
        "runner_offline" -> "电脑 Runner 处于离线状态，恢复连线后可继续。"
        "session_control_lost", "host_generation_mismatch" -> "会话控制已断开，请返回主机页重新接入。"
        "approval_not_pending" -> "该审批已处理或已过期。"
        "session_busy_or_unknown" -> "Agent 正在执行任务，请稍候。"
        "question_not_pending" -> "问题已处理或已过期。"
        else ->
            when ((error as? GatewayException)?.status) {
                401, 403 -> "认证失效，请到设置更新访问令牌。"
                408, 504 -> "请求超时，请检查网络后重试。"
                null -> "暂时无法连接 Gateway，请检查网络与电脑连接。"
                else -> "Gateway 暂时不可用，请稍后重试。"
            }
    }

/** The runner redacts local paths before they leave the machine. */
private fun readableText(value: String): String = value.replace("[local_path_redacted]", "（本机路径已隐藏）")

// =========================================================================
// Session screen
//
// Header -> turn failure -> approval / question banners -> transcript ->
// accessory keys -> composer. Polling pauses whenever the app leaves the
// foreground.
// =========================================================================

private sealed interface ConversationBlock {
    data class Single(val item: TranscriptItem) : ConversationBlock

    data class Tools(val items: List<TranscriptItem>) : ConversationBlock
}

private fun groupTranscript(items: List<TranscriptItem>): List<ConversationBlock> {
    val blocks = mutableListOf<ConversationBlock>()
    val tools = mutableListOf<TranscriptItem>()
    fun flush() {
        if (tools.size == 1) blocks.add(ConversationBlock.Single(tools.first()))
        if (tools.size > 1) blocks.add(ConversationBlock.Tools(tools.toList()))
        tools.clear()
    }
    for (item in items) {
        if (item.kind == TranscriptKind.TOOL) {
            tools.add(item)
        } else {
            flush()
            blocks.add(ConversationBlock.Single(item))
        }
    }
    flush()
    return blocks
}

@Composable
fun RemoteScreen(
    gateway: AgentGateway,
    runnerId: String,
    hubId: String,
    snapshot: JSONObject,
    onBack: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var events by remember { mutableStateOf(listOf<JSONObject>()) }
    var commands by remember { mutableStateOf(listOf<JSONObject>()) }
    var draft by remember { mutableStateOf("") }
    var error by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var dismissedFailure by remember { mutableStateOf<String?>(null) }
    val attempts = remember { mutableMapOf<String, JSONObject>() }
    val listState = rememberLazyListState()
    val focusManager = LocalFocusManager.current

    val session =
        remember(snapshot, runnerId, hubId) {
            snapshot.optJSONArray("sessions")?.objects()?.find {
                it.optString("runner_id") == runnerId &&
                    it.optJSONObject("binding")?.optString("hub_session_id") == hubId
            }
        }
    val binding = session?.optJSONObject("binding")
    val connected = session?.optBoolean("online") == true && binding?.optBoolean("control_connected") == true
    val executionState = binding?.optString("execution_state") ?: "unknown"
    val idle = connected && executionState in listOf("idle", "completed")
    val generation = binding?.optString("host_generation").orEmpty()

    // Event polling. Only diffed payloads update state, so an idle poll does
    // not recompose the transcript.
    LaunchedEffect(gateway, runnerId, hubId, lifecycle) {
        events = emptyList()
        commands = emptyList()
        var cursor = 0L
        lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                var hadActivity = false
                try {
                    val batch =
                        withContext(Dispatchers.IO) {
                            gateway.remoteRequest(
                                "/events?runner=${encoded(runnerId)}&hub=${encoded(hubId)}&after=$cursor",
                            ).getJSONArray("events").objects()
                        }
                    if (batch.isNotEmpty()) {
                        cursor = batch.last().optLong("cursor", cursor)
                        events = (events + batch).takeLast(1_200)
                        hadActivity = true
                    }
                    val commandList =
                        withContext(Dispatchers.IO) {
                            gateway.remoteRequest("/sessions/${encoded(runnerId)}/${encoded(hubId)}")
                                .optJSONArray("commands")?.objects().orEmpty()
                        }
                    if (commandList.toString() != commands.toString()) {
                        commands = commandList
                        hadActivity = true
                    }
                    if (error.isNotEmpty()) error = ""
                } catch (cancel: CancellationException) {
                    throw cancel
                } catch (failure: Exception) {
                    error = sessionError(failure)
                }
                delay(if (hadActivity) 500 else 900)
            }
        }
    }

    val live = connected && executionState == "running"
    val conversation =
        remember(events, commands, hubId, live) {
            buildTranscript(events, commands, hubId, live)
        }
    val blocks = remember(conversation) { groupTranscript(conversation) }
    val approvals = remember(events, hubId) { remoteApprovals(events, hubId) }
    val questions = remember(events, hubId) { remoteQuestions(events, hubId) }
    val turnFailure = remember(events, hubId) { latestTurnFailure(events, hubId) }
    val usage = remember(events, hubId) { latestContextUsage(events, hubId) }
    var showUsage by remember { mutableStateOf(false) }

    // Follow new output only while the reader is already at the bottom, so
    // scrolling back through history is never yanked away.
    val lastMessageLength = conversation.lastOrNull()?.text?.length ?: 0
    LaunchedEffect(blocks.size, lastMessageLength) {
        if (blocks.isEmpty()) return@LaunchedEffect
        val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: -1
        if (lastVisible >= blocks.lastIndex - 1) {
            listState.scrollToItem(blocks.lastIndex)
        }
    }

    fun submit(action: JSONObject, onDone: () -> Unit = {}) {
        if (busy) return
        busy = true
        val key = action.optString("action") + ":" + action.toString()
        scope.launch {
            try {
                val body =
                    attempts.getOrPut(key) {
                        JSONObject()
                            .put("client_cmd_id", UUID.randomUUID().toString())
                            .put("hub_session_id", hubId)
                            .put("host_generation", generation.ifBlank { JSONObject.NULL })
                            .put("command", action)
                    }
                withContext(Dispatchers.IO) {
                    gateway.remoteRequest("/runners/${encoded(runnerId)}/commands", body)
                }
                attempts.remove(key)
                error = ""
                if (action.optString("action") == "send_prompt") draft = ""
                onDone()
            } catch (cancel: CancellationException) {
                throw cancel
            } catch (failure: Exception) {
                if (failure is GatewayException && failure.status in 400..499) attempts.remove(key)
                error = sessionError(failure)
            } finally {
                busy = false
            }
        }
    }

    fun sendPrompt(text: String) {
        val trimmed = text.trim()
        if (trimmed.isEmpty() || !idle || busy) return
        focusManager.clearFocus()
        submit(JSONObject().put("action", "send_prompt").put("text", trimmed))
    }

    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .statusBarsPadding()
                .navigationBarsPadding()
                .imePadding(),
    ) {
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OrcaIconButton(onClick = onBack) {
                OrcaBackChevron(MaterialTheme.colorScheme.onSurface)
            }
            Column(
                modifier = Modifier.weight(1f).padding(start = OrcaSpace.xs),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
            ) {
                Text(
                    text = workspaceLabel(binding?.optString("workspace_path").orEmpty()),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    OrcaStatusDot(
                        color = if (connected) stateColor(executionState) else OrcaColors.accentAmber,
                        5.dp,
                    )
                    Text(
                        text = if (connected) stateLabel(executionState) else "正在恢复控制…",
                        style = OrcaType.mono,
                        color = if (connected) stateColor(executionState) else OrcaColors.accentAmber,
                        maxLines = 1,
                    )
                }
            }
            if (usage != null) {
                ContextCapacityRing(usage = usage, onToggle = { showUsage = !showUsage })
            }
            if (connected && !idle) {
                Surface(
                    onClick = { submit(JSONObject().put("action", "interrupt")) },
                    modifier = Modifier.height(32.dp),
                    enabled = !busy,
                    shape = RoundedCornerShape(OrcaRadius.button),
                    color = OrcaColors.accentRed.copy(alpha = 0.12f),
                    border = BorderStroke(1.dp, OrcaColors.accentRed.copy(alpha = 0.5f)),
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = OrcaSpace.md),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                    ) {
                        OrcaStopIcon(OrcaColors.accentRed)
                        Text(
                            text = "停止",
                            style = MaterialTheme.typography.labelMedium,
                            color = OrcaColors.accentRed,
                        )
                    }
                }
            }
        }

        if (error.isNotBlank()) {
            OrcaInlineError(error, Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs))
        }

        if (showUsage && usage != null) {
            ContextCapacityStrip(
                usage = usage,
                modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xxs),
            )
        }

        if (turnFailure != null && !turnFailure.retrying && turnFailure.message != dismissedFailure) {
            TurnFailureCard(
                failure = turnFailure,
                onDismiss = { dismissedFailure = turnFailure.message },
                modifier = Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
            )
        }

        if (approvals.isNotEmpty() || questions.isNotEmpty()) {
            LazyRow(
                modifier = Modifier.fillMaxWidth(),
                contentPadding = PaddingValues(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                items(approvals.entries.toList(), key = { it.key }) { (id, approval) ->
                    ApprovalCard(
                        approval = approval,
                        enabled = connected && !busy,
                        onApprove = {
                            submit(
                                JSONObject()
                                    .put("action", "resolve_approval")
                                    .put("approval_id", id)
                                    .put("approved", true),
                            )
                        },
                        onDecline = {
                            submit(
                                JSONObject()
                                    .put("action", "resolve_approval")
                                    .put("approval_id", id)
                                    .put("approved", false),
                            )
                        },
                    )
                }
                items(questions.entries.toList(), key = { "q-" + it.key }) { (id, question) ->
                    QuestionCard(
                        question = question,
                        enabled = connected && !busy,
                        onAnswer = { answer ->
                            submit(
                                JSONObject()
                                    .put("action", "answer_question")
                                    .put("question_id", id)
                                    .put(
                                        "answer",
                                        if (answer is List<*>) JSONArray(answer) else answer,
                                    ),
                            )
                        },
                    )
                }
            }
        }

        LazyColumn(
            state = listState,
            modifier = Modifier.weight(1f),
            contentPadding = PaddingValues(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            if (blocks.isEmpty()) {
                item {
                    OrcaCard {
                        OrcaNoticeCard(
                            title = if (connected) "等待 Agent 输出" else "等待电脑连线",
                            body =
                                if (connected) "消息、思考过程与工具执行会实时出现在这里；历史对话在恢复会话时载入。"
                                else "保持 Runner 在线，手机端会自动恢复这条会话的控制权。",
                            icon = { OrcaTerminalIcon(MaterialTheme.colorScheme.onSurfaceVariant, 22.dp) },
                        )
                    }
                }
            }
            items(blocks, key = { block ->
                when (block) {
                    is ConversationBlock.Single -> block.item.id
                    is ConversationBlock.Tools -> "group:" + block.items.first().id
                }
            }) { block ->
                when (block) {
                    is ConversationBlock.Single ->
                        when (block.item.kind) {
                            TranscriptKind.USER -> UserBubble(block.item.text)
                            TranscriptKind.ASSISTANT -> AssistantMessage(block.item)
                            TranscriptKind.TOOL -> ToolCard(block.item)
                            TranscriptKind.SYSTEM -> SystemLine(block.item)
                        }
                    is ConversationBlock.Tools -> ToolGroup(block.items)
                }
            }
        }

        TerminalAccessoryBar(
            enabled = connected && !busy,
            onKey = { key ->
                when (key) {
                    "Esc" -> draft = ""
                    "Tab" -> draft = "$draft  "
                    "Ctrl+C" -> submit(JSONObject().put("action", "interrupt"))
                }
            },
            onMacro = { macro ->
                draft = macro
                if (idle && connected && !busy) sendPrompt(macro)
            },
        )

        Surface(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
            shape = RoundedCornerShape(OrcaRadius.card),
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        ) {
            Row(
                modifier = Modifier.padding(start = OrcaSpace.rowH, end = OrcaSpace.sm, top = OrcaSpace.xs, bottom = OrcaSpace.xs),
                verticalAlignment = Alignment.Bottom,
            ) {
                Box(modifier = Modifier.weight(1f).padding(vertical = OrcaSpace.sm)) {
                    if (draft.isEmpty()) {
                        Text(
                            text =
                                when {
                                    !connected -> "等待电脑连线…"
                                    !idle -> "Agent 正在执行中…"
                                    else -> "输入指令，或使用上方快捷键…"
                                },
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                        )
                    }
                    BasicTextField(
                        value = draft,
                        onValueChange = { draft = it },
                        modifier = Modifier.fillMaxWidth(),
                        enabled = idle && connected && !busy,
                        textStyle =
                            MaterialTheme.typography.bodyMedium.copy(
                                color = MaterialTheme.colorScheme.onSurface,
                            ),
                        cursorBrush = SolidColor(OrcaColors.accentBlue),
                        maxLines = 5,
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                        keyboardActions = KeyboardActions(onSend = { sendPrompt(draft) }),
                    )
                }
                Box(
                    modifier =
                        Modifier
                            .padding(bottom = OrcaSpace.xs)
                            .size(36.dp)
                            .background(
                                if (idle && connected && !busy && draft.isNotBlank()) OrcaColors.accentBlue
                                else MaterialTheme.colorScheme.surfaceVariant,
                                CircleShape,
                            )
                            .clickable(enabled = idle && connected && !busy && draft.isNotBlank()) {
                                sendPrompt(draft)
                            },
                    contentAlignment = Alignment.Center,
                ) {
                    OrcaSendIcon(
                        tint =
                            if (idle && connected && !busy && draft.isNotBlank()) Color.White
                            else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                    )
                }
            }
        }
    }
}

// =========================================================================
// Intervention banners
// =========================================================================

@Composable
private fun TurnFailureCard(
    failure: TurnFailure,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(OrcaRadius.card),
        color = OrcaColors.accentRed.copy(alpha = 0.08f),
        border = BorderStroke(1.dp, OrcaColors.accentRed.copy(alpha = 0.45f)),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    OrcaStatusDot(OrcaColors.accentRed, 6.dp)
                    Text(
                        text = "这一轮没有完成",
                        style = MaterialTheme.typography.titleSmall,
                        color = OrcaColors.accentRed,
                    )
                }
                OrcaTextAction(label = "关闭", onClick = onDismiss)
            }
            Text(
                text = readableText(failure.message),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

@Composable
private fun ApprovalCard(
    approval: JSONObject,
    enabled: Boolean,
    onApprove: () -> Unit,
    onDecline: () -> Unit,
) {
    val tool = approval.optString("tool_name").ifBlank { "工具授权" }
    val risk = approval.optString("risk_level")
    val body =
        approval.optString("command_preview")
            .ifBlank { approval.optString("description") }
            .ifBlank { approval.optString("path") }
            .ifBlank { "Agent 请求执行受保护操作，请确认是否放行。" }
    val diff = approval.optString("diff")
    var expanded by remember(approval.toString()) { mutableStateOf(false) }

    Surface(
        modifier = Modifier.width(288.dp),
        shape = RoundedCornerShape(OrcaRadius.card),
        color = OrcaColors.accentAmber.copy(alpha = 0.08f),
        border = BorderStroke(1.dp, OrcaColors.accentAmber.copy(alpha = 0.55f)),
    ) {
        Column(
            modifier = Modifier.padding(OrcaSpace.md),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    OrcaStatusDot(OrcaColors.accentAmber, 6.dp)
                    Text(
                        text = "需要授权",
                        style = MaterialTheme.typography.titleSmall,
                        color = OrcaColors.accentAmber,
                    )
                }
                OrcaChip(
                    text = if (risk == "high") "高风险" else tool,
                    color = if (risk == "high") OrcaColors.accentRed else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                text = readableText(body),
                style = OrcaType.mono,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = if (expanded) Int.MAX_VALUE else 3,
                overflow = TextOverflow.Ellipsis,
            )
            if (diff.isNotBlank()) {
                OrcaTextAction(
                    label = if (expanded) "收起变更" else "查看变更",
                    onClick = { expanded = !expanded },
                )
                AnimatedVisibility(visible = expanded) {
                    DiffPreview(diff)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm)) {
                OrcaSecondaryButton(
                    label = "拒绝",
                    modifier = Modifier.weight(1f),
                    enabled = enabled && approval.optBoolean("can_decline", true),
                    onClick = onDecline,
                )
                OrcaPrimaryButton(
                    label = "批准",
                    modifier = Modifier.weight(1f),
                    enabled = enabled && approval.optBoolean("can_approve", true),
                    onClick = onApprove,
                )
            }
        }
    }
}

@Composable
private fun DiffPreview(diff: String) {
    Column(
        modifier =
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(OrcaRadius.button))
                .background(OrcaColors.bgInset)
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(OrcaRadius.button))
                .padding(OrcaSpace.sm),
        verticalArrangement = Arrangement.spacedBy(1.dp),
    ) {
        diff.lineSequence().take(60).forEach { line ->
            val color =
                when {
                    line.startsWith("+") && !line.startsWith("+++") -> OrcaColors.accentGreen
                    line.startsWith("-") && !line.startsWith("---") -> OrcaColors.accentRed
                    else -> MaterialTheme.colorScheme.onSurfaceVariant
                }
            Text(
                text = line.ifEmpty { " " },
                style = OrcaType.code.copy(fontSize = 11.sp),
                color = color,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun QuestionCard(
    question: JSONObject,
    enabled: Boolean,
    onAnswer: (Any) -> Unit,
) {
    val header = question.optString("header").ifBlank { "Agent 提问" }
    val prompt = question.optString("prompt").ifBlank { "请选择一个选项以继续。" }
    val options = question.optJSONArray("options")?.objects().orEmpty()
    val multi = question.optBoolean("multi_select", false)
    // Selection is per question and survives a poll-driven recomposition, but
    // not a new question: a submitted answer clears the card anyway.
    var picked by remember(question.optString("question_id")) { mutableStateOf(setOf<String>()) }

    Surface(
        modifier = Modifier.width(288.dp),
        shape = RoundedCornerShape(OrcaRadius.card),
        color = OrcaColors.accentBlue.copy(alpha = 0.08f),
        border = BorderStroke(1.dp, OrcaColors.accentBlue.copy(alpha = 0.55f)),
    ) {
        Column(
            modifier = Modifier.padding(OrcaSpace.md),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                OrcaStatusDot(OrcaColors.accentBlue, 6.dp)
                Text(
                    text = header,
                    style = MaterialTheme.typography.titleSmall,
                    color = OrcaColors.accentBlue,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (multi) {
                    Text(
                        text = "可多选",
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Text(
                text = prompt,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
            if (options.isEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm)) {
                    OrcaSecondaryButton("跳过", Modifier.weight(1f), enabled) { onAnswer("") }
                    OrcaPrimaryButton("继续", Modifier.weight(1f), enabled) { onAnswer("继续") }
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                    options.forEach { option ->
                        val label = option.optString("label")
                        val selected = label in picked
                        Surface(
                            onClick = {
                                if (multi) {
                                    picked = if (selected) picked - label else picked + label
                                } else {
                                    onAnswer(label)
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                            enabled = enabled,
                            shape = RoundedCornerShape(OrcaRadius.button),
                            color =
                                if (selected) OrcaColors.accentBlue.copy(alpha = 0.18f)
                                else MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f),
                            border =
                                BorderStroke(
                                    1.dp,
                                    if (selected) OrcaColors.accentBlue
                                    else MaterialTheme.colorScheme.outline,
                                ),
                        ) {
                            Row(
                                modifier =
                                    Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                            ) {
                                if (multi) SelectionBox(selected)
                                Column(Modifier.weight(1f)) {
                                    Text(
                                        text = label,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurface,
                                        maxLines = 2,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                    val description = option.optString("description")
                                    if (description.isNotBlank()) {
                                        Text(
                                            text = description,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            maxLines = 2,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
                if (multi) {
                    OrcaPrimaryButton(
                        label = if (picked.isEmpty()) "选择后提交" else "提交 ${picked.size} 项选择",
                        modifier = Modifier.fillMaxWidth(),
                        enabled = enabled && picked.isNotEmpty(),
                    ) {
                        // The wire contract takes a string or a string array;
                        // sending an array is what the desktop console does.
                        onAnswer(picked.toList())
                    }
                }
            }
        }
    }
}

/** Square check affordance; `OrcaCheckIcon` is a bare glyph without a box. */
@Composable
private fun SelectionBox(selected: Boolean) {
    Box(
        modifier =
            Modifier
                .size(14.dp)
                .clip(RoundedCornerShape(3.dp))
                .background(if (selected) OrcaColors.accentBlue else Color.Transparent)
                .border(1.dp, if (selected) OrcaColors.accentBlue else OrcaColors.borderStrong, RoundedCornerShape(3.dp)),
        contentAlignment = Alignment.Center,
    ) {
        if (selected) OrcaCheckIcon(OrcaColors.onBright, 10.dp)
    }
}

// =========================================================================
// Transcript rows
// =========================================================================

@Composable
private fun UserBubble(text: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.End,
    ) {
        Surface(
            modifier = Modifier.widthIn(max = 300.dp),
            shape = RoundedCornerShape(OrcaRadius.card, OrcaRadius.card, OrcaSpace.xs, OrcaRadius.card),
            color = MaterialTheme.colorScheme.surfaceVariant,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        ) {
            Text(
                text = text,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
            )
        }
    }
}

private val THINK_BLOCK = Regex("<(?:think|thinking|thought)>([\\s\\S]*?)(?:</(?:think|thinking|thought)>|$)", RegexOption.IGNORE_CASE)

/**
 * Splits a `<think>` block out of a message. An unterminated block is a
 * reasoning item that is still streaming, so the accordion stays live.
 */
private fun splitThinking(text: String, streaming: Boolean): Triple<String?, String, Boolean> {
    val match = THINK_BLOCK.find(text)
        ?: return Triple(null, text, false)
    val block = match.groupValues[1].trim()
    val closed = match.value.contains("</think", ignoreCase = true) ||
        match.value.contains("</thinking", ignoreCase = true) ||
        match.value.contains("</thought", ignoreCase = true)
    val clean = text.removeRange(match.range).trim()
    return Triple(block.takeIf { it.isNotEmpty() }, clean, streaming && !closed)
}

@Composable
private fun AssistantMessage(item: TranscriptItem) {
    val (thinking, body, thinkingLive) = remember(item.text, item.streaming) { splitThinking(item.text, item.streaming) }
    Column(modifier = Modifier.fillMaxWidth()) {
        if (thinking != null) {
            ThinkingAccordion(thought = thinking, isGenerating = thinkingLive)
        }
        if (body.isNotEmpty() || thinking == null) {
            RenderMarkdownContent(content = body)
        }
        if (item.streaming) StreamingCaret()
    }
}

/** A caret that behaves like a live cursor instead of a spinner. */
@Composable
private fun StreamingCaret() {
    val transition = rememberInfiniteTransition(label = "streaming-caret")
    val alpha by
        transition.animateFloat(
            initialValue = 0.25f,
            targetValue = 1f,
            animationSpec = infiniteRepeatable(tween(900), RepeatMode.Reverse),
            label = "streaming-caret-alpha",
        )
    Row(
        modifier = Modifier.padding(top = OrcaSpace.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
    ) {
        Box(
            Modifier
                .alpha(alpha)
                .size(width = 7.dp, height = 14.dp)
                .background(OrcaColors.accentBlue, RoundedCornerShape(1.dp)),
        )
        Text(
            text = "生成中",
            style = OrcaType.mono,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun toolStateColor(state: ToolState?): Color =
    when (state) {
        ToolState.RUNNING -> OrcaColors.accentAmber
        ToolState.FAILED -> OrcaColors.accentRed
        else -> OrcaColors.textMuted
    }

private fun toolStateLabel(state: ToolState?): String =
    when (state) {
        ToolState.RUNNING -> "执行中…"
        ToolState.FAILED -> "执行失败"
        else -> "已完成"
    }

private fun readableOutput(value: String): String {
    val lines = value.lineSequence().take(120).toList()
    val joined = lines.joinToString("\n")
    return if (value.lineSequence().count() > 120) "$joined\n…（输出过长，已截断）" else joined
}

@Composable
private fun ToolCard(item: TranscriptItem) {
    val clipboard = LocalClipboardManager.current
    var expanded by remember(item.id) { mutableStateOf(false) }
    val title = item.title ?: "工具调用"
    val output = item.output
    val command = item.command
    val hasDetail = !command.isNullOrBlank() || !output.isNullOrBlank()
    val diff = command?.takeIf { it.contains("\n+") || it.startsWith("---") || it.startsWith("@@") }

    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(OrcaRadius.card),
        color = OrcaColors.bgInset,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .then(if (hasDetail) Modifier.clickable { expanded = !expanded } else Modifier)
                        .padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                OrcaStatusDot(toolStateColor(item.state), 5.dp)
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Text(
                        text = title,
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    command?.takeIf { it.isNotBlank() }?.let {
                        Text(
                            text = it.lineSequence().first().take(140),
                            style = OrcaType.mono,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                Text(
                    text = toolStateLabel(item.state),
                    style = OrcaType.mono,
                    color = toolStateColor(item.state),
                )
                if (hasDetail) {
                    OrcaChevronDown(
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        size = 10.dp,
                        expanded = expanded,
                    )
                }
            }
            AnimatedVisibility(visible = expanded && hasDetail) {
                Column(
                    modifier = Modifier.padding(start = OrcaSpace.md, end = OrcaSpace.md, bottom = OrcaSpace.md),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    if (diff != null) {
                        DiffPreview(diff)
                    } else if (!command.isNullOrBlank()) {
                        ToolSection(label = "命令 / 参数", text = readableOutput(command), onCopy = {
                            clipboard.setText(AnnotatedString(command))
                        })
                    }
                    output?.takeIf { it.isNotBlank() && !it.contains("[local_path_redacted]") }?.let {
                        ToolSection(label = "执行输出", text = readableOutput(readableText(it)), onCopy = {
                            clipboard.setText(AnnotatedString(it))
                        })
                    }
                }
            }
        }
    }
}

@Composable
private fun ToolSection(label: String, text: String, onCopy: () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = label,
                style = OrcaType.mono,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OrcaTextAction(label = "复制", onClick = onCopy)
        }
        Text(
            text = text,
            style = OrcaType.code,
            color = MaterialTheme.colorScheme.onSurface,
            modifier =
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(OrcaRadius.button))
                    .background(OrcaColors.bgRaised.copy(alpha = 0.5f))
                    .padding(OrcaSpace.sm),
        )
    }
}

@Composable
private fun SystemLine(item: TranscriptItem) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .width(1.dp)
                .height(14.dp)
                .background(OrcaColors.accentPurple.copy(alpha = 0.7f)),
        )
        Text(
            text = listOfNotNull(item.title, readableText(item.text)).joinToString(" · "),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun ToolGroup(items: List<TranscriptItem>) {
    var expanded by remember(items.first().id) { mutableStateOf(false) }
    val running = items.any { it.state == ToolState.RUNNING }
    val failed = items.any { it.state == ToolState.FAILED }
    val color =
        when {
            failed -> OrcaColors.accentRed
            running -> OrcaColors.accentAmber
            else -> OrcaColors.textMuted
        }
    val label = when {
        failed -> "异常"
        running -> "执行中"
        else -> "完成"
    }
    val preview =
        items.take(3).joinToString(" · ") {
            (it.title ?: "工具调用") + (it.command?.lineSequence()?.firstOrNull()?.takeIf { cmd -> cmd.isNotBlank() }?.let { cmd -> " “$cmd”" } ?: "")
        }

    Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
        Surface(
            onClick = { expanded = !expanded },
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(OrcaRadius.card),
            color = OrcaColors.bgInset,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        ) {
            Row(
                modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                OrcaStatusDot(color, 5.dp)
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Text(
                        text = "已执行 ${items.size} 个工具与操作",
                        style = OrcaType.rowTitle,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                    )
                    Text(
                        text = preview,
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                Text(text = label, style = OrcaType.mono, color = color)
                OrcaChevronDown(
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    size = 10.dp,
                    expanded = expanded,
                )
            }
        }
        AnimatedVisibility(visible = expanded) {
            Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                items.forEach { ToolCard(it) }
            }
        }
    }
}

// =========================================================================
// Accessory keys — ported from Orca `terminal-key-definitions.ts`
// =========================================================================

private data class AccessoryKey(val label: String, val danger: Boolean = false)

private val ACCESSORY_KEYS =
    listOf(
        AccessoryKey("Esc"),
        AccessoryKey("Tab"),
        AccessoryKey("Ctrl+C", danger = true),
    )

private val ACCESSORY_MACROS =
    listOf(
        "继续",
        "git status",
        "运行测试",
        "git diff",
    )

@Composable
private fun TerminalAccessoryBar(
    enabled: Boolean,
    onKey: (String) -> Unit,
    onMacro: (String) -> Unit,
) {
    LazyRow(
        modifier = Modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = OrcaSpace.lg),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        items(ACCESSORY_KEYS) { key ->
            val tint = if (key.danger) OrcaColors.accentRed else MaterialTheme.colorScheme.onSurface
            Surface(
                onClick = { onKey(key.label) },
                enabled = enabled,
                shape = RoundedCornerShape(OrcaRadius.button),
                color = if (key.danger) OrcaColors.accentRed.copy(alpha = 0.10f) else MaterialTheme.colorScheme.surfaceVariant,
                border =
                    BorderStroke(
                        1.dp,
                        if (key.danger) OrcaColors.accentRed.copy(alpha = 0.45f) else MaterialTheme.colorScheme.outline,
                    ),
            ) {
                Text(
                    text = key.label,
                    style = OrcaType.mono,
                    color = tint,
                    modifier = Modifier.padding(horizontal = OrcaSpace.sm, vertical = 5.dp),
                )
            }
        }
        item {
            Box(
                Modifier
                    .width(1.dp)
                    .height(16.dp)
                    .background(MaterialTheme.colorScheme.outline),
            )
        }
        items(ACCESSORY_MACROS) { macro ->
            Surface(
                onClick = { onMacro(macro) },
                enabled = enabled,
                shape = RoundedCornerShape(OrcaRadius.button),
                color = Color.Transparent,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
            ) {
                Text(
                    text = macro,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = OrcaSpace.sm, vertical = 5.dp),
                )
            }
        }
    }
}

// =========================================================================
// Markdown rendering
// =========================================================================

@Composable
fun RenderMarkdownContent(content: String) {
    // The cache survives recomposition of the streaming row, so each delta only
    // re-parses the changed tail instead of the whole message.
    val cache = remember { MarkdownSegmentCache() }
    val segments = remember(content, cache) { cache.update(content) }
    Column(Modifier.fillMaxWidth()) {
        segments.forEachIndexed { index, seg ->
            if (seg.isCodeBlock) {
                // Keyed by index and language so Compose preserves block identity
                // across streaming deltas without remounting the surface.
                key("block-" + index + "-" + seg.language) {
                    CodeBlockSurface(code = seg.text, language = seg.language)
                }
            } else {
                Text(
                    text = buildInlineMarkdown(seg.text),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(vertical = OrcaSpace.xxs),
                )
            }
        }
    }
}

// =========================================================================
// Approval / question state
// =========================================================================

internal fun remoteApprovals(events: List<JSONObject>, hubSessionId: String): Map<String, JSONObject> {
    val pending = linkedMapOf<String, JSONObject>()
    for (envelope in events) {
        val payload = envelope.optJSONObject("payload") ?: continue
        when (payload.optString("type")) {
            "ready" -> {
                pending.clear()
                payload.optJSONArray("pending_approvals")?.objects()
                    ?.filter { it.optString("hub_session_id") == hubSessionId }
                    ?.forEach { pending[it.getString("approval_id")] = it }
            }
            "host_lost" -> pending.clear()
            "receipt" -> {
                val receipt = payload.optJSONObject("receipt") ?: continue
                if (receipt.optString("status") == "accepted") {
                    receipt.optJSONObject("result")
                        ?.optString("approval_id")
                        ?.takeIf { it.isNotEmpty() }
                        ?.let(pending::remove)
                }
            }
            "event" -> {
                val event = payload.optJSONObject("event") ?: continue
                when (event.optString("kind")) {
                    "approval_requested" ->
                        event.optJSONObject("approval")
                            ?.takeIf { it.optString("hub_session_id") == hubSessionId }
                            ?.let { pending[it.getString("approval_id")] = it }
                    "approval_resolved" ->
                        event.optString("approval_id").takeIf { it.isNotEmpty() }?.let(pending::remove)
                    "turn/completed" ->
                        if (event.optJSONObject("binding")?.optString("hub_session_id") == hubSessionId) {
                            pending.clear()
                        }
                }
            }
        }
    }
    return pending
}

internal fun remoteQuestions(events: List<JSONObject>, hubSessionId: String): Map<String, JSONObject> {
    val pending = linkedMapOf<String, JSONObject>()
    for (envelope in events) {
        val payload = envelope.optJSONObject("payload") ?: continue
        when (payload.optString("type")) {
            "ready" -> {
                pending.clear()
                payload.optJSONArray("pending_questions")?.objects()
                    ?.filter { it.optString("hub_session_id") == hubSessionId }
                    ?.forEach { pending[it.getString("question_id")] = it }
            }
            "host_lost" -> pending.clear()
            "event" -> {
                val event = payload.optJSONObject("event") ?: continue
                when (event.optString("kind")) {
                    "question_requested" ->
                        event.optJSONObject("question")
                            ?.takeIf { it.optString("hub_session_id") == hubSessionId }
                            ?.let { pending[it.getString("question_id")] = it }
                    "question_resolved" ->
                        event.optString("question_id").takeIf { it.isNotEmpty() }?.let(pending::remove)
                    "turn/completed" ->
                        if (event.optJSONObject("binding")?.optString("hub_session_id") == hubSessionId) {
                            pending.clear()
                        }
                }
            }
        }
    }
    return pending
}
