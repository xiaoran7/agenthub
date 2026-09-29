package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.ui.text.TextStyle
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
fun buildInlineMarkdown(text: String, isDark: Boolean): AnnotatedString {
    val codeBg = if (isDark) Color(0xFF2C2D32) else Color(0xFFE9E8E3)
    val codeColor = if (isDark) Color(0xFF93C5FD) else Color(0xFF1D4ED8)

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
                            fontSize = 13.sp
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
 * Claude / Anthropic 风格代码块
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
            .padding(vertical = 5.dp),
        shape = RoundedCornerShape(8.dp),
        color = Color(0xFF181716),
        border = androidx.compose.foundation.BorderStroke(0.5.dp, Color(0xFF383632))
    ) {
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .background(Color(0xFF201F1D))
                    .padding(horizontal = 12.dp, vertical = 6.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Text(
                    text = language.ifBlank { "code" },
                    style = MaterialTheme.typography.labelSmall,
                    color = Color(0xFFA8A69E)
                )
                Text(
                    text = if (copied) "已复制" else "复制",
                    style = MaterialTheme.typography.labelSmall,
                    color = if (copied) StatusGreenBright else Color(0xFFD6D4CC),
                    fontWeight = FontWeight.Medium,
                    modifier = Modifier
                        .clip(RoundedCornerShape(4.dp))
                        .clickable {
                            clipboardManager.setText(AnnotatedString(code))
                            copied = true
                        }
                        .padding(horizontal = 6.dp, vertical = 2.dp)
                )
            }
            Text(
                text = code.trimEnd(),
                style = TextStyle(
                    fontFamily = FontFamily.Monospace,
                    fontSize = 12.5.sp,
                    lineHeight = 18.sp,
                    color = Color(0xFFF5F4EF)
                ),
                modifier = Modifier.padding(12.dp)
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
            .padding(vertical = 3.dp),
        shape = RoundedCornerShape(8.dp),
        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
        border = androidx.compose.foundation.BorderStroke(0.5.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.4f))
    ) {
        Column(Modifier.padding(horizontal = 10.dp, vertical = 7.dp)) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { expanded = !expanded },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Box(
                        Modifier
                            .size(6.dp)
                            .background(
                                if (isGenerating) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
                                CircleShape
                            )
                    )
                    Text(
                        text = if (isGenerating) "思考中…" else "思考过程 (${thought.length} 字)",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        fontWeight = FontWeight.Medium
                    )
                }
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp)
                ) {
                    Text(
                        text = if (expanded) "收起" else "展开",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                    ChevronDownIcon(expanded = expanded, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            AnimatedVisibility(visible = expanded) {
                Text(
                    text = thought.trim(),
                    style = MaterialTheme.typography.bodySmall.copy(
                        fontFamily = FontFamily.Monospace,
                        fontSize = 11.5.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    ),
                    modifier = Modifier.padding(top = 8.dp)
                )
            }
        }
    }
}

@Composable
private fun ChevronDownIcon(expanded: Boolean, tint: Color) {
    Canvas(modifier = Modifier.size(10.dp)) {
        val stroke = 1.4.dp.toPx()
        val midY = size.height * 0.5f
        if (expanded) {
            // Chevron up
            drawLine(
                color = tint,
                start = Offset(size.width * 0.15f, midY + size.height * 0.2f),
                end = Offset(size.width * 0.5f, midY - size.height * 0.2f),
                strokeWidth = stroke,
                cap = StrokeCap.Round
            )
            drawLine(
                color = tint,
                start = Offset(size.width * 0.5f, midY - size.height * 0.2f),
                end = Offset(size.width * 0.85f, midY + size.height * 0.2f),
                strokeWidth = stroke,
                cap = StrokeCap.Round
            )
        } else {
            // Chevron down
            drawLine(
                color = tint,
                start = Offset(size.width * 0.15f, midY - size.height * 0.2f),
                end = Offset(size.width * 0.5f, midY + size.height * 0.2f),
                strokeWidth = stroke,
                cap = StrokeCap.Round
            )
            drawLine(
                color = tint,
                start = Offset(size.width * 0.5f, midY + size.height * 0.2f),
                end = Offset(size.width * 0.85f, midY - size.height * 0.2f),
                strokeWidth = stroke,
                cap = StrokeCap.Round
            )
        }
    }
}
