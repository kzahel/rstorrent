package org.rstorrent.bootstrap

import android.content.Context
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentView

internal object ProductPowerPreference {
    private const val PREFERENCES = "product_power"
    private const val PREVENT_SLEEP = "prevent_sleep_during_active_downloads"

    fun read(context: Context): Boolean =
        context
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getBoolean(PREVENT_SLEEP, true)

    fun persist(
        context: Context,
        enabled: Boolean,
    ): Boolean =
        context
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(PREVENT_SLEEP, enabled)
            .commit()
}

internal fun requiresSleepInhibition(torrents: Collection<TorrentView>): Boolean =
    torrents.any { requiresSleepInhibition(it.operationalState) }

internal fun requiresSleepInhibition(state: TorrentOperationalState): Boolean =
    when (state) {
        TorrentOperationalState.STARTING,
        TorrentOperationalState.DOWNLOADING,
        TorrentOperationalState.CHECKING,
        -> true
        TorrentOperationalState.QUEUED,
        TorrentOperationalState.STOPPING,
        TorrentOperationalState.SEEDING,
        TorrentOperationalState.PAUSED,
        TorrentOperationalState.ERROR,
        -> false
    }
