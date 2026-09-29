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
// Claude/Anthropic Design System Palette (Warm Neutrals & Refined Accents)
// =========================================================================

// Light theme warm cream/terracotta tokens
val LightBgPrimary = Color(0xFFFAF9F5)
val LightBgSurface = Color(0xFFFFFFFF)
val LightBgSurfaceVariant = Color(0xFFF0EEE6)
val LightTextPrimary = Color(0xFF1F1E1B)
val LightTextSecondary = Color(0xFF383530)
val LightTextMuted = Color(0xFF6B6860)
val LightBorderSubtle = Color(0xFFE5E3DC)
val LightBorderContrast = Color(0xFFD0CDC3)
val LightBrandAccent = Color(0xFFC2410C)

// Dark theme charcoal/amber tokens
val DarkBgPrimary = Color(0xFF1F1E1D)
val DarkBgSurface = Color(0xFF262523)
val DarkBgSurfaceVariant = Color(0xFF181716)
val DarkTextPrimary = Color(0xFFF5F4EF)
val DarkTextSecondary = Color(0xFFD6D4CC)
val DarkTextMuted = Color(0xFFA8A69E)
val DarkBorderSubtle = Color(0xFF3D3B37)
val DarkBorderContrast = Color(0xFF605C55)
val DarkBrandAccent = Color(0xFFD97706)

// Semantic status colors across themes
val StatusBlue = Color(0xFF2563EB)
val StatusAmber = Color(0xFFD97706)
val StatusGreen = Color(0xFF17683D)
val StatusGreenBright = Color(0xFF10B981)
val StatusRed = Color(0xFFB42318)
val StatusRedBright = Color(0xFFEF4444)

// Legacy color aliases for backwards compatibility
val Paper = LightBgPrimary
val Surface = LightBgSurface
val Ink = LightTextPrimary
val Muted = LightTextMuted
val Line = LightBorderSubtle
val PinkSignal = LightBrandAccent
val SoftPink = Color(0xFFFFF7ED)
val WorkingBlue = StatusBlue
val WaitingAmber = StatusAmber
val SuccessGreen = StatusGreen
val DangerRed = StatusRed

private val LightColors =
    lightColorScheme(
        primary = LightBrandAccent,
        onPrimary = Color.White,
        primaryContainer = Color(0xFFFFEDE5),
        onPrimaryContainer = Color(0xFF7C2D12),
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
        onPrimary = Color(0xFF151514),
        primaryContainer = Color(0xFF452405),
        onPrimaryContainer = Color(0xFFFFDDBA),
        secondary = DarkTextSecondary,
        onSecondary = Color(0xFF151514),
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
