package org.rstorrent.bootstrap

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProductCompanionPreferenceTest {
    @Test
    fun companionRequiresChromeOsAndExplicitEnablement() {
        assertFalse(ProductCompanionPreference.shouldStart(isChromeOs = false, enabled = false))
        assertFalse(ProductCompanionPreference.shouldStart(isChromeOs = false, enabled = true))
        assertFalse(ProductCompanionPreference.shouldStart(isChromeOs = true, enabled = false))
        assertTrue(ProductCompanionPreference.shouldStart(isChromeOs = true, enabled = true))
    }
}
