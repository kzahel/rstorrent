package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import org.rstorrent.session.uniffi.ActivePiece
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticEvent
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.IndexRange
import org.rstorrent.session.uniffi.ProgressAssessment
import org.rstorrent.session.uniffi.ProgressDisposition
import org.rstorrent.session.uniffi.ProgressPhase
import org.rstorrent.session.uniffi.ProgressReason
import org.rstorrent.session.uniffi.StorageState
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView
import org.rstorrent.session.uniffi.ViewPatch
import org.rstorrent.session.uniffi.ViewSnapshot
import org.rstorrent.session.uniffi.ViewUpdate
import org.rstorrent.session.uniffi.ViewUpdatePayload

class ProductStateReducerTest {
    @Test
    fun diagnosticSnapshotsAndPatchesRemainOrderedAndBounded() {
        val event =
            DiagnosticEvent(
                "7",
                "1000",
                DiagnosticSeverity.WARNING,
                DiagnosticCategory.DISCOVERY,
                "discovery_exhausted",
                TORRENT_ID,
                "No discovery source",
                emptyList(),
            )
        val snapshot =
            update(
                "1",
                "0",
                "0",
                ViewUpdatePayload.Snapshot(
                    ViewSnapshot.Diagnostics(listOf(event), "3"),
                ),
            )
        val patched =
            update(
                "2",
                "0",
                "0",
                ViewUpdatePayload.Patch(
                    ViewPatch.Diagnostics(
                        listOf(event.copy(sequence = "8", code = "retry_scheduled")),
                        "3",
                    ),
                ),
            )
        val reduced =
            ProductStateReducer.reduce(
                ProductStateReducer.reduce(ProductState(), snapshot),
                patched,
            )
        assertEquals(listOf("7", "8"), reduced.diagnostics.map { it.sequence })
        assertEquals("3", reduced.diagnosticDropped)
    }

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
            2U.toUShort(),
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
            StorageState.STAGING,
            true,
            100_000U,
            65_536U,
            "16384",
            "16384",
            "8192",
            ProgressAssessment(
                ProgressDisposition.ACTIVE,
                ProgressPhase.TRANSFER,
                ProgressReason.TRANSFERRING_PIECES,
                emptyList(),
            ),
            null,
        )

    companion object {
        private const val TORRENT_ID = "0123456789abcdef0123456789abcdef01234567"
    }
}
