package com.agentplatform.mobile.ui

import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.compositionLocalOf
import com.agentplatform.mobile.data.AppearanceTheme

// =========================================================================
// Appearance selection
//
// The chosen theme is a composition local rather than a parameter on every
// composable: `OrcaColors` is read from dozens of call sites, and threading an
// extra argument through all of them would make the palette a per-screen
// concern instead of an app-wide one.
//
// The default is GRAPHITE, so a composable rendered outside
// `AgentPlatformTheme` (a preview, a test) still gets the original palette.
// =========================================================================

/** The appearance in force for this subtree. */
val LocalAppearance: ProvidableCompositionLocal<AppearanceTheme> =
    compositionLocalOf { AppearanceTheme.GRAPHITE }
