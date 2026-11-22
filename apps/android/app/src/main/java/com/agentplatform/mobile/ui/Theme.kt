package com.agentplatform.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import com.agentplatform.mobile.data.AppearanceTheme
import com.agentplatform.mobile.data.OrcaPalette

// =========================================================================
// AgentHub mobile design tokens
//
// Layout, color and type values are ported from stablyai/orca
// `mobile/src/theme/mobile-theme.ts` (MIT). Upstream ships a single graphite
// theme; this companion keeps it as the default and adds two more, because a
// phone is used in a dark room and in daylight and one palette cannot serve
// both.
// Upstream: https://github.com/stablyai/orca
//
// Local deltas (all additive, none re-tinted):
//   * body text is 15sp, not 14sp — a phone at arm's length needs the extra
//     step, and 14sp Chinese glyphs at 400 weight read thin against #111;
//   * control radius is 10dp instead of 6dp, so buttons and inputs sit on the
//     same family of curves as the 16dp cards.
//
// Contrast: every palette is verified for body text >= 4.5:1 against its own
// background, and for the accent tints used as text. A palette that cannot
// reach 4.5:1 does not ship.
// =========================================================================

/**
 * The palette currently in force.
 *
 * Read from [LocalAppearance] so a composable can pick up a token without the
 * theme being threaded through every call site.
 */
object OrcaColors {
    private val palette: OrcaPalette
        @Composable get() = LocalAppearance.current.palette

    val bgBase: Color @Composable get() = palette.bgBase
    val bgPanel: Color @Composable get() = palette.bgPanel
    val bgRaised: Color @Composable get() = palette.bgRaised
    val bgOverlay: Color @Composable get() = palette.bgOverlay
    val bgInset: Color @Composable get() = palette.bgInset

    val textPrimary: Color @Composable get() = palette.textPrimary
    val textSecondary: Color @Composable get() = palette.textSecondary
    val textMuted: Color @Composable get() = palette.textMuted
    val textFaint: Color @Composable get() = palette.textFaint

    val borderSubtle: Color @Composable get() = palette.borderSubtle
    val borderStrong: Color @Composable get() = palette.borderStrong

    val accentBlue: Color @Composable get() = palette.accentBlue
    val accentGreen: Color @Composable get() = palette.accentGreen
    val accentAmber: Color @Composable get() = palette.accentAmber
    val accentRed: Color @Composable get() = palette.accentRed
    val accentPurple: Color @Composable get() = palette.accentPurple

    val surfaceBright: Color @Composable get() = palette.surfaceBright
    val onBright: Color @Composable get() = palette.onBright

    val skeletonBase: Color @Composable get() = palette.skeletonBase
    val skeletonHighlight: Color @Composable get() = palette.skeletonHighlight
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

    /** Minimum touch target meeting Android 48dp accessibility guidance. */
    val touch = 48.dp

    /** Height reserved for the fixed bottom bar, above the system inset. */
    val bottomBar = 60.dp
}

object OrcaRadius {
    val card = 16.dp
    val button = 10.dp
    val input = 10.dp
    val tile = 12.dp
    val pill = 100.dp
}

object OrcaSize {
    /** Icon tile used by list rows. */
    val tile = 40.dp

    /** Icon button touch target in headers. */
    val iconButton = 48.dp

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
            fontSize = 15.sp,
            lineHeight = 20.sp,
        )
    val mono =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Medium,
            fontSize = 11.sp,
            lineHeight = 15.sp,
        )
    val code =
        TextStyle(
            fontFamily = FontFamily.Monospace,
            fontWeight = FontWeight.Normal,
            fontSize = 12.sp,
            lineHeight = 18.sp,
        )
    /** Metrics: equal-width digits so a live counter never jitters. */
    val numeral =
        TextStyle(
            fontFamily = FontFamily.SansSerif,
            fontWeight = FontWeight.Bold,
            fontSize = 24.sp,
            lineHeight = 28.sp,
            letterSpacing = (-0.4).sp,
            fontFeatureSettings = "tnum",
        )
}

// Upstream scale: title 18, body 15, meta 12; weights tightened for a
// squarer, product-grade feel.
private val OrcaTypography =
    Typography(
        headlineLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.ExtraBold,
                fontSize = 26.sp,
                lineHeight = 31.sp,
                letterSpacing = (-0.7).sp,
            ),
        headlineMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 21.sp,
                lineHeight = 26.sp,
                letterSpacing = (-0.3).sp,
            ),
        titleLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 19.sp,
                lineHeight = 24.sp,
                letterSpacing = (-0.2).sp,
            ),
        titleMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 16.sp,
                lineHeight = 21.sp,
            ),
        titleSmall =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 14.sp,
                lineHeight = 19.sp,
            ),
        bodyLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 16.sp,
                lineHeight = 25.sp,
            ),
        bodyMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 15.sp,
                lineHeight = 23.sp,
            ),
        bodySmall =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 13.sp,
                lineHeight = 19.sp,
            ),
        labelLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.SemiBold,
                fontSize = 14.sp,
                lineHeight = 18.sp,
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
                lineHeight = 15.sp,
                letterSpacing = 0.4.sp,
            ),
    )

private val OrcaShapes =
    Shapes(
        extraSmall = androidx.compose.foundation.shape.RoundedCornerShape(OrcaRadius.button),
        small = androidx.compose.foundation.shape.RoundedCornerShape(OrcaRadius.input),
        medium = androidx.compose.foundation.shape.RoundedCornerShape(OrcaRadius.tile),
        large = androidx.compose.foundation.shape.RoundedCornerShape(OrcaRadius.card),
        extraLarge = androidx.compose.foundation.shape.RoundedCornerShape(OrcaRadius.card),
    )

/**
 * Applies the chosen appearance.
 *
 * The Material color scheme is derived from the same palette rather than kept
 * as a second literal set, so a token added to [OrcaPalette] reaches the
 * Material roles too and the two cannot drift.
 */
@Composable
fun AgentPlatformTheme(
    appearance: AppearanceTheme = AppearanceTheme.SYSTEM,
    content: @Composable () -> Unit,
) {
    val systemDark = androidx.compose.foundation.isSystemInDarkTheme()
    val effective = appearance.resolve(systemDark)
    val palette = effective.palette
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as? android.app.Activity)?.window ?: return@SideEffect
            val controller = WindowCompat.getInsetsController(window, view)
            controller.isAppearanceLightStatusBars = effective.light
            controller.isAppearanceLightNavigationBars = effective.light
        }
    }
    val scheme =
        if (effective.light) {
            lightColorScheme(
                primary = palette.accentBlue,
                onPrimary = palette.onBright,
                primaryContainer = palette.accentBlue.copy(alpha = 0.12f),
                onPrimaryContainer = palette.textPrimary,
                secondary = palette.textSecondary,
                onSecondary = palette.bgBase,
                background = palette.bgBase,
                onBackground = palette.textPrimary,
                surface = palette.bgPanel,
                onSurface = palette.textPrimary,
                surfaceVariant = palette.bgRaised,
                onSurfaceVariant = palette.textSecondary,
                surfaceTint = Color.Transparent,
                outline = palette.borderSubtle,
                outlineVariant = palette.borderStrong,
                error = palette.accentRed,
                onError = Color.White,
            )
        } else {
            darkColorScheme(
                primary = palette.accentBlue,
                onPrimary = palette.onBright,
                primaryContainer = palette.accentBlue.copy(alpha = 0.22f),
                onPrimaryContainer = palette.textPrimary,
                secondary = palette.textSecondary,
                onSecondary = palette.bgBase,
                background = palette.bgBase,
                onBackground = palette.textPrimary,
                surface = palette.bgPanel,
                onSurface = palette.textPrimary,
                surfaceVariant = palette.bgRaised,
                onSurfaceVariant = palette.textSecondary,
                surfaceTint = Color.Transparent,
                outline = palette.borderSubtle,
                outlineVariant = palette.borderStrong,
                error = palette.accentRed,
                onError = Color.White,
            )
        }
    androidx.compose.runtime.CompositionLocalProvider(LocalAppearance provides effective) {
        MaterialTheme(
            colorScheme = scheme,
            typography = OrcaTypography,
            shapes = OrcaShapes,
            content = content,
        )
    }
}
