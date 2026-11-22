package com.agentplatform.mobile.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

// =========================================================================
// Monochrome vector icon primitives (zero icon-font / zero emoji dependency)
// =========================================================================

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

@Composable
fun OrcaCopyIcon(tint: Color, size: Dp = 14.dp) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.5.dp.toPx()
        drawRoundRect(
            color = tint,
            topLeft = Offset(w * 0.32f, h * 0.30f),
            size = Size(w * 0.54f, h * 0.56f),
            cornerRadius = CornerRadius(2.dp.toPx(), 2.dp.toPx()),
            style = Stroke(width = stroke),
        )
        val back = Path().apply {
            moveTo(w * 0.24f, h * 0.68f)
            lineTo(w * 0.16f, h * 0.68f)
            lineTo(w * 0.16f, h * 0.16f)
            lineTo(w * 0.66f, h * 0.16f)
            lineTo(w * 0.66f, h * 0.24f)
        }
        drawPath(back, color = tint, style = Stroke(width = stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}

@Composable
fun OrcaQueueIcon(tint: Color, size: Dp = 15.dp) {
    Canvas(modifier = Modifier.size(size)) {
        val w = this.size.width
        val h = this.size.height
        val stroke = 1.6.dp.toPx()
        drawLine(tint, Offset(w * 0.14f, h * 0.26f), Offset(w * 0.86f, h * 0.26f), stroke, StrokeCap.Round)
        drawLine(tint, Offset(w * 0.14f, h * 0.52f), Offset(w * 0.62f, h * 0.52f), stroke, StrokeCap.Round)
        drawLine(tint, Offset(w * 0.14f, h * 0.78f), Offset(w * 0.54f, h * 0.78f), stroke, StrokeCap.Round)
        drawLine(tint, Offset(w * 0.78f, h * 0.56f), Offset(w * 0.78f, h * 0.88f), stroke, StrokeCap.Round)
        drawLine(tint, Offset(w * 0.62f, h * 0.72f), Offset(w * 0.94f, h * 0.72f), stroke, StrokeCap.Round)
    }
}
