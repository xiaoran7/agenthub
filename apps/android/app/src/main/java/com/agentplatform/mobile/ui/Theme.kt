package com.agentplatform.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// =========================================================================
// AgentHub mobile design tokens
//
// Layout, color and type values are ported from stablyai/orca
// `mobile/src/theme/mobile-theme.ts` (MIT). Upstream ships a single graphite
// theme, so this companion is dark-only as well; a separate light palette was
// a local invention that diluted the visual identity.
// Upstream: https://github.com/stablyai/orca
// =========================================================================

object OrcaColors {
    // Surfaces (upstream `colors.bg*`)
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
    val accentPurple = Color(0xFFA78BFA)

    // Inverted call-to-action (upstream `surfaceBright` in dark mode)
    val surfaceBright = Color(0xFFF5F5F5)
    val onBright = Color(0xFF111111)
}

/**
 * Four-point spacing grid. Every gap in the UI comes from this scale, so
 * padding stays optically aligned across screens.
 */
object OrcaSpace {
    val xxs = 2.dp
    val xs = 4.dp
    val sm = 8.dp
    val md = 12.dp
    val lg = 16.dp
    val xl = 24.dp
    val xxl = 32.dp

    /** Horizontal inset inside a grouped row. */
    val rowH = 14.dp

    /** Vertical inset inside a grouped row. */
    val rowV = 12.dp
}

object OrcaRadius {
    val card = 14.dp
    val button = 6.dp
    val input = 8.dp
    val tile = 12.dp
    val pill = 100.dp
}

object OrcaSize {
    /** Icon tile used by list rows. */
    val tile = 40.dp

    /** Icon button touch target in headers. */
    val iconButton = 40.dp

    /** Visible icon glyph. */
    val icon = 18.dp
}

/**
 * Styles that fall outside the Material 3 roles.
 * Section labels stay sans-serif for Chinese legibility; machine identifiers
 * and key caps use the mono face.
 */
object OrcaType {
    val section =
        TextStyle(
            fontFamily = FontFamily.SansSerif,
            fontWeight = FontWeight.SemiBold,
            fontSize = 12.sp,
            lineHeight = 16.sp,
            letterSpacing = 0.3.sp,
        )
    val rowTitle =
        TextStyle(
            fontFamily = FontFamily.SansSerif,
            fontWeight = FontWeight.SemiBold,
            fontSize = 14.sp,
            lineHeight = 18.sp,
        )
    val mono =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Medium,
            fontSize = 11.sp,
            lineHeight = 14.sp,
        )
    val code =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Normal,
            fontSize = 12.sp,
            lineHeight = 18.sp,
        )
}

private val OrcaColorScheme =
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
        surfaceTint = Color.Transparent,
        outline = OrcaColors.borderSubtle,
        outlineVariant = OrcaColors.borderStrong,
        error = OrcaColors.accentRed,
        onError = Color.White,
    )

// Upstream scale: title 18, body 14, meta 12; weights tightened for a
// squarer, product-grade feel.
private val Typography =
    Typography(
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
                letterSpacing = 0.4.sp,
            ),
    )

@Composable
fun AgentPlatformTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = OrcaColorScheme,
        typography = Typography,
        content = content,
    )
}
