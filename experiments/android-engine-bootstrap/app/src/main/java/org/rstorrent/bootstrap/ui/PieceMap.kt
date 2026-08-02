/*
 * Adapted from the author's earlier JSTorrent PieceMap.kt at commit
 * 0cad4dacf540f5be42ee53c4f1e1da27aa1b3685.
 */
package org.rstorrent.bootstrap.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import kotlin.math.ceil
import kotlin.math.min
import org.rstorrent.session.uniffi.ActivePiece
import org.rstorrent.session.uniffi.IndexRange

@Composable
fun PieceMap(
    piecesTotal: UInt,
    verified: List<IndexRange>,
    active: List<ActivePiece>,
    modifier: Modifier = Modifier,
) {
    val verifiedColor = MaterialTheme.colorScheme.primary
    val emptyColor = MaterialTheme.colorScheme.surfaceVariant
    val partialColor = Color(0xFFFF9800)
    val requestedColor = Color(0xFF00BCD4)
    val respondedColor = Color(0xFF4CAF50)
    val geometry =
        remember(piecesTotal) {
            val total = piecesTotal.toULong()
            val cells = min(total, MAX_RENDERED_CELLS.toULong()).toInt()
            val columns =
                when {
                    cells <= 10 -> cells
                    cells <= 50 -> min(cells, 25)
                    cells <= 200 -> min(cells, 40)
                    cells <= 1_000 -> 50
                    else -> 80
                }
            val rows = if (columns == 0) 0 else ceil(cells.toDouble() / columns).toInt()
            RenderGeometry(cells, columns, rows)
        }
    val height = (geometry.rows * 6 + 8).dp

    Canvas(
        modifier =
            modifier
                .fillMaxWidth()
                .height(height)
                .padding(4.dp),
    ) {
        if (geometry.cells == 0 || geometry.columns == 0) return@Canvas
        val cellWidth = size.width / geometry.columns
        val cellHeight = min(cellWidth, 6.dp.toPx())
        var verifiedIndex = 0
        for (cell in 0 until geometry.cells) {
            val start = bucketStart(cell, geometry.cells, piecesTotal)
            val end = bucketStart(cell + 1, geometry.cells, piecesTotal)
            while (
                verifiedIndex < verified.size &&
                verified[verifiedIndex].endExclusive.toULong() <= start
            ) {
                verifiedIndex += 1
            }
            val range = verified.getOrNull(verifiedIndex)
            val hasVerified =
                range != null &&
                    range.start.toULong() < end &&
                    range.endExclusive.toULong() > start
            val activeInBucket =
                active.firstOrNull {
                    it.pieceIndex.toULong() >= start &&
                        it.pieceIndex.toULong() < end
                }
            val activeColor =
                when {
                    activeInBucket == null -> null
                    activeInBucket.received.isNotEmpty() -> respondedColor
                    activeInBucket.requested.isNotEmpty() -> requestedColor
                    else -> partialColor
                }
            val color = activeColor ?: if (hasVerified) verifiedColor else emptyColor
            val column = cell % geometry.columns
            val row = cell / geometry.columns
            drawRect(
                color = color,
                topLeft = Offset(column * cellWidth + 1f, row * cellHeight + 1f),
                size = Size((cellWidth - 2f).coerceAtLeast(1f), (cellHeight - 2f).coerceAtLeast(1f)),
            )
        }
    }
}

private data class RenderGeometry(
    val cells: Int,
    val columns: Int,
    val rows: Int,
)

private fun bucketStart(
    cell: Int,
    cells: Int,
    pieces: UInt,
): ULong {
    if (cells == 0) return 0UL
    return pieces.toULong() * cell.toULong() / cells.toULong()
}

private const val MAX_RENDERED_CELLS = 4_000
