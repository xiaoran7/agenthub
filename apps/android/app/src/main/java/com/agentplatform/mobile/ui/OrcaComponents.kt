package com.agentplatform.mobile.ui

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
import androidx.compose.foundation.layout.defaultMinSize
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
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.util.Locale

// =========================================================================
// AgentHub shared surface, row, chip & button primitives
// =========================================================================

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

@Composable
fun OrcaSectionHeader(
    title: String,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier =
            modifier
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

@Composable
fun OrcaListGroup(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    OrcaCard(modifier) {
        Column(content = content)
    }
}

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
        modifier =
            modifier
                .fillMaxWidth()
                .heightIn(min = OrcaSpace.touch)
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
                    maxLines = 2,
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

/** Accessible text action with 48dp minimum interactive touch target. */
@Composable
fun OrcaTextAction(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Box(
        modifier =
            modifier
                .minimumInteractiveComponentSize()
                .defaultMinSize(minHeight = 36.dp)
                .clip(RoundedCornerShape(OrcaRadius.button))
                .clickable(enabled = enabled, onClick = onClick)
                .padding(horizontal = OrcaSpace.sm, vertical = OrcaSpace.xs),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelMedium,
            fontWeight = FontWeight.SemiBold,
            color =
                if (enabled) OrcaColors.accentBlue
                else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
            maxLines = 1,
        )
    }
}

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
            modifier = modifier.minimumInteractiveComponentSize(),
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

@Composable
fun OrcaPrimaryButton(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.heightIn(min = OrcaSpace.touch),
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
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
fun OrcaSecondaryButton(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        modifier = modifier.heightIn(min = OrcaSpace.touch),
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
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

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

@Composable
fun OrcaIconTile(
    modifier: Modifier = Modifier,
    size: Dp = OrcaSize.tile,
    content: @Composable BoxScope.() -> Unit,
) {
    Box(
        modifier =
            modifier
                .size(size)
                .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(OrcaRadius.tile)),
        contentAlignment = Alignment.Center,
        content = content,
    )
}

@Composable
fun HostStatusTile(
    online: Boolean,
    modifier: Modifier = Modifier,
    size: Dp = OrcaSize.tile,
) {
    val dot = if (online) OrcaColors.accentGreen else OrcaColors.textMuted
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

@Composable
fun OrcaNoticeCard(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    icon: (@Composable () -> Unit)? = null,
) {
    Column(
        modifier =
            modifier
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

@Composable
fun OrcaStepRow(index: Int, title: String, body: String) {
    Row(
        modifier =
            Modifier
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

internal fun compactTokens(tokens: Long): String =
    when {
        tokens >= 1_000_000 -> {
            val millions = tokens / 1_000_000.0
            if (millions >= 10) "${millions.toLong()}M"
            else String.format(Locale.US, "%.1f", millions) + "M"
        }
        tokens >= 1_000 -> "${tokens / 1_000}k"
        else -> tokens.toString()
    }

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
    val track = OrcaColors.borderStrong
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
            drawCircle(track, radius = radius, style = Stroke(stroke))
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
                    if (window != null) {
                        " / " + compactTokens(window) + " · " +
                            String.format(Locale.US, "%.0f", fraction * 100f) + "%"
                    } else {
                        " tokens · 容量未上报"
                    },
            style = OrcaType.mono,
            color = if (window != null) tint else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
