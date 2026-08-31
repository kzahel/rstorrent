package org.rstorrent.bootstrap

import java.math.BigInteger
import org.rstorrent.session.uniffi.FileIndexRange
import org.rstorrent.session.uniffi.FileSelectionOverride
import org.rstorrent.session.uniffi.FileSelectionView
import org.rstorrent.session.uniffi.FileView
import org.rstorrent.session.uniffi.PendingFileSelectionBase
import org.rstorrent.session.uniffi.TorrentView

internal const val MAX_PENDING_FILE_SELECTION_OVERRIDES = 4_096

internal data class PendingFileSelectionSummary(
    val count: Long,
    val bytes: BigInteger,
)

internal data class PendingFileOverride(
    val selected: Boolean,
    val initialSelected: Boolean,
    val lengthBytes: BigInteger,
)

internal data class PendingFileSelectionDraft(
    val base: PendingFileSelectionBase = PendingFileSelectionBase.CURRENT,
    private val overrides: Map<UInt, PendingFileOverride> = emptyMap(),
) {
    fun selected(file: FileView): Boolean =
        overrides[file.fileIndex]?.selected
            ?: when (base) {
                PendingFileSelectionBase.ALL -> true
                PendingFileSelectionBase.NONE -> false
                PendingFileSelectionBase.CURRENT -> file.selection != FileSelectionView.SKIPPED
            }

    /** Returns null when adding this exception would exceed the closed draft bound. */
    fun toggle(file: FileView): PendingFileSelectionDraft? {
        require(!file.padding) { "padding files are not selectable" }
        val nextSelected = !selected(file)
        val baseline = baseline(file)
        val next = overrides.toMutableMap()
        if (nextSelected == baseline) {
            next.remove(file.fileIndex)
        } else {
            if (file.fileIndex !in next && next.size >= MAX_PENDING_FILE_SELECTION_OVERRIDES) {
                return null
            }
            next[file.fileIndex] =
                PendingFileOverride(
                    selected = nextSelected,
                    initialSelected = file.selection != FileSelectionView.SKIPPED,
                    lengthBytes = file.lengthBytes.toBigInteger(),
                )
        }
        return copy(overrides = next)
    }

    fun selectAll(): PendingFileSelectionDraft =
        PendingFileSelectionDraft(PendingFileSelectionBase.ALL)

    fun selectNone(): PendingFileSelectionDraft =
        PendingFileSelectionDraft(PendingFileSelectionBase.NONE)

    fun summary(torrent: TorrentView): PendingFileSelectionSummary {
        var count =
            when (base) {
                PendingFileSelectionBase.ALL -> torrent.selectableFileCount.toLong()
                PendingFileSelectionBase.NONE -> 0L
                PendingFileSelectionBase.CURRENT -> torrent.selectedFileCount.toLong()
            }
        var bytes =
            when (base) {
                PendingFileSelectionBase.ALL -> torrent.selectableFileBytes.toBigInteger()
                PendingFileSelectionBase.NONE -> BigInteger.ZERO
                PendingFileSelectionBase.CURRENT -> torrent.selectedFileBytes.toBigInteger()
            }
        overrides.values.forEach { override ->
            val baseline =
                when (base) {
                    PendingFileSelectionBase.ALL -> true
                    PendingFileSelectionBase.NONE -> false
                    PendingFileSelectionBase.CURRENT -> override.initialSelected
                }
            if (override.selected != baseline) {
                if (override.selected) {
                    count += 1
                    bytes += override.lengthBytes
                } else {
                    count -= 1
                    bytes -= override.lengthBytes
                }
            }
        }
        return PendingFileSelectionSummary(count, bytes)
    }

    fun compactOverrides(): List<FileSelectionOverride> {
        val compact = mutableListOf<FileSelectionOverride>()
        overrides.entries.sortedBy(Map.Entry<UInt, PendingFileOverride>::key).forEach { entry ->
            val previous = compact.lastOrNull()
            if (
                previous != null &&
                previous.selected == entry.value.selected &&
                previous.range.endExclusive == entry.key
            ) {
                previous.range = previous.range.copy(endExclusive = entry.key + 1U)
            } else {
                compact +=
                    FileSelectionOverride(
                        FileIndexRange(entry.key, entry.key + 1U),
                        entry.value.selected,
                    )
            }
        }
        return compact
    }

    private fun baseline(file: FileView): Boolean =
        when (base) {
            PendingFileSelectionBase.ALL -> true
            PendingFileSelectionBase.NONE -> false
            PendingFileSelectionBase.CURRENT -> file.selection != FileSelectionView.SKIPPED
        }
}
