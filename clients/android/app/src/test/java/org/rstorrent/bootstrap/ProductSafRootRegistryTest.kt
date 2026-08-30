package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ProductSafRootRegistryTest {
    @Test
    fun legacySingletonMigratesToDownloadsWithoutRequiringALiveGrant() {
        val legacy = "content://provider/tree/primary%3ADownload"

        val state = ProductSafRootRegistry.initialState(null, legacy)

        assertEquals(
            listOf(ProductSafRootGrant("downloads", "Downloads", legacy, 1)),
            state.roots,
        )
        assertNull(state.pending)
        assertNull(state.selectionCandidate)
    }

    @Test
    fun versionedRegistryRoundTripsPendingRepairAndRetainedRoots() {
        val first =
            ProductSafRootGrant(
                "root_a",
                "Folder A",
                "content://provider/tree/a",
                1,
            )
        val replacement =
            ProductSafRootGrant(
                "root_a",
                "Repaired A",
                "content://provider/tree/a2",
                2,
            )
        val state =
            ProductSafRootRegistryState(
                roots =
                    listOf(
                        replacement,
                        ProductSafRootGrant(
                            "root_b",
                            "Folder B",
                            "content://provider/tree/b",
                            1,
                        ),
                    ),
                pending =
                    ProductSafRootOperation(
                        ProductSafRootOperationKind.REPAIR,
                        "root_a",
                        "Repaired A",
                        replacement.treeUri,
                        false,
                        first,
                    ),
                selectionCandidate = replacement.treeUri,
            )

        assertEquals(state, ProductSafRootRegistryCodec.decode(ProductSafRootRegistryCodec.encode(state)))
    }

    @Test(expected = IllegalArgumentException::class)
    fun duplicateTreeUrisAreRejected() {
        ProductSafRootRegistryCodec.encode(
            ProductSafRootRegistryState(
                roots =
                    listOf(
                        ProductSafRootGrant("root_a", "A", "content://provider/tree/a", 1),
                        ProductSafRootGrant("root_b", "B", "content://provider/tree/a", 1),
                    ),
            ),
        )
    }
}
