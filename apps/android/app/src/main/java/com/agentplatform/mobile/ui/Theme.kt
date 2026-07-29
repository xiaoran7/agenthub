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

val Paper = Color(0xFFFBFAF7)
val Surface = Color(0xFFFFFFFF)
val Ink = Color(0xFF111111)
val Muted = Color(0xFF777570)
val Line = Color(0xFFE2DFD9)
val PinkSignal = Color(0xFFEF7FA7)
val SoftPink = Color(0xFFFFEDF3)
val WorkingBlue = Color(0xFF3974E8)
val WaitingAmber = Color(0xFFCB7600)
val SuccessGreen = Color(0xFF25845A)
val DangerRed = Color(0xFFB64040)

private val LightColors =
    lightColorScheme(
        primary = Ink,
        onPrimary = Surface,
        secondary = PinkSignal,
        onSecondary = Ink,
        background = Paper,
        onBackground = Ink,
        surface = Surface,
        onSurface = Ink,
        surfaceVariant = Color(0xFFF4F2EE),
        onSurfaceVariant = Muted,
        outline = Line,
        error = DangerRed,
    )

private val DarkColors =
    darkColorScheme(
        primary = Color(0xFFF7F5F0),
        onPrimary = Color(0xFF151515),
        secondary = PinkSignal,
        background = Color(0xFF11110F),
        onBackground = Color(0xFFF5F2EC),
        surface = Color(0xFF1B1B18),
        onSurface = Color(0xFFF5F2EC),
        surfaceVariant = Color(0xFF25241F),
        onSurfaceVariant = Color(0xFFB9B5AD),
        outline = Color(0xFF3A3933),
        error = Color(0xFFFF8D8D),
    )

private val Typography =
    androidx.compose.material3.Typography(
        displayLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Black,
                fontSize = 44.sp,
                lineHeight = 49.sp,
                letterSpacing = (-1.8).sp,
            ),
        headlineLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Black,
                fontSize = 34.sp,
                lineHeight = 39.sp,
                letterSpacing = (-1.2).sp,
            ),
        headlineMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.ExtraBold,
                fontSize = 25.sp,
                lineHeight = 31.sp,
                letterSpacing = (-0.6).sp,
            ),
        titleLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 20.sp,
                lineHeight = 26.sp,
            ),
        titleMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 16.sp,
                lineHeight = 22.sp,
            ),
        bodyLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 16.sp,
                lineHeight = 24.sp,
            ),
        bodyMedium =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Normal,
                fontSize = 14.sp,
                lineHeight = 21.sp,
            ),
        labelLarge =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 14.sp,
                lineHeight = 18.sp,
            ),
        labelSmall =
            TextStyle(
                fontFamily = FontFamily.SansSerif,
                fontWeight = FontWeight.Bold,
                fontSize = 11.sp,
                lineHeight = 15.sp,
                letterSpacing = 1.5.sp,
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
