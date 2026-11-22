package com.agentplatform.mobile.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.agentplatform.mobile.data.EngineKind

// =========================================================================
// Provider glyphs
//
// One monochrome line glyph per engine, so a session row says which engine it
// belongs to before the title is read. The rules these follow:
//
//   * single-colour line work, 1.6dp stroke, no fill — the tile behind them is
//     already tinted, and a second colour would read as a status;
//   * drawn on a 24x24 grid and optically centred, so a 16dp glyph sits in the
//     40dp tile without looking low or left (a terminal caret, for instance,
//     has no descender and needs a nudge down to look centred);
//   * no emoji and no letterforms: a glyph has to survive being shrunk to 12dp,
//     and a Latin initial does not.
//
// A candidate (resumable) session gets a distinct glyph rather than a tint of
// the same one: "there is a conversation here" and "this phone is driving it
// now" are different facts, and colour alone never carries meaning here.
// =========================================================================

/** The glyph for one engine. */
enum class ProviderGlyph {
    /** Angular prompt caret — the Codex / terminal family. */
    TERMINAL,

    /** Anthropic-style starburst. */
    STARBURST,

    /** Sparkle — the Gemini / ZCode family. */
    SPARKLE,

    /** Speech bubble — Claude Desktop, a chat client rather than a CLI. */
    BUBBLE,

    /** Orbit — Antigravity. */
    ORBIT,

    /** Grok's angular slash. */
    SLASH,
    ;

    companion object {
        /**
         * Total over [EngineKind.entries]: a `when` that silently gains a
         * fallback branch here is how a new engine ends up with a glyph
         * borrowed from another one.
         */
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

/** A history entry the phone found on disk and can resume. */
enum class SessionGlyphTone {
    /** A session this phone controls right now. */
    ACTIVE,

    /** A conversation found on disk; resuming it makes it active. */
    RESUMABLE,
    ;
}

/**
 * Draws the glyph for a row.
 *
 * @param size visible glyph box; the tile around it stays [OrcaSize.tile], so
 *   this is the ink, not the hit area.
 */
@Composable
fun ProviderGlyphIcon(
    engine: EngineKind,
    tone: SessionGlyphTone,
    tint: Color,
    modifier: Modifier = Modifier,
    size: Dp = 16.dp,
) {
    Box(modifier = modifier.size(size), contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(size)) { drawGlyph(ProviderGlyph.of(engine), tone, tint) }
    }
}

/**
 * Shared drawing routine.
 *
 * Kept out of the composable so the geometry is one function a reader can hold
 * in their head, and so every glyph shares the same stroke weight and cap.
 */
private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawGlyph(
    glyph: ProviderGlyph,
    tone: SessionGlyphTone,
    tint: Color,
) {
    val w = size.width
    val h = size.height
    val stroke = 1.6.dp.toPx()
    fun line(
        x1: Float,
        y1: Float,
        x2: Float,
        y2: Float,
    ) = drawLine(tint, Offset(w * x1, h * y1), Offset(w * x2, h * y2), stroke, StrokeCap.Round)

    fun path(builder: Path.() -> Unit) {
        val p = Path().apply(builder)
        drawPath(p, tint, style = Stroke(width = stroke, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }

    when (glyph) {
        ProviderGlyph.TERMINAL -> {
            // A caret and a baseline. The caret is nudged up 0.04 so the pair
            // reads centred: the baseline at 0.72 already carries the weight.
            path {
                moveTo(w * 0.20f, h * 0.28f)
                lineTo(w * 0.44f, h * 0.46f)
                lineTo(w * 0.20f, h * 0.64f)
            }
            line(0.50f, 0.70f, 0.80f, 0.70f)
        }

        ProviderGlyph.STARBURST -> {
            // Six spokes from the centre. Short enough that at 16dp they stay
            // separate instead of collapsing into a blob.
            val cx = w * 0.5f
            val cy = h * 0.5f
            for (i in 0 until 6) {
                val angle = (Math.PI / 3.0 * i) - Math.PI / 2.0
                val cos = Math.cos(angle).toFloat()
                val sin = Math.sin(angle).toFloat()
                drawLine(
                    tint,
                    Offset(cx + cos * w * 0.13f, cy + sin * h * 0.13f),
                    Offset(cx + cos * w * 0.36f, cy + sin * h * 0.36f),
                    stroke,
                    StrokeCap.Round,
                )
            }
        }

        ProviderGlyph.SPARKLE -> {
            // A four-point star: two crossing curves, no straight cross, so it
            // reads as a sparkle rather than a plus sign.
            path {
                moveTo(w * 0.50f, h * 0.16f)
                cubicTo(w * 0.56f, h * 0.40f, w * 0.60f, h * 0.44f, w * 0.84f, h * 0.50f)
                cubicTo(w * 0.60f, h * 0.56f, w * 0.56f, h * 0.60f, w * 0.50f, h * 0.84f)
                cubicTo(w * 0.44f, h * 0.60f, w * 0.40f, h * 0.56f, w * 0.16f, h * 0.50f)
                cubicTo(w * 0.40f, h * 0.44f, w * 0.44f, h * 0.40f, w * 0.50f, h * 0.16f)
                close()
            }
        }

        ProviderGlyph.BUBBLE -> {
            // A rounded speech bubble with a tail. The tail is what tells it
            // apart from a plain rounded square at small sizes.
            drawRoundRect(
                color = tint,
                topLeft = Offset(w * 0.16f, h * 0.22f),
                size = Size(w * 0.68f, h * 0.46f),
                cornerRadius = CornerRadius(3.dp.toPx(), 3.dp.toPx()),
                style = Stroke(width = stroke),
            )
            path {
                moveTo(w * 0.34f, h * 0.68f)
                lineTo(w * 0.34f, h * 0.84f)
                lineTo(w * 0.50f, h * 0.68f)
            }
        }

        ProviderGlyph.ORBIT -> {
            // A filled centre with an ellipse through it: one body, one orbit.
            drawCircle(tint, radius = w * 0.09f, center = Offset(w * 0.5f, h * 0.5f))
            drawOval(
                color = tint,
                topLeft = Offset(w * 0.10f, h * 0.32f),
                size = Size(w * 0.80f, h * 0.36f),
                style = Stroke(width = stroke),
            )
        }

        ProviderGlyph.SLASH -> {
            // Grok's diagonal, drawn twice with a gap so it reads as a mark
            // and not as a stray line.
            line(0.30f, 0.72f, 0.70f, 0.28f)
            line(0.16f, 0.86f, 0.28f, 0.76f)
        }
    }
}

/**
 * The tile for a session row.
 *
 * An active session gets the provider glyph; a resumable one gets a plus,
 * because the useful question at a glance is "can I open this now?" — the
 * provider is secondary until it is opened.
 */
@Composable
fun SessionEntryTile(
    engine: EngineKind,
    resumable: Boolean,
    modifier: Modifier = Modifier,
) {
    val tint = MaterialTheme.colorScheme.onSurfaceVariant
    OrcaIconTile(modifier) {
        if (resumable) {
            OrcaPlusIcon(tint, 16.dp)
        } else {
            ProviderGlyphIcon(
                engine = engine,
                tone = SessionGlyphTone.ACTIVE,
                tint = tint,
                size = 16.dp,
            )
        }
    }
}
