package org.rstorrent.bootstrap

import android.content.Context

internal class ProductLifecyclePreferenceStore(context: Context) {
    private val preferences =
        context.getSharedPreferences(PREFERENCE_FILE, Context.MODE_PRIVATE)

    fun read(): ProductLifecyclePreferences =
        ProductLifecyclePreferences(
            backgroundDownloadsEnabled = preferences.getBoolean(KEY_BACKGROUND_DOWNLOADS, false),
            completionPolicy =
                ProductBackgroundCompletionPolicy.decode(
                    preferences.getString(KEY_COMPLETION_POLICY, null),
                ),
        )

    fun write(value: ProductLifecyclePreferences): Boolean =
        preferences
            .edit()
            .putBoolean(KEY_BACKGROUND_DOWNLOADS, value.backgroundDownloadsEnabled)
            .putString(KEY_COMPLETION_POLICY, value.completionPolicy.persistedValue)
            .commit()

    fun reset(): Boolean = preferences.edit().clear().commit()

    companion object {
        private const val PREFERENCE_FILE = "product_lifecycle"
        private const val KEY_BACKGROUND_DOWNLOADS = "background_downloads_enabled"
        private const val KEY_COMPLETION_POLICY = "background_completion_policy"
    }
}
