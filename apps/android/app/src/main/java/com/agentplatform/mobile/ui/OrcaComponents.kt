package com.agentplatform.mobile.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.agentplatform.mobile.data.AgentType
import com.agentplatform.mobile.data.DeviceRecord

// =========================================================================
// Orca Mobile Shared Primitives
// Mirrors stablyai/orca mobile/src/components/*
// =========================================================================

/** Status dot: 7dp, matches Orca `StatusDot`. */
@Composable
fun OrcaStatusDot(
    color: Color,
    size: Int = 7,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier
            .size(size.dp)
            .background(color, CircleShape),
    )
}

/** Uppercase section label: 11sp, weight 600, muted, letter-spacing 0.6. */
@Composable
fun OrcaSectionLabel(text: String, modifier: Modifier = Modifier) {
    Text(
        text = text.uppercase(),
        style = MaterialTheme.typography.labelSmall.copy(letterSpacing = 0.6.sp),
        fontWeight = FontWeight.SemiBold,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = modifier.padding(start = 4.dp, bottom = 6.dp),
    )
}

/** Orca-style panel card: bgPanel, 1px border, radius 14. */
@Composable
fun OrcaCard(
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    val shape = RoundedCornerShape(14.dp)
    Surface(
        modifier =
            modifier
                .fillMaxWidth()
                .clip(shape)
                .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier),
        shape = shape,
        color = MaterialTheme.colorScheme.surface,
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
        content = content,
    )
}

/** Section header row: label on the left, optional trailing action. */
@Composable
fun OrcaSectionHeader(
    title: String,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(top = 18.dp, bottom = 6.dp, start = 4.dp, end = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = title.uppercase(),
            style = MaterialTheme.typography.labelSmall.copy(letterSpacing = 0.6.sp),
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        trailing?.invoke()
    }
}

/** Small pill chip: fully rounded, 999 radius, subtle border. */
@Composable
fun OrcaChip(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    background: Color = MaterialTheme.colorScheme.surfaceVariant,
    onClick: (() -> Unit)? = null,
) {
    Surface(
        modifier =
            modifier
                .clip(RoundedCornerShape(999.dp))
                .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier),
        shape = RoundedCornerShape(999.dp),
        color = background,
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.bodySmall.copy(fontSize = 11.sp),
            color = color,
            maxLines = 1,
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 3.dp),
        )
    }
}

/** Primary CTA: bright surface with dark text (Orca inverted button in dark mode). */
@Composable
fun OrcaPrimaryButton(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val dark = MaterialTheme.colorScheme.background == OrcaColors.bgBase
    val fill = if (dark) OrcaColors.surfaceBright else MaterialTheme.colorScheme.primary
    val content = if (dark) OrcaColors.onBrightPrimary else Color.White
    Surface(
        modifier =
            modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(6.dp))
                .clickable(enabled = enabled, onClick = onClick),
        shape = RoundedCornerShape(6.dp),
        color = if (enabled) fill else fill.copy(alpha = 0.35f),
    ) {
        Box(
            modifier = Modifier.padding(vertical = 11.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                label,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Bold,
                color = if (enabled) content else content.copy(alpha = 0.6f),
            )
        }
    }
}

/** Secondary button: border + panel fill. */
@Composable
fun OrcaSecondaryButton(
    label: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Surface(
        modifier =
            modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(6.dp))
                .clickable(enabled = enabled, onClick = onClick),
        shape = RoundedCornerShape(6.dp),
        color = MaterialTheme.colorScheme.surface,
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Box(
            modifier = Modifier.padding(vertical = 10.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                label,
                style = MaterialTheme.typography.titleSmall,
                color =
                    if (enabled) MaterialTheme.colorScheme.onSurface
                    else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f),
            )
        }
    }
}

/** 46x46 rounded-square icon tile, radius 13, bgRaised — matches Orca host/resume cards. */
@Composable
fun OrcaIconTile(
    modifier: Modifier = Modifier,
    size: Int = 46,
    tint: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    content: @Composable () -> Unit,
) {
    Box(
        modifier = modifier
            .size(size.dp)
            .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(13.dp)),
        contentAlignment = Alignment.Center,
    ) {
        content()
    }
}

// -------------------------------------------------------------------------
// Icons (vector-drawn to avoid an icon dependency, matching Orca glyph set)
// -------------------------------------------------------------------------

@Composable
fun OrcaLogo(size: Int = 22, tint: Color = MaterialTheme.colorScheme.onBackground) {
    Canvas(modifier = Modifier.size((size * 1.57f).dp, size.dp)) {
        val w = this.size.width
        val h = this.size.height
        // Stylized wave-mark: body arc + dorsal fin + tail, single silhouette.
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
        // Dorsal fin cut-out
        val fin = Path().apply {
            moveTo(w * 0.46f, h * 0.20f)
            lineTo(w * 0.54f, h * 0.04f)
            lineTo(w * 0.62f, h * 0.22f)
            close()
        }
        drawPath(fin, color = tint)
        // Eye negative space
        drawCircle(
            color = tint.copy(alpha = 0.0f),
            radius = h * 0.05f,
            center = Offset(w * 0.30f, h * 0.52f),
        )
    }
}

@Composable
fun OrcaMonitorIcon(tint: Color, size: Int = 20) {
    Canvas(modifier = Modifier.size(size.dp)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        drawRoundRect(
            color = tint,
            topLeft = Offset(w * 0.10f, h * 0.16f),
            size = androidx.compose.ui.geometry.Size(w * 0.80f, h * 0.56f),
            cornerRadius = androidx.compose.ui.geometry.CornerRadius(2.dp.toPx(), 2.dp.toPx()),
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
fun OrcaTerminalIcon(tint: Color, size: Int = 18) {
    Canvas(modifier = Modifier.size(size.dp)) {
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
fun OrcaChevronRight(tint: Color, size: Int = 16) {
    Canvas(modifier = Modifier.size(size.dp)) {
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
fun OrcaChevronDown(tint: Color, size: Int = 14, expanded: Boolean = false) {
    Canvas(modifier = Modifier.size(size.dp)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        if (expanded) {
            drawLine(
                color = tint,
                start = Offset(w * 0.20f, h * 0.62f),
                end = Offset(w * 0.50f, h * 0.34f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
            drawLine(
                color = tint,
                start = Offset(w * 0.50f, h * 0.34f),
                end = Offset(w * 0.80f, h * 0.62f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
        } else {
            drawLine(
                color = tint,
                start = Offset(w * 0.20f, h * 0.38f),
                end = Offset(w * 0.50f, h * 0.66f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
            drawLine(
                color = tint,
                start = Offset(w * 0.50f, h * 0.66f),
                end = Offset(w * 0.80f, h * 0.38f),
                strokeWidth = stroke,
                cap = StrokeCap.Round,
            )
        }
    }
}

@Composable
fun OrcaBackChevron(tint: Color, size: Int = 20) {
    Canvas(modifier = Modifier.size(size.dp)) {
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
fun OrcaRefreshIcon(tint: Color, size: Int = 18) {
    Canvas(modifier = Modifier.size(size.dp)) {
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
        drawLine(
            color = tint,
            start = Offset(tipX - 5.dp.toPx(), tipY),
            end = Offset(tipX, tipY),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(tipX, tipY),
            end = Offset(tipX, tipY + 5.dp.toPx()),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun OrcaPlusIcon(tint: Color, size: Int = 18) {
    Canvas(modifier = Modifier.size(size.dp)) {
        val stroke = 1.9.dp.toPx()
        drawLine(
            color = tint,
            start = Offset(this.size.width * 0.5f, this.size.height * 0.18f),
            end = Offset(this.size.width * 0.5f, this.size.height * 0.82f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(this.size.width * 0.18f, this.size.height * 0.5f),
            end = Offset(this.size.width * 0.82f, this.size.height * 0.5f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun OrcaMoreIcon(tint: Color, size: Int = 18) {
    Canvas(modifier = Modifier.size(size.dp)) {
        val r = 1.5.dp.toPx()
        val cx = this.size.width * 0.5f
        drawCircle(tint, r, Offset(cx, this.size.height * 0.22f))
        drawCircle(tint, r, Offset(cx, this.size.height * 0.50f))
        drawCircle(tint, r, Offset(cx, this.size.height * 0.78f))
    }
}

@Composable
fun OrcaSendIcon(tint: Color, size: Int = 14) {
    Canvas(modifier = Modifier.size(size.dp)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.8.dp.toPx()
        drawLine(
            color = tint,
            start = Offset(w * 0.5f, h * 0.88f),
            end = Offset(w * 0.5f, h * 0.14f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.20f, h * 0.44f),
            end = Offset(w * 0.50f, h * 0.14f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = Offset(w * 0.80f, h * 0.44f),
            end = Offset(w * 0.50f, h * 0.14f),
            strokeWidth = stroke,
            cap = StrokeCap.Round,
        )
    }
}

@Composable
fun OrcaStopIcon(tint: Color, size: Int = 12) {
    Canvas(modifier = Modifier.size(size.dp)) {
        drawRoundRect(
            color = tint,
            cornerRadius = androidx.compose.ui.geometry.CornerRadius(1.5.dp.toPx()),
        )
    }
}

@Composable
fun OrcaSettingsIcon(tint: Color, size: Int = 18) {
    Canvas(modifier = Modifier.size(size.dp)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        drawCircle(
            color = tint,
            radius = w * 0.20f,
            center = Offset(w * 0.5f, h * 0.5f),
            style = Stroke(width = stroke),
        )
        val outer = w * 0.38f
        repeat(6) { index ->
            val angle = (index * 60f) * (Math.PI / 180.0)
            val sx = w * 0.5f + (outer * Math.cos(angle)).toFloat()
            val sy = h * 0.5f + (outer * Math.sin(angle)).toFloat()
            val ex = w * 0.5f + (outer * 1.24f * Math.cos(angle)).toFloat()
            val ey = h * 0.5f + (outer * 1.24f * Math.sin(angle)).toFloat()
            drawLine(tint, Offset(sx, sy), Offset(ex, ey), stroke, StrokeCap.Round)
        }
    }
}

/** Agent provider dot color, matching Orca's per-provider identity colors. */
fun agentAccent(type: AgentType): Color =
    when (type) {
        AgentType.CLAUDE_CODE -> OrcaColors.providerClaude
        AgentType.CODEX -> OrcaColors.providerCodex
        AgentType.ANTIGRAVITY -> OrcaColors.providerAntigravity
        AgentType.GROK_BUILD -> OrcaColors.providerGrok
        AgentType.UNKNOWN -> OrcaColors.providerUnknown
    }

@Composable
fun AgentProviderDot(type: AgentType, size: Int = 7) {
    Box(
        Modifier
            .size(size.dp)
            .background(agentAccent(type), CircleShape),
    )
}

/** "Host 1"-style label derived from the runner/device id. */
fun hostDisplayName(deviceId: String): String {
    val tail = deviceId.takeLast(6).uppercase()
    return "Host $tail"
}

/** Two-tone inline metadata row used across home rows. */
@Composable
fun OrcaMetaRow(
    primary: String,
    secondary: String? = null,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = primary,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (secondary != null) {
            Text(
                "·",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
            )
            Text(
                text = secondary,
                style = MaterialTheme.typography.bodySmall,
                fontFamily = FontFamily.Monospace,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** Empty/error state card, Orca wording style. */
@Composable
fun OrcaNoticeCard(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    tint: Color = MaterialTheme.colorScheme.onSurfaceVariant,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(vertical = 26.dp, horizontal = 12.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            title,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            color = tint,
        )
        Text(
            body,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            lineHeight = 18.sp,
        )
    }
}

/** Simple stat card used in the home hero row (Agents / Agent time / PRs). */
@Composable
fun OrcaStatCard(
    value: String,
    label: String,
    modifier: Modifier = Modifier,
    highlight: Color? = null,
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(10.dp),
        color = MaterialTheme.colorScheme.surface,
        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 9.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Text(
                value,
                style = MaterialTheme.typography.titleLarge.copy(fontSize = 18.sp),
                fontWeight = FontWeight.Bold,
                color = highlight ?: MaterialTheme.colorScheme.onSurface,
            )
            Text(
                label,
                style = MaterialTheme.typography.bodySmall.copy(fontSize = 11.sp),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** Thin divider matching Orca's inset separators. */
@Composable
fun OrcaDivider(modifier: Modifier = Modifier, insetStart: Int = 16) {
    Spacer(
        modifier
            .fillMaxWidth()
            .height(1.dp)
            .padding(start = insetStart.dp)
            .background(MaterialTheme.colorScheme.outline),
    )
}

/** Host status ring: 46dp tile ring used by Orca's host cards. */
@Composable
fun HostStatusTile(
    online: Boolean,
    modifier: Modifier = Modifier,
    size: Int = 46,
) {
    val ring = if (online) OrcaColors.accentGreen else MaterialTheme.colorScheme.outline.copy(alpha = 0.9f)
    Box(
        modifier
            .size(size.dp)
            .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(13.dp)),
        contentAlignment = Alignment.Center,
    ) {
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
                .background(ring, CircleShape),
        )
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

/** 32dp square icon button used in Orca headers. */
@Composable
internal fun OrcaIconButton(
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = Modifier
            .size(32.dp)
            .clip(RoundedCornerShape(6.dp))
            .clickable(onClick = onClick),
        shape = RoundedCornerShape(6.dp),
        color = Color.Transparent,
    ) {
        Box(contentAlignment = Alignment.Center) { content() }
    }
}

/** Inline error strip shared by all Orca screens. */
@Composable
internal fun OrcaInlineError(message: String, modifier: Modifier = Modifier) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(6.dp),
        color = OrcaColors.accentRed.copy(alpha = 0.10f),
        border = androidx.compose.foundation.BorderStroke(1.dp, OrcaColors.accentRed.copy(alpha = 0.5f)),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 9.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            OrcaStatusDot(color = OrcaColors.accentRed, size = 7)
            Text(
                message,
                style = MaterialTheme.typography.bodySmall,
                color = OrcaColors.accentRed,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

fun deviceStatusText(device: DeviceRecord, activeSessions: Int): String =
    when {
        device.revoked -> "已撤销访问"
        device.online && activeSessions > 0 -> "已连接 · $activeSessions 个活动会话"
        device.online -> "已连接"
        else -> "已断开"
    }
