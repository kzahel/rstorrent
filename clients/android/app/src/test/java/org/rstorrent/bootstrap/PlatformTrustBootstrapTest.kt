package org.rstorrent.bootstrap

import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class PlatformTrustBootstrapTest {
    @Test
    fun initialization_is_once_only_across_concurrent_service_paths() {
        val once = OnceProcessInitializer()
        val calls = AtomicInteger()
        val ready = CountDownLatch(2)
        val release = CountDownLatch(1)
        val executor = Executors.newFixedThreadPool(2)

        repeat(2) {
            executor.submit {
                ready.countDown()
                release.await()
                once.run { calls.incrementAndGet() }
            }
        }
        check(ready.await(5, TimeUnit.SECONDS))
        release.countDown()
        executor.shutdown()
        check(executor.awaitTermination(5, TimeUnit.SECONDS))

        assertEquals(1, calls.get())
    }

    @Test
    fun failed_initialization_is_explicit_and_retryable() {
        val once = OnceProcessInitializer()
        val calls = AtomicInteger()

        assertThrows(IllegalStateException::class.java) {
            once.run {
                calls.incrementAndGet()
                error("scripted initialization failure")
            }
        }
        once.run { calls.incrementAndGet() }
        once.run { calls.incrementAndGet() }

        assertEquals(2, calls.get())
    }
}
