package com.agentplatform.mobile.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class MotionTest {
    @Test
    fun `stagger delay is bounded for long and invalid lists`() {
        assertEquals(0, staggerDelayMillis(-1))
        assertEquals(0, staggerDelayMillis(0))
        assertEquals(90, staggerDelayMillis(2))
        assertEquals(MotionTokens.MaxStagger, staggerDelayMillis(100))
    }
}
