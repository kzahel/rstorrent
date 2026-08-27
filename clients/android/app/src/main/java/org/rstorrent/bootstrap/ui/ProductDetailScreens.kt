@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package org.rstorrent.bootstrap.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.rstorrent.bootstrap.DiskViewState
import org.rstorrent.bootstrap.FileCatalogViewState
import org.rstorrent.bootstrap.SwarmViewState
import org.rstorrent.bootstrap.TrackerCatalogViewState
import org.rstorrent.session.uniffi.FilePriority
import org.rstorrent.session.uniffi.FileSelectionView
import org.rstorrent.session.uniffi.FileView
import org.rstorrent.session.uniffi.PeerView

@Composable
internal fun FilesScreen(
    catalog: FileCatalogViewState?,
    onSetPriority: (FileView, FilePriority) -> Unit,
    onDownloadNow: (FileView) -> Unit,
    onOpen: (FileView) -> Unit,
    onPage: (UInt) -> Unit,
) {
    if (catalog == null) {
        ProductLoading("File catalog")
        return
    }
    val files = catalog.files.values.filterNot(FileView::padding).sortedBy(FileView::fileIndex)
    LazyColumn(Modifier.fillMaxSize()) {
        item("summary") {
            ListItem(
                headlineContent = { Text("${files.size} files") },
                supportingContent = {
                    Text(
                        "${catalog.state.name.lowercase()} · " +
                            "showing ${catalog.page.offset + files.size.toUInt()} of ${catalog.page.total}",
                    )
                },
            )
            HorizontalDivider()
            CatalogPager(catalog.page, onPage)
        }
        items(files, key = FileView::fileId) { file ->
            val wanted = file.selection != FileSelectionView.SKIPPED
            var priorityMenuExpanded by remember(file.fileId) { mutableStateOf(false) }
            val length = file.lengthBytes.toULongOrNull() ?: 0UL
            val done = file.verifiedBytes.toULongOrNull() ?: 0UL
            val complete = length > 0UL && done >= length
            Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 10.dp)) {
                Row(Modifier.fillMaxWidth()) {
                    Box {
                        OutlinedButton(
                            onClick = { priorityMenuExpanded = true },
                            enabled = file.selection != null,
                        ) {
                            Text(
                                when (file.selection) {
                                    FileSelectionView.HIGH -> "High"
                                    FileSelectionView.SKIPPED -> "Skip"
                                    else -> "Normal"
                                },
                            )
                        }
                        DropdownMenu(
                            expanded = priorityMenuExpanded,
                            onDismissRequest = { priorityMenuExpanded = false },
                        ) {
                            listOf(
                                "High" to FilePriority.HIGH,
                                "Normal" to FilePriority.NORMAL,
                                "Skip" to FilePriority.SKIP,
                            ).forEach { (label, priority) ->
                                DropdownMenuItem(
                                    text = { Text(label) },
                                    onClick = {
                                        priorityMenuExpanded = false
                                        onSetPriority(file, priority)
                                    },
                                )
                            }
                        }
                    }
                    Column(Modifier.weight(1f).padding(top = 8.dp)) {
                        Text(
                            file.path.joinToString("/"),
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                            fontWeight = FontWeight.Medium,
                        )
                        Text(
                            "${formatBytes(file.verifiedBytes)} / ${formatBytes(file.lengthBytes)}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                LinearProgressIndicator(
                    progress = { if (length == 0UL) 0f else (done.toDouble() / length.toDouble()).toFloat() },
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(
                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                    horizontalArrangement = Arrangement.End,
                ) {
                    OutlinedButton(onClick = { onDownloadNow(file) }, enabled = wanted && !complete) {
                        Text("Download now")
                    }
                    Spacer(Modifier.padding(4.dp))
                    Button(onClick = { onOpen(file) }, enabled = complete) { Text("Open") }
                }
            }
            HorizontalDivider()
        }
    }
}

@Composable
internal fun TrackersScreen(
    catalog: TrackerCatalogViewState?,
    onPage: (UInt) -> Unit,
) {
    if (catalog == null) {
        ProductLoading("Tracker catalog")
        return
    }
    val trackers = catalog.trackers.values.sortedWith(compareBy({ it.tier }, { it.url }))
    LazyColumn(Modifier.fillMaxSize()) {
        item("summary") {
            ListItem(
                headlineContent = { Text("${trackers.size} trackers") },
                supportingContent = { Text(catalog.state.name.lowercase()) },
            )
            HorizontalDivider()
            CatalogPager(catalog.page, onPage)
        }
        items(trackers, key = { it.trackerId }) { tracker ->
            ListItem(
                headlineContent = {
                    Text(tracker.url, maxLines = 2, overflow = TextOverflow.Ellipsis)
                },
                supportingContent = {
                    Column {
                        Text(
                            "${tracker.status.name.lowercase()} · ${tracker.transport.name.lowercase()} · " +
                                "tier ${tracker.tier}",
                        )
                        Text(
                            tracker.lastError
                                ?: "Peers ${tracker.lastPeerCount ?: 0U} · attempts ${tracker.totalAttempts}",
                            color =
                                if (tracker.lastError == null) {
                                    MaterialTheme.colorScheme.onSurfaceVariant
                                } else {
                                    MaterialTheme.colorScheme.error
                                },
                        )
                    }
                },
            )
            HorizontalDivider()
        }
    }
}

@Composable
private fun CatalogPager(
    page: org.rstorrent.session.uniffi.CatalogPageView,
    onPage: (UInt) -> Unit,
) {
    if (page.offset == 0U && page.nextOffset == null) return
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        OutlinedButton(
            onClick = { onPage(page.offset - minOf(page.offset, page.limit)) },
            enabled = page.offset > 0U,
        ) { Text("Previous") }
        Text(
            "${page.offset + 1U}–${minOf(page.offset + page.limit, page.total)}",
            modifier = Modifier.padding(top = 12.dp),
        )
        OutlinedButton(
            onClick = { page.nextOffset?.let(onPage) },
            enabled = page.nextOffset != null,
        ) { Text("Next") }
    }
    HorizontalDivider()
}

@Composable
internal fun PeersScreen(
    peers: Collection<PeerView>?,
    swarm: SwarmViewState?,
) {
    if (peers == null && swarm == null) {
        ProductLoading("Peer and swarm views")
        return
    }
    val rows = peers.orEmpty().sortedBy(PeerView::remoteEndpoint)
    LazyColumn(Modifier.fillMaxSize()) {
        item("summary") {
            ListItem(
                headlineContent = { Text("${rows.size} connected peers") },
                supportingContent = {
                    Text(
                        swarm?.let {
                            "${it.peers.size} known · ${it.state.name.lowercase()} · cap ${it.maximumRecords}"
                        } ?: "Swarm catalog is loading…",
                    )
                },
            )
            HorizontalDivider()
        }
        items(rows, key = PeerView::connectionId) { peer -> PeerRow(peer) }
        if (rows.isEmpty()) {
            item("empty") { ProductLoading("No peers connected") }
        }
    }
}

@Composable
private fun PeerRow(peer: PeerView) {
    ListItem(
        headlineContent = {
            Text(peer.clientName ?: peer.remoteEndpoint, maxLines = 1, overflow = TextOverflow.Ellipsis)
        },
        supportingContent = {
            Column {
                Text(
                    "${peer.remoteEndpoint} · ${peer.direction.name.lowercase()} · " +
                        peer.lifecycle.name.lowercase(),
                    fontFamily = FontFamily.Monospace,
                )
                Text(
                    "↓ ${formatRate(peer.payloadDownloadRateBytes)} · " +
                        "↑ ${formatRate(peer.payloadUploadRateBytes)} · " +
                        "requests ${peer.pendingRequests ?: 0U}",
                )
            }
        },
    )
    HorizontalDivider()
}

@Composable
internal fun DiskSummary(disk: DiskViewState?, torrentId: String) {
    val rows = disk?.pieces?.values?.filter { it.torrentId == torrentId }.orEmpty()
    Card(Modifier.fillMaxWidth().padding(top = 16.dp)) {
        Column(Modifier.padding(12.dp)) {
            Text("Storage pipeline", fontWeight = FontWeight.SemiBold)
            if (disk == null) {
                Text("Loading…")
            } else {
                Text(
                    "${disk.pipeline.pressure.name.lowercase()} · " +
                        "resident ${formatBytes(disk.pipeline.residentBytes)} · " +
                        "${rows.size} active pieces",
                )
                Text(
                    "write ${formatRate(disk.pipeline.writeRateBytes)} · " +
                        "hash ${formatRate(disk.pipeline.hashRateBytes)}",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ProductLoading(label: String) {
    Column(Modifier.fillMaxWidth().padding(24.dp)) {
        Text(label, style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text("Loading…", color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}
