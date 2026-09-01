package org.rstorrent.bootstrap

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProductForegroundSessionEpochTest {
    @Test
    fun countsColdProductAndHomeReentryButNotDuplicatePresentation() {
        ProductForegroundSessionEpoch.onProcessStart()
        ProductForegroundSessionEpoch.showProductSurface()
        assertTrue(ProductForegroundSessionEpoch.claimCurrent())
        assertFalse(ProductForegroundSessionEpoch.claimCurrent())

        ProductForegroundSessionEpoch.showProductSurface()
        assertFalse(ProductForegroundSessionEpoch.claimCurrent())

        ProductForegroundSessionEpoch.onProcessStop()
        ProductForegroundSessionEpoch.onProcessStart()
        assertTrue(ProductForegroundSessionEpoch.claimCurrent())
        assertFalse(ProductForegroundSessionEpoch.claimCurrent())

        ProductForegroundSessionEpoch.hideProductSurface()
        ProductForegroundSessionEpoch.onProcessStop()
    }
}
