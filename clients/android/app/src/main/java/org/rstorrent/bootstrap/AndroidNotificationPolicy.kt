package org.rstorrent.bootstrap

import java.security.MessageDigest
import java.util.ArrayDeque
import org.rstorrent.session.uniffi.StorageState
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView

internal enum class ProductNotificationCategory {
    DOWNLOAD_COMPLETE,
    NEEDS_ATTENTION,
}

internal sealed interface ProductNotificationRoute {
    data object Torrent : ProductNotificationRoute

    data class StorageRepair(val rootId: String) : ProductNotificationRoute
}

internal data class ProductNotificationEdge(
    val category: ProductNotificationCategory,
    val torrentId: String,
    val displayName: String?,
    val route: ProductNotificationRoute,
)

internal data class ProductNotificationReduction(
    val edges: List<ProductNotificationEdge> = emptyList(),
    val removedTorrentIds: List<String> = emptyList(),
)

/** Runtime-free edge policy over the authoritative torrent-list projection. */
internal class AndroidNotificationPolicy(
    private val removedLimit: Int = REMOVED_TORRENT_LIMIT,
) {
    private data class Entry(
        val state: TorrentState,
        val storageState: StorageState,
        val receivedBytes: ULong?,
        val verifiedPieceCount: UInt,
        val completionArmed: Boolean,
        val attentionActive: Boolean,
    )

    private val entries = mutableMapOf<String, Entry>()
    private val removedOrder = ArrayDeque<String>()
    private val removed = mutableSetOf<String>()

    internal val removedHistorySize: Int
        get() = removed.size

    fun baseline(torrents: Collection<TorrentView>) {
        entries.clear()
        removedOrder.clear()
        removed.clear()
        torrents.forEach { torrent -> entries[torrent.torrentId] = torrent.baselineEntry() }
    }

    fun reset() {
        entries.clear()
        removedOrder.clear()
        removed.clear()
    }

    fun applyPatch(
        torrents: Collection<TorrentView>,
        removedTorrentIds: Collection<String>,
    ): ProductNotificationReduction {
        val cancellations = removedTorrentIds.distinct()
        cancellations.forEach(::remove)
        val edges = buildList {
            torrents.forEach { torrent -> reduce(torrent)?.let(::add) }
        }
        return ProductNotificationReduction(edges, cancellations)
    }

    private fun remove(torrentId: String) {
        entries.remove(torrentId)
        if (removed.add(torrentId)) {
            removedOrder.addLast(torrentId)
        } else {
            removedOrder.remove(torrentId)
            removedOrder.addLast(torrentId)
        }
        while (removedOrder.size > removedLimit) {
            removed.remove(removedOrder.removeFirst())
        }
    }

    private fun reduce(torrent: TorrentView): ProductNotificationEdge? {
        val torrentId = torrent.torrentId
        val previous = entries[torrentId]
        if (previous == null) {
            val tombstoned = torrentId in removed
            entries[torrentId] = torrent.baselineEntry()
            if (!torrent.hasTerminalNotificationState()) clearRemoved(torrentId)
            if (tombstoned) return null
            return null
        }

        clearRemoved(torrentId)
        val attention = torrent.needsAttention()
        val attentionEntered = attention && !previous.attentionActive
        val checkingTransition =
            previous.state == TorrentState.CHECKING || torrent.state == TorrentState.CHECKING
        val received = torrent.receivedBytes.toULongOrNull()
        val ordinaryProgress =
            !checkingTransition &&
                (
                    increased(previous.receivedBytes, received) ||
                        (
                            torrent.verifiedPieceCount > previous.verifiedPieceCount &&
                                (
                                    previous.state == TorrentState.DOWNLOADING ||
                                        torrent.state == TorrentState.DOWNLOADING
                                )
                        )
                )
        var armed = if (checkingTransition) false else previous.completionArmed || ordinaryProgress
        val completionEntered =
            armed &&
                torrent.state == TorrentState.COMPLETE &&
                torrent.storageState == StorageState.AVAILABLE

        val edge =
            when {
                attentionEntered -> {
                    armed = false
                    ProductNotificationEdge(
                        ProductNotificationCategory.NEEDS_ATTENTION,
                        torrentId,
                        notificationDisplayName(torrent),
                        if (torrent.storageState == StorageState.NEEDS_REPAIR) {
                            ProductNotificationRoute.StorageRepair(torrent.storageRoot)
                        } else {
                            ProductNotificationRoute.Torrent
                        },
                    )
                }
                completionEntered -> {
                    armed = false
                    ProductNotificationEdge(
                        ProductNotificationCategory.DOWNLOAD_COMPLETE,
                        torrentId,
                        notificationDisplayName(torrent),
                        ProductNotificationRoute.Torrent,
                    )
                }
                else -> null
            }

        entries[torrentId] =
            Entry(
                torrent.state,
                torrent.storageState,
                received,
                torrent.verifiedPieceCount,
                armed,
                attention,
            )
        return edge
    }

    private fun clearRemoved(torrentId: String) {
        if (removed.remove(torrentId)) removedOrder.remove(torrentId)
    }

    private fun TorrentView.baselineEntry() =
        Entry(
            state,
            storageState,
            receivedBytes.toULongOrNull(),
            verifiedPieceCount,
            completionArmed = false,
            attentionActive = needsAttention(),
        )

    private fun TorrentView.needsAttention(): Boolean =
        state == TorrentState.ERROR ||
            state == TorrentState.NEEDS_REPAIR ||
            storageState == StorageState.NEEDS_REPAIR

    private fun TorrentView.hasTerminalNotificationState(): Boolean =
        needsAttention() || state == TorrentState.COMPLETE

    companion object {
        const val REMOVED_TORRENT_LIMIT = 256
    }
}

internal fun notificationDisplayName(torrent: TorrentView): String? =
    boundedNotificationName(torrent.displayName ?: torrent.sourceDisplayName)

internal fun boundedNotificationName(value: String?): String? {
    val normalized = value?.trim()?.replace(Regex("\\s+"), " ").orEmpty()
    if (normalized.isEmpty()) return null
    val scalarCount = normalized.codePointCount(0, normalized.length)
    if (scalarCount <= MAX_NOTIFICATION_NAME_SCALARS) return normalized
    val end = normalized.offsetByCodePoints(0, MAX_NOTIFICATION_NAME_SCALARS - 1)
    return normalized.substring(0, end) + "…"
}

internal fun productNotificationTag(
    category: ProductNotificationCategory,
    torrentId: String,
): String {
    val digest =
        MessageDigest
            .getInstance("SHA-256")
            .digest("${category.name}:$torrentId".toByteArray(Charsets.UTF_8))
            .joinToString("") { byte -> "%02x".format(byte) }
    return "rstorrent-${category.name.lowercase()}-$digest"
}

private fun increased(
    previous: ULong?,
    current: ULong?,
): Boolean = previous != null && current != null && current > previous

private const val MAX_NOTIFICATION_NAME_SCALARS = 120
