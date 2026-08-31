package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Test
import org.rstorrent.session.uniffi.ProgressAssessment
import org.rstorrent.session.uniffi.ProgressDisposition
import org.rstorrent.session.uniffi.ProgressPhase
import org.rstorrent.session.uniffi.ProgressReason
import org.rstorrent.session.uniffi.SeedAdmissionView
import org.rstorrent.session.uniffi.StorageState
import org.rstorrent.session.uniffi.TorrentEtaView
import org.rstorrent.session.uniffi.TorrentLifetimeView
import org.rstorrent.session.uniffi.TorrentOperationalState
import org.rstorrent.session.uniffi.TorrentProtocolIdentities
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentTransferLimits
import org.rstorrent.session.uniffi.TorrentSeedingView
import org.rstorrent.session.uniffi.TorrentView
import org.rstorrent.session.uniffi.TransferRateLimit

class ProductOngoingNotificationTest {
    @Test
    fun ongoingTextIsGenericAndCountBased() {
        assertEquals(
            ProductOngoingNotificationMessage(ProductOngoingNotificationMessage.Kind.OPENING_PROFILE),
            productOngoingNotificationMessage(ProductState()),
        )
        assertEquals(
            ProductOngoingNotificationMessage(ProductOngoingNotificationMessage.Kind.NEEDS_ATTENTION),
            productOngoingNotificationMessage(
                ProductState(
                    ready = false,
                    error = ProductError.Technical("/secret/path magnet:?xt=private"),
                ),
            ),
        )
        assertEquals(
            ProductOngoingNotificationMessage(
                ProductOngoingNotificationMessage.Kind.DOWNLOADING,
                1,
            ),
            productOngoingNotificationMessage(
                ProductState(ready = true, torrents = mapOf("one" to torrent("one"))),
            ),
        )
        assertEquals(
            ProductOngoingNotificationMessage(
                ProductOngoingNotificationMessage.Kind.DOWNLOADING,
                2,
            ),
            productOngoingNotificationMessage(
                ProductState(
                    ready = true,
                    torrents = mapOf("one" to torrent("one"), "two" to torrent("two")),
                ),
            ),
        )
        assertEquals(
            ProductOngoingNotificationMessage(ProductOngoingNotificationMessage.Kind.READY_CHROME),
            productOngoingNotificationMessage(ProductState(ready = true, companionPort = 9876U)),
        )
        assertEquals(
            ProductOngoingNotificationMessage(ProductOngoingNotificationMessage.Kind.READY),
            productOngoingNotificationMessage(ProductState(ready = true)),
        )
    }

    private fun torrent(id: String) =
        TorrentView(
            torrentId = id,
            protocolIdentities = TorrentProtocolIdentities(v1 = null, v2 = null),
            displayName = "Secret name",
            sourceDisplayName = null,
            state = TorrentState.DOWNLOADING,
            operationalState = TorrentOperationalState.DOWNLOADING,
            downloadQueuePosition = null,
            transferLimits =
                TorrentTransferLimits(TransferRateLimit.Unlimited, TransferRateLimit.Unlimited),
            storageState = StorageState.AVAILABLE,
            storageRoot = "secret-root",
            metadataAvailable = true,
            awaitingFileSelection = false,
            pendingFileSelectionPosition = null,
            fileCatalogId = null,
            selectableFileCount = 0U,
            selectedFileCount = 0U,
            selectableFileBytes = "0",
            selectedFileBytes = "0",
            pieceCount = 1U,
            totalSizeBytes = "1",
            verifiedPieceCount = 0U,
            requestedBytes = "1",
            receivedBytes = "1",
            storedBytes = "0",
            activePeerConnections = 1U,
            configuredTrackerCount = 1U,
            payloadDownloadRateBytes = "1",
            requiredPayloadBytes = "1",
            remainingPayloadBytes = "1",
            etaPayloadDownloadRateBytes = "1",
            eta = TorrentEtaView.Unavailable,
            lifetime = TorrentLifetimeView("0", "1", "0", "0", "0", "0"),
            seeding = TorrentSeedingView(SeedAdmissionView.INELIGIBLE, null),
            progress =
                ProgressAssessment(
                    ProgressDisposition.ACTIVE,
                    ProgressPhase.TRANSFER,
                    ProgressReason.TRANSFERRING_PIECES,
                    emptyList(),
                ),
            checking = null,
            archived = false,
            removalState = null,
            deleteDataSupported = true,
            forceRecheckAvailable = true,
            error = null,
        )
}
