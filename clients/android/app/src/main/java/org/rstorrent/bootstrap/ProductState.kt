package org.rstorrent.bootstrap

import org.rstorrent.session.uniffi.ActivePiece
import org.rstorrent.session.uniffi.ClientSettingsRuntimeView
import org.rstorrent.session.uniffi.CatalogPageView
import org.rstorrent.session.uniffi.DhtInspectionView
import org.rstorrent.session.uniffi.DiagnosticEvent
import org.rstorrent.session.uniffi.DiskPieceView
import org.rstorrent.session.uniffi.DiskPipelineView
import org.rstorrent.session.uniffi.FileCatalogState
import org.rstorrent.session.uniffi.FileView
import org.rstorrent.session.uniffi.IndexRange
import org.rstorrent.session.uniffi.PeerView
import org.rstorrent.session.uniffi.SpeedHistoryView
import org.rstorrent.session.uniffi.StorageSettingsSnapshot
import org.rstorrent.session.uniffi.SwarmCatalogState
import org.rstorrent.session.uniffi.SwarmCountsView
import org.rstorrent.session.uniffi.SwarmPeerView
import org.rstorrent.session.uniffi.TrackerCatalogState
import org.rstorrent.session.uniffi.TrackerView
import org.rstorrent.session.uniffi.TorrentView
import org.rstorrent.session.uniffi.ViewPatch
import org.rstorrent.session.uniffi.ViewSnapshot
import org.rstorrent.session.uniffi.ViewUpdate
import org.rstorrent.session.uniffi.ViewUpdatePayload

data class PieceActivityState(
    val torrentId: String,
    val pieceCount: UInt,
    val verified: List<IndexRange>,
    val active: List<ActivePiece>,
)

data class FileCatalogViewState(
    val state: FileCatalogState,
    val filesystemContentBase: String?,
    val page: CatalogPageView,
    val files: Map<String, FileView>,
)

data class TrackerCatalogViewState(
    val state: TrackerCatalogState,
    val page: CatalogPageView,
    val trackers: Map<String, TrackerView>,
)

data class SwarmViewState(
    val state: SwarmCatalogState,
    val capturedMillis: String,
    val maximumRecords: UInt,
    val counts: SwarmCountsView,
    val peers: Map<String, SwarmPeerView>,
)

data class DiskViewState(
    val pipeline: DiskPipelineView,
    val pieces: Map<String, DiskPieceView>,
)

data class ProductState(
    val ready: Boolean = false,
    val error: String? = null,
    val storageRootReady: Boolean = false,
    val storageRootLabel: String? = null,
    val preventSleepDuringActiveDownloads: Boolean = true,
    val selectedTorrent: String? = null,
    val torrents: Map<String, TorrentView> = emptyMap(),
    val storage: StorageSettingsSnapshot? = null,
    val clientSettings: ClientSettingsRuntimeView? = null,
    val pieces: Map<String, PieceActivityState> = emptyMap(),
    val files: Map<String, FileCatalogViewState> = emptyMap(),
    val trackers: Map<String, TrackerCatalogViewState> = emptyMap(),
    val peers: Map<String, Map<String, PeerView>> = emptyMap(),
    val swarms: Map<String, SwarmViewState> = emptyMap(),
    val disk: DiskViewState? = null,
    val dht: DhtInspectionView? = null,
    val speed: SpeedHistoryView? = null,
    val diagnostics: List<DiagnosticEvent> = emptyList(),
    val diagnosticSourceEvicted: String = "0",
    val diagnosticLocalEvicted: ULong = 0UL,
    val diagnosticResets: ULong = 0UL,
    internal val streams: Map<String, StreamPosition> = emptyMap(),
)

data class StreamPosition(
    val epoch: String,
    val sequence: String,
    val revision: String,
)

internal class ViewContinuityException(message: String) : Exception(message)

internal class ViewResetRequiredException : Exception("view stream requires a snapshot")

internal object ProductStateReducer {
    fun reduce(
        state: ProductState,
        update: ViewUpdate,
    ): ProductState {
        require(update.contractVersion == 2.toUShort()) {
            "unsupported view contract ${update.contractVersion}"
        }
        val current = state.streams[update.streamId]
        when (val payload = update.payload) {
            is ViewUpdatePayload.ResetRequired -> throw ViewResetRequiredException()
            is ViewUpdatePayload.Patch -> {
                if (
                    current == null ||
                    current.epoch != update.epoch ||
                    update.sequence.toULong() != current.sequence.toULong() + 1UL ||
                    current.revision != update.baseRevision
                ) {
                    throw ViewContinuityException("view patch does not continue its stream")
                }
                return applyPatch(state.withPosition(update), payload.patch)
            }
            is ViewUpdatePayload.Snapshot ->
                return applySnapshot(state.withPosition(update), payload.snapshot)
        }
    }

    private fun ProductState.withPosition(update: ViewUpdate): ProductState =
        copy(
            ready = true,
            error = null,
            streams =
                streams +
                    (
                        update.streamId to
                            StreamPosition(
                                update.epoch,
                                update.sequence,
                                update.revision,
                            )
                    ),
        )

    private fun applySnapshot(
        state: ProductState,
        snapshot: ViewSnapshot,
    ): ProductState =
        when (snapshot) {
            is ViewSnapshot.TorrentList ->
                state.copy(
                    torrents = snapshot.torrents.associateBy(TorrentView::torrentId),
                    storage = snapshot.storage,
                    clientSettings = snapshot.clientSettings,
                )
            is ViewSnapshot.Torrent -> {
                val torrent = snapshot.torrent
                if (torrent == null) {
                    state
                } else {
                    state.copy(torrents = state.torrents + (torrent.torrentId to torrent))
                }
            }
            is ViewSnapshot.PieceActivity ->
                state.copy(
                    pieces =
                        state.pieces +
                            (
                                snapshot.torrentId to
                                    PieceActivityState(
                                        snapshot.torrentId,
                                        snapshot.pieceCount,
                                        snapshot.verified,
                                        snapshot.active,
                                    )
                            ),
                )
            is ViewSnapshot.Peers ->
                state.copy(
                    peers = state.peers + (snapshot.torrentId to snapshot.peers.associateBy(PeerView::connectionId)),
                )
            is ViewSnapshot.Swarm ->
                state.copy(
                    swarms =
                        state.swarms +
                            (
                                snapshot.torrentId to
                                    SwarmViewState(
                                        snapshot.state,
                                        snapshot.capturedMillis,
                                        snapshot.maximumRecords,
                                        snapshot.counts,
                                        snapshot.peers.associateBy(SwarmPeerView::peerRecordId),
                                    )
                            ),
                )
            is ViewSnapshot.SessionDisk ->
                state.copy(
                    disk = DiskViewState(snapshot.pipeline, snapshot.pieces.associateBy(DiskPieceView::rowId)),
                )
            is ViewSnapshot.SessionDht -> state.copy(dht = snapshot.inspection)
            is ViewSnapshot.SessionSpeed -> state.copy(speed = snapshot.history)
            is ViewSnapshot.Files ->
                state.copy(
                    files =
                        state.files +
                            (
                                snapshot.torrentId to
                                    FileCatalogViewState(
                                        snapshot.state,
                                        snapshot.filesystemContentBase,
                                        snapshot.page,
                                        snapshot.files.associateBy(FileView::fileId),
                                    )
                            ),
                )
            is ViewSnapshot.Trackers ->
                state.copy(
                    trackers =
                        state.trackers +
                            (
                                snapshot.torrentId to
                                    TrackerCatalogViewState(
                                        snapshot.state,
                                        snapshot.page,
                                        snapshot.trackers.associateBy(TrackerView::trackerId),
                                    )
                            ),
                )
            is ViewSnapshot.Diagnostics ->
                state.copy(
                    diagnostics = snapshot.events.takeLast(512),
                    diagnosticSourceEvicted = snapshot.retention.sourceEvictedCount,
                    diagnosticLocalEvicted =
                        state.diagnosticLocalEvicted +
                            (snapshot.events.size - 512).coerceAtLeast(0).toULong(),
                )
        }

    private fun applyPatch(
        state: ProductState,
        patch: ViewPatch,
    ): ProductState =
        when (patch) {
            is ViewPatch.TorrentList -> {
                val torrents = state.torrents.toMutableMap()
                patch.removed.forEach(torrents::remove)
                patch.upsert.forEach { torrents[it.torrentId] = it }
                state.copy(
                    torrents = torrents,
                    storage = patch.storage ?: state.storage,
                    clientSettings = patch.clientSettings ?: state.clientSettings,
                )
            }
            is ViewPatch.Torrent -> {
                val torrent = patch.torrent
                if (torrent == null) {
                    state
                } else {
                    state.copy(torrents = state.torrents + (torrent.torrentId to torrent))
                }
            }
            is ViewPatch.PieceActivity -> {
                var verified = state.pieces[patch.torrentId]?.verified.orEmpty()
                patch.cleared.forEach { verified = removeRange(verified, it) }
                patch.verified.forEach { verified = insertRange(verified, it) }
                val active =
                    state.pieces[patch.torrentId]
                        ?.active
                        .orEmpty()
                        .associateByTo(mutableMapOf(), ActivePiece::pieceId)
                patch.activeRemoved.forEach(active::remove)
                patch.activeUpsert.forEach { active[it.pieceId] = it }
                state.copy(
                    pieces =
                        state.pieces +
                            (
                                patch.torrentId to
                                    PieceActivityState(
                                        patch.torrentId,
                                        patch.pieceCount,
                                        verified,
                                        active.values.sortedWith(
                                            compareBy(ActivePiece::pieceIndex)
                                                .thenBy(ActivePiece::attempt),
                                        ),
                                    )
                            ),
                )
            }
            is ViewPatch.Peers -> {
                val peers = state.peers[patch.torrentId].orEmpty().toMutableMap()
                patch.removed.forEach(peers::remove)
                patch.upsert.forEach { peers[it.connectionId] = it }
                state.copy(peers = state.peers + (patch.torrentId to peers))
            }
            is ViewPatch.Swarm -> {
                val peers = state.swarms[patch.torrentId]?.peers.orEmpty().toMutableMap()
                patch.removed.forEach(peers::remove)
                patch.upsert.forEach { peers[it.peerRecordId] = it }
                state.copy(
                    swarms =
                        state.swarms +
                            (
                                patch.torrentId to
                                    SwarmViewState(
                                        patch.state,
                                        patch.capturedMillis,
                                        patch.maximumRecords,
                                        patch.counts,
                                        peers,
                                    )
                            ),
                )
            }
            is ViewPatch.SessionDisk -> {
                val pieces = state.disk?.pieces.orEmpty().toMutableMap()
                patch.removed.forEach(pieces::remove)
                patch.upsert.forEach { pieces[it.rowId] = it }
                state.copy(disk = DiskViewState(patch.pipeline, pieces))
            }
            is ViewPatch.SessionDht -> state.copy(dht = patch.inspection)
            is ViewPatch.SessionSpeed -> state.copy(speed = patch.history)
            is ViewPatch.Files -> {
                val current = state.files[patch.torrentId] ?: return state
                val files = current.files.toMutableMap()
                patch.removed.forEach(files::remove)
                patch.upsert.forEach { files[it.fileId] = it }
                state.copy(files = state.files + (patch.torrentId to current.copy(files = files)))
            }
            is ViewPatch.Trackers -> {
                val current = state.trackers[patch.torrentId] ?: return state
                val trackers = current.trackers.toMutableMap()
                patch.removed.forEach(trackers::remove)
                patch.upsert.forEach { trackers[it.trackerId] = it }
                state.copy(
                    trackers = state.trackers + (patch.torrentId to current.copy(trackers = trackers)),
                )
            }
            is ViewPatch.Diagnostics -> {
                val events =
                    (state.diagnostics + patch.events)
                        .associateBy(DiagnosticEvent::sequence)
                        .values
                        .sortedBy { it.sequence.toULong() }
                val retained = events.takeLast(512)
                state.copy(
                    diagnostics = retained,
                    diagnosticSourceEvicted = patch.retention.sourceEvictedCount,
                    diagnosticLocalEvicted =
                        state.diagnosticLocalEvicted + (events.size - retained.size).toULong(),
                )
            }
        }

    private fun insertRange(
        ranges: List<IndexRange>,
        inserted: IndexRange,
    ): List<IndexRange> {
        val output = mutableListOf<IndexRange>()
        var start = inserted.start
        var end = inserted.endExclusive
        var placed = false
        for (range in ranges) {
            when {
                range.endExclusive < start -> output += range
                end < range.start -> {
                    if (!placed) {
                        output += IndexRange(start, end)
                        placed = true
                    }
                    output += range
                }
                else -> {
                    start = minOf(start, range.start)
                    end = maxOf(end, range.endExclusive)
                }
            }
        }
        if (!placed) output += IndexRange(start, end)
        return output
    }

    private fun removeRange(
        ranges: List<IndexRange>,
        removed: IndexRange,
    ): List<IndexRange> =
        buildList {
            for (range in ranges) {
                if (
                    range.endExclusive <= removed.start ||
                    range.start >= removed.endExclusive
                ) {
                    add(range)
                    continue
                }
                if (range.start < removed.start) {
                    add(IndexRange(range.start, removed.start))
                }
                if (range.endExclusive > removed.endExclusive) {
                    add(IndexRange(removed.endExclusive, range.endExclusive))
                }
            }
        }
}
