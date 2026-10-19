package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
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

private fun JSONArray.objects(): List<JSONObject> = (0 until length()).map { getJSONObject(it) }
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

data class UiMessage(
    val id: String,
    val role: MessageRole,
    val text: String,
    val isStreaming: Boolean = false,
    val toolTitle: String? = null,
)

// =========================================================================
// Session screen
//
// Header -> approval / question banners -> transcript -> accessory keys ->
// composer. Polling pauses whenever the app leaves the foreground.
// =========================================================================

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

    val conversation = remember(events, commands) { parseStructuredConversation(events, commands) }
    val approvals = remember(events, hubId) { remoteApprovals(events, hubId) }
    val questions = remember(events, hubId) { remoteQuestions(events, hubId) }

    // Follow new output only while the reader is already at the bottom, so
    // scrolling back through history is never yanked away.
    val lastMessageLength = conversation.lastOrNull()?.text?.length ?: 0
    LaunchedEffect(conversation.size, lastMessageLength) {
        if (conversation.isEmpty()) return@LaunchedEffect
        val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: -1
        if (lastVisible >= conversation.lastIndex - 1) {
            listState.scrollToItem(conversation.lastIndex)
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
                                    .put("answer", answer),
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
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.md),
        ) {
            if (conversation.isEmpty()) {
                item {
                    OrcaCard {
                        OrcaNoticeCard(
                            title = if (connected) "等待 Agent 输出" else "等待电脑连线",
                            body =
                                if (connected) "会话建立后，消息、思考过程与工具调用会实时出现在这里。"
                                else "保持 Runner 在线，手机端会自动恢复这条会话的控制权。",
                            icon = { OrcaTerminalIcon(MaterialTheme.colorScheme.onSurfaceVariant, 22.dp) },
                        )
                    }
                }
            }
            items(conversation, key = { it.id }) { message ->
                when (message.role) {
                    MessageRole.USER -> UserBubble(message.text)
                    MessageRole.ASSISTANT -> AssistantMessage(message)
                    MessageRole.TOOL -> ToolLine(message.toolTitle ?: "工具调用")
                    MessageRole.SYSTEM -> SystemLine(message.text)
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
                text = body,
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
    onAnswer: (String) -> Unit,
) {
    val header = question.optString("header").ifBlank { "Agent 提问" }
    val prompt = question.optString("prompt").ifBlank { "请选择一个选项以继续。" }
    val options = question.optJSONArray("options")?.objects().orEmpty()

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
                    options.take(4).forEach { option ->
                        val label = option.optString("label")
                        Surface(
                            onClick = { onAnswer(label) },
                            modifier = Modifier.fillMaxWidth(),
                            enabled = enabled,
                            shape = RoundedCornerShape(OrcaRadius.button),
                            color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f),
                            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                        ) {
                            Text(
                                text = label,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurface,
                                modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            }
        }
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

@Composable
private fun AssistantMessage(message: UiMessage) {
    Column(modifier = Modifier.fillMaxWidth()) {
        if (message.text.contains("<think>") && message.text.contains("</think>")) {
            val start = message.text.indexOf("<think>") + 7
            val end = message.text.indexOf("</think>")
            if (end > start) {
                ThinkingAccordion(
                    thought = message.text.substring(start, end),
                    isGenerating = message.isStreaming,
                )
                RenderMarkdownContent(
                    content = message.text.substring(end + 8),
                )
                return@Column
            }
        }
        RenderMarkdownContent(content = message.text)
    }
}

@Composable
private fun ToolLine(title: String) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
    ) {
        OrcaStatusDot(OrcaColors.accentPurple, 5.dp)
        Text(
            text = title,
            style = OrcaType.mono,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun SystemLine(text: String) {
    Text(
        text = text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
        modifier = Modifier.fillMaxWidth(),
    )
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
    val segments = remember(content) { parseMarkdownSegments(content) }
    Column(Modifier.fillMaxWidth()) {
        segments.forEach { seg ->
            if (seg.isCodeBlock) {
                CodeBlockSurface(code = seg.text, language = seg.language)
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
// Event parsing
// =========================================================================

private fun parseStructuredConversation(
    events: List<JSONObject>,
    commands: List<JSONObject>,
): List<UiMessage> {
    val result = linkedMapOf<String, UiMessage>()
    val prompts =
        commands
            .mapNotNull { command ->
                val receipt = command.optJSONObject("receipt") ?: return@mapNotNull null
                if (receipt.optString("status") != "accepted") return@mapNotNull null
                val turn = receipt.optJSONObject("result")?.optJSONObject("turn")?.optString("id") ?: return@mapNotNull null
                val text = command.optJSONObject("command")?.optJSONObject("command")?.optString("text").orEmpty()
                turn to text
            }
            .toMap()

    for (envelope in events) {
        val payload = envelope.optJSONObject("payload") ?: continue
        if (payload.optString("type") != "event") continue
        val event = payload.optJSONObject("event") ?: continue
        val p = event.optJSONObject("raw")?.optJSONObject("params") ?: continue
        val item = p.optJSONObject("item")
        val id = p.optString("itemId").ifBlank { item?.optString("id").orEmpty() }

        when (event.optString("kind")) {
            "turn/started" -> {
                val turn = p.optJSONObject("turn")?.optString("id").orEmpty()
                prompts[turn]?.takeIf { it.isNotBlank() }?.let {
                    result["user:$turn"] = UiMessage(id = "user:$turn", role = MessageRole.USER, text = it)
                }
            }
            "item/agentMessage/delta" -> {
                if (id.isNotEmpty()) {
                    val prev = result[id]?.text.orEmpty()
                    result[id] =
                        UiMessage(
                            id = id,
                            role = MessageRole.ASSISTANT,
                            text = prev + p.optString("delta"),
                            isStreaming = true,
                        )
                }
            }
            "item/started" -> {
                if (id.isNotEmpty()) {
                    when (item?.optString("type")) {
                        "reasoning" -> {
                            val summary = reasoningText(item)
                            if (summary.isNotBlank()) {
                                result[id] =
                                    UiMessage(
                                        id = id,
                                        role = MessageRole.ASSISTANT,
                                        text = "<think>$summary</think>",
                                        isStreaming = true,
                                    )
                            }
                        }
                        "commandExecution" -> {
                            val command = item.optString("command").trim()
                            if (command.isNotBlank()) {
                                result["tool:$id"] =
                                    UiMessage(
                                        id = "tool:$id",
                                        role = MessageRole.TOOL,
                                        text = "",
                                        toolTitle = "$ ${command.take(96)}",
                                    )
                            }
                        }
                    }
                }
            }
            "item/completed" -> {
                if (id.isNotEmpty()) {
                    when (item?.optString("type")) {
                        "userMessage" -> {
                            val content = item.optJSONArray("content")
                            val text =
                                if (content != null) {
                                    (0 until content.length())
                                        .mapNotNull { content.optJSONObject(it) }
                                        .filter { it.optString("type") == "text" }
                                        .map { it.optString("text") }
                                        .joinToString("\n")
                                } else {
                                    item.optString("text")
                                }
                            if (text.isNotBlank()) {
                                result[id] = UiMessage(id = id, role = MessageRole.USER, text = text)
                            }
                        }
                        "agentMessage" -> {
                            result[id] =
                                UiMessage(
                                    id = id,
                                    role = MessageRole.ASSISTANT,
                                    text = item.optString("text"),
                                    isStreaming = false,
                                )
                        }
                        "reasoning" -> {
                            val summary = reasoningText(item)
                            if (summary.isNotBlank()) {
                                result[id] =
                                    UiMessage(
                                        id = id,
                                        role = MessageRole.ASSISTANT,
                                        text = "<think>$summary</think>",
                                    )
                            }
                        }
                        "commandExecution" -> {
                            val command = item.optString("command").trim()
                            val exit = item.optInt("exitCode", Int.MIN_VALUE)
                            val title =
                                if (command.isNotBlank()) {
                                    "$ ${command.take(96)}" + if (exit != Int.MIN_VALUE) "  ·  exit $exit" else ""
                                } else {
                                    "命令执行完成"
                                }
                            result["tool:$id"] =
                                UiMessage(id = "tool:$id", role = MessageRole.TOOL, text = "", toolTitle = title)
                        }
                        "fileChange", "fileChangeStart" -> {
                            result["tool:$id"] =
                                UiMessage(
                                    id = "tool:$id",
                                    role = MessageRole.TOOL,
                                    text = "",
                                    toolTitle = "文件变更 · " + item.optString("path").ifBlank { id.take(8) },
                                )
                        }
                    }
                }
            }
        }
    }

    return result.values.toList()
}

private fun reasoningText(item: JSONObject): String {
    val summary = item.optJSONArray("summary")
    if (summary != null && summary.length() > 0) {
        return (0 until summary.length())
            .mapNotNull { summary.optJSONObject(it) }
            .joinToString("\n") { it.optString("text") }
            .trim()
    }
    return item.optString("text").trim()
}

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
