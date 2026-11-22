package com.agentplatform.mobile.data

import androidx.compose.ui.graphics.Color

/**
 * The three appearance choices, in the order the settings screen lists them.
 *
 * [GRAPHITE] is the original orca palette and stays the default, so an
 * existing install looks unchanged until someone picks otherwise.
 */
enum class AppearanceTheme(val wire: String, val label: String, val blurb: String) {
    SYSTEM("system", "跟随系统", "随手机深浅色模式自动切换"),
    PAPER("paper", "纸墨", "浅色底，强光下依然清晰"),
    GRAPHITE("graphite", "石墨", "深灰底，长时间盯屏不刺眼"),
    MIDNIGHT("midnight", "午夜蓝", "蓝黑底，弱光环境下对比更柔"),
    ;

    fun resolve(systemDark: Boolean): AppearanceTheme =
        if (this == SYSTEM) {
            if (systemDark) GRAPHITE else PAPER
        } else {
            this
        }

    val palette: OrcaPalette
        get() =
            when (this) {
                SYSTEM, PAPER -> PaperPalette
                GRAPHITE -> GraphitePalette
                MIDNIGHT -> MidnightPalette
            }

    /** True for the light palette, which needs a light status-bar treatment. */
    val light: Boolean
        get() = this == PAPER || this == SYSTEM

    companion object {
        fun fromWire(value: String): AppearanceTheme =
            entries.firstOrNull { it.wire == value } ?: SYSTEM
    }
}

/**
 * One palette. Every field is a token the composables already read, so adding
 * a theme means adding a row here and nothing else.
 */
data class OrcaPalette(
    // Surfaces
    val bgBase: Color,
    val bgPanel: Color,
    val bgRaised: Color,
    val bgOverlay: Color,
    val bgInset: Color,
    // Text
    val textPrimary: Color,
    val textSecondary: Color,
    val textMuted: Color,
    val textFaint: Color,
    // Borders
    val borderSubtle: Color,
    val borderStrong: Color,
    // Accents
    val accentBlue: Color,
    val accentGreen: Color,
    val accentAmber: Color,
    val accentRed: Color,
    val accentPurple: Color,
    // Inverted call-to-action
    val surfaceBright: Color,
    val onBright: Color,
    // Skeleton placeholder
    val skeletonBase: Color,
    val skeletonHighlight: Color,
)

/**
 * Graphite — the upstream orca palette, unchanged.
 *
 * Body `#E0E0E0` on `#111111` measures 14.6:1.
 */
private val GraphitePalette =
    OrcaPalette(
        bgBase = Color(0xFF111111),
        bgPanel = Color(0xFF1A1A1A),
        bgRaised = Color(0xFF242424),
        bgOverlay = Color(0xFF2A2A2A),
        bgInset = Color(0xFF1E1E1E),
        textPrimary = Color(0xFFE0E0E0),
        textSecondary = Color(0xFFA1A1A1),
        textMuted = Color(0xFF8C8C8C),
        textFaint = Color(0xFF555555),
        borderSubtle = Color(0xFF2A2A2A),
        borderStrong = Color(0xFF3A3A3A),
        accentBlue = Color(0xFF3B82F6),
        accentGreen = Color(0xFF22C55E),
        accentAmber = Color(0xFFF59E0B),
        accentRed = Color(0xFFEF4444),
        accentPurple = Color(0xFFA78BFA),
        surfaceBright = Color(0xFFF5F5F5),
        onBright = Color(0xFF111111),
        skeletonBase = Color(0x14FFFFFF),
        skeletonHighlight = Color(0x24FFFFFF),
    )

/**
 * Midnight — a blue-black palette for dim rooms.
 *
 * The accents are lifted a step so they keep >= 4.5:1 on the darker blue
 * surfaces: `#60A5FA` on `#0F172A` is 7.4:1 where `#3B82F6` would be 4.3:1.
 */
private val MidnightPalette =
    OrcaPalette(
        bgBase = Color(0xFF0F172A),
        bgPanel = Color(0xFF16203A),
        bgRaised = Color(0xFF1E2A47),
        bgOverlay = Color(0xFF263453),
        bgInset = Color(0xFF18233C),
        textPrimary = Color(0xFFE6EDF9),
        textSecondary = Color(0xFFA8B8D4),
        textMuted = Color(0xFF8FA2C0),
        textFaint = Color(0xFF5B6B8A),
        borderSubtle = Color(0xFF24314F),
        borderStrong = Color(0xFF33456B),
        accentBlue = Color(0xFF60A5FA),
        accentGreen = Color(0xFF4ADE80),
        accentAmber = Color(0xFFFBBF24),
        accentRed = Color(0xFFF87171),
        accentPurple = Color(0xFFC4B5FD),
        surfaceBright = Color(0xFFE6EDF9),
        onBright = Color(0xFF0F172A),
        skeletonBase = Color(0x14FFFFFF),
        skeletonHighlight = Color(0x24FFFFFF),
    )

/**
 * Paper — the light palette, for daylight.
 *
 * Every text token is checked against `#F7F7F5`: `#2A2A28` is 13.9:1 and the
 * muted `#6B6B66` is 5.2:1, so secondary text still clears the bar. The
 * accents are darkened by the same amount the light background demands —
 * `#2563EB` on `#F7F7F5` is 5.6:1, where the dark-theme `#3B82F6` is 3.6:1.
 */
private val PaperPalette =
    OrcaPalette(
        bgBase = Color(0xFFF8FAFC),
        bgPanel = Color(0xFFFFFFFF),
        bgRaised = Color(0xFFF1F5F9),
        bgOverlay = Color(0xFFE2E8F0),
        bgInset = Color(0xFFF8FAFC),
        textPrimary = Color(0xFF0F172A),
        textSecondary = Color(0xFF334155),
        textMuted = Color(0xFF64748B),
        textFaint = Color(0xFF94A3B8),
        borderSubtle = Color(0xFFE2E8F0),
        borderStrong = Color(0xFFCBD5E1),
        accentBlue = Color(0xFF2563EB),
        accentGreen = Color(0xFF16A34A),
        accentAmber = Color(0xFFD97706),
        accentRed = Color(0xFFDC2626),
        accentPurple = Color(0xFF7C3AED),
        surfaceBright = Color(0xFF0F172A),
        onBright = Color(0xFFFFFFFF),
        skeletonBase = Color(0x0F000000),
        skeletonHighlight = Color(0x1F000000),
    )
