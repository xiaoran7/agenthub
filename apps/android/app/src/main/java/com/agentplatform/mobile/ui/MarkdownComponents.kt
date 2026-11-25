package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay

private val TABLE_SEPARATOR_REGEX =
    Regex("^\\|?(?:\\s*:?-{3,}:?\\s*\\|)+\\s*:?-{3,}:?\\s*\\|?$")
private val HEADING_PREFIX_REGEX = Regex("^#{1,4}\\s+")

@Composable
fun buildInlineMarkdown(text: String): AnnotatedString {
    val codeBg = MaterialTheme.colorScheme.surfaceVariant
    val codeColor = OrcaColors.accentBlue
    val linkColor = OrcaColors.accentBlue
    return remember(text, codeBg, codeColor, linkColor) {
        parseInlineMarkdown(text, codeBg, codeColor, linkColor)
    }
}

/** Renders prose blocks including headings, blockquotes, lists, and horizontally scrollable tables. */
@Composable
fun MarkdownProseBlocks(
    text: String,
    modifier: Modifier = Modifier,
    proseStyle: TextStyle = MaterialTheme.typography.bodyMedium,
) {
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
            if (trimmed.startsWith("#### ") || trimmed.startsWith("### ")) {
                Text(
                    text = buildInlineMarkdown(trimmed.replaceFirst(HEADING_PREFIX_REGEX, "").trim()),
                    style = MaterialTheme.typography.titleSmall.copy(fontWeight = FontWeight.SemiBold),
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(top = 4.dp),
                )
                idx++
                continue
            }
            if (trimmed.startsWith("## ") || trimmed.startsWith("# ")) {
                val heading = trimmed.replaceFirst(HEADING_PREFIX_REGEX, "")
                Text(
                    text = buildInlineMarkdown(heading),
                    style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.SemiBold),
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
                        style = proseStyle,
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
                style = proseStyle,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }
}

@Composable
private fun MarkdownTableSurface(
    headers: List<String>,
    rows: List<List<String>>,
) {
    val colCount = maxOf(headers.size, rows.maxOfOrNull { it.size } ?: 0)
    if (colCount == 0) return
    val scroll = rememberScrollState()
    val colors = MaterialTheme.colorScheme
    val density = LocalDensity.current
    val minColPx = with(density) { 68.dp.roundToPx() }
    val maxColPx = with(density) { 232.dp.roundToPx() }
    val dividerPx = with(density) { 1.dp.roundToPx().coerceAtLeast(1) }
    val allRows =
        remember(headers, rows, colCount) {
            listOf(headers) + rows
        }

    Surface(
        modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.xs),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = OrcaColors.bgInset,
        border = BorderStroke(1.dp, colors.outline),
    ) {
        Box(modifier = Modifier.fillMaxWidth().horizontalScroll(scroll)) {
            Layout(
                content = {
                    allRows.forEachIndexed { rowIdx, _ ->
                        val bg =
                            when {
                                rowIdx == 0 -> colors.surfaceVariant
                                rowIdx % 2 == 0 -> colors.surfaceVariant.copy(alpha = 0.28f)
                                else -> Color.Transparent
                            }
                        Box(Modifier.background(bg))
                    }
                    for (r in 1 until allRows.size) {
                        Box(Modifier.background(colors.outline.copy(alpha = 0.7f)))
                    }
                    allRows.forEachIndexed { rowIdx, row ->
                        val isHeader = rowIdx == 0
                        for (colIdx in 0 until colCount) {
                            val rawCell = row.getOrNull(colIdx).orEmpty()
                            Box(
                                modifier = Modifier.padding(horizontal = 12.dp, vertical = if (isHeader) 9.dp else 8.dp),
                                contentAlignment = Alignment.CenterStart,
                            ) {
                                Text(
                                    text = buildInlineMarkdown(rawCell),
                                    style =
                                        if (isHeader) {
                                            MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.SemiBold)
                                        } else {
                                            MaterialTheme.typography.bodySmall.copy(lineHeight = 19.sp)
                                        },
                                    color = colors.onSurface,
                                )
                            }
                        }
                    }
                },
            ) { measurables, constraints ->
                val rowCount = allRows.size
                val dividerCount = (rowCount - 1).coerceAtLeast(0)
                val bgMeasurables = measurables.subList(0, rowCount)
                val dividerMeasurables = measurables.subList(rowCount, rowCount + dividerCount)
                val cellMeasurables = measurables.subList(rowCount + dividerCount, measurables.size)

                val colWidths = IntArray(colCount) { minColPx }
                val firstPass =
                    cellMeasurables.mapIndexed { idx, m ->
                        val col = idx % colCount
                        val p = m.measure(Constraints(minWidth = 0, maxWidth = maxColPx))
                        if (p.width > colWidths[col]) colWidths[col] = p.width
                        p
                    }
                val totalWidth = maxOf(colWidths.sum(), constraints.minWidth)
                val extra = totalWidth - colWidths.sum()
                if (extra > 0 && colCount > 0) {
                    colWidths[colCount - 1] += extra
                }

                val rowHeights = IntArray(rowCount)
                firstPass.forEachIndexed { idx, p ->
                    val r = idx / colCount
                    if (p.height > rowHeights[r]) rowHeights[r] = p.height
                }

                val bgPlaceables =
                    bgMeasurables.mapIndexed { r, m ->
                        m.measure(Constraints.fixed(totalWidth, rowHeights[r]))
                    }
                val dividerPlaceables =
                    dividerMeasurables.map { m ->
                        m.measure(Constraints.fixed(totalWidth, dividerPx))
                    }
                val totalHeight = rowHeights.sum() + dividerCount * dividerPx

                layout(totalWidth, totalHeight) {
                    var y = 0
                    for (r in 0 until rowCount) {
                        if (r > 0) {
                            dividerPlaceables[r - 1].placeRelative(0, y)
                            y += dividerPx
                        }
                        bgPlaceables[r].placeRelative(0, y)
                        var x = 0
                        for (c in 0 until colCount) {
                            val cell = firstPass[r * colCount + c]
                            val cy = y + ((rowHeights[r] - cell.height) / 2).coerceAtLeast(0)
                            cell.placeRelative(x, cy)
                            x += colWidths[c]
                        }
                        y += rowHeights[r]
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
    modifier: Modifier = Modifier,
    isGenerating: Boolean = false,
) {
    val clipboard = LocalClipboardManager.current
    var userToggled by remember { mutableStateOf<Boolean?>(null) }
    var copied by remember { mutableStateOf(false) }
    val expanded = userToggled ?: isGenerating
    val chevronRotation by animateFloatAsState(
        targetValue = if (expanded) 180f else 0f,
        animationSpec = tween(MotionTokens.Quick),
        label = "thinking-chevron",
    )

    LaunchedEffect(copied) {
        if (copied) {
            delay(1800)
            copied = false
        }
    }

    Surface(
        modifier = modifier.fillMaxWidth().padding(vertical = OrcaSpace.xxs),
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
                    Box(Modifier.graphicsLayer { rotationZ = chevronRotation }) {
                        OrcaChevronDown(
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                            size = 12.dp,
                            expanded = false,
                        )
                    }
                }
            }
            AnimatedVisibility(
                visible = expanded,
                enter =
                    expandVertically(
                        animationSpec = tween(MotionTokens.Standard),
                        expandFrom = Alignment.Top,
                    ) + fadeIn(tween(MotionTokens.Standard)),
                exit =
                    shrinkVertically(
                        animationSpec = tween(MotionTokens.Quick),
                        shrinkTowards = Alignment.Top,
                    ) + fadeOut(tween(MotionTokens.Quick)),
            ) {
                Text(
                    text = thought.trim(),
                    style = OrcaType.code.copy(color = MaterialTheme.colorScheme.onSurfaceVariant),
                    modifier = Modifier.padding(top = OrcaSpace.xs, bottom = OrcaSpace.xs),
                )
            }
        }
    }
}
