package com.agentplatform.mobile.ui

import androidx.compose.animation.ContentTransform
import androidx.compose.animation.SizeTransform
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally

object MotionTokens {
    const val Quick = 150
    const val Standard = 240

    /** Emphasized duration for full-screen push/pop navigation transitions. */
    const val Emphasized = 280
    const val StaggerStep = 45
    const val MaxStagger = 270

    /** Smooth iOS/Material decelerating curve for screen pushes and pops. */
    val StandardEasing = CubicBezierEasing(0.22f, 1.0f, 0.36f, 1.0f)
    val ExitEasing = CubicBezierEasing(0.4f, 0.0f, 1.0f, 1.0f)
}

internal fun staggerDelayMillis(index: Int): Int =
    (index.coerceAtLeast(0) * MotionTokens.StaggerStep).coerceAtMost(MotionTokens.MaxStagger)

internal enum class RouteTransitionDirection {
    /** Entering a deeper screen: incoming slides in from right, previous shifts left. */
    FORWARD,

    /** Returning to a parent screen: current slides out to right, parent enters from left. */
    BACKWARD,

    /** Lateral or auth state change: gentle crossfade with no misleading horizontal slide. */
    CROSSFADE,
}

/**
 * Total depth ordering for all screens so every push (`from < to`) and every
 * back navigation (`from > to`) resolves to a consistent direction and z-index:
 *
 *   sessions (0) < settings (1) < hosts (2) < launch (3) < session (4)
 */
internal fun routeLayerRank(route: String): Int =
    when (route) {
        DEST_SESSIONS -> 0
        DEST_SETTINGS -> 1
        DEST_HOSTS -> 2
        "launch" -> 3
        "session" -> 4
        else -> -1
    }

internal fun routeTransitionDirection(
    from: String,
    to: String,
    useSideRail: Boolean = false,
): RouteTransitionDirection {
    if (from == to) return RouteTransitionDirection.CROSSFADE
    if (from == "gate" || to == "gate") return RouteTransitionDirection.CROSSFADE
    val topLevel = setOf(DEST_SESSIONS, DEST_HOSTS, DEST_SETTINGS)
    if (useSideRail && from in topLevel && to in topLevel) {
        return RouteTransitionDirection.CROSSFADE
    }
    val fromRank = routeLayerRank(from)
    val toRank = routeLayerRank(to)
    return when {
        fromRank < 0 || toRank < 0 -> RouteTransitionDirection.CROSSFADE
        toRank > fromRank -> RouteTransitionDirection.FORWARD
        toRank < fromRank -> RouteTransitionDirection.BACKWARD
        else -> RouteTransitionDirection.CROSSFADE
    }
}

internal fun screenTransitionSpec(
    from: String,
    to: String,
    useSideRail: Boolean,
): ContentTransform {
    val direction = routeTransitionDirection(from, to, useSideRail)
    val targetZ = routeLayerRank(to).coerceAtLeast(0).toFloat()
    val enterSpec = tween<androidx.compose.ui.unit.IntOffset>(MotionTokens.Emphasized, easing = MotionTokens.StandardEasing)
    val exitSpec = tween<androidx.compose.ui.unit.IntOffset>(MotionTokens.Emphasized, easing = MotionTokens.StandardEasing)
    return when (direction) {
        RouteTransitionDirection.FORWARD -> {
            val enter =
                slideInHorizontally(enterSpec) { fullWidth -> (fullWidth * 0.92f).toInt() } +
                    fadeIn(tween(MotionTokens.Quick, easing = MotionTokens.StandardEasing))
            val exit =
                slideOutHorizontally(exitSpec) { fullWidth -> -(fullWidth * 0.24f).toInt() } +
                    fadeOut(tween(MotionTokens.Standard, easing = MotionTokens.ExitEasing), targetAlpha = 0.85f)
            ContentTransform(
                targetContentEnter = enter,
                initialContentExit = exit,
                targetContentZIndex = targetZ,
                sizeTransform = SizeTransform(clip = false),
            )
        }
        RouteTransitionDirection.BACKWARD -> {
            val enter =
                slideInHorizontally(enterSpec) { fullWidth -> -(fullWidth * 0.24f).toInt() } +
                    fadeIn(tween(MotionTokens.Standard, easing = MotionTokens.StandardEasing), initialAlpha = 0.85f)
            val exit =
                slideOutHorizontally(exitSpec) { fullWidth -> (fullWidth * 0.95f).toInt() } +
                    fadeOut(tween(MotionTokens.Emphasized, easing = MotionTokens.ExitEasing))
            ContentTransform(
                targetContentEnter = enter,
                initialContentExit = exit,
                targetContentZIndex = targetZ,
                sizeTransform = SizeTransform(clip = false),
            )
        }
        RouteTransitionDirection.CROSSFADE -> {
            ContentTransform(
                targetContentEnter = fadeIn(tween(MotionTokens.Standard)),
                initialContentExit = fadeOut(tween(MotionTokens.Quick)),
                targetContentZIndex = targetZ,
                sizeTransform = SizeTransform(clip = false),
            )
        }
    }
}
