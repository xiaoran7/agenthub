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
// Orca Mobile Design Tokens
//
// Ported from stablyai/orca `mobile/src/theme/mobile-theme.ts`.
// Upstream: https://github.com/stablyai/orca (MIT / Apache-2.0)
// Token values below intentionally mirror upstream so the AgentHub companion
// stays visually interchangeable with the Orca mobile app.
// =========================================================================

object OrcaColors {
    // Dark surfaces (upstream `colors.bg*`)
    val bgBase = Color(0xFF111111)
    val bgPanel = Color(0xFF1A1A1A)
    val bgRaised = Color(0xFF242424)
    val bgOverlay = Color(0xFF2A2A2A)
    val bgInset = Color(0xFF1E1E1E)

    // Text (upstream `colors.text*`)
    val textPrimary = Color(0xFFE0E0E0)
    val textSecondary = Color(0xFFA1A1A1)
    val textMuted = Color(0xFF8C8C8C)
    val textFaint = Color(0xFF555555)

    // Borders
    val borderSubtle = Color(0xFF2A2A2A)
    val borderStrong = Color(0xFF3A3A3A)

    // Accents (upstream `colors.accent*` / status)
    val accentBlue = Color(0xFF3B82F6)
    val accentGreen = Color(0xFF22C55E)
    val accentAmber = Color(0xFFF59E0B)
    val accentRed = Color(0xFFEF4444)
    val accentMerge = Color(0xFF16A34A)
    val accentPurple = Color(0xFFA78BFA)
    val accentOrange = Color(0xFFF97316)

    // Bright surface (used for primary CTA fill in dark mode)
    val surfaceBright = Color(0xFFF5F5F5)
    val onBrightPrimary = Color(0xFF111111)
    val onBrightSecondary = Color(0xFF4A4A4A)

    // Git / diff (upstream)
    val diffAdded = Color(0xFF81B88B)
    val diffDeleted = Color(0xFFC74E39)
    val chatDiffAdded = Color(0xFFA5CCAC)
    val chatDiffRemoved = Color(0xFFD68273)

    // Terminal
    val terminalBg = Color(0xFF1A1B26)

    // Provider identity dots (Orca agent icons)
    val providerClaude = Color(0xFFC86646)
    val providerCodex = Color(0xFF3B82F6)
    val providerAntigravity = Color(0xFF22C55E)
    val providerGrok = Color(0xFFA78BFA)
    val providerUnknown = Color(0xFF8C8C8C)

    // Light surfaces: Orca ships dark-first, but AgentHub honors the system theme.
    val lightBgBase = Color(0xFFF7F7F7)
    val lightBgPanel = Color(0xFFFFFFFF)
    val lightBgRaised = Color(0xFFEFEFEF)
    val lightBgInset = Color(0xFFE8E8E8)
    val lightTextPrimary = Color(0xFF141414)
    val lightTextSecondary = Color(0xFF4A4A4A)
    val lightTextMuted = Color(0xFF6B6B6B)
    val lightTextFaint = Color(0xFF9A9A9A)
    val lightBorderSubtle = Color(0xFFE2E2E2)
    val lightBorderStrong = Color(0xFFCACACA)
    val lightAccentBlue = Color(0xFF2563EB)
}

object OrcaSpacing {
    val xs = 4
    val sm = 8
    val md = 12
    val lg = 16
    val xl = 24
}

object OrcaRadii {
    val row = 6
    val card = 14
    val button = 6
    val input = 6
}

// Legacy aliases kept so the rest of the app (and tests) still compile.
val LightBgPrimary = OrcaColors.lightBgBase
val LightBgSurface = OrcaColors.lightBgPanel
val LightBgSurfaceVariant = OrcaColors.lightBgRaised
val LightTextPrimary = OrcaColors.lightTextPrimary
val LightTextSecondary = OrcaColors.lightTextSecondary
val LightTextMuted = OrcaColors.lightTextMuted
val LightBorderSubtle = OrcaColors.lightBorderSubtle
val LightBorderContrast = OrcaColors.lightBorderStrong
val LightBrandAccent = OrcaColors.lightAccentBlue

val DarkBgPrimary = OrcaColors.bgBase
val DarkBgSurface = OrcaColors.bgPanel
val DarkBgSurfaceVariant = OrcaColors.bgRaised
val DarkTextPrimary = OrcaColors.textPrimary
val DarkTextSecondary = OrcaColors.textSecondary
val DarkTextMuted = OrcaColors.textMuted
val DarkBorderSubtle = OrcaColors.borderSubtle
val DarkBorderContrast = OrcaColors.borderStrong
val DarkBrandAccent = OrcaColors.accentBlue

val StatusBlue = OrcaColors.accentBlue
val StatusAmber = OrcaColors.accentAmber
val StatusGreen = OrcaColors.accentMerge
val StatusGreenBright = OrcaColors.accentGreen
val StatusRed = OrcaColors.accentRed
val StatusRedBright = OrcaColors.accentRed
val StatusPurple = OrcaColors.accentPurple

private val LightColors =
    lightColorScheme(
        primary = OrcaColors.lightAccentBlue,
        onPrimary = Color.White,
        primaryContainer = Color(0xFFE4EDFD),
        onPrimaryContainer = Color(0xFF1D4ED8),
        secondary = OrcaColors.lightTextSecondary,
        onSecondary = Color.White,
        background = OrcaColors.lightBgBase,
        onBackground = OrcaColors.lightTextPrimary,
        surface = OrcaColors.lightBgPanel,
        onSurface = OrcaColors.lightTextPrimary,
        surfaceVariant = OrcaColors.lightBgRaised,
        onSurfaceVariant = OrcaColors.lightTextSecondary,
        outline = OrcaColors.lightBorderSubtle,
        outlineVariant = OrcaColors.lightBorderStrong,
        error = OrcaColors.accentRed,
        onError = Color.White,
    )

private val DarkColors =
    darkColorScheme(
        primary = OrcaColors.accentBlue,
        onPrimary = Color.White,
        primaryContainer = Color(0xFF1E3A5F),
        onPrimaryContainer = Color(0xFFBFDBFE),
        secondary = OrcaColors.textSecondary,
        onSecondary = OrcaColors.bgBase,
        background = OrcaColors.bgBase,
        onBackground = OrcaColors.textPrimary,
        surface = OrcaColors.bgPanel,
        onSurface = OrcaColors.textPrimary,
        surfaceVariant = OrcaColors.bgRaised,
        onSurfaceVariant = OrcaColors.textSecondary,
        outline = OrcaColors.borderSubtle,
        outlineVariant = OrcaColors.borderStrong,
        error = OrcaColors.accentRed,
        onError = Color.White,
    )

// Upstream tokens: title 18, body 14, meta 12, mono family.
private val Typography =
    androidx.compose.material3.Typography(
        displayLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.ExtraBold,
                fontSize = 32.sp,
                lineHeight = 36.sp,
                letterSpacing = (-0.8).sp,
            ),
        headlineLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.ExtraBold,
                fontSize = 24.sp,
                lineHeight = 28.sp,
                letterSpacing = (-0.6).sp,
            ),
        headlineMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 20.sp,
                lineHeight = 25.sp,
                letterSpacing = (-0.3).sp,
            ),
        titleLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 18.sp,
                lineHeight = 23.sp,
                letterSpacing = (-0.2).sp,
            ),
        titleMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 15.sp,
                lineHeight = 20.sp,
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
                fontSize = 14.sp,
                lineHeight = 21.sp,
            ),
        bodyMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 14.sp,
                lineHeight = 20.sp,
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
                fontFamily = FontFamily.Monospace,
                fontWeight = FontWeight.Medium,
                fontSize = 11.sp,
                lineHeight = 14.sp,
                letterSpacing = 0.6.sp,
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
