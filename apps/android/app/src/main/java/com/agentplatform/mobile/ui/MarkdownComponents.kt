package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.util.regex.Pattern
import kotlinx.coroutines.delay

enum class MessageRole {
    USER,
    ASSISTANT,
    TOOL,
    SYSTEM,
}

data class FormattedSegment(
    val isCodeBlock: Boolean = false,
    val language: String = "",
    val text: String = "",
    val unclosed: Boolean = false,
)

private val CLOSED_FENCE_PATTERN: Pattern =
    Pattern.compile("```([a-zA-Z0-9_-]*)\\r?\\n([\\s\\S]*?)```")
private val UNCLOSED_FENCE_REGEX =
    Regex("```([a-zA-Z0-9_-]*)\\r?\\n([\\s\\S]*)$")
private val TABLE_SEPARATOR_REGEX =
    Regex("^\\|?(?:\\s*:?-{3,}:?\\s*\\|)+\\s*:?-{3,}:?\\s*\\|?$")

/**
 * Splits Markdown into closed code blocks, trailing streaming (unclosed) code
 * fences, and prose segments.
 */
fun parseMarkdownSegments(content: String): List<FormattedSegment> {
    if (!content.contains("```")) {
        return if (content.isEmpty()) emptyList() else listOf(FormattedSegment(text = content))
    }
    val segments = mutableListOf<FormattedSegment>()
    val matcher = CLOSED_FENCE_PATTERN.matcher(content)
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
        segments.add(FormattedSegment(isCodeBlock = true, language = lang, text = code, unclosed = false))
        lastIndex = matcher.end()
    }

    if (lastIndex < content.length) {
        val remaining = content.substring(lastIndex)
        if (remaining.isNotEmpty()) {
            val openMatch = UNCLOSED_FENCE_REGEX.find(remaining)
            if (openMatch != null) {
                val before = remaining.substring(0, openMatch.range.first)
                if (before.isNotEmpty()) {
                    segments.add(FormattedSegment(isCodeBlock = false, text = before))
                }
                segments.add(
                    FormattedSegment(
                        isCodeBlock = true,
                        language = openMatch.groupValues[1].trim(),
                        text = openMatch.groupValues[2],
                        unclosed = true,
                    ),
                )
            } else {
                segments.add(FormattedSegment(isCodeBlock = false, text = remaining))
            }
        }
    }
    return segments
}

class MarkdownSegmentCache {
    private var source: String = ""
    private var segments: List<FormattedSegment> = emptyList()

    fun update(content: String): List<FormattedSegment> {
        if (content == source) return segments
        val appended =
            if (source.isNotEmpty() && content.startsWith(source)) content.substring(source.length)
            else null
        val fenceMayBeSplit =
            appended != null &&
                (
                    (source.endsWith("`") && !source.endsWith("```")) ||
                        appended.startsWith("`")
                    )
        segments =
            if (appended != null && !fenceMayBeSplit && !appended.contains("```")) {
                val last = segments.lastOrNull()
                when {
                    last == null -> listOf(FormattedSegment(text = appended))
                    last.isCodeBlock && !last.unclosed ->
                        segments + FormattedSegment(text = appended)
                    else ->
                        segments.dropLast(1) + last.copy(text = last.text + appended)
                }
            } else {
                parseMarkdownSegments(content)
            }
        source = content
        return segments
    }
}

@Composable
fun buildInlineMarkdown(text: String): AnnotatedString {
    val codeBg = MaterialTheme.colorScheme.surfaceVariant
    val codeColor = OrcaColors.accentBlue
    val linkColor = OrcaColors.accentBlue

    return remember(text, codeBg, codeColor, linkColor) {
        if (!text.contains("**") && !text.contains('`') && !text.contains('[')) {
            return@remember AnnotatedString(text)
        }
        buildAnnotatedString {
            var i = 0
            while (i < text.length) {
                if (text.startsWith("**", i)) {
                    val end = text.indexOf("**", i + 2)
                    if (end != -1) {
                        pushStyle(SpanStyle(fontWeight = FontWeight.Bold))
                        append(text.substring(i + 2, end))
                        pop()
                        i = end + 2
                        continue
                    }
                }
                if (text.startsWith("`", i)) {
                    val end = text.indexOf("`", i + 1)
                    if (end != -1) {
                        pushStyle(
                            SpanStyle(
                                fontFamily = FontFamily.Monospace,
                                background = codeBg,
                                color = codeColor,
                                fontSize = 12.sp,
                            ),
                        )
                        append(" ${text.substring(i + 1, end)} ")
                        pop()
                        i = end + 1
                        continue
                    }
                }
                if (text[i] == '[') {
                    val closeBracket = text.indexOf("](", i + 1)
                    if (closeBracket != -1) {
                        val closeParen = text.indexOf(')', closeBracket + 2)
                        if (closeParen != -1) {
                            val label = text.substring(i + 1, closeBracket)
                            pushStyle(
                                SpanStyle(
                                    color = linkColor,
                                    textDecoration = TextDecoration.Underline,
                                    fontWeight = FontWeight.Medium,
                                ),
                            )
                            append(label)
                            pop()
                            i = closeParen + 1
                            continue
                        }
                    }
                }
                append(text[i])
                i++
            }
        }
    }
}

/** Renders prose blocks including headings, blockquotes, lists, and horizontally scrollable tables. */
@Composable
fun MarkdownProseBlocks(text: String, modifier: Modifier = Modifier) {
    val lines = remember(text) { text.lines() }
    Column(modifier = modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        var idx = 0
        while (idx < lines.size) {
            val line = lines[idx]
            val trimmed = line.trim()
            if (trimmed.isEmpty()) {
                idx++
                continue
            }
            // GFM Table detection: header row + separator row (`| --- | --- |`)
            if (
                trimmed.contains('|') &&
                idx + 1 < lines.size &&
                TABLE_SEPARATOR_REGEX.matches(lines[idx + 1].trim())
            ) {
                val headers = splitTableCells(trimmed)
                val rows = mutableListOf<List<String>>()
                idx += 2
                while (idx < lines.size && lines[idx].trim().contains('|')) {
                    rows.add(splitTableCells(lines[idx].trim()))
                    idx++
                }
                MarkdownTableSurface(headers = headers, rows = rows)
                continue
            }
            if (trimmed.startsWith("### ")) {
                Text(
                    text = buildInlineMarkdown(trimmed.removePrefix("### ").trim()),
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(top = 4.dp),
                )
                idx++
                continue
            }
            if (trimmed.startsWith("## ") || trimmed.startsWith("# ")) {
                val heading = trimmed.replaceFirst(Regex("^#{1,2}\\s+"), "")
                Text(
                    text = buildInlineMarkdown(heading),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(top = 6.dp),
                )
                idx++
                continue
            }
            if (trimmed.startsWith("> ")) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 2.dp),
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    Box(
                        Modifier
                            .width(3.dp)
                            .height(20.dp)
                            .background(OrcaColors.borderStrong, RoundedCornerShape(1.5.dp)),
                    )
                    Text(
                        text = buildInlineMarkdown(trimmed.removePrefix("> ").trim()),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.weight(1f),
                    )
                }
                idx++
                continue
            }
            // Coalesce consecutive plain/list lines into one paragraph block
            val buf = StringBuilder(line)
            idx++
            while (idx < lines.size) {
                val next = lines[idx]
                val nextTrim = next.trim()
                if (
                    nextTrim.isEmpty() ||
                    nextTrim.startsWith("#") ||
                    nextTrim.startsWith("> ") ||
                    (
                        nextTrim.contains('|') &&
                            idx + 1 < lines.size &&
                            TABLE_SEPARATOR_REGEX.matches(lines[idx + 1].trim())
                        )
                ) {
                    break
                }
                buf.append('\n').append(next)
                idx++
            }
            Text(
                text = buildInlineMarkdown(buf.toString()),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

private fun splitTableCells(row: String): List<String> =
    row.trim().removePrefix("|").removeSuffix("|").split('|').map { it.trim() }

@Composable
private fun MarkdownTableSurface(
    headers: List<String>,
    rows: List<List<String>>,
) {
    val scroll = rememberScrollState()
    Surface(
        modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.xs),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = OrcaColors.bgInset,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column(modifier = Modifier.horizontalScroll(scroll)) {
            Row(
                modifier =
                    Modifier
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(horizontal = OrcaSpace.sm, vertical = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(OrcaSpace.md),
            ) {
                headers.forEach { cell ->
                    Text(
                        text = cell,
                        style = MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.Bold),
                        color = MaterialTheme.colorScheme.onSurface,
                        modifier = Modifier.widthIn(min = 84.dp, max = 220.dp),
                    )
                }
            }
            rows.forEach { row ->
                Box(Modifier.fillMaxWidth().height(1.dp).background(MaterialTheme.colorScheme.outline))
                Row(
                    modifier = Modifier.padding(horizontal = OrcaSpace.sm, vertical = 6.dp),
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.md),
                ) {
                    headers.indices.forEach { colIdx ->
                        Text(
                            text = row.getOrNull(colIdx).orEmpty(),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurface,
                            modifier = Modifier.widthIn(min = 84.dp, max = 220.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
fun CodeBlockSurface(
    code: String,
    language: String,
    modifier: Modifier = Modifier,
) {
    val clipboardManager = LocalClipboardManager.current
    var copied by remember { mutableStateOf(false) }
    val scroll = rememberScrollState()

    LaunchedEffect(copied) {
        if (copied) {
            delay(2000)
            copied = false
        }
    }

    Surface(
        modifier = modifier.fillMaxWidth().padding(vertical = OrcaSpace.xs),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = OrcaColors.bgInset,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        .padding(start = OrcaSpace.md, end = OrcaSpace.xs, top = 2.dp, bottom = 2.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = language.ifBlank { "code" },
                    style = OrcaType.mono,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Row(
                    modifier =
                        Modifier
                            .minimumInteractiveComponentSize()
                            .defaultMinSize(minHeight = 36.dp)
                            .clip(RoundedCornerShape(OrcaRadius.button))
                            .clickable {
                                clipboardManager.setText(AnnotatedString(code))
                                copied = true
                            }
                            .padding(horizontal = OrcaSpace.sm, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
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
            Box(modifier = Modifier.fillMaxWidth().horizontalScroll(scroll).padding(OrcaSpace.md)) {
                Text(
                    text = code.trimEnd(),
                    style = OrcaType.code,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }
        }
    }
}

@Composable
fun ThinkingAccordion(
    thought: String,
    isGenerating: Boolean = false,
    modifier: Modifier = Modifier,
) {
    val clipboard = LocalClipboardManager.current
    var userToggled by remember { mutableStateOf<Boolean?>(null) }
    var copied by remember { mutableStateOf(false) }
    val expanded = userToggled ?: isGenerating

    LaunchedEffect(copied) {
        if (copied) {
            delay(1800)
            copied = false
        }
    }

    Surface(
        modifier =
            modifier
                .fillMaxWidth()
                .animateContentSize()
                .padding(vertical = OrcaSpace.xxs),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.4f),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column(Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.xs)) {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .defaultMinSize(minHeight = 38.dp)
                        .clickable { userToggled = !expanded },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
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
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    if (expanded && thought.isNotBlank()) {
                        Text(
                            text = if (copied) "已复制" else "复制思考",
                            style = OrcaType.mono,
                            color =
                                if (copied) OrcaColors.accentGreen
                                else MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier =
                                Modifier
                                    .clip(RoundedCornerShape(6.dp))
                                    .clickable {
                                        val quoted =
                                            thought.trim().lines().joinToString("\n") { "> $it" }
                                        clipboard.setText(AnnotatedString(quoted))
                                        copied = true
                                    }
                                    .padding(horizontal = 6.dp, vertical = 4.dp),
                        )
                    }
                    OrcaChevronDown(
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        size = 12.dp,
                        expanded = expanded,
                    )
                }
            }
            AnimatedVisibility(visible = expanded) {
                Text(
                    text = thought.trim(),
                    style = OrcaType.code.copy(color = MaterialTheme.colorScheme.onSurfaceVariant),
                    modifier = Modifier.padding(top = OrcaSpace.xs, bottom = OrcaSpace.xs),
                )
            }
        }
    }
}
