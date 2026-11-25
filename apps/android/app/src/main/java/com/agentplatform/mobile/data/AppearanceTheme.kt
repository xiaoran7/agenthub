package com.agentplatform.mobile.data

import androidx.compose.ui.graphics.Color

/**
 * Appearance choices shown in Settings.
 *
 * [SYSTEM] defaults to Qoder-inspired [FOREST_DARK] in dark mode and
 * [MINT_LIGHT] in light mode, while preserving the classic [PAPER], [GRAPHITE],
 * and [MIDNIGHT] palettes for existing installs.
 */
enum class AppearanceTheme(val wire: String, val label: String, val blurb: String) {
    SYSTEM("system", "跟随系统", "默认使用森墨 / 青岚风格，随系统深浅色切换"),
    FOREST_DARK("forest_dark", "森墨", "深邃林墨底配翡翠薄荷绿，沉浸耐看"),
    MINT_LIGHT("mint_light", "青岚", "浅灰白底，搭配低饱和森绿"),
    PAPER("paper", "纸墨", "暖浅纸色底，简洁高对比"),
    GRAPHITE("graphite", "石墨", "纯深灰底，经典极简暗色"),
    MIDNIGHT("midnight", "午夜蓝", "蓝黑底，弱光环境下对比更柔"),
    ;

    fun resolve(systemDark: Boolean): AppearanceTheme =
        if (this == SYSTEM) {
            if (systemDark) FOREST_DARK else MINT_LIGHT
        } else {
            this
        }

    val palette: OrcaPalette
        get() =
            when (this) {
                SYSTEM, MINT_LIGHT -> MintLightPalette
                FOREST_DARK -> ForestDarkPalette
                PAPER -> PaperPalette
                GRAPHITE -> GraphitePalette
                MIDNIGHT -> MidnightPalette
            }

    /** True for light palettes, which need light status/navigation bars. */
    val light: Boolean
        get() = this == MINT_LIGHT || this == PAPER || this == SYSTEM

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
    // Primary call-to-action pill
    val surfaceBright: Color,
    val onBright: Color,
    // Skeleton placeholder
    val skeletonBase: Color,
    val skeletonHighlight: Color,
)

/**
 * Forest Dark — Qoder Mobile-inspired obsidian-forest palette.
 *
 * Body `#F4F5F3` on `#0F1110` measures 17.3:1; muted `#858E88` measures 5.7:1;
 * primary mint `#73CD94` measures 10.4:1; CTA `#0D1A12` on `#8EE5A1` is 12.5:1.
 */
private val ForestDarkPalette =
    OrcaPalette(
        bgBase = Color(0xFF0F1110),
        bgPanel = Color(0xFF171A18),
        bgRaised = Color(0xFF1F2421),
        bgOverlay = Color(0xFF262C28),
        bgInset = Color(0xFF141715),
        textPrimary = Color(0xFFF4F5F3),
        textSecondary = Color(0xFFB4BCB7),
        textMuted = Color(0xFF858E88),
        textFaint = Color(0xFF565F59),
        borderSubtle = Color(0xFF272E2A),
        borderStrong = Color(0xFF36423B),
        accentBlue = Color(0xFF73CD94),
        accentGreen = Color(0xFF8EE5A1),
        accentAmber = Color(0xFFF6C85F),
        accentRed = Color(0xFFFF7A7E),
        accentPurple = Color(0xFFC9A6E8),
        surfaceBright = Color(0xFF73CD94),
        onBright = Color(0xFF0B1710),
        skeletonBase = Color(0x14FFFFFF),
        skeletonHighlight = Color(0x268EE5A1),
    )

/** Neutral light surfaces with forest-green primary actions. */
private val MintLightPalette =
    OrcaPalette(
        bgBase = Color(0xFFF7F7F6),
        bgPanel = Color(0xFFFFFFFF),
        bgRaised = Color(0xFFEEF0ED),
        bgOverlay = Color(0xFFE7EAE7),
        bgInset = Color(0xFFF1F2F0),
        textPrimary = Color(0xFF171917),
        textSecondary = Color(0xFF535953),
        textMuted = Color(0xFF676E68),
        textFaint = Color(0xFF838B84),
        borderSubtle = Color(0xFFE2E4E1),
        borderStrong = Color(0xFFBFC9C0),
        accentBlue = Color(0xFF486858),
        accentGreen = Color(0xFF2D7553),
        accentAmber = Color(0xFFA66316),
        accentRed = Color(0xFFC43B44),
        accentPurple = Color(0xFF6D4CA5),
        surfaceBright = Color(0xFF486858),
        onBright = Color(0xFFFFFFFF),
        skeletonBase = Color(0x12171917),
        skeletonHighlight = Color(0x22486858),
    )

/**
 * Graphite — classic neutral dark palette.
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
 * `#60A5FA` on `#0F172A` is 7.4:1.
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
 * Paper — warm daylight palette.
 *
 * `#2A2A28` on `#F7F7F5` is 13.9:1; `#6B6B66` is 5.2:1.
 */
private val PaperPalette =
    OrcaPalette(
        bgBase = Color(0xFFF7F7F5),
        bgPanel = Color(0xFFFFFFFF),
        bgRaised = Color(0xFFF0EFEA),
        bgOverlay = Color(0xFFE8E6DF),
        bgInset = Color(0xFFF3F2EE),
        textPrimary = Color(0xFF2A2A28),
        textSecondary = Color(0xFF575652),
        textMuted = Color(0xFF6B6B66),
        textFaint = Color(0xFF9C9B94),
        borderSubtle = Color(0xFFE2E0D8),
        borderStrong = Color(0xFFC9C6BC),
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
