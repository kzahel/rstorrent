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
import androidx.compose.foundation.lazy.rememberLazyListState
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
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.rstorrent.bootstrap.ProductEngineService
import org.rstorrent.bootstrap.ProductState
import org.rstorrent.session.uniffi.TorrentState
import org.rstorrent.session.uniffi.TorrentView
import org.rstorrent.session.uniffi.DiagnosticCategory
import org.rstorrent.session.uniffi.DiagnosticEvent
import org.rstorrent.session.uniffi.DiagnosticProfile
import org.rstorrent.session.uniffi.DiagnosticSeverity

private val RSTorrentColors =
    darkColorScheme(
        primary = Color(0xFF55D6A7),
        secondary = Color(0xFFE9AA4F),
        background = Color(0xFF0B1116),
        surface = Color(0xFF121B22),
        surfaceVariant = Color(0xFF293541),
    )

@Composable
fun ProductApp(
    service: ProductEngineService?,
    onSelectStorage: () -> Unit,
) {
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
                ProductContent(service, state, onSelectStorage)
            }
        }
    }
}

@Composable
private fun ProductContent(
    service: ProductEngineService,
    state: ProductState,
    onSelectStorage: () -> Unit,
) {
    var magnet by remember { mutableStateOf("") }
    var diagnosticProfile by remember { mutableStateOf(DiagnosticProfile.NORMAL) }
    var diagnosticSeverity by remember { mutableStateOf(DiagnosticSeverity.INFO) }
    var diagnosticCategories by remember { mutableStateOf(emptySet<DiagnosticCategory>()) }
    var diagnosticTorrentOnly by remember { mutableStateOf(false) }
    var diagnosticSearch by remember { mutableStateOf("") }
    var diagnosticAutoscroll by remember { mutableStateOf(false) }
    val torrents = state.torrents.values.sortedBy { it.torrentId }
    val listState = rememberLazyListState()
    LaunchedEffect(state.diagnostics.size, diagnosticAutoscroll) {
        if (diagnosticAutoscroll && state.diagnostics.isNotEmpty()) {
            listState.animateScrollToItem(1 + torrents.size)
        }
    }
    LazyColumn(
        modifier = Modifier.fillMaxSize().padding(horizontal = 16.dp),
        state = listState,
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
            Spacer(Modifier.height(12.dp))
            Button(onClick = onSelectStorage) {
                Text(
                    if (state.storageRootReady) {
                        "Change download folder"
                    } else {
                        "Select download folder"
                    },
                )
            }
            state.storageRootLabel?.let {
                Text(
                    it,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                )
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
                enabled = state.ready && state.storageRootReady && magnet.isNotBlank(),
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
        item {
            DiagnosticsPanel(
                events = state.diagnostics,
                dropped = state.diagnosticDropped,
                resets = state.diagnosticResets,
                selectedTorrent = state.selectedTorrent,
                progressLabel =
                    state.selectedTorrent
                        ?.let(state.torrents::get)
                        ?.progress
                        ?.let {
                            "${it.disposition.name.lowercase()} · " +
                                "${it.phase.name.lowercase()} · " +
                                it.reason.name.lowercase().replace('_', ' ')
                        },
                profile = diagnosticProfile,
                severity = diagnosticSeverity,
                categories = diagnosticCategories,
                torrentOnly = diagnosticTorrentOnly,
                search = diagnosticSearch,
                autoscroll = diagnosticAutoscroll,
                onProfile = {
                    diagnosticProfile = it
                    service.configureDiagnostics(
                        diagnosticProfile,
                        diagnosticSeverity,
                        diagnosticCategories.toList(),
                        diagnosticTorrentOnly,
                    )
                },
                onSeverity = {
                    diagnosticSeverity = it
                    service.configureDiagnostics(
                        diagnosticProfile,
                        diagnosticSeverity,
                        diagnosticCategories.toList(),
                        diagnosticTorrentOnly,
                    )
                },
                onCategory = { category ->
                    diagnosticCategories =
                        if (category in diagnosticCategories) {
                            diagnosticCategories - category
                        } else {
                            diagnosticCategories + category
                        }
                    service.configureDiagnostics(
                        diagnosticProfile,
                        diagnosticSeverity,
                        diagnosticCategories.toList(),
                        diagnosticTorrentOnly,
                    )
                },
                onTorrentOnly = {
                    diagnosticTorrentOnly = !diagnosticTorrentOnly
                    service.configureDiagnostics(
                        diagnosticProfile,
                        diagnosticSeverity,
                        diagnosticCategories.toList(),
                        diagnosticTorrentOnly,
                    )
                },
                onSearch = { diagnosticSearch = it },
                onAutoscroll = { diagnosticAutoscroll = !diagnosticAutoscroll },
            )
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
            Text(
                "${torrent.progress.disposition.name.lowercase()} · " +
                    "${torrent.progress.phase.name.lowercase()} · " +
                    torrent.progress.reason.name.lowercase().replace('_', ' '),
                color =
                    if (torrent.progress.disposition.name == "BLOCKED") {
                        MaterialTheme.colorScheme.secondary
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                style = MaterialTheme.typography.bodySmall,
            )
            torrent.error?.let {
                Text(
                    it,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
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

@Composable
private fun DiagnosticsPanel(
    events: List<DiagnosticEvent>,
    dropped: String,
    resets: ULong,
    selectedTorrent: String?,
    progressLabel: String?,
    profile: DiagnosticProfile,
    severity: DiagnosticSeverity,
    categories: Set<DiagnosticCategory>,
    torrentOnly: Boolean,
    search: String,
    autoscroll: Boolean,
    onProfile: (DiagnosticProfile) -> Unit,
    onSeverity: (DiagnosticSeverity) -> Unit,
    onCategory: (DiagnosticCategory) -> Unit,
    onTorrentOnly: () -> Unit,
    onSearch: (String) -> Unit,
    onAutoscroll: () -> Unit,
) {
    val clipboard = LocalClipboardManager.current
    val needle = search.trim().lowercase()
    val visible =
        events.filter { event ->
            (!torrentOnly || event.torrentId == selectedTorrent) &&
                (
                    needle.isEmpty() ||
                        listOf(
                            event.code,
                            event.category.name,
                            event.severity.name,
                            event.summary,
                        ).any { it.lowercase().contains(needle) }
                )
        }
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
        colors = CardDefaults.cardColors(containerColor = Color(0xFF0D151B)),
    ) {
        Column(
            modifier = Modifier.padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text("Diagnostics", style = MaterialTheme.typography.titleLarge)
            Text(
                "${visible.size} shown · $dropped dropped · $resets resyncs",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
            progressLabel?.let {
                Text(
                    "Selected progress · $it",
                    color =
                        if (it.startsWith("blocked")) {
                            MaterialTheme.colorScheme.secondary
                        } else {
                            MaterialTheme.colorScheme.primary
                        },
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            visible.lastOrNull()?.let { latest ->
                Text(
                    "Latest · ${latest.severity.name.lowercase()} · " +
                        latest.category.name.lowercase(),
                    color =
                        if (latest.severity == DiagnosticSeverity.WARNING) {
                            MaterialTheme.colorScheme.secondary
                        } else {
                            MaterialTheme.colorScheme.primary
                        },
                    style = MaterialTheme.typography.labelSmall,
                )
                Text(
                    latest.code,
                    fontFamily = FontFamily.Monospace,
                    style = MaterialTheme.typography.labelSmall,
                )
                Text(latest.summary, style = MaterialTheme.typography.bodySmall)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                DiagnosticProfile.entries.forEach { value ->
                    Button(onClick = { onProfile(value) }, enabled = value != profile) {
                        Text(value.name.lowercase())
                    }
                }
            }
            Text("Minimum severity", style = MaterialTheme.typography.labelSmall)
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                DiagnosticSeverity.entries.forEach { value ->
                    Button(onClick = { onSeverity(value) }, enabled = value != severity) {
                        Text(value.name.lowercase().take(3))
                    }
                }
            }
            Text("Categories", style = MaterialTheme.typography.labelSmall)
            DiagnosticCategory.entries.chunked(3).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    row.forEach { category ->
                        Button(
                            onClick = { onCategory(category) },
                            enabled = category !in categories,
                        ) {
                            Text(category.name.lowercase().take(9))
                        }
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Button(
                    onClick = onTorrentOnly,
                    enabled = selectedTorrent != null,
                ) {
                    Text(if (torrentOnly) "Selected torrent" else "Global scope")
                }
                Button(onClick = onAutoscroll) {
                    Text(if (autoscroll) "Pause scroll" else "Resume scroll")
                }
            }
            OutlinedTextField(
                value = search,
                onValueChange = onSearch,
                modifier = Modifier.fillMaxWidth(),
                label = { Text("Search diagnostics") },
            )
            Button(
                onClick = {
                    val text =
                        visible
                            .joinToString("\n") {
                                "${it.timestampMillis} ${it.severity.name.lowercase()} " +
                                    "${it.category.name.lowercase()} ${it.code} ${it.summary}"
                            }.take(64 * 1024)
                    clipboard.setText(AnnotatedString(text))
                },
            ) {
                Text("Copy shown")
            }
            if (profile == DiagnosticProfile.TRACE) {
                Text(
                    "Trace is high volume and session-scoped.",
                    color = MaterialTheme.colorScheme.secondary,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            if (visible.isEmpty()) {
                Text(
                    "No diagnostics match the current filters.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                visible.takeLast(80).forEach { DiagnosticRow(it) }
            }
        }
    }
}

@Composable
private fun DiagnosticRow(event: DiagnosticEvent) {
    Column {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                "${event.severity.name.lowercase()} · ${event.category.name.lowercase()}",
                color =
                    when (event.severity) {
                        DiagnosticSeverity.ERROR -> MaterialTheme.colorScheme.error
                        DiagnosticSeverity.WARNING -> MaterialTheme.colorScheme.secondary
                        else -> MaterialTheme.colorScheme.primary
                    },
                style = MaterialTheme.typography.labelSmall,
            )
            Text(
                event.code,
                fontFamily = FontFamily.Monospace,
                style = MaterialTheme.typography.labelSmall,
            )
        }
        Text(event.summary, style = MaterialTheme.typography.bodySmall)
    }
}
