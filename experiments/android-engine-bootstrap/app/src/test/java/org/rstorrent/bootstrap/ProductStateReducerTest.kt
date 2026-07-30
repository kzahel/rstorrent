package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import org.rstorrent.session.uniffi.ActivePiece
import org.rstorrent.session.uniffi.IndexRange
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView
import org.rstorrent.session.uniffi.ViewPatch
import org.rstorrent.session.uniffi.ViewSnapshot
import org.rstorrent.session.uniffi.ViewUpdate
import org.rstorrent.session.uniffi.ViewUpdatePayload

class ProductStateReducerTest {
    @Test
    fun pieceRangesRemainExactAboveLegacyU16Boundary() {
        val snapshot =
            update(
                sequence = "1",
                baseRevision = "0",
                revision = "1",
                payload =
                    ViewUpdatePayload.Snapshot(
                        ViewSnapshot.PieceActivity(
                            TORRENT_ID,
                            100_000U,
                            listOf(IndexRange(65_534U, 65_537U)),
                            ActivePiece(
                                90_000U,
                                16_384U,
                                listOf(IndexRange(0U, 16_384U)),
                                listOf(IndexRange(0U, 8_192U)),
                                emptyList(),
                            ),
                        ),
                    ),
            )
        val patch =
            update(
                sequence = "2",
                baseRevision = "1",
                revision = "2",
                payload =
                    ViewUpdatePayload.Patch(
                        ViewPatch.PieceActivity(
                            TORRENT_ID,
                            100_000U,
                            listOf(IndexRange(99_998U, 100_000U)),
                            listOf(IndexRange(65_535U, 65_536U)),
                            null,
                        ),
                    ),
            )

        val state =
            ProductStateReducer.reduce(
                ProductStateReducer.reduce(ProductState(), snapshot),
                patch,
            )

        assertEquals(
            listOf(
                IndexRange(65_534U, 65_535U),
                IndexRange(65_536U, 65_537U),
                IndexRange(99_998U, 100_000U),
            ),
            state.pieces.getValue(TORRENT_ID).verified,
        )
        assertEquals(100_000U, state.pieces.getValue(TORRENT_ID).pieceCount)
    }

    @Test
    fun listPatchesConvergeAndSequenceGapsAreRejected() {
        val initial =
            ProductStateReducer.reduce(
                ProductState(),
                update(
                    sequence = "1",
                    baseRevision = "0",
                    revision = "7",
                    payload =
                        ViewUpdatePayload.Snapshot(
                            ViewSnapshot.TorrentList(
                                listOf(torrent("first", TorrentState.DOWNLOADING)),
                            ),
                        ),
                ),
            )
        val converged =
            ProductStateReducer.reduce(
                initial,
                update(
                    sequence = "2",
                    baseRevision = "7",
                    revision = "8",
                    payload =
                        ViewUpdatePayload.Patch(
                            ViewPatch.TorrentList(
                                listOf(torrent("second", TorrentState.PAUSED)),
                                listOf("first"),
                            ),
                        ),
                ),
            )

        assertEquals(setOf("second"), converged.torrents.keys)
        assertThrows(ViewContinuityException::class.java) {
            ProductStateReducer.reduce(
                converged,
                update(
                    sequence = "4",
                    baseRevision = "8",
                    revision = "9",
                    payload =
                        ViewUpdatePayload.Patch(
                            ViewPatch.TorrentList(emptyList(), emptyList()),
                        ),
                ),
            )
        }
    }

    private fun update(
        sequence: String,
        baseRevision: String,
        revision: String,
        payload: ViewUpdatePayload,
    ): ViewUpdate =
        ViewUpdate(
            1U.toUShort(),
            "stream-1",
            "epoch-1",
            sequence,
            baseRevision,
            revision,
            payload,
        )

    private fun torrent(
        id: String,
        state: TorrentState,
    ): TorrentView =
        TorrentView(
            id,
            state,
            true,
            100_000U,
            65_536U,
            "16384",
            "16384",
            "8192",
            null,
        )

    companion object {
        private const val TORRENT_ID = "0123456789abcdef0123456789abcdef01234567"
    }
}
