package com.agentplatform.mobile.ui

object MotionTokens {
    const val Quick = 140
    const val Standard = 220

    /** Retained for the route transition spec. */
    const val Emphasized = 280
    const val StaggerStep = 45
    const val MaxStagger = 270
}

internal fun staggerDelayMillis(index: Int): Int =
    (index.coerceAtLeast(0) * MotionTokens.StaggerStep).coerceAtMost(MotionTokens.MaxStagger)
