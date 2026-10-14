package app.snot.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/** The same palette as the desktop app, so a note looks alike on both. */
@Immutable
data class SnotColors(
    val dark: Boolean,
    val appBg: Color,
    val listBg: Color,
    val pageBg: Color,
    val cardBg: Color,
    val border: Color,
    val text: Color,
    val textMuted: Color,
    val textFaint: Color,
    val accent: Color,
    val accentText: Color,
    val accentSoft: Color,
    val selection: Color,
    val danger: Color,
    val paperRule: Color,
    val popoverBg: Color,
    val quoteBar: Color,
    val codeBg: Color,
)

private val Light = SnotColors(
    dark = false,
    appBg = Color(0xFFF3F2EF),
    listBg = Color(0xFFFAF9F7),
    pageBg = Color(0xFFFFFFFF),
    cardBg = Color(0xFFFFFFFF),
    border = Color(0xFFE4E2DD),
    text = Color(0xFF1F1F22),
    textMuted = Color(0xFF66656B),
    textFaint = Color(0xFF9D9CA2),
    accent = Color(0xFFE8710A),
    accentText = Color(0xFFFFFFFF),
    accentSoft = Color(0x24E8710A),
    selection = Color(0x401A73E8),
    danger = Color(0xFFD93025),
    paperRule = Color(0x2E1A73E8),
    popoverBg = Color(0xFFFFFFFF),
    quoteBar = Color(0xFFD8D6D0),
    codeBg = Color(0xFFF3F2EF),
)

private val Dark = SnotColors(
    dark = true,
    appBg = Color(0xFF141416),
    listBg = Color(0xFF18181B),
    pageBg = Color(0xFF1C1C1F),
    cardBg = Color(0xFF202024),
    border = Color(0xFF2C2C31),
    text = Color(0xFFE8E8EA),
    textMuted = Color(0xFFA4A4AA),
    textFaint = Color(0xFF6E6E75),
    accent = Color(0xFFFFA657),
    accentText = Color(0xFF1C1C1F),
    accentSoft = Color(0x26FFA657),
    selection = Color(0x4A79B8FF),
    danger = Color(0xFFFF7B72),
    paperRule = Color(0x2679B8FF),
    popoverBg = Color(0xFF26262B),
    quoteBar = Color(0xFF3A3A40),
    codeBg = Color(0xFF232327),
)

val LocalSnotColors = staticCompositionLocalOf { Light }

object Snot {
    val colors: SnotColors
        @Composable get() = LocalSnotColors.current
}

/** `0xAARRGGBB` from the core's palette. */
fun argb(v: UInt): Color = Color(v.toInt())

@Composable
fun SnotTheme(content: @Composable () -> Unit) {
    val c = if (isSystemInDarkTheme()) Dark else Light
    val scheme = if (c.dark) {
        darkColorScheme(
            primary = c.accent,
            onPrimary = c.accentText,
            primaryContainer = c.accent,
            onPrimaryContainer = c.accentText,
            secondaryContainer = c.accentSoft,
            onSecondaryContainer = c.text,
            background = c.listBg,
            onBackground = c.text,
            surface = c.listBg,
            onSurface = c.text,
            onSurfaceVariant = c.textMuted,
            surfaceContainer = c.popoverBg,
            surfaceContainerLow = c.appBg,
            surfaceContainerHigh = c.popoverBg,
            surfaceContainerHighest = c.cardBg,
            outline = c.border,
            outlineVariant = c.border,
            error = c.danger,
        )
    } else {
        lightColorScheme(
            primary = c.accent,
            onPrimary = c.accentText,
            primaryContainer = c.accent,
            onPrimaryContainer = c.accentText,
            secondaryContainer = c.accentSoft,
            onSecondaryContainer = c.text,
            background = c.listBg,
            onBackground = c.text,
            surface = c.listBg,
            onSurface = c.text,
            onSurfaceVariant = c.textMuted,
            surfaceContainer = c.popoverBg,
            surfaceContainerLow = c.appBg,
            surfaceContainerHigh = c.popoverBg,
            surfaceContainerHighest = c.cardBg,
            outline = c.border,
            outlineVariant = c.border,
            error = c.danger,
        )
    }
    CompositionLocalProvider(LocalSnotColors provides c) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}
