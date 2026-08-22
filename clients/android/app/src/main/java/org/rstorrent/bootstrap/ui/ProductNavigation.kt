package org.rstorrent.bootstrap.ui

import java.util.Locale
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView

internal object ProductRoutes {
    const val LIBRARY = "torrent_list"
    const val DETAIL = "torrent_detail/{torrentId}"
    const val SPEED = "speed_history"
    const val DHT = "dht_info"
    const val LOGS = "logs"
    const val SETTINGS = "settings"
    const val SETTINGS_STORAGE = "settings/storage"
    const val SETTINGS_SPEED = "settings/speed_connection_limits"
    const val SETTINGS_NOTIFICATIONS = "settings/notifications"
    const val SETTINGS_NETWORK = "settings/network"
    const val SETTINGS_POWER = "settings/power"
    const val SETTINGS_ADVANCED = "settings/advanced"

    fun detail(torrentId: String): String = "torrent_detail/$torrentId"
}

enum class LibraryFilter(
    val label: String,
) {
    ALL("All"),
    ACTIVE("Active"),
    QUEUED("Queued"),
    FINISHED("Finished"),
    ;

    fun matches(torrent: TorrentView): Boolean =
        when (this) {
            ALL -> true
            ACTIVE ->
                torrent.operationalState in
                    setOf(
                        TorrentOperationalState.STARTING,
                        TorrentOperationalState.DOWNLOADING,
                        TorrentOperationalState.CHECKING,
                        TorrentOperationalState.STOPPING,
                        TorrentOperationalState.SEEDING,
                    )
            QUEUED -> torrent.operationalState == TorrentOperationalState.QUEUED
            FINISHED -> torrent.state == TorrentState.COMPLETE || torrent.archived
        }
}

enum class LibrarySort(
    val label: String,
) {
    STABLE("Queue / stable order"),
    NAME("Name"),
    DOWNLOAD_SPEED("Download speed"),
}

internal fun filteredAndSortedTorrents(
    torrents: Collection<TorrentView>,
    filter: LibraryFilter,
    sort: LibrarySort,
): List<TorrentView> {
    val comparator =
        when (sort) {
            LibrarySort.STABLE ->
                compareBy<TorrentView> { it.downloadQueuePosition ?: UInt.MAX_VALUE }
                    .thenBy(TorrentView::torrentId)
            LibrarySort.NAME ->
                compareBy<TorrentView> {
                    (it.displayName ?: it.torrentId).lowercase(Locale.ROOT)
                }.thenBy(TorrentView::torrentId)
            LibrarySort.DOWNLOAD_SPEED ->
                compareByDescending<TorrentView> {
                    it.payloadDownloadRateBytes.toULongOrNull() ?: 0UL
                }.thenBy { it.displayName ?: it.torrentId }
        }
    return torrents.filter(filter::matches).sortedWith(comparator)
}

enum class TorrentDetailTab(
    val label: String,
) {
    DETAILS("Details"),
    STATUS("Status"),
    FILES("Files"),
    TRACKERS("Trackers"),
    PEERS("Peers"),
    PIECES("Pieces"),
}
