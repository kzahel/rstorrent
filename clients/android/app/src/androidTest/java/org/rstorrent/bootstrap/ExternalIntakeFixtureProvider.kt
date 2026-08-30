package org.rstorrent.bootstrap

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.util.Base64
import java.io.FileNotFoundException
import java.io.IOException
import java.util.concurrent.atomic.AtomicInteger

class ExternalIntakeFixtureProvider : ContentProvider() {
    override fun onCreate(): Boolean = true

    override fun getType(uri: Uri): String =
        when (case(uri)) {
            DIRECTORY -> DocumentsContract.Document.MIME_TYPE_DIR
            GENERIC_NAME, GENERIC_REJECTED -> "application/octet-stream"
            else -> BITTORRENT_MIME_TYPE
        }

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor = queryResult(uri, projection)

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
        cancellationSignal: CancellationSignal?,
    ): Cursor = queryResult(uri, projection)

    override fun openFile(
        uri: Uri,
        mode: String,
    ): ParcelFileDescriptor = openFixture(uri, null)

    override fun openFile(
        uri: Uri,
        mode: String,
        signal: CancellationSignal?,
    ): ParcelFileDescriptor = openFixture(uri, signal)

    override fun call(
        method: String,
        arg: String?,
        extras: Bundle?,
    ): Bundle? =
        when (method) {
            "configure" -> {
                configuredPayload =
                    extras?.getString("payload_base64")?.let {
                        Base64.decode(it, Base64.DEFAULT)
                    } ?: DEFAULT_TORRENT
                delayedOpens.set(0)
                Bundle.EMPTY
            }
            "reset" -> {
                configuredPayload = DEFAULT_TORRENT
                delayedOpens.set(0)
                Bundle.EMPTY
            }
            else -> super.call(method, arg, extras)
        }

    override fun insert(
        uri: Uri,
        values: ContentValues?,
    ): Uri? = throw UnsupportedOperationException()

    override fun delete(
        uri: Uri,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = throw UnsupportedOperationException()

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = throw UnsupportedOperationException()

    private fun queryResult(
        uri: Uri,
        projection: Array<out String>?,
    ): Cursor {
        val columns = projection ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        val cursor = MatrixCursor(columns)
        val fixture = case(uri)
        val row = cursor.newRow()
        columns.forEach { column ->
            when (column) {
                OpenableColumns.DISPLAY_NAME ->
                    row.add(
                        when (fixture) {
                            GENERIC_NAME -> "generic-fixture.torrent"
                            GENERIC_REJECTED -> "generic-fixture.bin"
                            DIRECTORY -> "fixture-directory"
                            else -> "fixture.torrent"
                        },
                    )
                OpenableColumns.SIZE ->
                    row.add(
                        when (fixture) {
                            EMPTY -> 0L
                            OVERSIZED -> MAX_TORRENT_SOURCE_BYTES.toLong() + 1L
                            OVERSIZED_STREAM -> null
                            else -> configuredPayload.size.toLong()
                        },
                    )
                else -> row.add(null)
            }
        }
        return cursor
    }

    private fun openFixture(
        uri: Uri,
        signal: CancellationSignal?,
    ): ParcelFileDescriptor {
        val fixture = case(uri)
        if (fixture == DENIED) throw SecurityException("fixture denial")
        if (fixture == FAILING) throw FileNotFoundException("fixture failure")
        val pipe = ParcelFileDescriptor.createPipe()
        val read = pipe[0]
        val write = pipe[1]
        val writer =
            Thread {
                try {
                    ParcelFileDescriptor.AutoCloseOutputStream(write).use { output ->
                        when (fixture) {
                            EMPTY -> Unit
                            OVERSIZED, OVERSIZED_STREAM -> {
                                val buffer = ByteArray(16 * 1024)
                                var remaining = MAX_TORRENT_SOURCE_BYTES.toLong() + 1L
                                while (remaining > 0L) {
                                    val count = minOf(buffer.size.toLong(), remaining).toInt()
                                    output.write(buffer, 0, count)
                                    remaining -= count
                                }
                            }
                            DELAYED_ONCE -> {
                                if (delayedOpens.getAndIncrement() == 0) Thread.sleep(31_000L)
                                output.write(configuredPayload)
                            }
                            else -> output.write(configuredPayload)
                        }
                    }
                } catch (_: InterruptedException) {
                    runCatching { write.close() }
                } catch (_: IOException) {
                    Unit
                }
            }.apply {
                name = "external-intake-fixture-$fixture"
                isDaemon = true
            }
        signal?.setOnCancelListener {
            writer.interrupt()
            runCatching { write.close() }
        }
        writer.start()
        return read
    }

    private fun case(uri: Uri): String = uri.lastPathSegment ?: VALID

    companion object {
        const val VALID = "valid"
        const val EMPTY = "empty"
        const val OVERSIZED = "oversized"
        const val OVERSIZED_STREAM = "oversized-stream"
        const val DENIED = "denied"
        const val DELAYED_ONCE = "delayed-once"
        const val FAILING = "failing"
        const val DIRECTORY = "directory"
        const val GENERIC_NAME = "generic"
        const val GENERIC_REJECTED = "generic-rejected"

        private val DEFAULT_TORRENT =
            "d4:infod6:lengthi1e4:name7:fixture12:piece lengthi16384e6:pieces20:".toByteArray() +
                ByteArray(20) +
                "ee".toByteArray()
        private val delayedOpens = AtomicInteger()
        @Volatile private var configuredPayload = DEFAULT_TORRENT
    }
}
