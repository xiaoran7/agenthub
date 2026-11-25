package com.agentplatform.mobile.ui

import androidx.annotation.DrawableRes
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.R
import com.agentplatform.mobile.data.EngineKind
import java.util.Locale

// =========================================================================
// Provider brand icons (official color VectorDrawables & high-DPI PNGs)
// =========================================================================

/** The logical glyph family for one engine (retained for contract tests). */
enum class ProviderGlyph {
    TERMINAL,
    STARBURST,
    SPARKLE,
    BUBBLE,
    ORBIT,
    SLASH,
    ;

    companion object {
        fun of(engine: EngineKind): ProviderGlyph =
            when (engine) {
                EngineKind.CODEX -> TERMINAL
                EngineKind.CLAUDE_CODE -> STARBURST
                EngineKind.CLAUDE_DESKTOP -> BUBBLE
                EngineKind.ANTIGRAVITY -> ORBIT
                EngineKind.DEEPSEEK_HARNESS -> SPARKLE
                EngineKind.PLUGIN -> SLASH
            }
    }
}

enum class SessionGlyphTone {
    ACTIVE,
    RESUMABLE,
    ;
}

internal data class BrandIconSpec(
    @DrawableRes val resId: Int,
    val monochrome: Boolean = false,
)

/**
 * Maps any [EngineKind] or plugin wire/display identifier to its real embedded
 * official brand icon resource.
 */
internal fun resolveBrandIcon(
    engine: EngineKind,
    reportedProvider: String? = null,
): BrandIconSpec {
    val norm = reportedProvider?.trim()?.lowercase(Locale.US).orEmpty()
    if (engine == EngineKind.PLUGIN && norm.isNotEmpty()) {
        return when {
            norm.contains("gemini_cli") || norm.contains("gemini cli") || norm == "zcode" ->
                BrandIconSpec(R.drawable.ic_brand_gemini_cli)
            norm.contains("gemini") -> BrandIconSpec(R.drawable.ic_brand_gemini)
            norm.contains("grok") || norm.contains("xai") ->
                BrandIconSpec(R.drawable.ic_brand_grok, monochrome = true)
            norm.contains("qwen") -> BrandIconSpec(R.drawable.ic_brand_qwen)
            norm.contains("deepseek") -> BrandIconSpec(R.drawable.ic_brand_deepseek)
            norm.contains("claude_code") || norm.contains("claude-code") ->
                BrandIconSpec(R.drawable.ic_brand_claude_code)
            norm.contains("claude") -> BrandIconSpec(R.drawable.ic_brand_claude)
            norm.contains("codex") -> BrandIconSpec(R.drawable.ic_brand_codex)
            norm.contains("openai") -> BrandIconSpec(R.drawable.ic_brand_openai, monochrome = true)
            norm.contains("antigravity") -> BrandIconSpec(R.drawable.ic_brand_antigravity)
            else -> BrandIconSpec(R.drawable.ic_brand_gemini)
        }
    }
    return when (engine) {
        EngineKind.CODEX -> BrandIconSpec(R.drawable.ic_brand_codex)
        EngineKind.CLAUDE_CODE -> BrandIconSpec(R.drawable.ic_brand_claude_code)
        EngineKind.CLAUDE_DESKTOP -> BrandIconSpec(R.drawable.ic_brand_claude)
        EngineKind.ANTIGRAVITY -> BrandIconSpec(R.drawable.ic_brand_antigravity)
        EngineKind.DEEPSEEK_HARNESS -> BrandIconSpec(R.drawable.ic_brand_deepseek)
        EngineKind.PLUGIN -> BrandIconSpec(R.drawable.ic_brand_gemini)
    }
}

/** Renders the real official Agent brand icon (color SVG / crisp high-DPI PNG). */
@Composable
fun AgentBrandIcon(
    engine: EngineKind,
    modifier: Modifier = Modifier,
    reportedProvider: String? = null,
    size: Dp = 20.dp,
    monochromeTint: Color = MaterialTheme.colorScheme.onSurface,
) {
    val spec = resolveBrandIcon(engine, reportedProvider)
    Image(
        painter = painterResource(id = spec.resId),
        contentDescription = null,
        contentScale = ContentScale.Fit,
        colorFilter = if (spec.monochrome) ColorFilter.tint(monochromeTint) else null,
        modifier = modifier.size(size),
    )
}

/** Framed Agent brand icon tile for session list rows and engine cards. */
@Composable
fun AgentBrandBadge(
    engine: EngineKind,
    modifier: Modifier = Modifier,
    reportedProvider: String? = null,
    tileSize: Dp = 36.dp,
    iconSize: Dp = 21.dp,
    selected: Boolean = false,
) {
    val colors = MaterialTheme.colorScheme
    Surface(
        modifier = modifier.size(tileSize),
        shape = RoundedCornerShape(10.dp),
        color =
            if (selected) OrcaColors.accentBlue.copy(alpha = 0.14f)
            else colors.surfaceVariant.copy(alpha = 0.72f),
        border =
            BorderStroke(
                width = if (selected) 1.dp else 0.75.dp,
                color =
                    if (selected) OrcaColors.accentBlue.copy(alpha = 0.45f)
                    else OrcaColors.borderSubtle,
            ),
    ) {
        Box(contentAlignment = Alignment.Center) {
            AgentBrandIcon(
                engine = engine,
                reportedProvider = reportedProvider,
                size = iconSize,
            )
        }
    }
}

/** Official AgentHub 3D cube brand mark for the welcome hero. */
@Composable
fun AgentHubBrandMark(
    modifier: Modifier = Modifier,
    size: Dp = 44.dp,
) {
    Box(
        modifier =
            modifier
                .size(size)
                .clip(RoundedCornerShape(14.dp))
                .background(OrcaColors.bgPanel),
        contentAlignment = Alignment.Center,
    ) {
        Image(
            painter = painterResource(id = R.drawable.ic_launcher_brand_foreground),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.size(size * 1.18f),
        )
    }
}

/** Backwards-compatible entry point delegating to real brand icons. */
@Composable
fun ProviderGlyphIcon(
    engine: EngineKind,
    tone: SessionGlyphTone,
    tint: Color,
    modifier: Modifier = Modifier,
    size: Dp = 18.dp,
    reportedProvider: String? = null,
) {
    val unusedTone = tone
    Box(modifier = modifier.size(size), contentAlignment = Alignment.Center) {
        AgentBrandIcon(
            engine = engine,
            reportedProvider = reportedProvider,
            size = size,
            monochromeTint = tint,
        )
    }
}

/** Session row tile always displays the Agent's real brand icon; status lives in the subtitle. */
@Composable
fun SessionEntryTile(
    engine: EngineKind,
    resumable: Boolean,
    modifier: Modifier = Modifier,
    reportedProvider: String? = null,
) {
    val unused = resumable
    AgentBrandBadge(
        engine = engine,
        reportedProvider = reportedProvider,
        tileSize = OrcaSize.tile,
        iconSize = 22.dp,
        modifier = modifier,
    )
}
