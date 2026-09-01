package org.rstorrent.bootstrap

import android.content.Context

object ProductCompanionPreference {
    private const val PREFERENCES = "chromeos_companion"
    private const val ENABLED = "enabled"

    fun read(context: Context): Boolean =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getBoolean(ENABLED, false)

    fun enable(context: Context) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(ENABLED, true)
            .apply()
    }

    fun reset(context: Context): Boolean =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit().clear().commit()

    fun shouldStart(isChromeOs: Boolean, enabled: Boolean): Boolean =
        isChromeOs && enabled
}
