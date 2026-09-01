package org.rstorrent.bootstrap

import java.io.File
import java.nio.file.Files
import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ProductDataResetTest {
    @Test
    fun journalRoundTripsBoundsProgressFailureAndExplicitDowngrade() {
        val journal =
            ProductDataResetJournal.capture(
                deleteData = true,
                torrentIds = listOf(TORRENT_A, TORRENT_B),
                roots =
                    listOf(
                        ProductSafRootGrant(
                            "root_a",
                            "Folder A",
                            "content://provider/tree/a",
                            1,
                        ),
                    ),
                operationId = "123e4567-e89b-12d3-a456-426614174000",
            ).copy(
                deleteRemainingData = false,
                nextTorrentIndex = 1,
                phase = ProductDataResetPhase.REMOVING_TORRENTS,
                failure = ProductDataResetFailure("provider_refused", "delete failed", TORRENT_B),
            )

        val encoded = ProductDataResetJournalCodec.encode(journal)
        assertTrue(encoded.length <= ProductDataResetJournal.MAX_ENCODED_BYTES)
        val decoded = ProductDataResetJournalCodec.decode(encoded)
        assertEquals(journal, decoded)
        assertTrue(decoded.downgradedToKeep)
        assertEquals(1, decoded.completedTorrentCount)
    }

    @Test
    fun journalRejectsCorruptionFutureVersionDuplicatesAndOversizedFailure() {
        val journal =
            ProductDataResetJournal.capture(
                deleteData = false,
                torrentIds = listOf(TORRENT_A),
                roots = emptyList(),
                operationId = "123e4567-e89b-12d3-a456-426614174000",
            )
        val encoded = ProductDataResetJournalCodec.encode(journal)
        val corrupted = encoded.toCharArray().also { it[it.lastIndex] = if (it.last() == 'A') 'B' else 'A' }
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournalCodec.decode(corrupted.concatToString())
        }

        val decoded = Base64.getUrlDecoder().decode(encoded)
        decoded[3] = 2
        val future = Base64.getUrlEncoder().withoutPadding().encodeToString(decoded)
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournalCodec.decode(future)
        }
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournal.capture(
                deleteData = false,
                torrentIds = listOf(TORRENT_A, TORRENT_A),
                roots = emptyList(),
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournalCodec.encode(
                journal.copy(
                    failure = ProductDataResetFailure("failure", "x".repeat(513)),
                ),
            )
        }
    }

    @Test
    fun fixedPrivateProfileResetPreservesSiblingsAndRejectsSymlinks() {
        val root = Files.createTempDirectory("rstorrent-data-reset").toFile()
        try {
            val profile = File(root, ProductPrivateProfileReset.PROFILE_DIRECTORY)
            val nested = File(profile, "default/nested")
            assertTrue(nested.mkdirs())
            File(nested, "session.db").writeText("profile")
            val sibling = File(root, "unrelated.txt")
            sibling.writeText("preserve")

            ProductPrivateProfileReset.reset(root, profile)

            assertTrue(profile.isDirectory)
            assertEquals(0, profile.listFiles()?.size)
            assertEquals("preserve", sibling.readText())

            val outside = File(root.parentFile, "outside-profile")
            assertThrows(IllegalArgumentException::class.java) {
                ProductPrivateProfileReset.reset(root, outside)
            }
            assertTrue(profile.delete())
            if (runCatching { Files.createSymbolicLink(profile.toPath(), sibling.toPath()) }.isSuccess) {
                assertTrue(Files.isSymbolicLink(profile.toPath()))
                assertThrows(IllegalArgumentException::class.java) {
                    ProductPrivateProfileReset.reset(root, profile)
                }
            }
        } finally {
            root.deleteRecursively()
        }
    }

    private companion object {
        const val TORRENT_A = "t1-000102030405060708090a0b0c0d0e0f"
        const val TORRENT_B = "t1-101112131415161718191a1b1c1d1e1f"
    }
}
