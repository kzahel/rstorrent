package org.rstorrent.bootstrap

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class BootstrapContractTest {
    @Test
    fun acceptsBoundedRunIds() {
        assertEquals("run-01.avd", BootstrapContract.requireRunId("run-01.avd"))
    }

    @Test
    fun rejectsTraversalAndUnboundedRunIds() {
        assertThrows(IllegalArgumentException::class.java) {
            BootstrapContract.requireRunId("../escape")
        }
        assertThrows(IllegalArgumentException::class.java) {
            BootstrapContract.requireRunId("x".repeat(65))
        }
    }
}
