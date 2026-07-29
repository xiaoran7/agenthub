package com.agentplatform.mobile.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.TextStyle

internal object MotionTokens {
    const val Quick = 180
    const val Standard = 360
    const val Emphasized = 460
    const val StaggerStep = 45
    const val MaxStagger = 270
}

internal fun staggerDelayMillis(index: Int): Int =
    (index.coerceAtLeast(0) * MotionTokens.StaggerStep).coerceAtMost(MotionTokens.MaxStagger)

@Composable
internal fun MotionEntrance(
    motionKey: Any?,
    modifier: Modifier = Modifier,
    index: Int = 0,
    content: @Composable () -> Unit,
) {
    var visible by remember(motionKey) { mutableStateOf(false) }
    LaunchedEffect(motionKey) { visible = true }
    val delay = staggerDelayMillis(index)

    AnimatedVisibility(
        visible = visible,
        modifier = modifier,
        enter =
            fadeIn(
                animationSpec =
                    tween(
                        durationMillis = MotionTokens.Standard,
                        delayMillis = delay,
                    ),
            ) +
                slideInVertically(
                    animationSpec =
                        tween(
                            durationMillis = MotionTokens.Emphasized,
                            delayMillis = delay,
                        ),
                    initialOffsetY = { fullHeight -> fullHeight.coerceAtMost(36) },
                ),
        exit = ExitTransition.None,
    ) {
        content()
    }
}

@Composable
internal fun MotionSelection(
    active: Boolean,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val scale by
        animateFloatAsState(
            targetValue = if (active) 1f else 0.96f,
            animationSpec = tween(MotionTokens.Quick),
            label = "selection-scale",
        )
    val alpha by
        animateFloatAsState(
            targetValue = if (active) 1f else 0.88f,
            animationSpec = tween(MotionTokens.Quick),
            label = "selection-alpha",
        )
    Box(
        modifier =
            modifier
                .graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                }
                .alpha(alpha),
    ) {
        content()
    }
}

@Composable
internal fun AnimatedMetricText(
    value: String,
    style: TextStyle,
    color: Color = Color.Unspecified,
) {
    AnimatedContent(
        targetState = value,
        transitionSpec = {
            (
                fadeIn(tween(MotionTokens.Standard)) +
                    slideInVertically(tween(MotionTokens.Standard)) { height -> height / 2 }
            ).togetherWith(
                fadeOut(tween(MotionTokens.Quick)) +
                    slideOutVertically(tween(MotionTokens.Quick)) { height -> -height / 3 },
            )
        },
        label = "metric-$value",
    ) { target ->
        Text(target, style = style, color = color)
    }
}
