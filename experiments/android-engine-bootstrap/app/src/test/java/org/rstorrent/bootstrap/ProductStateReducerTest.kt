package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import org.rstorrent.session.uniffi.ActivePiece
import org.rstorrent.session.uniffi.ActivePieceStageView
import org.rstorrent.session.uniffi.CatalogPageRequest
import org.rstorrent.session.uniffi.CatalogPageView
import org.rstorrent.session.uniffi.Command
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticEvent
import org.rstorrent.session.uniffi.DiagnosticRetention
import org.rstorrent.session.uniffi.DiagnosticSeverity
import org.rstorrent.session.uniffi.DeliveryPolicy
import org.rstorrent.session.uniffi.FileCatalogState
import org.rstorrent.session.uniffi.FileIndexRange
import org.rstorrent.session.uniffi.FilePriority
import org.rstorrent.session.uniffi.IndexRange
import org.rstorrent.session.uniffi.ProgressAssessment
import org.rstorrent.session.uniffi.ProgressDisposition
import org.rstorrent.session.uniffi.ProgressPhase
import org.rstorrent.session.uniffi.ProgressReason
import org.rstorrent.session.uniffi.StorageState
import org.rstorrent.session.uniffi.StorageSettingsSnapshot
import org.rstorrent.session.uniffi.SubscriptionSpec
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView
import org.rstorrent.session.uniffi.ViewPatch
import org.rstorrent.session.uniffi.ViewProjection
import org.rstorrent.session.uniffi.ViewSelector
import org.rstorrent.session.uniffi.ViewSnapshot
import org.rstorrent.session.uniffi.ViewUpdate
import org.rstorrent.session.uniffi.ViewUpdatePayload

class ProductStateReducerTest {
    @Test
    fun highCardinalityCatalogUsesRangesAndPagesInTheKotlinContract() {
        val fileCount = 374_998U
        val command =
            Command.SetFilePriorityRanges(
                TORRENT_ID,
                listOf(FileIndexRange(0U, fileCount)),
                FilePriority.NORMAL,
            )
        val subscription =
            SubscriptionSpec(
                ViewSelector.Torrent(TORRENT_ID),
                ViewProjection.FILES,
                DeliveryPolicy(0U, 256U * 1024U),
                null,
                CatalogPageRequest(fileCount - 1_024U, 1_024U),
            )
        val snapshot =
            ViewSnapshot.Files(
                TORRENT_ID,
                FileCatalogState.AVAILABLE,
                null,
                CatalogPageView(fileCount - 1_024U, 1_024U, fileCount, null),
                emptyList(),
            )

        assertEquals(1, command.ranges.size)
        assertEquals(fileCount, command.ranges.single().endExclusive)
        assertEquals(1_024U, subscription.catalogPage?.limit)
        assertEquals(fileCount, snapshot.page.total)
        assertEquals(emptyList<Any>(), snapshot.files)
    }

    @Test
    fun safRemovalUsesOnlyTheThreeNativeManagedArtifactRoles() {
        assertEquals(
            listOf("test", ".test.rstorrent-staging", ".test.rstorrent-parts"),
            managedRemovalNames("test"),
        )
    }

    @Test
    fun safRemovalIsRepeatableAndTreatsMissingArtifactsAsSuccess() {
        val documents = managedRemovalNames("test").toMutableSet()
        val deleted = mutableListOf<String>()
        repeat(2) {
            deleteManagedArtifacts(
                "test",
                find = { name -> name.takeIf(documents::contains) },
                delete = { name ->
                    deleted += name
                    documents.remove(name)
                },
            )
        }
        assertEquals(managedRemovalNames("test"), deleted)
        assertEquals(emptySet<String>(), documents)
    }

    @Test
    fun safRemovalSurfacesProviderRefusalWithoutContinuing() {
        val attempted = mutableListOf<String>()
        assertThrows(IllegalStateException::class.java) {
            deleteManagedArtifacts(
                "test",
                find = { it },
                delete = { name ->
                    attempted += name
                    false
                },
            )
        }
        assertEquals(listOf("test"), attempted)
    }

    @Test
    fun diagnosticSnapshotsAndPatchesRemainOrderedAndBounded() {
        val event =
            DiagnosticEvent(
                "7",
                "1000",
                DiagnosticSeverity.WARNING,
                DiagnosticCategory("discovery.peer"),
                "discovery_exhausted",
                TORRENT_ID,
                "No discovery source",
                emptyList(),
                emptyList(),
            )
        val snapshot =
            update(
                "1",
                "0",
                "0",
                ViewUpdatePayload.Snapshot(
                    ViewSnapshot.Diagnostics(listOf(event), DiagnosticRetention("3", "4")),
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
                        DiagnosticRetention("3", "4"),
                    ),
                ),
            )
        val reduced =
            ProductStateReducer.reduce(
                ProductStateReducer.reduce(ProductState(), snapshot),
                patched,
            )
        assertEquals(listOf("7", "8"), reduced.diagnostics.map { it.sequence })
        assertEquals("3", reduced.diagnosticSourceEvicted)
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
                            listOf(
                                ActivePiece(
                                    "90000:1",
                                    90_000U,
                                    1U,
                                    16_384U,
                                    ActivePieceStageView.RECEIVED,
                                    listOf(IndexRange(0U, 16_384U)),
                                    listOf(IndexRange(0U, 8_192U)),
                                    emptyList(),
                                    "10",
                                    null,
                                ),
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
                            emptyList(),
                            listOf("90000:1"),
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
        assertEquals(emptyList<ActivePiece>(), state.pieces.getValue(TORRENT_ID).active)
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
                                storage(),
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
                                null,
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
                            ViewPatch.TorrentList(emptyList(), emptyList(), null),
                        ),
                ),
            )
        }
    }

    private fun storage(): StorageSettingsSnapshot =
        StorageSettingsSnapshot(emptyList(), null, false)

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
            torrentId = id,
            displayName = "Verified torrent",
            state = state,
            storageState = StorageState.STAGING,
            metadataAvailable = true,
            pieceCount = 100_000U,
            verifiedPieceCount = 65_536U,
            requestedBytes = "16384",
            receivedBytes = "16384",
            storedBytes = "8192",
            activePeerConnections = 0U,
            configuredTrackerCount = 2U,
            payloadDownloadRateBytes = "0",
            progress = ProgressAssessment(
                ProgressDisposition.ACTIVE,
                ProgressPhase.TRANSFER,
                ProgressReason.TRANSFERRING_PIECES,
                emptyList(),
            ),
            archived = false,
            removalState = null,
            deleteManagedDataSupported = true,
            error = null,
        )

    companion object {
        private const val TORRENT_ID = "0123456789abcdef0123456789abcdef01234567"
    }
}
