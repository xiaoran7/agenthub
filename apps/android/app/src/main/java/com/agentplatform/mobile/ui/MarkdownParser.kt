package com.agentplatform.mobile.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.sp
import java.util.regex.Pattern

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
                        appended.startsWith("`") ||
                        source.substringAfterLast('\n').contains("```")
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

/**
 * Pure inline Markdown parser supporting nested formatting (e.g. bold wrapping
 * inline code `` **`#6004`（你）** ``, italic, strikethrough, multi-backtick code,
 * and links). Unit-testable without a Compose runtime.
 */
internal fun parseInlineMarkdown(
    text: String,
    codeBg: Color,
    codeColor: Color,
    linkColor: Color,
): AnnotatedString {
    if (
        !text.contains('*') &&
        !text.contains('`') &&
        !text.contains('[') &&
        !text.contains("~~") &&
        !text.contains("__") &&
        !text.contains('\\')
    ) {
        return AnnotatedString(text)
    }
    return buildAnnotatedString {
        appendInlineMarkdown(
            text = text,
            start = 0,
            end = text.length,
            codeBg = codeBg,
            codeColor = codeColor,
            linkColor = linkColor,
            depth = 0,
        )
    }
}

private fun AnnotatedString.Builder.appendInlineMarkdown(
    text: String,
    start: Int,
    end: Int,
    codeBg: Color,
    codeColor: Color,
    linkColor: Color,
    depth: Int,
) {
    if (depth > 8) {
        append(text.substring(start, end))
        return
    }
    var i = start
    while (i < end) {
        val ch = text[i]

        // 1. Escaped markdown punctuation (`\*`, `` \` ``, `\|`, `\[`)
        if (ch == '\\' && i + 1 < end && text[i + 1] in "\\`*_{}[]()#+-.!|~") {
            append(text[i + 1])
            i += 2
            continue
        }

        // 2. Inline code span (`code` or ``code with ` tick``) — never parsed recursively
        if (ch == '`') {
            var ticks = 1
            while (i + ticks < end && text[i + ticks] == '`') ticks++
            val closeIdx = findClosingBackticks(text, i + ticks, end, ticks)
            if (closeIdx != -1) {
                var inner = text.substring(i + ticks, closeIdx)
                if (inner.length >= 2 && inner.startsWith(' ') && inner.endsWith(' ') && inner.any { it != ' ' }) {
                    inner = inner.substring(1, inner.length - 1)
                }
                pushStyle(
                    SpanStyle(
                        fontFamily = FontFamily.Monospace,
                        background = codeBg,
                        color = codeColor,
                        fontSize = 12.5.sp,
                    ),
                )
                append("\u2009$inner\u2009")
                pop()
                i = closeIdx + ticks
                continue
            }
        }

        // 3. Bold (**...** or __...__) with recursive inner parsing
        if ((ch == '*' && i + 1 < end && text[i + 1] == '*') ||
            (ch == '_' && i + 1 < end && text[i + 1] == '_')
        ) {
            val delim = if (ch == '*') "**" else "__"
            val closeIdx = findClosingDelimiter(text, i + 2, end, delim)
            if (closeIdx != -1 && closeIdx > i + 2) {
                pushStyle(SpanStyle(fontWeight = FontWeight.SemiBold))
                appendInlineMarkdown(text, i + 2, closeIdx, codeBg, codeColor, linkColor, depth + 1)
                pop()
                i = closeIdx + 2
                continue
            }
        }

        // 4. Strikethrough (~~...~~) with recursive inner parsing
        if (ch == '~' && i + 1 < end && text[i + 1] == '~') {
            val closeIdx = findClosingDelimiter(text, i + 2, end, "~~")
            if (closeIdx != -1 && closeIdx > i + 2) {
                pushStyle(SpanStyle(textDecoration = TextDecoration.LineThrough))
                appendInlineMarkdown(text, i + 2, closeIdx, codeBg, codeColor, linkColor, depth + 1)
                pop()
                i = closeIdx + 2
                continue
            }
        }

        // 5. Italic (*...*) — avoid matching bullet/math spaces
        if (ch == '*' && i + 1 < end && !text[i + 1].isWhitespace() && text[i + 1] != '*') {
            val closeIdx = findClosingSingleStar(text, i + 1, end)
            if (closeIdx != -1 && closeIdx > i + 1) {
                pushStyle(SpanStyle(fontStyle = FontStyle.Italic))
                appendInlineMarkdown(text, i + 1, closeIdx, codeBg, codeColor, linkColor, depth + 1)
                pop()
                i = closeIdx + 1
                continue
            }
        }

        // 6. Markdown link [label](url) with recursive inner parsing on label
        if (ch == '[') {
            val closeBracket = findClosingLinkBracket(text, i + 1, end)
            if (closeBracket != -1 && closeBracket + 1 < end && text[closeBracket + 1] == '(') {
                val closeParen = findClosingParen(text, closeBracket + 2, end)
                if (closeParen != -1) {
                    pushStyle(
                        SpanStyle(
                            color = linkColor,
                            textDecoration = TextDecoration.Underline,
                            fontWeight = FontWeight.Medium,
                        ),
                    )
                    appendInlineMarkdown(text, i + 1, closeBracket, codeBg, codeColor, linkColor, depth + 1)
                    pop()
                    i = closeParen + 1
                    continue
                }
            }
        }

        append(ch)
        i++
    }
}

private fun findClosingBackticks(text: String, from: Int, end: Int, count: Int): Int {
    var i = from
    while (i < end) {
        if (text[i] == '`') {
            var run = 1
            while (i + run < end && text[i + run] == '`') run++
            if (run == count) return i
            i += run
            continue
        }
        i++
    }
    return -1
}

private fun findClosingDelimiter(text: String, from: Int, end: Int, delim: String): Int {
    var i = from
    while (i <= end - delim.length) {
        if (text[i] == '\\') {
            i += 2
            continue
        }
        if (text[i] == '`') {
            var ticks = 1
            while (i + ticks < end && text[i + ticks] == '`') ticks++
            val closeCode = findClosingBackticks(text, i + ticks, end, ticks)
            if (closeCode != -1) {
                i = closeCode + ticks
                continue
            }
        }
        if (text.regionMatches(i, delim, 0, delim.length)) {
            return i
        }
        i++
    }
    return -1
}

private fun findClosingSingleStar(text: String, from: Int, end: Int): Int {
    var i = from
    while (i < end) {
        if (text[i] == '\\') {
            i += 2
            continue
        }
        if (text[i] == '`') {
            var ticks = 1
            while (i + ticks < end && text[i + ticks] == '`') ticks++
            val closeCode = findClosingBackticks(text, i + ticks, end, ticks)
            if (closeCode != -1) {
                i = closeCode + ticks
                continue
            }
        }
        if (text[i] == '*' && (i + 1 >= end || text[i + 1] != '*') && !text[i - 1].isWhitespace()) {
            return i
        }
        i++
    }
    return -1
}

private fun findClosingLinkBracket(text: String, from: Int, end: Int): Int {
    var i = from
    var nested = 0
    while (i < end) {
        if (text[i] == '\\') {
            i += 2
            continue
        }
        if (text[i] == '`') {
            var ticks = 1
            while (i + ticks < end && text[i + ticks] == '`') ticks++
            val closeCode = findClosingBackticks(text, i + ticks, end, ticks)
            if (closeCode != -1) {
                i = closeCode + ticks
                continue
            }
        }
        if (text[i] == '[') nested++
        else if (text[i] == ']') {
            if (nested == 0) return i
            nested--
        }
        i++
    }
    return -1
}

private fun findClosingParen(text: String, from: Int, end: Int): Int {
    var i = from
    var nested = 0
    while (i < end) {
        if (text[i] == '\\') {
            i += 2
            continue
        }
        if (text[i] == '(') nested++
        else if (text[i] == ')') {
            if (nested == 0) return i
            nested--
        }
        i++
    }
    return -1
}

/**
 * Splits a Markdown table row into cells while preserving escaped pipes (`\|`)
 * and pipes inside inline backtick code spans (`` `a | b` ``).
 */
internal fun splitTableCells(row: String): List<String> {
    var s = row.trim()
    if (s.startsWith('|')) s = s.substring(1)
    if (s.endsWith('|') && !s.endsWith("\\|")) s = s.substring(0, s.length - 1)
    val cells = mutableListOf<String>()
    val buf = StringBuilder()
    var i = 0
    var codeTicks = 0
    while (i < s.length) {
        val ch = s[i]
        if (ch == '\\' && i + 1 < s.length && s[i + 1] == '|') {
            buf.append('|')
            i += 2
            continue
        }
        if (ch == '`') {
            var run = 1
            while (i + run < s.length && s[i + run] == '`') run++
            if (codeTicks == 0) codeTicks = run
            else if (codeTicks == run) codeTicks = 0
            repeat(run) { buf.append('`') }
            i += run
            continue
        }
        if (ch == '|' && codeTicks == 0) {
            cells.add(buf.toString().trim())
            buf.setLength(0)
            i++
            continue
        }
        buf.append(ch)
        i++
    }
    cells.add(buf.toString().trim())
    return cells
}
