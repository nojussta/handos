package com.handos.bmw.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val DarkColors = darkColorScheme(
    primary = Color(0xFF4C9AFF),
    onPrimary = Color(0xFF00244D),
    primaryContainer = Color(0xFF0E3A6D),
    onPrimaryContainer = Color(0xFFD6E6FF),
    secondary = Color(0xFF7FD4C1),
    background = Color(0xFF0E1116),
    surface = Color(0xFF151A22),
    surfaceVariant = Color(0xFF1E2530),
    onBackground = Color(0xFFECEFF4),
    onSurface = Color(0xFFECEFF4),
    onSurfaceVariant = Color(0xFF9AA5B4),
    error = Color(0xFFFF6B6B),
)

private val LightColors = lightColorScheme(
    primary = Color(0xFF0057D8),
    onPrimary = Color(0xFFFFFFFF),
    primaryContainer = Color(0xFFDCE8FF),
    onPrimaryContainer = Color(0xFF001A44),
    secondary = Color(0xFF007A67),
    background = Color(0xFFF6F8FB),
    surface = Color(0xFFFFFFFF),
    surfaceVariant = Color(0xFFE9EEF5),
    onBackground = Color(0xFF11161F),
    onSurface = Color(0xFF11161F),
    onSurfaceVariant = Color(0xFF556070),
    error = Color(0xFFBA1A1A),
)

@Composable
fun HandosTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        content = content,
    )
}
