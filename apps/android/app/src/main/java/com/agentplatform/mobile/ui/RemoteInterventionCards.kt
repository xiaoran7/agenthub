package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.data.PendingApproval
import com.agentplatform.mobile.data.PendingQuestion
import com.agentplatform.mobile.data.optStringOrEmpty
import org.json.JSONArray
import org.json.JSONObject

// =========================================================================
// Intervention banners: TurnFailureCard, ApprovalCard, DiffPreview, QuestionCard
// =========================================================================

@Composable
internal fun TurnFailureCard(
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
internal fun ApprovalCard(
    approval: PendingApproval,
    enabled: Boolean,
    onApprove: () -> Unit,
    onDecline: () -> Unit,
) {
    var expanded by remember(approval.approvalId) { mutableStateOf(false) }
    val highRisk = approval.riskLevel == "high"
    val bodyScroll = rememberScrollState()
    Surface(
        modifier = Modifier.widthIn(min = 280.dp, max = 440.dp),
        shape = RoundedCornerShape(OrcaRadius.card),
        color = OrcaColors.bgPanel,
        border = BorderStroke(1.dp, OrcaColors.accentAmber.copy(alpha = 0.6f)),
        shadowElevation = 2.dp,
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
                StatusPill(
                    label = if (highRisk) "高风险" else approval.toolName ?: "工具",
                    color = if (highRisk) OrcaColors.accentRed else OrcaColors.accentAmber,
                )
            }
            Column(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .heightIn(max = if (expanded) 190.dp else 96.dp)
                        .verticalScroll(bodyScroll),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                Text(
                    text = readableText(approval.summary),
                    style = OrcaType.mono,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = if (expanded) Int.MAX_VALUE else 3,
                    overflow = TextOverflow.Ellipsis,
                )
                val diff = approval.diff
                if (!diff.isNullOrBlank()) {
                    AnimatedVisibility(
                        visible = expanded,
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
                    ) { DiffPreview(diff) }
                }
            }
            if (!approval.diff.isNullOrBlank()) {
                OrcaTextAction(
                    label = if (expanded) "收起变更" else "查看变更",
                    onClick = { expanded = !expanded },
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm)) {
                OrcaSecondaryButton(
                    label = "拒绝",
                    modifier = Modifier.weight(1f),
                    enabled = enabled && approval.canDecline,
                    onClick = onDecline,
                )
                OrcaPrimaryButton(
                    label = "批准",
                    modifier = Modifier.weight(1f),
                    enabled = enabled && approval.canApprove,
                    onClick = onApprove,
                )
            }
        }
    }
}

/** Parses structured Claude Edit/Write JSON tool payloads into unified diff lines when needed. */
internal fun normalizeDiffText(raw: String): String {
    val trimmed = raw.trim()
    if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
        val parsed = runCatching { JSONObject(trimmed) }.getOrNull()
        if (parsed != null) {
            val oldStr = parsed.optStringOrEmpty("old_string").ifEmpty { parsed.optStringOrEmpty("oldText") }
            val newStr = parsed.optStringOrEmpty("new_string").ifEmpty { parsed.optStringOrEmpty("newText") }
            val content = parsed.optStringOrEmpty("content")
            if (oldStr.isNotEmpty() || newStr.isNotEmpty()) {
                val sb = StringBuilder()
                if (oldStr.isNotEmpty()) {
                    oldStr.lines().forEach { sb.append("- ").append(it).append('\n') }
                }
                if (newStr.isNotEmpty()) {
                    newStr.lines().forEach { sb.append("+ ").append(it).append('\n') }
                }
                return sb.toString().trimEnd()
            }
            if (content.isNotEmpty()) {
                return content.lines().joinToString("\n") { "+ $it" }
            }
        }
    }
    return raw
}

private const val INITIAL_DIFF_LINES = 80
private const val DIFF_PAGE_STEP = 240

@Composable
internal fun DiffPreview(diff: String) {
    val normalized = remember(diff) { normalizeDiffText(diff) }
    val allLines = remember(normalized) { normalized.lines() }
    var visibleLimit by remember(diff) { mutableStateOf(INITIAL_DIFF_LINES) }
    val added = remember(allLines) { allLines.count { it.startsWith("+") && !it.startsWith("+++") } }
    val removed = remember(allLines) { allLines.count { it.startsWith("-") && !it.startsWith("---") } }
    val visibleLines =
        remember(allLines, visibleLimit) {
            if (allLines.size <= visibleLimit) allLines else allLines.take(visibleLimit)
        }
    val hiddenCount = (allLines.size - visibleLines.size).coerceAtLeast(0)
    val hScroll = rememberScrollState()
    val defaultLineColor = MaterialTheme.colorScheme.onSurfaceVariant
    val addedLineColor = OrcaColors.accentGreen
    val removedLineColor = OrcaColors.accentRed
    val diffText =
        remember(visibleLines, defaultLineColor, addedLineColor, removedLineColor) {
            buildAnnotatedString {
                visibleLines.forEachIndexed { index, line ->
                    if (index > 0) append('\n')
                    val color =
                        when {
                            line.startsWith("+") && !line.startsWith("+++") -> addedLineColor
                            line.startsWith("-") && !line.startsWith("---") -> removedLineColor
                            else -> defaultLineColor
                        }
                    withStyle(SpanStyle(color = color)) {
                        append(line.ifEmpty { " " })
                    }
                }
            }
        }

    Column(
        modifier =
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(OrcaRadius.button))
                .background(OrcaColors.bgInset)
                .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(OrcaRadius.button))
                .padding(OrcaSpace.sm),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (added > 0 || removed > 0) {
            Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm)) {
                if (added > 0) {
                    Text(
                        text = "+$added",
                        style = OrcaType.mono,
                        color = OrcaColors.accentGreen,
                    )
                }
                if (removed > 0) {
                    Text(
                        text = "-$removed",
                        style = OrcaType.mono,
                        color = OrcaColors.accentRed,
                    )
                }
            }
        }
        Text(
            text = diffText,
            style = OrcaType.code.copy(fontSize = 11.sp, lineHeight = 16.sp),
            softWrap = false,
            modifier = Modifier.fillMaxWidth().horizontalScroll(hScroll),
        )
        if (hiddenCount > 0) {
            val nextStep = minOf(hiddenCount, DIFF_PAGE_STEP)
            OrcaTextAction(
                label =
                    if (hiddenCount <= DIFF_PAGE_STEP) "展开剩余 $hiddenCount 行变更"
                    else "再展开 $nextStep 行（共剩余 $hiddenCount 行）",
                onClick = { visibleLimit += DIFF_PAGE_STEP },
            )
        }
    }
}

@Composable
internal fun QuestionCard(
    question: PendingQuestion,
    enabled: Boolean,
    submitting: Boolean = false,
    onAnswer: (Any) -> Unit,
) {
    var picked by remember(question.questionId) { mutableStateOf(setOf<String>()) }
    var customAnswer by remember(question.questionId) { mutableStateOf("") }
    val interactionEnabled = enabled && !submitting
    val submitAnswer: (Any) -> Unit = { answer ->
        if (interactionEnabled) {
            onAnswer(answer)
        }
    }
    val scroll = rememberScrollState()

    Surface(
        modifier = Modifier.widthIn(min = 280.dp, max = 440.dp),
        shape = RoundedCornerShape(OrcaRadius.card),
        color = OrcaColors.accentBlue.copy(alpha = 0.08f),
        border = BorderStroke(1.dp, OrcaColors.accentBlue.copy(alpha = 0.55f)),
    ) {
        Column(
            modifier =
                Modifier
                    .padding(OrcaSpace.md)
                    .heightIn(max = 310.dp)
                    .verticalScroll(scroll),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                OrcaStatusDot(OrcaColors.accentBlue, 6.dp)
                Text(
                    text = question.header ?: "Agent 提问",
                    style = MaterialTheme.typography.titleSmall,
                    color = OrcaColors.accentBlue,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (question.multiSelect) {
                    Text(
                        text = "可多选",
                        style = OrcaType.mono,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Text(
                text = question.prompt,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurface,
            )
            if (question.options.isNotEmpty()) {
                Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
                    question.options.forEach { option ->
                        val selected = option.label in picked
                        Surface(
                            onClick = {
                                if (question.multiSelect) {
                                    picked =
                                        if (selected) picked - option.label
                                        else picked + option.label
                                } else {
                                    submitAnswer(option.label)
                                }
                            },
                            modifier = Modifier.fillMaxWidth(),
                            enabled = interactionEnabled,
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
                                    Modifier.padding(
                                        horizontal = OrcaSpace.md,
                                        vertical = OrcaSpace.sm,
                                    ),
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                            ) {
                                if (question.multiSelect) SelectionBox(selected)
                                Column(
                                    modifier = Modifier.weight(1f),
                                    verticalArrangement = Arrangement.spacedBy(2.dp),
                                ) {
                                    Text(
                                        text = option.label,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurface,
                                    )
                                    option.description?.takeIf { it.isNotBlank() }?.let { desc ->
                                        Text(
                                            text = desc,
                                            style = MaterialTheme.typography.labelSmall,
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        )
                                    }
                                    option.preview?.takeIf { it.isNotBlank() }?.let { preview ->
                                        Text(
                                            text = preview,
                                            style = OrcaType.code.copy(fontSize = 11.sp),
                                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                                            maxLines = 3,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
                if (question.multiSelect && picked.isNotEmpty()) {
                    OrcaPrimaryButton(
                        label = "提交 ${picked.size} 项选择",
                        modifier = Modifier.fillMaxWidth(),
                        enabled = interactionEnabled,
                    ) {
                        val combined =
                            if (customAnswer.isNotBlank()) picked.toList() + customAnswer.trim()
                            else picked.toList()
                        submitAnswer(combined)
                    }
                }
            }

            // Custom free-text answer box (matches Web QuestionCard custom answer input)
            OutlinedTextField(
                value = customAnswer,
                onValueChange = { customAnswer = it },
                enabled = interactionEnabled,
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
                placeholder = {
                    Text(
                        text = "或者输入自定义回答…",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.65f),
                    )
                },
                textStyle = MaterialTheme.typography.bodySmall.copy(color = MaterialTheme.colorScheme.onSurface),
                shape = RoundedCornerShape(OrcaRadius.input),
                colors =
                    OutlinedTextFieldDefaults.colors(
                        focusedContainerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
                        unfocusedContainerColor = Color.Transparent,
                        focusedBorderColor = OrcaColors.accentBlue,
                        unfocusedBorderColor = MaterialTheme.colorScheme.outline,
                    ),
            )

            Row(horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm)) {
                if (question.options.isEmpty()) {
                    OrcaSecondaryButton("跳过", Modifier.weight(1f), interactionEnabled) {
                        // Contract requires non-blank answer string (`!value.trim()` rejects "")
                        submitAnswer("跳过")
                    }
                }
                if (customAnswer.isNotBlank() || question.options.isEmpty()) {
                    OrcaPrimaryButton(
                        label = if (customAnswer.isNotBlank()) "发送回答" else "继续",
                        modifier = Modifier.weight(1f),
                        enabled = interactionEnabled,
                    ) {
                        submitAnswer(customAnswer.trim().ifBlank { "继续" })
                    }
                }
            }
        }
    }
}

@Composable
private fun SelectionBox(selected: Boolean) {
    androidx.compose.foundation.layout.Box(
        modifier =
            Modifier
                .size(16.dp)
                .clip(RoundedCornerShape(4.dp))
                .background(if (selected) OrcaColors.accentBlue else Color.Transparent)
                .border(
                    1.dp,
                    if (selected) OrcaColors.accentBlue else OrcaColors.borderStrong,
                    RoundedCornerShape(4.dp),
                ),
        contentAlignment = Alignment.Center,
    ) {
        if (selected) OrcaCheckIcon(Color.White, 11.dp)
    }
}

@Composable
internal fun RemoteInterventionRail(
    approvals: List<PendingApproval>,
    questions: List<PendingQuestion>,
    enabled: Boolean,
    submittingQuestionIds: Set<String>,
    onResolveApproval: (PendingApproval, Boolean) -> Unit,
    onAnswerQuestion: (PendingQuestion, Any) -> Unit,
) {
    if (approvals.isEmpty() && questions.isEmpty()) return
    val uniqueApprovals = remember(approvals) { approvals.distinctBy { it.approvalId } }
    val uniqueQuestions = remember(questions) { questions.distinctBy { it.questionId } }
    LazyRow(
        modifier = Modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
    ) {
        items(uniqueApprovals, key = { "approval:${it.approvalId}" }) { approval ->
            ApprovalCard(
                approval = approval,
                enabled = enabled,
                onApprove = { onResolveApproval(approval, true) },
                onDecline = { onResolveApproval(approval, false) },
            )
        }
        items(uniqueQuestions, key = { "question:${it.questionId}" }) { question ->
            QuestionCard(
                question = question,
                enabled = enabled,
                submitting = question.questionId in submittingQuestionIds,
                onAnswer = { answer -> onAnswerQuestion(question, answer) },
            )
        }
    }
}

internal fun resolveApprovalCommand(approvalId: String, approved: Boolean): JSONObject =
    JSONObject()
        .put("action", "resolve_approval")
        .put("approval_id", approvalId)
        .put("approved", approved)

internal fun answerQuestionCommand(questionId: String, answer: Any): JSONObject =
    JSONObject()
        .put("action", "answer_question")
        .put("question_id", questionId)
        .put("answer", if (answer is List<*>) JSONArray(answer) else answer)
