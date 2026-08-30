package org.rstorrent.bootstrap

import java.io.IOException
import java.io.InputStream
import java.util.concurrent.atomic.AtomicBoolean
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlinx.coroutines.runBlocking

class BoundedTorrentSourceReaderTest {
    @Test
    fun acceptsOneByteAndTheExact64MiBLimit() = runBlocking {
        val one = BoundedTorrentSourceReader.read({ GeneratingInputStream(1) }, knownLength = 1)
        assertEquals(1, one.sourceBytes)
        assertEquals(1, one.bytes.size)

        val exact =
            BoundedTorrentSourceReader.read(
                { GeneratingInputStream(MAX_TORRENT_SOURCE_BYTES.toLong()) },
                knownLength = MAX_TORRENT_SOURCE_BYTES.toLong(),
            )
        assertEquals(MAX_TORRENT_SOURCE_BYTES, exact.sourceBytes)
        assertEquals(MAX_TORRENT_SOURCE_BYTES, exact.bytes.size)
        assertTrue(exact.peakOwnedBytes <= MAX_TORRENT_SOURCE_BYTES * 2)
    }

    @Test
    fun rejectsEmptyAnd64MiBPlusOne() = runBlocking {
        expect<EmptyTorrentSourceException> {
            BoundedTorrentSourceReader.read({ GeneratingInputStream(0) })
        }
        expect<OversizedTorrentSourceException> {
            BoundedTorrentSourceReader.read(
                { GeneratingInputStream(MAX_TORRENT_SOURCE_BYTES.toLong() + 1L) },
            )
        }
    }

    @Test
    fun rejectsAKnownOversizedLengthWithoutOpening() = runBlocking {
        var opened = false
        expect<OversizedTorrentSourceException> {
            BoundedTorrentSourceReader.read(
                openInput = {
                    opened = true
                    GeneratingInputStream(1)
                },
                knownLength = MAX_TORRENT_SOURCE_BYTES.toLong() + 1L,
            )
        }
        assertFalse(opened)
    }

    @Test
    fun closesTheInputAfterReadFailureAndCancellation() = runBlocking {
        val failing = FailingInputStream()
        expect<TorrentSourceProviderException> {
            BoundedTorrentSourceReader.read({ failing })
        }
        assertTrue(failing.closed)

        val cancelled = GeneratingInputStream(1024)
        expect<TorrentSourceCancelledException> {
            BoundedTorrentSourceReader.read(
                openInput = { cancelled },
                cancelled = { cancelled.bytesRead > 0 },
            )
        }
        assertTrue(cancelled.closed)
    }

    @Test
    fun deadlineInterruptsAndClosesAHostileRead() = runBlocking {
        val interrupted = AtomicBoolean(false)
        val blocking = BlockingInputStream(interrupted)
        expect<TorrentSourceReadTimeoutException> {
            BoundedTorrentSourceReader.read(
                openInput = { blocking },
                timeoutMillis = 50,
            )
        }
        assertTrue(interrupted.get())
        assertTrue(blocking.closed)
    }

    private suspend inline fun <reified T : Throwable> expect(block: suspend () -> Unit) {
        try {
            block()
            throw AssertionError("expected ${T::class.simpleName}")
        } catch (error: Throwable) {
            if (error !is T) throw error
        }
    }

    private class GeneratingInputStream(private var remaining: Long) : InputStream() {
        var closed = false
        var bytesRead = 0L

        override fun read(): Int {
            if (remaining == 0L) return -1
            remaining -= 1
            bytesRead += 1
            return 0
        }

        override fun read(
            buffer: ByteArray,
            offset: Int,
            length: Int,
        ): Int {
            if (remaining == 0L) return -1
            val count = minOf(remaining, length.toLong()).toInt()
            buffer.fill(0, offset, offset + count)
            remaining -= count
            bytesRead += count
            return count
        }

        override fun close() {
            closed = true
        }
    }

    private class FailingInputStream : InputStream() {
        var closed = false

        override fun read(): Int = throw IOException("sentinel provider path")

        override fun close() {
            closed = true
        }
    }

    private class BlockingInputStream(private val interrupted: AtomicBoolean) : InputStream() {
        var closed = false

        override fun read(): Int {
            try {
                Thread.sleep(10_000)
            } catch (error: InterruptedException) {
                interrupted.set(true)
                throw IOException("interrupted")
            }
            return -1
        }

        override fun close() {
            closed = true
        }
    }
}
