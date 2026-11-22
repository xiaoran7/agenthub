package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay

// =========================================================================
// Transcript rows: UserBubble, AssistantMessage, ToolCard, ToolGroup, Skeleton
// =========================================================================

@Composable
internal fun AgentThinkingSkeleton(agentLabel: String) {
    val transition = rememberInfiniteTransition(label = "shimmer")
    val shimmerTranslate by
        transition.animateFloat(
            initialValue = -1f,
            targetValue = 2f,
            animationSpec =
                infiniteRepeatable(
                    animation = tween(durationMillis = 1800),
                    repeatMode = RepeatMode.Restart,
                ),
            label = "shimmerTranslate",
        )
    val dotAlpha by
        transition.animateFloat(
            initialValue = 0.35f,
            targetValue = 1f,
            animationSpec =
                infiniteRepeatable(
                    animation = tween(durationMillis = 900),
                    repeatMode = RepeatMode.Reverse,
                ),
            label = "dotAlpha",
        )

    Column(
        modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.xs),
        verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
        ) {
            Box(
                modifier =
                    Modifier
                        .size(6.dp)
                        .clip(CircleShape)
                        .background(OrcaColors.accentAmber.copy(alpha = dotAlpha)),
            )
            Text(
                text = agentLabel.ifBlank { "Agent" },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(OrcaRadius.tile),
            color = OrcaColors.bgPanel,
            border = BorderStroke(1.dp, OrcaColors.borderSubtle),
        ) {
            val shimmerBrush =
                Brush.linearGradient(
                    colors =
                        listOf(
                            OrcaColors.bgPanel,
                            OrcaColors.bgRaised,
                            OrcaColors.bgPanel,
                        ),
                    start = Offset(shimmerTranslate * 600f, 0f),
                    end = Offset((shimmerTranslate + 1f) * 600f, 200f),
                )
            Column(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .background(shimmerBrush)
                        .padding(OrcaSpace.sm),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    Box(
                        modifier =
                            Modifier
                                .size(6.dp)
                                .clip(CircleShape)
                                .background(OrcaColors.accentAmber),
                    )
                    Text(
                        text = "正在接入思考与工作流…",
                        style = OrcaType.mono.copy(fontSize = 12.sp, fontWeight = FontWeight.Medium),
                        color = OrcaColors.accentAmber,
                    )
                }
                Box(
                    modifier =
                        Modifier
                            .fillMaxWidth(0.85f)
                            .height(10.dp)
                            .clip(RoundedCornerShape(5.dp))
                            .background(OrcaColors.borderSubtle.copy(alpha = 0.6f)),
                )
                Box(
                    modifier =
                        Modifier
                            .fillMaxWidth(0.55f)
                            .height(10.dp)
                            .clip(RoundedCornerShape(5.dp))
                            .background(OrcaColors.borderSubtle.copy(alpha = 0.6f)),
                )
            }
        }
    }
}

@Composable
internal fun UserBubble(text: String, sending: Boolean = false) {
    if (isRedactedPathOnly(text)) return
    val clipboard = LocalClipboardManager.current
    val isLight = LocalAppearance.current.light
    var copied by remember { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1600)
            copied = false
        }
    }

    val bubbleBg =
        if (isLight) OrcaColors.surfaceBright
        else MaterialTheme.colorScheme.primaryContainer
    val bubbleText =
        if (isLight) OrcaColors.onBright
        else OrcaColors.textPrimary
    val bubbleBorder =
        when {
            sending -> OrcaColors.accentAmber
            isLight -> OrcaColors.surfaceBright
            else -> OrcaColors.accentBlue.copy(alpha = 0.45f)
        }

    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.xxs),
        horizontalArrangement = Arrangement.End,
    ) {
        Surface(
            modifier =
                Modifier
                    .widthIn(min = 44.dp, max = 520.dp)
                    .clickable {
                        clipboard.setText(AnnotatedString(text))
                        copied = true
                    },
            shape =
                RoundedCornerShape(
                    topStart = 18.dp,
                    topEnd = 18.dp,
                    bottomStart = 18.dp,
                    bottomEnd = 4.dp,
                ),
            color = bubbleBg,
            border = BorderStroke(1.dp, bubbleBorder),
        ) {
            Column(
                modifier = Modifier.padding(horizontal = 15.dp, vertical = 10.dp),
                horizontalAlignment = Alignment.End,
            ) {
                Text(
                    text = readableText(text),
                    style =
                        MaterialTheme.typography.bodyMedium.copy(
                            fontSize = 15.sp,
                            lineHeight = 22.sp,
                        ),
                    color = bubbleText,
                )
                if (sending || copied) {
                    Row(
                        modifier = Modifier.padding(top = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        if (sending) {
                            Box(
                                modifier =
                                    Modifier
                                        .size(4.dp)
                                        .clip(CircleShape)
                                        .background(OrcaColors.accentAmber),
                            )
                            Text(
                                text = "发送中…",
                                style = OrcaType.mono.copy(fontSize = 10.sp),
                                color = OrcaColors.accentAmber,
                            )
                        } else {
                            Text(
                                text = "已复制",
                                style = OrcaType.mono.copy(fontSize = 10.sp),
                                color = bubbleText.copy(alpha = 0.8f),
                            )
                        }
                    }
                }
            }
        }
    }
}

private val THINK_BLOCK =
    Regex(
        "<(?:think|thinking|thought)>([\\s\\S]*?)(?:</(?:think|thinking|thought)>|$)",
        RegexOption.IGNORE_CASE,
    )

/** Extracts all `<think>`/`<thinking>` blocks so multi-block thoughts never leak raw XML. */
internal fun splitThinking(text: String, streaming: Boolean): Triple<String?, String, Boolean> {
    val matches = THINK_BLOCK.findAll(text).toList()
    if (matches.isEmpty()) return Triple(null, text, false)
    val thoughts =
        matches
            .mapNotNull { it.groupValues[1].trim().takeIf { chunk -> chunk.isNotEmpty() } }
            .joinToString("\n\n")
    val last = matches.last()
    val closed =
        last.value.contains("</think", ignoreCase = true) ||
            last.value.contains("</thinking", ignoreCase = true) ||
            last.value.contains("</thought", ignoreCase = true)
    val clean = text.replace(THINK_BLOCK, "").trim()
    return Triple(thoughts.takeIf { it.isNotEmpty() }, clean, streaming && !closed)
}

@Composable
internal fun AssistantMessage(item: TranscriptItem, agentLabel: String = "Agent") {
    if (isRedactedPathOnly(item.text)) return
    val clipboard = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    val (thinking, body, thinkingLive) =
        remember(item.text, item.streaming) { splitThinking(item.text, item.streaming) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1800)
            copied = false
        }
    }

    Column(
        modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.xs),
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
            modifier = Modifier.fillMaxWidth().padding(bottom = 4.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
            ) {
                Box(
                    modifier =
                        Modifier
                            .size(20.dp)
                            .clip(CircleShape)
                            .background(OrcaColors.accentBlue.copy(alpha = 0.12f)),
                    contentAlignment = Alignment.Center,
                ) {
                    OrcaLogo(size = 13.dp, tint = OrcaColors.accentBlue)
                }
                Text(
                    text = agentLabel.ifBlank { "Agent" },
                    style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Bold),
                    color = MaterialTheme.colorScheme.onSurface,
                )
                if (item.streaming) {
                    OrcaStatusDot(OrcaColors.accentAmber, 4.dp)
                    Text(
                        text = "正在生成…",
                        style = OrcaType.mono,
                        color = OrcaColors.accentAmber,
                    )
                }
            }
            if (body.isNotBlank()) {
                Row(
                    modifier =
                        Modifier
                            .defaultMinSize(minHeight = 32.dp)
                            .clip(RoundedCornerShape(6.dp))
                            .clickable {
                                clipboard.setText(AnnotatedString(body))
                                copied = true
                            }
                            .padding(horizontal = OrcaSpace.xs, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    if (copied) OrcaCheckIcon(OrcaColors.accentGreen, 12.dp)
                    else OrcaCopyIcon(MaterialTheme.colorScheme.onSurfaceVariant, 12.dp)
                    Text(
                        text = if (copied) "已复制" else "复制",
                        style = OrcaType.mono,
                        color =
                            if (copied) OrcaColors.accentGreen
                            else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        if (thinking != null) {
            ThinkingAccordion(thought = thinking, isGenerating = thinkingLive)
        }
        if (body.isNotEmpty() || thinking == null) {
            Surface(
                shape =
                    RoundedCornerShape(
                        topStart = 4.dp,
                        topEnd = 16.dp,
                        bottomStart = 16.dp,
                        bottomEnd = 16.dp,
                    ),
                color = MaterialTheme.colorScheme.surface,
                border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.8f)),
                modifier = Modifier.fillMaxWidth(),
            ) {
                SelectionContainer {
                    Box(
                        modifier =
                            Modifier.padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.md),
                    ) {
                        RenderMarkdownContent(content = readableText(body))
                    }
                }
            }
        }
        if (item.streaming && body.isEmpty()) StreamingCaret()
    }
}

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

@Composable
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

internal fun readableOutput(value: String): String {
    val lines = value.lineSequence().take(121).toList()
    return if (lines.size > 120) {
        lines.take(120).joinToString("\n") + "\n…（输出过长，已截断）"
    } else {
        lines.joinToString("\n")
    }
}

@Composable
internal fun ToolCard(item: TranscriptItem) {
    val clipboard = LocalClipboardManager.current
    var expanded by remember(item.id) { mutableStateOf(false) }
    val title = item.title ?: "工具调用"
    val output = item.output
    val command = item.command
    val hasDetail = !command.isNullOrBlank() || !output.isNullOrBlank()
    val diff =
        command?.takeIf {
            it.contains("\n+") ||
                it.startsWith("---") ||
                it.startsWith("@@") ||
                (it.trim().startsWith("{") && (it.contains("\"old_string\"") || it.contains("\"new_string\"")))
        }
    val stateColor = toolStateColor(item.state)

    Surface(
        modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.8f)),
    ) {
        Column {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .defaultMinSize(minHeight = OrcaSpace.touch)
                        .then(if (hasDetail) Modifier.clickable { expanded = !expanded } else Modifier)
                        .padding(horizontal = OrcaSpace.md, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                Box(
                    modifier =
                        Modifier
                            .width(3.dp)
                            .height(28.dp)
                            .background(stateColor, RoundedCornerShape(1.5.dp)),
                )
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Surface(
                            shape = RoundedCornerShape(4.dp),
                            color = MaterialTheme.colorScheme.surfaceVariant,
                        ) {
                            Text(
                                text = title,
                                style = OrcaType.mono.copy(fontSize = 11.sp, fontWeight = FontWeight.Bold),
                                color = MaterialTheme.colorScheme.onSurface,
                                modifier = Modifier.padding(horizontal = 5.dp, vertical = 2.dp),
                            )
                        }
                        Text(
                            text = toolStateLabel(item.state),
                            style = OrcaType.mono.copy(fontSize = 10.sp),
                            color = stateColor,
                        )
                    }
                    command?.takeIf { it.isNotBlank() }?.let {
                        Text(
                            text = readableText(it.lineSequence().first().take(120)),
                            style = OrcaType.code.copy(fontSize = 11.sp),
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                if (hasDetail) {
                    OrcaChevronDown(
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        size = 12.dp,
                        expanded = expanded,
                    )
                }
            }
            AnimatedVisibility(visible = expanded && hasDetail) {
                Column(
                    modifier =
                        Modifier.padding(
                            start = OrcaSpace.md,
                            end = OrcaSpace.md,
                            bottom = OrcaSpace.md,
                        ),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    if (diff != null) {
                        DiffPreview(diff)
                    } else if (!command.isNullOrBlank()) {
                        ToolSection(
                            label = "命令 / 参数",
                            text = readableOutput(readableText(command)),
                            onCopy = { clipboard.setText(AnnotatedString(command)) },
                        )
                    }
                    output?.takeIf { it.isNotBlank() }?.let {
                        ToolSection(
                            label = "执行输出",
                            text = readableOutput(readableText(it)),
                            onCopy = { clipboard.setText(AnnotatedString(it)) },
                        )
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
        SelectionContainer {
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
}

@Composable
internal fun SystemLine(item: TranscriptItem) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .width(2.dp)
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
internal fun ToolGroup(items: List<TranscriptItem>) {
    var expanded by remember(items.first().id) { mutableStateOf(false) }
    val running = items.any { it.state == ToolState.RUNNING }
    val failed = items.any { it.state == ToolState.FAILED }
    val color =
        when {
            failed -> OrcaColors.accentRed
            running -> OrcaColors.accentAmber
            else -> OrcaColors.textMuted
        }
    val label =
        when {
            failed -> "异常"
            running -> "执行中"
            else -> "完成"
        }
    val preview =
        items
            .mapNotNull { item ->
                item.command
                    ?.lineSequence()
                    ?.firstOrNull { it.isNotBlank() }
                    ?.trim()
                    ?.let { cmd -> (item.title?.takeIf { it.isNotBlank() } ?: "工具") to cmd }
            }.distinctBy { it.second }
            .take(2)
            .joinToString(" · ") { (title, cmd) -> "$title $cmd" }

    Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
        Surface(
            onClick = { expanded = !expanded },
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(OrcaRadius.card),
            color = OrcaColors.bgInset,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        ) {
            Row(
                modifier =
                    Modifier
                        .defaultMinSize(minHeight = OrcaSpace.touch)
                        .padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                OrcaStatusDot(color, 5.dp)
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(1.dp),
                ) {
                    Text(
                        text = "执行了 ${items.size} 项操作",
                        style = OrcaType.rowTitle,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                    )
                    if (preview.isNotBlank()) {
                        Text(
                            text = readableText(preview),
                            style = OrcaType.mono,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
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

@Composable
fun RenderMarkdownContent(content: String) {
    val cache = remember { MarkdownSegmentCache() }
    val segments = remember(content, cache) { cache.update(content) }
    Column(Modifier.fillMaxWidth()) {
        segments.forEachIndexed { index, seg ->
            if (seg.isCodeBlock) {
                key("block-$index-${seg.language}") {
                    CodeBlockSurface(code = seg.text, language = seg.language)
                }
            } else {
                MarkdownProseBlocks(
                    text = seg.text,
                    modifier = Modifier.padding(vertical = OrcaSpace.xxs),
                )
            }
        }
    }
}

internal fun readableText(value: String): String =
    value.replace("[local_path_redacted]", "（本机路径已隐藏）")
