package com.agentplatform.mobile.ui

import java.util.Locale
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

// =========================================================================
// AgentHub shared primitives
//
// Every surface, row, chip and button in the app is built from this file so
// spacing, radii and type stay on the same grid. Layout values follow
// stablyai/orca mobile (MIT): 4/8/12/16/24 spacing, 14dp cards, 6dp controls.
// =========================================================================

/** Status dot used for host and session state. */
@Composable
fun OrcaStatusDot(
    color: Color,
    size: Dp = 6.dp,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier
            .size(size)
            .background(color, CircleShape),
    )
}

/** Uppercase section label with a trailing action slot. */
@Composable
fun OrcaSectionHeader(
    title: String,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(start = 2.dp, end = 2.dp, top = OrcaSpace.xl, bottom = OrcaSpace.sm),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = title,
            style = OrcaType.section,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        trailing?.invoke()
    }
}

/** Panel surface with a 1dp hairline border and 14dp radius. */
@Composable
fun OrcaCard(
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    val shape = RoundedCornerShape(OrcaRadius.card)
    val border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline)
    if (onClick != null) {
        Surface(
            onClick = onClick,
            modifier = modifier.fillMaxWidth(),
            shape = shape,
            color = MaterialTheme.colorScheme.surface,
            border = border,
            content = content,
        )
    } else {
        Surface(
            modifier = modifier.fillMaxWidth(),
            shape = shape,
            color = MaterialTheme.colorScheme.surface,
            border = border,
            content = content,
        )
    }
}

/** Card that groups rows; rows are separated by [OrcaRowDivider]. */
@Composable
fun OrcaListGroup(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    OrcaCard(modifier) {
        Column(content = content)
    }
}

/** Hairline separator inside a list group. */
@Composable
fun OrcaRowDivider(startInset: Dp = OrcaSpace.rowH) {
    Spacer(
        Modifier
            .fillMaxWidth()
            .padding(start = startInset)
            .height(1.dp)
            .background(MaterialTheme.colorScheme.outline),
    )
}

/**
 * The single row primitive for hosts, sessions, workspaces and settings.
 * Title is always one line; subtitle and meta are optional.
 */
@Composable
fun OrcaRow(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    meta: String? = null,
    statusColor: Color? = null,
    statusText: String? = null,
    titleColor: Color = Color.Unspecified,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable RowScope.() -> Unit)? = null,
    enabled: Boolean = true,
    onClick: (() -> Unit)? = null,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .then(
                if (onClick != null) Modifier.clickable(enabled = enabled, onClick = onClick)
                else Modifier,
            )
            .padding(horizontal = OrcaSpace.rowH, vertical = OrcaSpace.rowV),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.md),
    ) {
        leading?.invoke()
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
        ) {
            Text(
                text = title,
                style = OrcaType.rowTitle,
                color =
                    if (titleColor == Color.Unspecified) MaterialTheme.colorScheme.onSurface
                    else titleColor,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            subtitle?.takeIf { it.isNotBlank() }?.let {
                Text(
                    text = it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (statusText != null || meta != null) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
                ) {
                    if (statusColor != null) OrcaStatusDot(statusColor, 6.dp)
                    if (statusText != null) {
                        Text(
                            text = statusText,
                            style = MaterialTheme.typography.bodySmall,
                            color = statusColor ?: MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                        )
                    }
                    if (statusText != null && meta != null) OrcaDotSeparator()
                    meta?.takeIf { it.isNotBlank() }?.let {
                        Text(
                            text = it,
                            style = OrcaType.mono,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
        trailing?.invoke(this)
    }
}

@Composable
fun OrcaDotSeparator() {
    Text(
        "·",
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
    )
}

/** Small text button used for section-level actions such as "添加". */
@Composable
fun OrcaTextAction(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Text(
        text = label,
        style = MaterialTheme.typography.labelMedium,
        fontWeight = FontWeight.SemiBold,
        color =
            if (enabled) OrcaColors.accentBlue
            else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
        modifier = modifier
            .clip(RoundedCornerShape(OrcaRadius.button))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = OrcaSpace.sm, vertical = OrcaSpace.xs),
    )
}

/** Compact pill chip for endpoints and metadata. */
@Composable
fun OrcaChip(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    background: Color = MaterialTheme.colorScheme.surfaceVariant,
    onClick: (() -> Unit)? = null,
) {
    val shape = RoundedCornerShape(OrcaRadius.pill)
    if (onClick != null) {
        Surface(
            onClick = onClick,
            modifier = modifier,
            shape = shape,
            color = background,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        ) {
            OrcaChipLabel(text, color)
        }
    } else {
        Surface(
            modifier = modifier,
            shape = shape,
            color = background,
            border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        ) {
            OrcaChipLabel(text, color)
        }
    }
}

@Composable
private fun OrcaChipLabel(text: String, color: Color) {
    Text(
        text = text,
        style = OrcaType.mono,
        color = color,
        maxLines = 1,
        modifier = Modifier.padding(horizontal = OrcaSpace.sm, vertical = 4.dp),
    )
}

/** Inverted primary action, the only bright surface in the interface. */
@Composable
fun OrcaPrimaryButton(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.heightIn(min = 44.dp),
        enabled = enabled,
        shape = RoundedCornerShape(OrcaRadius.button),
        color = if (enabled) OrcaColors.surfaceBright else OrcaColors.bgRaised,
    ) {
        Box(
            modifier = Modifier.fillMaxWidth().padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.md),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelLarge,
                fontWeight = FontWeight.Bold,
                color =
                    if (enabled) OrcaColors.onBright
                    else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                maxLines = 1,
            )
        }
    }
}

/** Bordered secondary action. */
@Composable
fun OrcaSecondaryButton(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.heightIn(min = 44.dp),
        enabled = enabled,
        shape = RoundedCornerShape(OrcaRadius.button),
        color = Color.Transparent,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Box(
            modifier = Modifier.fillMaxWidth().padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.md),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelLarge,
                color =
                    if (enabled) MaterialTheme.colorScheme.onSurface
                    else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
                maxLines = 1,
            )
        }
    }
}

/** 40dp square icon button; the glyph itself stays 18dp. */
@Composable
fun OrcaIconButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    content: @Composable () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.size(OrcaSize.iconButton),
        enabled = enabled,
        shape = RoundedCornerShape(OrcaRadius.button),
        color = Color.Transparent,
    ) {
        Box(contentAlignment = Alignment.Center) { content() }
    }
}

/** Rounded-square icon tile used as a row lead. */
@Composable
fun OrcaIconTile(
    modifier: Modifier = Modifier,
    size: Dp = OrcaSize.tile,
    content: @Composable BoxScope.() -> Unit,
) {
    Box(
        modifier = modifier
            .size(size)
            .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(OrcaRadius.tile)),
        contentAlignment = Alignment.Center,
        content = content,
    )
}

// -------------------------------------------------------------------------
// Icons: vector-drawn so the app ships no icon dependency.
// -------------------------------------------------------------------------

@Composable
fun OrcaLogo(size: Dp = 20.dp, tint: Color = MaterialTheme.colorScheme.onBackground) {
    Canvas(modifier = Modifier.size(size * 1.45f, size)) {
        val w = this.size.width
        val h = this.size.height
        val body = Path().apply {
            moveTo(w * 0.04f, h * 0.62f)
            cubicTo(w * 0.18f, h * 0.20f, w * 0.52f, h * 0.08f, w * 0.78f, h * 0.26f)
            cubicTo(w * 0.96f, h * 0.38f, w * 0.98f, h * 0.62f, w * 0.86f, h * 0.78f)
            cubicTo(w * 0.72f, h * 0.96f, w * 0.42f, h * 0.98f, w * 0.24f, h * 0.84f)
            lineTo(w * 0.40f, h * 0.74f)
            cubicTo(w * 0.50f, h * 0.80f, w * 0.68f, h * 0.80f, w * 0.76f, h * 0.66f)
            cubicTo(w * 0.84f, h * 0.52f, w * 0.76f, h * 0.36f, w * 0.58f, h * 0.32f)
            cubicTo(w * 0.38f, h * 0.28f, w * 0.18f, h * 0.42f, w * 0.12f, h * 0.60f)
            close()
        }
        drawPath(body, color = tint)
        val fin = Path().apply {
            moveTo(w * 0.46f, h * 0.20f)
            lineTo(w * 0.54f, h * 0.04f)
            lineTo(w * 0.62f, h * 0.22f)
            close()
        }
        drawPath(fin, color = tint)
    }
}

@Composable
fun OrcaMonitorIcon(tint: Color, size: Dp = OrcaSize.icon) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        drawRoundRect(
            color = tint,
            topLeft = Offset(w * 0.10f, h * 0.16f),
            size = Size(w * 0.80f, h * 0.56f),
            cornerRadius = CornerRadius(2.dp.toPx(), 2.dp.toPx()),
            style = Stroke(width = stroke),
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.34f, h * 0.86f),
            end = Offset(w * 0.66f, h * 0.86f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.50f, h * 0.72f),
            end = Offset(w * 0.50f, h * 0.86f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun OrcaTerminalIcon(tint: Color, size: Dp = OrcaSize.icon) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.7.dp.toPx()
        val chevron = Path().apply {
            moveTo(w * 0.16f, h * 0.30f)
            lineTo(w * 0.42f, h * 0.50f)
            lineTo(w * 0.16f, h * 0.70f)
        }
        drawPath(
            chevron,
            color = tint,
            style = Stroke(width = stroke, cap = StrokeCap.Round, join = StrokeJoin.Round),
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.50f, h * 0.72f),
            end = Offset(w * 0.84f, h * 0.72f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun OrcaChevronRight(tint: Color, size: Dp = 16.dp) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        val path = Path().apply {
            moveTo(w * 0.36f, h * 0.22f)
            lineTo(w * 0.64f, h * 0.50f)
            lineTo(w * 0.36f, h * 0.78f)
        }
        drawPath(
            path,
            color = tint,
            style = Stroke(width = stroke, cap = StrokeCap.Round, join = StrokeJoin.Round),
        )
    }
}

@Composable
fun OrcaChevronDown(tint: Color, size: Dp = 14.dp, expanded: Boolean = false) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        if (expanded) {
            drawLine(tint, Offset(w * 0.20f, h * 0.62f), Offset(w * 0.50f, h * 0.34f), stroke, StrokeCap.Round)
            drawLine(tint, Offset(w * 0.50f, h * 0.34f), Offset(w * 0.80f, h * 0.62f), stroke, StrokeCap.Round)
        } else {
            drawLine(tint, Offset(w * 0.20f, h * 0.38f), Offset(w * 0.50f, h * 0.66f), stroke, StrokeCap.Round)
            drawLine(tint, Offset(w * 0.50f, h * 0.66f), Offset(w * 0.80f, h * 0.38f), stroke, StrokeCap.Round)
        }
    }
}

@Composable
fun OrcaBackChevron(tint: Color, size: Dp = 20.dp) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.9.dp.toPx()
        val path = Path().apply {
            moveTo(w * 0.62f, h * 0.20f)
            lineTo(w * 0.34f, h * 0.50f)
            lineTo(w * 0.62f, h * 0.80f)
        }
        drawPath(
            path,
            color = tint,
            style = Stroke(width = stroke, cap = StrokeCap.Round, join = StrokeJoin.Round),
        )
    }
}

@Composable
fun OrcaRefreshIcon(tint: Color, size: Dp = OrcaSize.icon) {
    Canvas(modifier = Modifier.size(size)) {
        val stroke = 1.7.dp.toPx()
        drawArc(
            color = tint,
            startAngle = 40f,
            sweepAngle = 280f,
            useCenter = false,
            style = Stroke(width = stroke, cap = StrokeCap.Round),
        )
        val tipX = this.size.width * 0.80f
        val tipY = this.size.height * 0.24f
        drawLine(tint, Offset(tipX - 5.dp.toPx(), tipY), Offset(tipX, tipY), stroke, StrokeCap.Round)
        drawLine(tint, Offset(tipX, tipY), Offset(tipX, tipY + 5.dp.toPx()), stroke, StrokeCap.Round)
    }
}

@Composable
fun OrcaPlusIcon(tint: Color, size: Dp = OrcaSize.icon) {
    Canvas(modifier = Modifier.size(size)) {
        val stroke = 1.9.dp.toPx()
        drawLine(
            tint,
            Offset(this.size.width * 0.5f, this.size.height * 0.18f),
            Offset(this.size.width * 0.5f, this.size.height * 0.82f),
            stroke,
            StrokeCap.Round,
        )
        drawLine(
            tint,
            Offset(this.size.width * 0.18f, this.size.height * 0.5f),
            Offset(this.size.width * 0.82f, this.size.height * 0.5f),
            stroke,
            StrokeCap.Round,
        )
    }
}

@Composable
fun OrcaSendIcon(tint: Color, size: Dp = 16.dp) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.8.dp.toPx()
        drawLine(tint, Offset(w * 0.5f, h * 0.88f), Offset(w * 0.5f, h * 0.14f), stroke, StrokeCap.Round)
        drawLine(tint, Offset(w * 0.20f, h * 0.44f), Offset(w * 0.50f, h * 0.14f), stroke, StrokeCap.Round)
        drawLine(tint, Offset(w * 0.80f, h * 0.44f), Offset(w * 0.50f, h * 0.14f), stroke, StrokeCap.Round)
    }
}

@Composable
fun OrcaStopIcon(tint: Color, size: Dp = 10.dp) {
    Canvas(modifier = Modifier.size(size)) {
        drawRoundRect(
            color = tint,
            cornerRadius = CornerRadius(1.5.dp.toPx()),
        )
    }
}

@Composable
fun OrcaSettingsIcon(tint: Color, size: Dp = OrcaSize.icon) {
    // Sliders glyph: unambiguous for "preferences" and visually distinct from
    // the refresh ring that sits next to it in the top bar.
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        val rows = listOf(0.28f to 0.68f, 0.72f to 0.34f)
        rows.forEach { (y, knobX) ->
            drawLine(
                tint,
                Offset(w * 0.14f, h * y),
                Offset(w * 0.86f, h * y),
                stroke,
                StrokeCap.Round,
            )
            drawCircle(
                color = tint,
                radius = w * 0.11f,
                center = Offset(w * knobX, h * y),
                style = Stroke(width = stroke),
            )
        }
    }
}

@Composable
fun OrcaCheckIcon(tint: Color, size: Dp = OrcaSize.icon) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val path = Path().apply {
            moveTo(w * 0.20f, h * 0.52f)
            lineTo(w * 0.42f, h * 0.74f)
            lineTo(w * 0.80f, h * 0.28f)
        }
        drawPath(
            path,
            color = tint,
            style = Stroke(width = 1.8.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round),
        )
    }
}

// -------------------------------------------------------------------------
// Composite blocks
// -------------------------------------------------------------------------

/** "Host 1A2B3C" label derived from the runner id. */
fun hostDisplayName(deviceId: String): String {
    if (deviceId.isBlank()) return "未知主机"
    val tail = deviceId.takeLast(6).uppercase()
    return "Host $tail"
}

/** Monitor tile with an online/offline dot in the corner. */
@Composable
fun HostStatusTile(
    online: Boolean,
    modifier: Modifier = Modifier,
    size: Dp = OrcaSize.tile,
) {
    val dot = if (online) OrcaColors.accentGreen else MaterialTheme.colorScheme.outlineVariant
    OrcaIconTile(modifier, size) {
        OrcaMonitorIcon(
            tint =
                if (online) MaterialTheme.colorScheme.onSurface
                else MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Box(
            Modifier
                .align(Alignment.BottomEnd)
                .padding(3.dp)
                .size(10.dp)
                .background(MaterialTheme.colorScheme.surfaceVariant, CircleShape)
                .padding(2.dp)
                .background(dot, CircleShape),
        )
    }
}

/** Three-up metric strip; the three figures share one surface. */
@Composable
fun OrcaStatStrip(
    modifier: Modifier = Modifier,
    stats: List<OrcaStat>,
) {
    OrcaCard(modifier) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            stats.forEachIndexed { index, stat ->
                if (index > 0) {
                    Spacer(
                        Modifier
                            .width(1.dp)
                            .height(26.dp)
                            .background(MaterialTheme.colorScheme.outline),
                    )
                }
                Column(
                    modifier = Modifier.weight(1f).padding(horizontal = OrcaSpace.md),
                    verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs),
                ) {
                    Text(
                        text = stat.value,
                        style = MaterialTheme.typography.headlineMedium,
                        color = stat.color ?: MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                    )
                    Text(
                        text = stat.label,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

data class OrcaStat(val value: String, val label: String, val color: Color? = null)

/** Empty/error placeholder inside a card. */
@Composable
fun OrcaNoticeCard(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    icon: (@Composable () -> Unit)? = null,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = OrcaSpace.xl, vertical = OrcaSpace.xl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
    ) {
        icon?.invoke()
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurface,
            textAlign = TextAlign.Center,
        )
        Text(
            text = body,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
    }
}

/** Inline error strip shared by every screen. */
@Composable
fun OrcaInlineError(message: String, modifier: Modifier = Modifier) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(OrcaRadius.button),
        color = OrcaColors.accentRed.copy(alpha = 0.10f),
        border = BorderStroke(1.dp, OrcaColors.accentRed.copy(alpha = 0.45f)),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = OrcaSpace.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            OrcaStatusDot(OrcaColors.accentRed, 6.dp)
            Text(
                text = message,
                style = MaterialTheme.typography.bodySmall,
                color = OrcaColors.accentRed,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

/** Two-line explanatory block used in onboarding. */
@Composable
fun OrcaStepRow(index: Int, title: String, body: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = OrcaSpace.rowH, vertical = OrcaSpace.md),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.md),
    ) {
        Text(
            text = "$index",
            style = OrcaType.mono,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(16.dp).padding(top = 2.dp),
        )
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        ) {
            Text(
                text = title,
                style = OrcaType.rowTitle,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text = body,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Agent execution-state label shared by host and session screens. */
internal fun stateLabel(state: String): String =
    when (state) {
        "running" -> "运行中"
        "awaiting_permission" -> "等待授权"
        "awaiting_input" -> "等待输入"
        "idle" -> "空闲"
        "completed" -> "已完成"
        "error" -> "异常"
        "terminated" -> "已终止"
        else -> "离线"
    }

internal fun stateColor(state: String): Color =
    when (state) {
        "running" -> OrcaColors.accentBlue
        "awaiting_permission", "awaiting_input" -> OrcaColors.accentAmber
        "idle", "completed" -> OrcaColors.accentGreen
        "error" -> OrcaColors.accentRed
        else -> OrcaColors.textMuted
    }

/** `128k` / `1.2M`: compact enough for a header, exact enough to trust. */
internal fun compactTokens(tokens: Long): String =
    when {
        tokens >= 1_000_000 -> {
            val millions = tokens / 1_000_000.0
            if (millions >= 10) "${millions.toLong()}M" else String.format(Locale.US, "%.1f", millions) + "M"
        }
        tokens >= 1_000 -> "${tokens / 1_000}k"
        else -> tokens.toString()
    }

/**
 * Context-window gauge for the session header.
 *
 * A ring is used instead of a bar because it has to live beside the state dot
 * without stealing a whole row from the transcript. The percentage is only
 * drawn when the provider reported a window: showing "42%" against a made-up
 * denominator would be a lie, so an unknown capacity says so in words.
 */
@Composable
internal fun ContextCapacityRing(
    usage: ContextUsage,
    onToggle: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val window = usage.window
    val fraction =
        if (window != null && window > 0) {
            (usage.used.toFloat() / window.toFloat()).coerceIn(0f, 1f)
        } else {
            0f
        }
    val tint =
        when {
            window == null -> OrcaColors.textMuted
            fraction >= 0.9f -> OrcaColors.accentRed
            fraction >= 0.7f -> OrcaColors.accentAmber
            else -> OrcaColors.accentBlue
        }
    Box(
        modifier =
            modifier
                .size(OrcaSize.iconButton)
                .clip(CircleShape)
                .clickable(onClick = onToggle),
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.size(20.dp)) {
            val stroke = 2.dp.toPx()
            val radius = (size.minDimension - stroke) / 2f
            drawCircle(OrcaColors.borderStrong, radius = radius, style = Stroke(stroke))
            if (window != null) {
                drawArc(
                    color = tint,
                    startAngle = -90f,
                    sweepAngle = 360f * fraction,
                    useCenter = false,
                    topLeft = Offset((size.width - radius * 2) / 2f, (size.height - radius * 2) / 2f),
                    size = Size(radius * 2, radius * 2),
                    style = Stroke(stroke, cap = StrokeCap.Round),
                )
            }
        }
    }
}

/** One-line capacity readout shown while the ring is expanded. */
@Composable
internal fun ContextCapacityStrip(usage: ContextUsage, modifier: Modifier = Modifier) {
    val window = usage.window
    val fraction =
        if (window != null && window > 0) {
            (usage.used.toFloat() / window.toFloat()).coerceIn(0f, 1f)
        } else {
            0f
        }
    val tint =
        when {
            window == null -> OrcaColors.textMuted
            fraction >= 0.9f -> OrcaColors.accentRed
            fraction >= 0.7f -> OrcaColors.accentAmber
            else -> OrcaColors.accentBlue
        }
    Row(
        modifier = modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
    ) {
        Text(
            text = "上下文",
            style = OrcaType.mono,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Box(
            Modifier
                .weight(1f)
                .height(3.dp)
                .clip(RoundedCornerShape(OrcaRadius.pill))
                .background(OrcaColors.borderStrong),
        ) {
            if (window != null) {
                Box(
                    Modifier
                        .fillMaxWidth(fraction)
                        .height(3.dp)
                        .background(tint),
                )
            }
        }
        Text(
            text =
                compactTokens(usage.used) +
                    if (window != null) " / " + compactTokens(window) + " · " +
                        String.format(Locale.US, "%.0f", fraction * 100f) + "%"
                    else " tokens · 容量未上报",
            style = OrcaType.mono,
            color = if (window != null) tint else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
