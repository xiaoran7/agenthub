package com.agentplatform.mobile.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.minimumInteractiveComponentSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// =========================================================================
// Shell navigation bars, metrics, flow-wrapping pills & skeleton primitives
// =========================================================================

@Composable
fun StatusPill(
    label: String,
    color: Color,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(OrcaRadius.pill),
        color = color.copy(alpha = 0.14f),
        border = BorderStroke(1.dp, color.copy(alpha = 0.45f)),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = OrcaSpace.sm, vertical = 3.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        ) {
            OrcaStatusDot(color, 5.dp)
            Text(
                text = label,
                style = OrcaType.mono,
                color = color,
                maxLines = 1,
            )
        }
    }
}

@Composable
fun Metric(
    value: String,
    label: String,
    modifier: Modifier = Modifier,
    color: Color? = null,
) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(OrcaSpace.xxs)) {
        Text(
            text = value,
            style = OrcaType.numeral,
            color = color ?: MaterialTheme.colorScheme.onSurface,
            maxLines = 1,
        )
        Text(
            text = label,
            style = OrcaType.mono,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
fun SkeletonRow(modifier: Modifier = Modifier) {
    val transition = rememberInfiniteTransition(label = "skeleton")
    val shift by
        transition.animateFloat(
            initialValue = -400f,
            targetValue = 800f,
            animationSpec = infiniteRepeatable(tween(1_500), RepeatMode.Restart),
            label = "skeleton-shift",
        )
    Box(
        modifier
            .fillMaxWidth()
            .height(OrcaSpace.touch)
            .clip(RoundedCornerShape(OrcaRadius.button))
            .background(
                Brush.linearGradient(
                    colors =
                        listOf(
                            OrcaColors.skeletonBase,
                            OrcaColors.skeletonHighlight,
                            OrcaColors.skeletonBase,
                        ),
                    start = Offset(shift, 0f),
                    end = Offset(shift + 320f, 0f),
                ),
            ),
    )
}

@Composable
fun SkeletonBlock(height: Dp, modifier: Modifier = Modifier) {
    val transition = rememberInfiniteTransition(label = "skeleton-block")
    val shift by
        transition.animateFloat(
            initialValue = -400f,
            targetValue = 1_200f,
            animationSpec = infiniteRepeatable(tween(1_700), RepeatMode.Restart),
            label = "skeleton-block-shift",
        )
    Box(
        modifier
            .fillMaxWidth()
            .height(height)
            .clip(RoundedCornerShape(OrcaRadius.card))
            .background(
                Brush.linearGradient(
                    colors =
                        listOf(
                            OrcaColors.skeletonBase,
                            OrcaColors.skeletonHighlight,
                            OrcaColors.skeletonBase,
                        ),
                    start = Offset(shift, 0f),
                    end = Offset(shift + 480f, 0f),
                ),
            ),
    )
}

@Composable
fun EmptyPanel(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    icon: (@Composable () -> Unit)? = null,
    action: (@Composable () -> Unit)? = null,
) {
    Column(
        modifier =
            modifier
                .fillMaxWidth()
                .padding(horizontal = OrcaSpace.xl, vertical = OrcaSpace.xxl),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
    ) {
        icon?.invoke()
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurface,
        )
        Text(
            text = body,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        action?.invoke()
    }
}

data class BottomDestination(
    val id: String,
    val label: String,
    val badge: Int = 0,
)

/**
 * Compact phone bottom bar (`heightIn(min = 60.dp)` so system `fontScale` up to
 * 2.0x expands cleanly without clipping labels, plus horizontal cutout insets).
 */
@Composable
fun BottomBar(
    destinations: List<BottomDestination>,
    current: String,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        color = OrcaColors.bgPanel,
        contentColor = MaterialTheme.colorScheme.onSurface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .navigationBarsPadding()
                    .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Horizontal))
                    .heightIn(min = OrcaSpace.bottomBar)
                    .padding(vertical = OrcaSpace.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            destinations.forEach { destination ->
                val active = destination.id == current
                val tint =
                    if (active) OrcaColors.accentBlue
                    else MaterialTheme.colorScheme.onSurfaceVariant
                Column(
                    modifier =
                        Modifier
                            .weight(1f)
                            .heightIn(min = OrcaSpace.touch)
                            .clip(RoundedCornerShape(OrcaRadius.button))
                            .clickable { onSelect(destination.id) }
                            .padding(vertical = 2.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    Box(contentAlignment = Alignment.TopEnd) {
                        Box(Modifier.padding(horizontal = 10.dp, vertical = 2.dp)) {
                            BottomGlyph(id = destination.id, tint = tint)
                        }
                        if (destination.badge > 0) {
                            DestinationBadge(count = destination.badge)
                        }
                    }
                    Text(
                        text = destination.label,
                        style = OrcaType.mono,
                        color = tint,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
    }
}

/**
 * Adaptive vertical navigation rail for tablets, foldables, and wide landscape
 * viewports (`>= 720.dp`), preserving vertical screen real estate.
 */
@Composable
fun SideNavigationRail(
    destinations: List<BottomDestination>,
    current: String,
    onSelect: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.fillMaxHeight().width(84.dp),
        color = OrcaColors.bgPanel,
        contentColor = MaterialTheme.colorScheme.onSurface,
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
    ) {
        Column(
            modifier =
                Modifier
                    .fillMaxHeight()
                    .statusBarsPadding()
                    .navigationBarsPadding()
                    .windowInsetsPadding(WindowInsets.displayCutout.only(WindowInsetsSides.Start))
                    .padding(vertical = OrcaSpace.lg, horizontal = OrcaSpace.xs),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(OrcaSpace.sm),
        ) {
            OrcaLogo(size = 20.dp, tint = MaterialTheme.colorScheme.onSurface)
            Box(Modifier.height(OrcaSpace.sm))
            destinations.forEach { destination ->
                val active = destination.id == current
                val tint =
                    if (active) OrcaColors.accentBlue
                    else MaterialTheme.colorScheme.onSurfaceVariant
                Surface(
                    onClick = { onSelect(destination.id) },
                    modifier = Modifier.fillMaxWidth().heightIn(min = 54.dp),
                    shape = RoundedCornerShape(OrcaRadius.tile),
                    color = if (active) OrcaColors.accentBlue.copy(alpha = 0.14f) else Color.Transparent,
                ) {
                    Column(
                        modifier = Modifier.padding(vertical = 6.dp, horizontal = 4.dp),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Box(contentAlignment = Alignment.TopEnd) {
                            Box(Modifier.padding(horizontal = 10.dp, vertical = 2.dp)) {
                                BottomGlyph(id = destination.id, tint = tint)
                            }
                            if (destination.badge > 0) {
                                DestinationBadge(count = destination.badge)
                            }
                        }
                        Text(
                            text = destination.label,
                            style = OrcaType.mono,
                            color = tint,
                            maxLines = 1,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun DestinationBadge(count: Int) {
    Surface(
        shape = CircleShape,
        color = OrcaColors.accentAmber,
    ) {
        Text(
            text = if (count > 99) "99+" else "$count",
            style = OrcaType.mono.copy(fontSize = 9.sp, fontWeight = FontWeight.Bold),
            color = OrcaColors.onBright,
            modifier = Modifier.padding(horizontal = 5.dp, vertical = 1.dp),
            maxLines = 1,
        )
    }
}

@Composable
private fun BottomGlyph(id: String, tint: Color) {
    when (id) {
        "sessions" -> OrcaTerminalIcon(tint, 19.dp)
        "hosts" -> OrcaMonitorIcon(tint, 19.dp)
        else -> OrcaSettingsIcon(tint, 19.dp)
    }
}

@Composable
fun OrcaFilterPill(
    label: String,
    active: Boolean,
    onClick: () -> Unit,
    enabled: Boolean = true,
) {
    val tint =
        if (active) OrcaColors.accentBlue
        else MaterialTheme.colorScheme.onSurfaceVariant
    Surface(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier.minimumInteractiveComponentSize().heightIn(min = 36.dp),
        shape = RoundedCornerShape(OrcaRadius.pill),
        color = if (active) OrcaColors.accentBlue.copy(alpha = 0.16f) else Color.Transparent,
        border =
            BorderStroke(
                1.dp,
                if (active) OrcaColors.accentBlue.copy(alpha = 0.6f)
                else MaterialTheme.colorScheme.outline,
            ),
    ) {
        Box(
            modifier = Modifier.padding(horizontal = OrcaSpace.md, vertical = 6.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = label,
                style = OrcaType.mono,
                color = if (enabled) tint else tint.copy(alpha = 0.38f),
                maxLines = 1,
            )
        }
    }
}

/** Flow-wrapping pill selector so narrow 360dp phones & large fontScale never clip. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun <T> FilterRow(
    options: List<Pair<T, String>>,
    selected: T,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    FlowRow(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
        verticalArrangement = Arrangement.spacedBy(OrcaSpace.xs),
    ) {
        options.forEach { (value, label) ->
            OrcaFilterPill(
                label = label,
                active = value == selected,
                onClick = { onSelect(value) },
                enabled = enabled,
            )
        }
    }
}

@Composable
fun KeyCap(
    label: String,
    enabled: Boolean,
    danger: Boolean = false,
    onClick: () -> Unit,
) {
    val tint = if (danger) OrcaColors.accentRed else MaterialTheme.colorScheme.onSurface
    Surface(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier.minimumInteractiveComponentSize(),
        shape = RoundedCornerShape(OrcaRadius.button),
        color =
            if (danger) OrcaColors.accentRed.copy(alpha = 0.10f)
            else MaterialTheme.colorScheme.surfaceVariant,
        border =
            BorderStroke(
                1.dp,
                if (danger) OrcaColors.accentRed.copy(alpha = 0.45f)
                else MaterialTheme.colorScheme.outline,
            ),
    ) {
        Row(
            modifier = Modifier.heightIn(min = 38.dp).padding(horizontal = OrcaSpace.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(text = label, style = OrcaType.mono, color = tint, maxLines = 1)
        }
    }
}

@Composable
fun MetricStrip(
    metrics: List<Triple<String, String, Color?>>,
    modifier: Modifier = Modifier,
) {
    OrcaCard(modifier) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(vertical = OrcaSpace.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            metrics.forEachIndexed { index, (value, label, color) ->
                if (index > 0) {
                    Box(
                        Modifier
                            .width(1.dp)
                            .height(28.dp)
                            .background(MaterialTheme.colorScheme.outline),
                    )
                }
                Metric(
                    value = value,
                    label = label,
                    modifier = Modifier.weight(1f).padding(horizontal = OrcaSpace.md),
                    color = color,
                )
            }
        }
    }
}
