package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.regex.Pattern

enum class MessageRole {
    USER, ASSISTANT, TOOL, SYSTEM
}

data class FormattedSegment(
    val isCodeBlock: Boolean = false,
    val language: String = "",
    val text: String = ""
)

/**
 * 结构化切分 Markdown 代码块与普通文本
 */
fun parseMarkdownSegments(content: String): List<FormattedSegment> {
    val segments = mutableListOf<FormattedSegment>()
    val codeBlockPattern = Pattern.compile("```([a-zA-Z0-9_-]*)\\r?\\n([\\s\\S]*?)```")
    val matcher = codeBlockPattern.matcher(content)
    var lastIndex = 0

    while (matcher.find()) {
        if (matcher.start() > lastIndex) {
            val textPart = content.substring(lastIndex, matcher.start())
            if (textPart.isNotEmpty()) {
                segments.add(FormattedSegment(isCodeBlock = false, text = textPart))
            }
        }
        val lang = matcher.group(1)?.trim() ?: ""
        val code = matcher.group(2) ?: ""
        segments.add(FormattedSegment(isCodeBlock = true, language = lang, text = code))
        lastIndex = matcher.end()
    }

    if (lastIndex < content.length) {
        val remaining = content.substring(lastIndex)
        if (remaining.isNotEmpty()) {
            segments.add(FormattedSegment(isCodeBlock = false, text = remaining))
        }
    }
    return segments
}

/**
 * 转换行内 **粗体** 与 `代码` 为 Compose AnnotatedString
 */
@Composable
fun buildInlineMarkdown(text: String): AnnotatedString {
    val codeBg = MaterialTheme.colorScheme.surfaceVariant
    val codeColor = OrcaColors.accentBlue

    return buildAnnotatedString {
        var i = 0
        while (i < text.length) {
            // **Bold**
            if (text.startsWith("**", i)) {
                val end = text.indexOf("**", i + 2)
                if (end != -1) {
                    val boldText = text.substring(i + 2, end)
                    pushStyle(SpanStyle(fontWeight = FontWeight.Bold))
                    append(boldText)
                    pop()
                    i = end + 2
                    continue
                }
            }
            // `Code`
            if (text.startsWith("`", i)) {
                val end = text.indexOf("`", i + 1)
                if (end != -1) {
                    val inlineCode = text.substring(i + 1, end)
                    pushStyle(
                        SpanStyle(
                            fontFamily = FontFamily.Monospace,
                            background = codeBg,
                            color = codeColor,
                            fontSize = 12.sp
                        )
                    )
                    append(" $inlineCode ")
                    pop()
                    i = end + 1
                    continue
                }
            }
            append(text[i])
            i++
        }
    }
}

/**
 * Claude / AI 风格代码块
 */
@Composable
fun CodeBlockSurface(
    code: String,
    language: String,
    modifier: Modifier = Modifier
) {
    val clipboardManager = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }

    LaunchedEffect(copied) {
        if (copied) {
            kotlinx.coroutines.delay(2000)
            copied = false
        }
    }

    Surface(
        modifier = modifier
            .fillMaxWidth()
            .padding(vertical = OrcaSpace.xs),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = OrcaColors.bgInset,
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline)
    ) {
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(MaterialTheme.colorScheme.surfaceVariant)
                    .padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.xs),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = language.ifBlank { "code" },
                    style = OrcaType.mono,
                    color = MaterialTheme.colorScheme.onSurfaceVariant
                )
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
                ) {
                    if (copied) OrcaCheckIcon(OrcaColors.accentGreen, 12.dp)
                    Text(
                        text = if (copied) "已复制" else "复制",
                        style = OrcaType.mono,
                        color = if (copied) OrcaColors.accentGreen else MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier
                            .clip(RoundedCornerShape(4.dp))
                            .clickable {
                                clipboardManager.setText(AnnotatedString(code))
                                copied = true
                            }
                            .padding(horizontal = OrcaSpace.xs, vertical = OrcaSpace.xxs)
                    )
                }
            }
            Text(
                text = code.trimEnd(),
                style = OrcaType.code,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.padding(OrcaSpace.md)
            )
        }
    }
}

/**
 * 思考折叠卡片 (Thinking Accordion)
 */
@Composable
fun ThinkingAccordion(
    thought: String,
    isGenerating: Boolean = false,
    modifier: Modifier = Modifier
) {
    var expanded by remember { mutableStateOf(false) }

    Surface(
        modifier = modifier
            .fillMaxWidth()
            .animateContentSize()
            .padding(vertical = OrcaSpace.xxs),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline)
    ) {
        Column(Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm)) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { expanded = !expanded },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs)
                ) {
                    OrcaStatusDot(
                        color = if (isGenerating) OrcaColors.accentAmber else OrcaColors.textMuted,
                        size = 5.dp,
                    )
                    Text(
                        text = if (isGenerating) "思考中…" else "思考过程",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs)
                ) {
                    OrcaChevronDown(
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        size = 10.dp,
                        expanded = expanded,
                    )
                }
            }
            AnimatedVisibility(visible = expanded) {
                Text(
                    text = thought.trim(),
                    style = OrcaType.code.copy(color = MaterialTheme.colorScheme.onSurfaceVariant),
                    modifier = Modifier.padding(top = OrcaSpace.sm)
                )
            }
        }
    }
}
