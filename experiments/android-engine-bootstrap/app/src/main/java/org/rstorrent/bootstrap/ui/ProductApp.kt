package org.rstorrent.bootstrap.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.rstorrent.bootstrap.ProductEngineService
import org.rstorrent.bootstrap.ProductState
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView

private val RSTorrentColors =
    darkColorScheme(
        primary = Color(0xFF55D6A7),
        secondary = Color(0xFFE9AA4F),
        background = Color(0xFF0B1116),
        surface = Color(0xFF121B22),
        surfaceVariant = Color(0xFF293541),
    )

@Composable
fun ProductApp(service: ProductEngineService?) {
    MaterialTheme(colorScheme = RSTorrentColors) {
        Surface(modifier = Modifier.fillMaxSize()) {
            if (service == null) {
                Column(modifier = Modifier.padding(24.dp)) {
                    Text("RSTorrent", style = MaterialTheme.typography.headlineLarge)
                    Spacer(Modifier.height(12.dp))
                    Text("Connecting to foreground engine…")
                }
            } else {
                val state by service.state.collectAsState()
                ProductContent(service, state)
            }
        }
    }
}

@Composable
private fun ProductContent(
    service: ProductEngineService,
    state: ProductState,
) {
    var magnet by remember { mutableStateOf("") }
    val torrents = state.torrents.values.sortedBy { it.torrentId }
    LazyColumn(
        modifier = Modifier.fillMaxSize().padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        item {
            Spacer(Modifier.height(20.dp))
            Text(
                "RSTorrent",
                style = MaterialTheme.typography.headlineLarge,
                fontWeight = FontWeight.Bold,
            )
            Text(
                if (state.ready) "Foreground engine connected" else "Opening durable profile",
                color = MaterialTheme.colorScheme.primary,
                style = MaterialTheme.typography.labelMedium,
            )
            state.error?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = MaterialTheme.colorScheme.error)
            }
            Spacer(Modifier.height(20.dp))
            OutlinedTextField(
                value = magnet,
                onValueChange = { magnet = it },
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Magnet link") },
                minLines = 2,
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = {
                    service.addMagnet(magnet)
                    magnet = ""
                },
                enabled = state.ready && magnet.isNotBlank(),
            ) {
                Text("Add magnet")
            }
            Spacer(Modifier.height(20.dp))
            Text(
                "Transfers · ${torrents.size}",
                style = MaterialTheme.typography.titleLarge,
            )
        }
        if (torrents.isEmpty()) {
            item {
                Card {
                    Text(
                        "No torrents yet. Add a controlled magnet to begin.",
                        modifier = Modifier.padding(20.dp),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        items(torrents, key = TorrentView::torrentId) { torrent ->
            TorrentCard(
                torrent = torrent,
                selected = state.selectedTorrent == torrent.torrentId,
                onSelect = { service.selectTorrent(torrent.torrentId) },
                onPause = { service.pause(torrent.torrentId) },
                onResume = { service.resume(torrent.torrentId) },
            )
            if (state.selectedTorrent == torrent.torrentId) {
                state.pieces[torrent.torrentId]?.let { pieces ->
                    Card(
                        shape = RoundedCornerShape(14.dp),
                        colors =
                            CardDefaults.cardColors(
                                containerColor = MaterialTheme.colorScheme.surface,
                            ),
                    ) {
                        Column(modifier = Modifier.padding(12.dp)) {
                            Text("Piece activity", fontWeight = FontWeight.SemiBold)
                            Text(
                                pieces.active?.let {
                                    "Active piece ${it.pieceIndex} · ${it.pieceLength} bytes"
                                } ?: "No active piece",
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                style = MaterialTheme.typography.bodySmall,
                            )
                            Spacer(Modifier.height(8.dp))
                            PieceMap(
                                piecesTotal = pieces.pieceCount,
                                verified = pieces.verified,
                                active = pieces.active,
                            )
                        }
                    }
                }
            }
        }
        item { Spacer(Modifier.height(28.dp)) }
    }
}

@Composable
private fun TorrentCard(
    torrent: TorrentView,
    selected: Boolean,
    onSelect: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
) {
    val percent =
        if (torrent.pieceCount == 0U) {
            0.0
        } else {
            torrent.verifiedPieceCount.toDouble() / torrent.pieceCount.toDouble() * 100.0
        }
    Card(
        modifier = Modifier.fillMaxWidth().clickable(onClick = onSelect),
        shape = RoundedCornerShape(14.dp),
        colors =
            CardDefaults.cardColors(
                containerColor =
                    if (selected) Color(0xFF183128) else MaterialTheme.colorScheme.surface,
            ),
    ) {
        Column(modifier = Modifier.padding(14.dp)) {
            Text(
                torrent.state.name.lowercase().replace('_', ' '),
                color = MaterialTheme.colorScheme.primary,
                style = MaterialTheme.typography.labelSmall,
            )
            Spacer(Modifier.height(5.dp))
            Text(
                torrent.torrentId,
                maxLines = 1,
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(5.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    "${torrent.verifiedPieceCount} / ${torrent.pieceCount} pieces",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(String.format("%.2f%%", percent), fontFamily = FontFamily.Monospace)
            }
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = if (torrent.state == TorrentState.PAUSED) onResume else onPause,
            ) {
                Text(if (torrent.state == TorrentState.PAUSED) "Resume" else "Pause")
            }
        }
    }
}
