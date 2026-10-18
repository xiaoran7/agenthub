package com.agentplatform.mobile.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

// =========================================================================
// Aozora UI & Orca Mission Control Design System Palette
// 70% Pro Cloud Architecture + 20% Modern Clean + 10% Precision Console
// =========================================================================

// Light theme: crisp cool-white, subtle slate lines, Sky Blue accent
val LightBgPrimary = Color(0xFFF8FAFC)
val LightBgSurface = Color(0xFFFFFFFF)
val LightBgSurfaceVariant = Color(0xFFF1F5F9)
val LightTextPrimary = Color(0xFF0F172A)
val LightTextSecondary = Color(0xFF334155)
val LightTextMuted = Color(0xFF64748B)
val LightBorderSubtle = Color(0xFFE2E8F0)
val LightBorderContrast = Color(0xFFCBD5E1)
val LightBrandAccent = Color(0xFF0284C7)

// Dark theme: deep mission-control void, subtle borders, Cyan accent
val DarkBgPrimary = Color(0xFF090D16)
val DarkBgSurface = Color(0xFF111827)
val DarkBgSurfaceVariant = Color(0xFF0F172A)
val DarkTextPrimary = Color(0xFFF8FAFC)
val DarkTextSecondary = Color(0xFFCBD5E1)
val DarkTextMuted = Color(0xFF94A3B8)
val DarkBorderSubtle = Color(0xFF1E293B)
val DarkBorderContrast = Color(0xFF334155)
val DarkBrandAccent = Color(0xFF38BDF8)

// Semantic status colors across themes
val StatusBlue = Color(0xFF0284C7)
val StatusAmber = Color(0xFFF59E0B)
val StatusGreen = Color(0xFF059669)
val StatusGreenBright = Color(0xFF10B981)
val StatusRed = Color(0xFFDC2626)
val StatusRedBright = Color(0xFFEF4444)

// Legacy color aliases for backwards compatibility
val Paper = LightBgPrimary
val Surface = LightBgSurface
val Ink = LightTextPrimary
val Muted = LightTextMuted
val Line = LightBorderSubtle
val PinkSignal = LightBrandAccent
val SoftPink = Color(0xFFF0F9FF)
val WorkingBlue = StatusBlue
val WaitingAmber = StatusAmber
val SuccessGreen = StatusGreen
val DangerRed = StatusRed

private val LightColors =
    lightColorScheme(
        primary = LightBrandAccent,
        onPrimary = Color.White,
        primaryContainer = Color(0xFFE0F2FE),
        onPrimaryContainer = Color(0xFF0369A1),
        secondary = LightTextSecondary,
        onSecondary = Color.White,
        background = LightBgPrimary,
        onBackground = LightTextPrimary,
        surface = LightBgSurface,
        onSurface = LightTextPrimary,
        surfaceVariant = LightBgSurfaceVariant,
        onSurfaceVariant = LightTextMuted,
        outline = LightBorderSubtle,
        outlineVariant = LightBorderContrast,
        error = StatusRed,
        onError = Color.White,
    )

private val DarkColors =
    darkColorScheme(
        primary = DarkBrandAccent,
        onPrimary = Color(0xFF090D16),
        primaryContainer = Color(0xFF0C4A6E),
        onPrimaryContainer = Color(0xFFBAE6FD),
        secondary = DarkTextSecondary,
        onSecondary = Color(0xFF090D16),
        background = DarkBgPrimary,
        onBackground = DarkTextPrimary,
        surface = DarkBgSurface,
        onSurface = DarkTextPrimary,
        surfaceVariant = DarkBgSurfaceVariant,
        onSurfaceVariant = DarkTextMuted,
        outline = DarkBorderSubtle,
        outlineVariant = DarkBorderContrast,
        error = StatusRedBright,
        onError = Color.White,
    )

private val Typography =
    androidx.compose.material3.Typography(
        displayLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 32.sp,
                lineHeight = 38.sp,
                letterSpacing = (-0.8).sp,
            ),
        headlineLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 24.sp,
                lineHeight = 30.sp,
                letterSpacing = (-0.5).sp,
            ),
        headlineMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 20.sp,
                lineHeight = 26.sp,
                letterSpacing = (-0.3).sp,
            ),
        titleLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 17.sp,
                lineHeight = 23.sp,
                letterSpacing = (-0.2).sp,
            ),
        titleMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Medium,
                fontSize = 15.sp,
                lineHeight = 21.sp,
            ),
        titleSmall =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 13.sp,
                lineHeight = 18.sp,
            ),
        bodyLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 15.sp,
                lineHeight = 22.sp,
            ),
        bodyMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 13.sp,
                lineHeight = 19.sp,
            ),
        bodySmall =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 12.sp,
                lineHeight = 16.sp,
            ),
        labelLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 13.sp,
                lineHeight = 17.sp,
            ),
        labelMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Medium,
                fontSize = 12.sp,
                lineHeight = 16.sp,
            ),
        labelSmall =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Medium,
                fontSize = 10.5.sp,
                lineHeight = 14.sp,
                letterSpacing = 0.5.sp,
            ),
    )

@Composable
fun AgentPlatformTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        typography = Typography,
        content = content,
    )
}
