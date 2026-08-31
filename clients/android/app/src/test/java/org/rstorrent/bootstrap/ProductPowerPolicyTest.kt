package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Test
import org.rstorrent.session.uniffi.TorrentOperationalState

class ProductPowerPolicyTest {
    @Test
    fun onlyStartingDownloadingAndCheckingPreventSleep() {
        val expected =
            mapOf(
                TorrentOperationalState.QUEUED to false,
                TorrentOperationalState.STARTING to true,
                TorrentOperationalState.DOWNLOADING to true,
                TorrentOperationalState.CHECKING to true,
                TorrentOperationalState.STOPPING to false,
                TorrentOperationalState.SEEDING to false,
                TorrentOperationalState.PAUSED to false,
                TorrentOperationalState.ERROR to false,
            )
        expected.forEach { (state, required) ->
            assertEquals(state.name, required, requiresSleepInhibition(state))
        }
    }

    @Test
    fun metadataCompletePendingSelectionDoesNotPreventSleep() {
        assertEquals(
            false,
            requiresSleepInhibition(
                TorrentOperationalState.DOWNLOADING,
                awaitingFileSelection = true,
                metadataAvailable = true,
            ),
        )
        assertEquals(
            true,
            requiresSleepInhibition(
                TorrentOperationalState.DOWNLOADING,
                awaitingFileSelection = true,
                metadataAvailable = false,
            ),
        )
    }
}
