package org.rstorrent.bootstrap

import android.content.Context

object ProductDataSyncQuotaFence {
    private const val PREFERENCES = "product_data_sync_quota"
    private const val EXHAUSTED = "exhausted"

    fun isExhausted(context: Context): Boolean =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getBoolean(EXHAUSTED, false)

    fun markExhausted(context: Context): Boolean =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(EXHAUSTED, true)
            .commit()

    fun clearForUserVisibleStart(context: Context): Boolean =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .remove(EXHAUSTED)
            .commit()
}
