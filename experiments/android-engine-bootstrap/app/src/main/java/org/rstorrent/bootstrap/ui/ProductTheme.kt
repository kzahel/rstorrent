package org.rstorrent.bootstrap.ui

import android.app.Activity
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext

enum class ProductThemeMode {
    SYSTEM,
    LIGHT,
    DARK,
}

private val LightColors =
    lightColorScheme(
        primary = Color(0xFF006A6A),
        secondary = Color(0xFF4A6363),
        tertiary = Color(0xFF4B607C),
    )

private val DarkColors =
    darkColorScheme(
        primary = Color(0xFF4FD8D8),
        secondary = Color(0xFFB1CCCC),
        tertiary = Color(0xFFB3C7E8),
    )

@Composable
fun RstorrentTheme(
    mode: ProductThemeMode,
    dynamicColor: Boolean,
    content: @Composable () -> Unit,
) {
    val dark =
        when (mode) {
            ProductThemeMode.SYSTEM -> isSystemInDarkTheme()
            ProductThemeMode.LIGHT -> false
            ProductThemeMode.DARK -> true
        }
    val context = LocalContext.current
    val colors =
        when {
            dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && dark ->
                dynamicDarkColorScheme(context)
            dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S ->
                dynamicLightColorScheme(context)
            dark -> DarkColors
            else -> LightColors
        }
    val activity = context as? Activity
    activity?.window?.let { window ->
        window.statusBarColor = colors.surface.value.toInt()
        window.navigationBarColor = colors.surface.value.toInt()
    }
    MaterialTheme(colorScheme = colors, content = content)
}
