package org.rstorrent.bootstrap

import org.rstorrent.session.uniffi.ActivePiece
import org.rstorrent.session.uniffi.DiagnosticEvent
import org.rstorrent.session.uniffi.IndexRange
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

data class ProductState(
    val ready: Boolean = false,
    val error: String? = null,
    val storageRootReady: Boolean = false,
    val storageRootLabel: String? = null,
    val selectedTorrent: String? = null,
    val torrents: Map<String, TorrentView> = emptyMap(),
    val pieces: Map<String, PieceActivityState> = emptyMap(),
    val diagnostics: List<DiagnosticEvent> = emptyList(),
    val diagnosticDropped: String = "0",
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
            is ViewSnapshot.Peers -> state
            is ViewSnapshot.SessionDisk -> state
            is ViewSnapshot.Files -> state
            is ViewSnapshot.Trackers -> state
            is ViewSnapshot.Diagnostics ->
                state.copy(
                    diagnostics = snapshot.events.takeLast(512),
                    diagnosticDropped = snapshot.droppedCount,
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
                state.copy(torrents = torrents)
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
            is ViewPatch.Peers -> state
            is ViewPatch.SessionDisk -> state
            is ViewPatch.Files -> state
            is ViewPatch.Trackers -> state
            is ViewPatch.Diagnostics -> {
                val events =
                    (state.diagnostics + patch.events)
                        .associateBy(DiagnosticEvent::sequence)
                        .values
                        .sortedBy { it.sequence.toULong() }
                        .takeLast(512)
                state.copy(
                    diagnostics = events,
                    diagnosticDropped = patch.droppedCount,
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
