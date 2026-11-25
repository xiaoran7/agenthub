package com.agentplatform.mobile.ui

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
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
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
                        .background(OrcaColors.accentGreen.copy(alpha = dotAlpha)),
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
        if (isLight) OrcaColors.bgRaised
        else OrcaColors.accentBlue.copy(alpha = 0.14f)
    val bubbleText = OrcaColors.textPrimary
    val bubbleBorder =
        when {
            sending -> OrcaColors.accentAmber
            isLight -> OrcaColors.accentBlue.copy(alpha = 0.16f)
            else -> OrcaColors.accentBlue.copy(alpha = 0.28f)
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
                    topStart = 20.dp,
                    topEnd = 20.dp,
                    bottomStart = 20.dp,
                    bottomEnd = 6.dp,
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
internal fun AssistantMessage(item: TranscriptItem) {
    if (isRedactedPathOnly(item.text)) return
    val clipboard = LocalClipboardManager.current
    var copied by remember(item.id) { mutableStateOf(false) }
    val (thinking, body, thinkingLive) =
        remember(item.text, item.streaming) { splitThinking(item.text, item.streaming) }

    LaunchedEffect(copied) {
        if (copied) {
            delay(1800)
            copied = false
        }
    }

    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
    ) {
        if (thinking != null) {
            ThinkingAccordion(thought = thinking, isGenerating = thinkingLive)
        }
        if (body.isNotBlank()) {
            SelectionContainer {
                RenderMarkdownContent(content = readableText(body))
            }
            if (!item.streaming) {
                IconButton(
                    onClick = {
                        clipboard.setText(AnnotatedString(body))
                        copied = true
                    },
                    modifier = Modifier.semantics {
                        contentDescription = if (copied) "已复制回复" else "复制回复"
                    },
                ) {
                    if (copied) OrcaCheckIcon(OrcaColors.accentGreen, 16.dp)
                    else OrcaCopyIcon(MaterialTheme.colorScheme.onSurfaceVariant, 16.dp)
                }
            }
        }
        if (item.streaming && body.isEmpty() && !thinkingLive) StreamingCaret()
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
                    proseStyle = MaterialTheme.typography.bodyLarge,
                )
            }
        }
    }
}

internal fun readableText(value: String): String =
    value.replace("[local_path_redacted]", "（本机路径已隐藏）")
