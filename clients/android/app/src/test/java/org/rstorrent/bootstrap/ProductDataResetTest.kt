package org.rstorrent.bootstrap

import java.io.File
import java.nio.ByteBuffer
import java.nio.file.Files
import java.util.Base64
import java.util.zip.CRC32
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class ProductDataResetTest {
    @Test
    fun journalAcceptsExactTargetAndRootBoundsAndRejectsOneMore() {
        val torrents =
            List(ProductDataResetJournal.MAX_TORRENTS) { index ->
                "t1-${index.toString(16).padStart(32, '0')}"
            }
        val roots =
            List(ProductSafRootRegistry.MAX_ROOTS) { index ->
                ProductSafRootGrant(
                    rootId = "root_$index",
                    label = "Folder $index",
                    treeUri = "content://provider/tree/$index",
                    generation = 1,
                )
            }
        val journal = ProductDataResetJournal.capture(true, torrents, roots)
        val encoded = ProductDataResetJournalCodec.encode(journal)

        assertEquals(27_135, encoded.toByteArray(Charsets.US_ASCII).size)
        assertTrue(encoded.toByteArray(Charsets.US_ASCII).size <= ProductDataResetJournal.MAX_ENCODED_BYTES)
        assertEquals(journal, ProductDataResetJournalCodec.decode(encoded))
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournal.capture(
                true,
                torrents + "t1-${ProductDataResetJournal.MAX_TORRENTS.toString(16).padStart(32, '0')}",
                roots,
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournal.capture(
                true,
                torrents,
                roots +
                    ProductSafRootGrant(
                        rootId = "root_overflow",
                        label = "Overflow",
                        treeUri = "content://provider/tree/overflow",
                        generation = 1,
                    ),
            )
        }
    }

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

        val futureBytes = Base64.getUrlDecoder().decode(encoded)
        ByteBuffer.wrap(futureBytes).putInt(3)
        val payloadSize = futureBytes.size - Long.SIZE_BYTES
        val checksum = CRC32().apply { update(futureBytes, 0, payloadSize) }.value
        ByteBuffer.wrap(futureBytes, payloadSize, Long.SIZE_BYTES).putLong(checksum)
        val future = Base64.getUrlEncoder().withoutPadding().encodeToString(futureBytes)
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
    fun journalRejectsSkippedPhaseWorkAndMismatchedFailureTarget() {
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
            )

        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournalCodec.encode(
                journal.copy(phase = ProductDataResetPhase.RESETTING_PROFILE),
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournalCodec.encode(
                journal.copy(
                    nextTorrentIndex = 1,
                    failure = ProductDataResetFailure("delete_failed", "failed", TORRENT_A),
                ),
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournalCodec.encode(
                journal.copy(
                    phase = ProductDataResetPhase.RESETTING_PREFERENCES,
                    nextTorrentIndex = 2,
                ),
            )
        }
        val retry = journal.copy(nextTorrentIndex = 1, retryCurrentTorrent = true)
        assertEquals(retry, ProductDataResetJournalCodec.decode(ProductDataResetJournalCodec.encode(retry)))
        assertThrows(IllegalArgumentException::class.java) {
            ProductDataResetJournalCodec.encode(
                retry.copy(
                    phase = ProductDataResetPhase.RESETTING_PROFILE,
                    nextTorrentIndex = 2,
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
