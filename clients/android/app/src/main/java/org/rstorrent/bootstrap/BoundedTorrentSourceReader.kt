package org.rstorrent.bootstrap

import java.io.IOException
import java.io.InputStream
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.supervisorScope
import kotlinx.coroutines.withTimeoutOrNull

internal const val MAX_TORRENT_SOURCE_BYTES = 64 * 1024 * 1024
internal const val TORRENT_SOURCE_READ_TIMEOUT_MILLIS = 30_000L
private const val TORRENT_SOURCE_READ_BUFFER_BYTES = 16 * 1024
private const val MAX_CONSECUTIVE_EMPTY_READS = 16

internal sealed class TorrentSourceReadException : Exception()

internal class EmptyTorrentSourceException : TorrentSourceReadException()

internal class OversizedTorrentSourceException : TorrentSourceReadException()

internal class TorrentSourceReadTimeoutException : TorrentSourceReadException()

internal class TorrentSourceProviderException : TorrentSourceReadException()

internal class TorrentSourceCancelledException : TorrentSourceReadException()

internal data class BoundedTorrentSource(
    val bytes: ByteArray,
    val sourceBytes: Int,
    val peakOwnedBytes: Int,
)

internal object BoundedTorrentSourceReader {
    suspend fun read(
        openInput: () -> InputStream,
        knownLength: Long? = null,
        maximumBytes: Int = MAX_TORRENT_SOURCE_BYTES,
        timeoutMillis: Long = TORRENT_SOURCE_READ_TIMEOUT_MILLIS,
        cancelled: () -> Boolean = { false },
        onCancel: () -> Unit = {},
    ): BoundedTorrentSource {
        require(maximumBytes > 0)
        require(timeoutMillis > 0)
        if (knownLength != null && knownLength > maximumBytes.toLong()) {
            throw OversizedTorrentSourceException()
        }
        try {
            return supervisorScope {
                val read =
                    async(Dispatchers.IO) {
                        runInterruptible {
                            openInput().use { input ->
                                readBlocking(input, knownLength, maximumBytes, cancelled)
                            }
                        }
                    }
                val result = withTimeoutOrNull(timeoutMillis) { read.await() }
                if (result != null) {
                    result
                } else {
                    onCancel()
                    read.cancelAndJoin()
                    throw TorrentSourceReadTimeoutException()
                }
            }
        } catch (error: CancellationException) {
            onCancel()
            throw error
        } catch (error: TorrentSourceReadException) {
            throw error
        } catch (error: SecurityException) {
            throw error
        } catch (error: IOException) {
            throw TorrentSourceProviderException()
        }
    }

    private fun readBlocking(
        input: InputStream,
        knownLength: Long?,
        maximumBytes: Int,
        cancelled: () -> Boolean,
    ): BoundedTorrentSource {
        val initialCapacity =
            knownLength
                ?.takeIf { it in 1..maximumBytes.toLong() }
                ?.toInt()
                ?: minOf(TORRENT_SOURCE_READ_BUFFER_BYTES, maximumBytes)
        val output = BoundedByteAccumulator(initialCapacity, maximumBytes)
        val buffer = ByteArray(TORRENT_SOURCE_READ_BUFFER_BYTES)
        var emptyReads = 0
        while (true) {
            if (cancelled()) throw TorrentSourceCancelledException()
            val remainingProbe = maximumBytes - output.size + 1
            val requested = minOf(buffer.size, remainingProbe)
            val count = input.read(buffer, 0, requested)
            if (count < 0) break
            if (count == 0) {
                emptyReads += 1
                if (emptyReads > MAX_CONSECUTIVE_EMPTY_READS) {
                    throw TorrentSourceProviderException()
                }
                continue
            }
            emptyReads = 0
            if (output.size + count > maximumBytes) {
                throw OversizedTorrentSourceException()
            }
            output.append(buffer, count)
        }
        if (output.size == 0) throw EmptyTorrentSourceException()
        return output.finish()
    }

    private class BoundedByteAccumulator(
        initialCapacity: Int,
        private val maximumBytes: Int,
    ) {
        private var storage = ByteArray(initialCapacity)
        var size: Int = 0
            private set
        private var peak = storage.size

        fun append(
            source: ByteArray,
            count: Int,
        ) {
            val required = size + count
            ensureCapacity(required)
            source.copyInto(storage, destinationOffset = size, endIndex = count)
            size = required
        }

        fun finish(): BoundedTorrentSource {
            val bytes =
                if (storage.size == size) {
                    storage
                } else {
                    peak = maxOf(peak, storage.size + size)
                    storage.copyOf(size)
                }
            return BoundedTorrentSource(bytes, size, peak)
        }

        private fun ensureCapacity(required: Int) {
            if (required <= storage.size) return
            var capacity = storage.size.coerceAtLeast(1)
            while (capacity < required) {
                capacity = minOf(maximumBytes, maxOf(required, capacity * 2))
            }
            peak = maxOf(peak, storage.size + capacity)
            storage = storage.copyOf(capacity)
        }
    }
}
