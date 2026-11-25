package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
internal fun ToolActivitySummary(items: List<TranscriptItem>, onOpen: () -> Unit) {
    val running = items.count { it.state == ToolState.RUNNING }
    val failed = items.count { it.state == ToolState.FAILED }
    val summary = remember(items) { summarizeToolGroup(items) }
    val dotColor =
        when {
            failed > 0 -> OrcaColors.accentRed
            running > 0 -> OrcaColors.accentAmber
            else -> OrcaColors.accentGreen
        }
    val summaryColor = MaterialTheme.colorScheme.onSurfaceVariant
    Surface(
        onClick = onOpen,
        shape = RoundedCornerShape(14.dp),
        color = OrcaColors.bgRaised.copy(alpha = 0.55f),
        border = BorderStroke(1.dp, OrcaColors.borderSubtle),
    ) {
        Row(
            modifier =
                Modifier
                    .heightIn(min = 42.dp)
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OrcaStatusDot(dotColor, 6.dp)
            Text(
                text = summary,
                modifier = Modifier.weight(1f, fill = false),
                style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            OrcaChevronRight(summaryColor, 13.dp)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ToolActivitySheet(items: List<TranscriptItem>, onDismiss: () -> Unit) {
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    val colors = MaterialTheme.colorScheme
    val completedCount = items.count { it.state == ToolState.COMPLETED }
    val runningCount = items.count { it.state == ToolState.RUNNING }
    val failedCount = items.count { it.state == ToolState.FAILED }
    val subtitle =
        buildString {
            append("共 ${items.size} 项")
            if (completedCount == items.size && items.isNotEmpty()) {
                append(" · 全部完成")
            } else {
                if (runningCount > 0) append(" · $runningCount 执行中")
                if (failedCount > 0) append(" · $failedCount 失败")
                if (completedCount > 0) append(" · $completedCount 完成")
            }
        }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        containerColor = colors.surface,
        shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
    ) {
        Column(modifier = Modifier.fillMaxWidth().fillMaxHeight(0.80f).navigationBarsPadding()) {
            Box(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = OrcaSpace.lg, vertical = OrcaSpace.xs)
                        .heightIn(min = 52.dp),
            ) {
                Column(
                    modifier = Modifier.align(Alignment.CenterStart).padding(end = 56.dp),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    Text(
                        text = "工具记录",
                        modifier = Modifier.semantics { heading() },
                        style = MaterialTheme.typography.titleLarge.copy(fontWeight = FontWeight.SemiBold),
                        color = colors.onSurface,
                    )
                    Text(
                        text = subtitle,
                        style = MaterialTheme.typography.bodySmall,
                        color = colors.onSurfaceVariant,
                    )
                }
                ChatRoundAction(
                    label = "关闭工具记录",
                    onClick = {
                        scope.launch {
                            runCatching { sheetState.hide() }
                            onDismiss()
                        }
                    },
                    modifier = Modifier.align(Alignment.CenterEnd),
                ) {
                    Canvas(Modifier.size(18.dp)) {
                        val inset = 4.dp.toPx()
                        val width = 1.7.dp.toPx()
                        drawLine(
                            colors.onSurfaceVariant,
                            Offset(inset, inset),
                            Offset(size.width - inset, size.height - inset),
                            width,
                            StrokeCap.Round,
                        )
                        drawLine(
                            colors.onSurfaceVariant,
                            Offset(size.width - inset, inset),
                            Offset(inset, size.height - inset),
                            width,
                            StrokeCap.Round,
                        )
                    }
                }
            }
            LazyColumn(
                modifier = Modifier.fillMaxWidth().weight(1f),
                contentPadding = PaddingValues(horizontal = OrcaSpace.lg, vertical = OrcaSpace.sm),
                verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
            ) {
                items(items, key = { it.displayKey }) { item ->
                    ToolActivityRow(item)
                }
            }
        }
    }
}

@Composable
private fun ToolActivityRow(item: TranscriptItem) {
    val clipboard = LocalClipboardManager.current
    val colors = MaterialTheme.colorScheme
    var expanded by rememberSaveable(item.displayKey) { mutableStateOf(false) }
    val model = remember(item.title, item.command, item.output, item.description) { formatToolActivity(item) }
    val chevronRotation by animateFloatAsState(
        targetValue = if (expanded) 180f else 0f,
        animationSpec = tween(MotionTokens.Quick),
        label = "tool-row-chevron",
    )
    val status =
        when (item.state) {
            ToolState.COMPLETED -> "已完成"
            ToolState.RUNNING -> "执行中"
            ToolState.FAILED -> "失败"
            null -> "未知"
        }
    val statusColor =
        when (item.state) {
            ToolState.COMPLETED -> OrcaColors.accentGreen
            ToolState.RUNNING -> OrcaColors.accentAmber
            ToolState.FAILED -> OrcaColors.accentRed
            null -> colors.onSurfaceVariant
        }

    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        color = OrcaColors.bgPanel,
        border =
            BorderStroke(
                1.dp,
                if (expanded) OrcaColors.accentBlue.copy(alpha = 0.4f) else OrcaColors.borderSubtle,
            ),
    ) {
        Column {
            Row(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = 62.dp)
                        .then(
                            if (model.hasDetail) {
                                Modifier.clickable(role = Role.Button) { expanded = !expanded }
                            } else {
                                Modifier
                            },
                        )
                        .semantics { stateDescription = if (expanded) "$status，已展开" else status }
                        .padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Box(modifier = Modifier.size(34.dp), contentAlignment = Alignment.Center) {
                    Box(
                        modifier =
                            Modifier
                                .size(34.dp)
                                .clip(RoundedCornerShape(10.dp))
                                .background(statusColor.copy(alpha = 0.12f)),
                        contentAlignment = Alignment.Center,
                    ) {
                        ToolCategoryIcon(category = model.category, tint = statusColor, size = 17.dp)
                    }
                    Box(
                        modifier =
                            Modifier
                                .align(Alignment.BottomEnd)
                                .size(11.dp)
                                .clip(CircleShape)
                                .background(OrcaColors.bgPanel)
                                .padding(1.5.dp)
                                .clip(CircleShape)
                                .background(statusColor),
                    )
                }
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                    ) {
                        Text(
                            text = model.title,
                            style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight.SemiBold),
                            color = colors.onSurface,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        model.diffStats?.let { (adds, dels) ->
                            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                                if (adds > 0) {
                                    Text(
                                        text = "+$adds",
                                        style = OrcaType.mono.copy(fontSize = 11.sp, fontWeight = FontWeight.SemiBold),
                                        color = OrcaColors.accentGreen,
                                    )
                                }
                                if (dels > 0) {
                                    Text(
                                        text = "-$dels",
                                        style = OrcaType.mono.copy(fontSize = 11.sp, fontWeight = FontWeight.SemiBold),
                                        color = OrcaColors.accentRed,
                                    )
                                }
                            }
                        }
                    }
                    if (!model.subtitle.isNullOrBlank()) {
                        Text(
                            text = model.subtitle,
                            style = MaterialTheme.typography.bodySmall,
                            color = colors.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                if (item.state != ToolState.COMPLETED) {
                    Surface(
                        shape = RoundedCornerShape(OrcaRadius.pill),
                        color = statusColor.copy(alpha = 0.12f),
                    ) {
                        Text(
                            text = status,
                            style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Medium),
                            color = statusColor,
                            modifier = Modifier.padding(horizontal = 7.dp, vertical = 3.dp),
                        )
                    }
                }
                if (model.hasDetail) {
                    Box(Modifier.graphicsLayer { rotationZ = chevronRotation }) {
                        OrcaChevronDown(colors.onSurfaceVariant, 14.dp)
                    }
                }
            }
            AnimatedVisibility(
                visible = expanded && model.hasDetail,
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
                Column(
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .padding(start = 12.dp, end = 12.dp, bottom = 12.dp),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
                ) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .heightIn(min = 1.dp)
                            .background(OrcaColors.borderSubtle),
                    )
                    if (model.rawToolName != null || model.outputIsTrivialOk) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            if (model.rawToolName != null) {
                                Text(
                                    text = model.rawToolName,
                                    style = OrcaType.mono.copy(fontSize = 11.sp),
                                    color = colors.onSurfaceVariant,
                                )
                            }
                            if (model.outputIsTrivialOk) {
                                Text(
                                    text = "返回：OK",
                                    style = MaterialTheme.typography.labelSmall,
                                    color = OrcaColors.accentGreen,
                                )
                            }
                        }
                    }
                    if (model.description != null) {
                        Text(
                            text = model.description,
                            style = MaterialTheme.typography.bodySmall,
                            color = colors.onSurface,
                        )
                    }
                    if (model.params.isNotEmpty()) {
                        ToolParamsCard(
                            params = model.params,
                            rawCopy = model.rawCommandForCopy,
                            onCopy = { raw -> clipboard.setText(AnnotatedString(raw)) },
                        )
                    }
                    if (model.diff != null) {
                        DiffPreview(model.diff)
                    } else if (model.commandCode != null) {
                        ToolSection(
                            label = "执行命令",
                            text = model.commandCode,
                            onCopy = {
                                model.rawCommandForCopy?.let { clipboard.setText(AnnotatedString(it)) }
                            },
                        )
                    }
                    if (model.outputPreview != null && !model.outputIsTrivialOk) {
                        ToolSection(
                            label = "执行输出",
                            text = model.outputPreview,
                            onCopy = {
                                model.rawOutputForCopy?.let { clipboard.setText(AnnotatedString(it)) }
                            },
                        )
                    }
                }
            }
        }
    }
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
private fun ToolParamsCard(
    params: List<ToolParamRow>,
    rawCopy: String?,
    onCopy: (String) -> Unit,
) {
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) {
        if (copied) {
            delay(1600)
            copied = false
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "参数详情",
                style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Medium),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (!rawCopy.isNullOrBlank()) {
                OrcaTextAction(
                    label = if (copied) "已复制" else "复制参数",
                    onClick = {
                        onCopy(rawCopy)
                        copied = true
                    },
                )
            }
        }
        Surface(
            modifier = Modifier.fillMaxWidth(),
            shape = RoundedCornerShape(12.dp),
            color = OrcaColors.bgRaised.copy(alpha = 0.55f),
        ) {
            Column(
                modifier = Modifier.fillMaxWidth().padding(10.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                params.forEach { row ->
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        Text(
                            text = row.label,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        SelectionContainer {
                            Text(
                                text = row.value,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurface,
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ToolSection(label: String, text: String, onCopy: () -> Unit) {
    var copied by remember { mutableStateOf(false) }
    val scroll = rememberScrollState()
    LaunchedEffect(copied) {
        if (copied) {
            delay(1600)
            copied = false
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Medium),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OrcaTextAction(
                label = if (copied) "已复制" else "复制",
                onClick = {
                    onCopy()
                    copied = true
                },
            )
        }
        Box(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(12.dp))
                    .background(OrcaColors.bgRaised.copy(alpha = 0.55f))
                    .horizontalScroll(scroll)
                    .padding(10.dp),
        ) {
            SelectionContainer {
                Text(
                    text = text,
                    style = OrcaType.code,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }
        }
    }
}

@Composable
private fun ToolCategoryIcon(
    category: ToolCategory,
    tint: Color,
    size: Dp = 17.dp,
) {
    Canvas(Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        when (category) {
            ToolCategory.COMMAND -> {
                val p = Path().apply {
                    moveTo(w * 0.18f, h * 0.30f)
                    lineTo(w * 0.44f, h * 0.50f)
                    lineTo(w * 0.18f, h * 0.70f)
                }
                drawPath(p, tint, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
                drawLine(tint, Offset(w * 0.52f, h * 0.72f), Offset(w * 0.84f, h * 0.72f), stroke, StrokeCap.Round)
            }
            ToolCategory.FILE_READ -> {
                drawRoundRect(
                    tint,
                    topLeft = Offset(w * 0.20f, h * 0.14f),
                    size = Size(w * 0.60f, h * 0.72f),
                    cornerRadius = CornerRadius(2.dp.toPx()),
                    style = Stroke(stroke),
                )
                drawLine(tint, Offset(w * 0.34f, h * 0.38f), Offset(w * 0.66f, h * 0.38f), stroke, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.34f, h * 0.54f), Offset(w * 0.66f, h * 0.54f), stroke, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.34f, h * 0.70f), Offset(w * 0.52f, h * 0.70f), stroke, StrokeCap.Round)
            }
            ToolCategory.FILE_EDIT -> {
                val p = Path().apply {
                    moveTo(w * 0.20f, h * 0.80f)
                    lineTo(w * 0.28f, h * 0.56f)
                    lineTo(w * 0.68f, h * 0.16f)
                    lineTo(w * 0.84f, h * 0.32f)
                    lineTo(w * 0.44f, h * 0.72f)
                    close()
                }
                drawPath(p, tint, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
            ToolCategory.SEARCH -> {
                drawCircle(tint, radius = w * 0.28f, center = Offset(w * 0.44f, h * 0.44f), style = Stroke(stroke))
                drawLine(tint, Offset(w * 0.64f, h * 0.64f), Offset(w * 0.86f, h * 0.86f), stroke, StrokeCap.Round)
            }
            ToolCategory.WEB -> {
                drawCircle(tint, radius = w * 0.36f, center = Offset(w * 0.50f, h * 0.50f), style = Stroke(stroke))
                drawLine(tint, Offset(w * 0.14f, h * 0.50f), Offset(w * 0.86f, h * 0.50f), stroke, StrokeCap.Round)
                drawOval(
                    tint,
                    topLeft = Offset(w * 0.32f, h * 0.14f),
                    size = Size(w * 0.36f, h * 0.72f),
                    style = Stroke(stroke),
                )
            }
            ToolCategory.FOCUS -> {
                drawCircle(tint, radius = w * 0.36f, center = Offset(w * 0.50f, h * 0.50f), style = Stroke(stroke))
                drawCircle(tint, radius = w * 0.14f, center = Offset(w * 0.50f, h * 0.50f))
            }
            ToolCategory.SUBAGENT -> {
                drawCircle(tint, radius = w * 0.14f, center = Offset(w * 0.50f, h * 0.24f), style = Stroke(stroke))
                drawCircle(tint, radius = w * 0.14f, center = Offset(w * 0.24f, h * 0.74f), style = Stroke(stroke))
                drawCircle(tint, radius = w * 0.14f, center = Offset(w * 0.76f, h * 0.74f), style = Stroke(stroke))
                drawLine(tint, Offset(w * 0.44f, h * 0.36f), Offset(w * 0.30f, h * 0.62f), stroke, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.56f, h * 0.36f), Offset(w * 0.70f, h * 0.62f), stroke, StrokeCap.Round)
            }
            ToolCategory.MCP, ToolCategory.OTHER -> {
                val p = Path().apply {
                    moveTo(w * 0.50f, h * 0.14f)
                    lineTo(w * 0.82f, h * 0.32f)
                    lineTo(w * 0.82f, h * 0.68f)
                    lineTo(w * 0.50f, h * 0.86f)
                    lineTo(w * 0.18f, h * 0.68f)
                    lineTo(w * 0.18f, h * 0.32f)
                    close()
                }
                drawPath(p, tint, style = Stroke(stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
        }
    }
}
