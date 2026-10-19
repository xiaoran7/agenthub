package com.agentplatform.mobile.ui

import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Test

class OrcaComponentsTest {
    @Test
    fun `compactTokens formats boundaries correctly and enforces US decimal point`() {
        val originalLocale = Locale.getDefault()
        try {
            // Emulate a locale that normally uses comma as decimal separator (e.g. Germany/France)
            Locale.setDefault(Locale.GERMANY)

            assertEquals("500", compactTokens(500))
            assertEquals("128k", compactTokens(128_000))
            assertEquals("1.2M", compactTokens(1_200_000))
            assertEquals("12M", compactTokens(12_400_000))
        } finally {
            Locale.setDefault(originalLocale)
        }
    }
}
